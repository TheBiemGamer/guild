//! Library grid: manually virtualized, cached library first, async disk-cached thumbnails.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::widget::scrollable::Viewport;
use iced::widget::{Space, center, column, container, image, row, scrollable, text};
use iced::{ContentFit, Element, Length, Size, Subscription, Task, window};

use crate::cache::{self, Dirs};
use crate::library::steam::{self, Steam};
use crate::library::{Game, Provider, ProviderKind};

const CARD_W: f32 = 180.0;
const CARD_H: f32 = 270.0;
const GAP: f32 = 16.0;
const ROW_H: f32 = CARD_H + GAP;
/// Off-screen rows built on each side, so fast scrolling doesn't show gaps.
const BUFFER_ROWS: usize = 2;
/// Handles further than this from the viewport are dropped to bound memory.
const KEEP_ROWS: usize = 12;
pub const WINDOW: Size = Size::new(1280.0, 800.0);

/// Thumbnail state per game key. Absent means not requested.
enum Slot {
    Pending,
    Ready(image::Handle),
    /// No art anywhere: the card stays a title card.
    NoArt,
}

#[derive(Debug, Clone)]
pub enum Loaded {
    Ready(image::Handle),
    /// Scrolled out of view before the job started.
    Skipped,
    NoArt,
}

#[derive(Debug, Clone)]
pub enum Message {
    CacheLoaded(Result<Vec<Game>, String>),
    Refreshed(Result<Vec<Game>, String>),
    Scrolled(Viewport),
    Resized(Size),
    Thumb(String, Loaded),
}

pub struct App {
    games: Vec<Game>,
    slots: HashMap<String, Slot>,
    dirs: Arc<Dirs>,
    http: reqwest::Client,
    size: Size,
    offset_y: f32,
    /// Keys still wanted; queued jobs for other games skip their work.
    wanted: Arc<Mutex<HashSet<String>>>,
    /// Shown when there is nothing to list.
    status: String,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let dirs = Arc::new(Dirs::new());
        let db = dirs.db.clone();
        let app = Self {
            games: Vec::new(),
            slots: HashMap::new(),
            dirs,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("default TLS backend is available"),
            size: WINDOW,
            offset_y: 0.0,
            wanted: Arc::default(),
            status: "Loading library…".into(),
        };
        let load = blocking(move || cache::db::open(&db).and_then(|conn| cache::db::load(&conn)));
        (app, Task::perform(load, |r| Message::CacheLoaded(r.map_err(|e| e.to_string()))))
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CacheLoaded(cached) => {
                // Refresh only after the cache is shown, so stale rows can't overwrite fresh ones.
                let shown = match cached {
                    Ok(games) => self.set_games(games),
                    Err(e) => {
                        eprintln!("library cache: {e}");
                        Task::none()
                    }
                };
                let db = self.dirs.db.clone();
                Task::batch([shown, Task::perform(blocking(move || refresh(&db)), Message::Refreshed)])
            }
            Message::Refreshed(Ok(games)) => {
                // Only a finished scan may claim the library is empty.
                self.status = "No installed games found.".into();
                self.set_games(games)
            }
            Message::Refreshed(Err(e)) => {
                eprintln!("steam: {e}");
                self.status = format!("Couldn't read the Steam library: {e}");
                Task::none()
            }
            Message::Scrolled(viewport) => {
                self.offset_y = viewport.absolute_offset().y;
                self.size = viewport.bounds().size();
                self.request_visible()
            }
            Message::Resized(size) => {
                self.size = size;
                self.request_visible()
            }
            Message::Thumb(key, loaded) => {
                match loaded {
                    Loaded::Ready(handle) => self.slots.insert(key, Slot::Ready(handle)),
                    Loaded::NoArt => self.slots.insert(key, Slot::NoArt),
                    // May have scrolled back into view while queued.
                    Loaded::Skipped => {
                        self.slots.remove(&key);
                        return self.request_visible();
                    }
                };
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        if self.games.is_empty() {
            return center(text(&self.status)).into();
        }
        let cols = columns(self.size.width);
        let rows = self.games.len().div_ceil(cols);
        let range = row_range(self.offset_y, self.size.height, rows, BUFFER_ROWS);

        // Spacers stand in for unbuilt rows so the scrollbar and offsets stay correct.
        let mut grid = column![Space::new().height(range.start as f32 * ROW_H)];
        for r in range.clone() {
            let games = &self.games[r * cols..((r + 1) * cols).min(self.games.len())];
            grid = grid.push(row(games.iter().map(|g| self.card(g))).spacing(GAP).height(ROW_H));
        }
        grid = grid.push(Space::new().height((rows - range.end) as f32 * ROW_H));

        scrollable(grid.padding(GAP))
            .on_scroll(Message::Scrolled)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        window::resize_events().map(|(_, size)| Message::Resized(size))
    }

    fn card<'a>(&self, game: &'a Game) -> Element<'a, Message> {
        let content: Element<'a, Message> = match self.slots.get(&game.key()) {
            // Contain: 2:3 covers fill the card, landscape headers sit centered.
            Some(Slot::Ready(handle)) => image(handle.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(ContentFit::Contain)
                .into(),
            // Title card while loading or when there is no art.
            _ => center(text(&game.title).size(18).center()).padding(12).into(),
        };
        container(content).width(CARD_W).height(CARD_H).style(container::bordered_box).into()
    }

    fn set_games(&mut self, mut games: Vec<Game>) -> Task<Message> {
        games.sort_by_cached_key(|g| g.title.to_lowercase());
        self.games = games;
        self.request_visible()
    }

    /// Queues thumbnails for visible games and drops handles far from the viewport.
    fn request_visible(&mut self) -> Task<Message> {
        let cols = columns(self.size.width);
        let len = self.games.len();
        let rows = len.div_ceil(cols);
        let to_items = |r: Range<usize>| r.start * cols..(r.end * cols).min(len);
        let visible = &self.games[to_items(row_range(self.offset_y, self.size.height, rows, BUFFER_ROWS))];
        let keep: HashSet<String> = self.games[to_items(row_range(self.offset_y, self.size.height, rows, KEEP_ROWS))]
            .iter()
            .map(Game::key)
            .collect();

        *self.wanted.lock().unwrap() = visible.iter().map(Game::key).collect();
        self.slots.retain(|key, slot| !matches!(slot, Slot::Ready(_)) || keep.contains(key));

        let missing: Vec<Game> = visible.iter().filter(|g| !self.slots.contains_key(&g.key())).cloned().collect();
        Task::batch(missing.into_iter().map(|game| {
            let key = game.key();
            self.slots.insert(key.clone(), Slot::Pending);
            let (http, dirs, wanted) = (self.http.clone(), self.dirs.clone(), self.wanted.clone());
            Task::perform(load_thumb(game, http, dirs, wanted), move |l| Message::Thumb(key.clone(), l))
        }))
    }
}

/// Scans the stores and saves the result. A failed save still shows the fresh list.
fn refresh(db: &std::path::Path) -> Result<Vec<Game>, String> {
    let games = Steam.list_games().map_err(|e| e.to_string())?;
    if let Err(e) = cache::db::open(db).and_then(|mut conn| cache::db::replace(&mut conn, ProviderKind::Steam, &games)) {
        eprintln!("library cache save: {e}");
    }
    Ok(games)
}

async fn load_thumb(game: Game, http: reqwest::Client, dirs: Arc<Dirs>, wanted: Arc<Mutex<HashSet<String>>>) -> Loaded {
    let key = game.key();
    if !wanted.lock().unwrap().contains(&key) {
        return Loaded::Skipped;
    }
    let Some(src) = steam::cover_source(&game, &http, &dirs.art).await else {
        return Loaded::NoArt;
    };
    // Source name in the key so a later real cover doesn't reuse a header thumbnail.
    let thumb_key = format!("{key}-{}", src.file_stem().unwrap_or_default().to_string_lossy());
    let result = blocking(move || cache::thumbnail(&src, &dirs.thumbs, &thumb_key).map_err(|e| (src, e))).await;
    match result {
        Ok(img) => {
            let (w, h) = img.dimensions();
            Loaded::Ready(image::Handle::from_rgba(w, h, img.into_raw()))
        }
        Err((src, e)) => {
            eprintln!("thumbnail {}: {e}", src.display());
            Loaded::NoArt
        }
    }
}

/// Runs disk or CPU-heavy work on tokio's blocking pool instead of an async worker.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.expect("blocking task panicked")
}

fn columns(width: f32) -> usize {
    // Left padding plus room for the scrollbar.
    (((width - GAP - 12.0) / (CARD_W + GAP)).floor() as usize).max(1)
}

/// Rows intersecting the viewport, widened by `buffer` on each side.
fn row_range(offset_y: f32, height: f32, rows: usize, buffer: usize) -> Range<usize> {
    let first = (offset_y / ROW_H).floor().max(0.0) as usize;
    let last = ((offset_y + height) / ROW_H).ceil() as usize;
    let end = (last + buffer).min(rows);
    first.saturating_sub(buffer).min(end)..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_range_covers_viewport_plus_buffer() {
        assert_eq!(row_range(0.0, 800.0, 100, 2), 0..5); // ceil(800/286)=3, +2
        assert_eq!(row_range(ROW_H * 10.5, 800.0, 100, 2), 8..16);
        assert_eq!(row_range(ROW_H * 98.0, 800.0, 100, 2), 96..100); // clamped at end
        assert_eq!(row_range(0.0, 800.0, 0, 2), 0..0);
        assert_eq!(row_range(ROW_H * 50.0, 800.0, 3, 2), 3..3); // stale offset past content
    }

    #[test]
    fn columns_never_zero() {
        assert_eq!(columns(0.0), 1);
        assert_eq!(columns(1280.0), 6);
    }
}
