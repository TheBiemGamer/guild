//! Library UI state: virtualized grid, detail view, search/filter/sort, launching and installs.
//! Disk, network and image decoding all run in tasks; `view.rs` only reads state.

mod view;

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::widget::operation;
use iced::widget::scrollable::{AbsoluteOffset, RelativeOffset, Viewport};
use iced::widget::image;
use iced::{Size, Subscription, Task, window};

use crate::cache::{self, Dirs};
use crate::library::epic::{self, Login};
use crate::library::{Game, Progress, ProviderKind};

const CARD_W: f32 = 180.0;
const CARD_H: f32 = 270.0;
const GAP: f32 = 16.0;
const ROW_H: f32 = CARD_H + GAP;
/// Off-screen rows built on each side, so fast scrolling doesn't show gaps.
const BUFFER_ROWS: usize = 2;
/// Handles further than this from the viewport are dropped to bound memory.
const KEEP_ROWS: usize = 12;
const GRID: &str = "grid";
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Title,
    LastPlayed,
    Playtime,
}

impl Sort {
    const ALL: [Sort; 3] = [Sort::Title, Sort::LastPlayed, Sort::Playtime];
}

impl fmt::Display for Sort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Sort::Title => "Title",
            Sort::LastPlayed => "Last played",
            Sort::Playtime => "Playtime",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Store {
    All,
    Only(ProviderKind),
}

impl Store {
    const ALL: [Store; 3] = [Store::All, Store::Only(ProviderKind::Steam), Store::Only(ProviderKind::Epic)];

    fn matches(self, provider: ProviderKind) -> bool {
        self == Store::All || self == Store::Only(provider)
    }
}

impl fmt::Display for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Store::All => "All stores",
            Store::Only(provider) => provider.name(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Play,
    Install,
    Uninstall,
}

/// One scan result per store.
pub type ScanResults = Vec<(ProviderKind, Result<Vec<Game>, String>)>;

#[derive(Debug, Clone)]
pub enum Message {
    CacheLoaded(Result<Vec<Game>, String>),
    /// Rescan Steam, and Epic too when `epic`: Legendary is slow and online, so it isn't polled.
    Refresh {
        epic: bool,
    },
    /// One result per scanned store, plus Epic's login state when Epic was scanned.
    Refreshed(ScanResults, Option<Login>),
    EpicLogIn,
    EpicLoggedIn(Result<(), String>),
    Scrolled(Viewport),
    Resized(Size),
    Thumb(String, Loaded),
    Search(String),
    SortBy(Sort),
    Store(Store),
    InstalledOnly(bool),
    /// Game key.
    Open(String),
    Back,
    DetailArt(String, Option<image::Handle>, Option<image::Handle>),
    Act(Action, String),
    /// Request result, and whether Epic must be rescanned (an Epic install or uninstall finished).
    Acted(Result<(), String>, bool),
    Tick,
    /// Key of the running game.
    Running(Option<String>),
    Progress(Vec<(String, Option<Progress>)>),
}

struct Detail {
    key: String,
    hero: Option<image::Handle>,
    logo: Option<image::Handle>,
}

pub struct App {
    /// Sorted by title.
    games: Vec<Game>,
    /// Indices into `games` after search, filters and sort.
    shown: Vec<usize>,
    slots: HashMap<String, Slot>,
    dirs: Arc<Dirs>,
    http: reqwest::Client,
    size: Size,
    offset_y: f32,
    /// Keys still wanted; queued jobs for other games skip their work.
    wanted: Arc<Mutex<HashSet<String>>>,
    /// Shown when there is nothing to list.
    status: String,
    running: Option<String>,
    /// Last failed store request, shown in the top bar.
    error: Option<String>,
    query: String,
    sort: Sort,
    store: Store,
    installed_only: bool,
    detail: Option<Detail>,
    /// Games polled for download progress each tick.
    tracked: HashSet<String>,
    progress: HashMap<String, Progress>,
    /// `None` until the first scan.
    epic_login: Option<Login>,
    logging_in: bool,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let dirs = Arc::new(Dirs::new());
        let db = dirs.db.clone();
        let app = Self {
            games: Vec::new(),
            shown: Vec::new(),
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
            running: None,
            error: None,
            query: String::new(),
            sort: Sort::Title,
            store: Store::All,
            installed_only: false,
            detail: None,
            tracked: HashSet::new(),
            progress: HashMap::new(),
            epic_login: None,
            logging_in: false,
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
                Task::batch([shown, Task::done(Message::Refresh { epic: true })])
            }
            Message::Refresh { epic } => Task::perform(blocking(move || scan(epic)), |(results, login)| {
                Message::Refreshed(results, login)
            }),
            Message::EpicLogIn => {
                self.logging_in = true;
                Task::perform(blocking(epic_log_in), Message::EpicLoggedIn)
            }
            Message::EpicLoggedIn(result) => {
                self.logging_in = false;
                self.error = result.err().map(|e| format!("Epic login: {e}"));
                Task::done(Message::Refresh { epic: true })
            }
            Message::Refreshed(results, login) => {
                if login.is_some() {
                    self.epic_login = login;
                }
                // Stores not scanned this time keep their games.
                let mut games: Vec<Game> =
                    self.games.iter().filter(|g| results.iter().all(|(p, _)| *p != g.provider)).cloned().collect();
                let (mut scanned, mut errors) = (Vec::new(), Vec::new());
                for (provider, result) in results {
                    match result {
                        Ok(list) => {
                            scanned.push(provider);
                            games.extend(list);
                        }
                        Err(e) => {
                            eprintln!("{}: {e}", provider.name());
                            errors.push(format!("Couldn't read the {} library: {e}", provider.name()));
                            // A store that failed this time keeps what we last knew.
                            games.extend(self.games.iter().filter(|g| g.provider == provider).cloned());
                        }
                    }
                }
                // Only a finished scan may claim the library is empty.
                self.status = if errors.is_empty() { "No installed games found.".into() } else { errors.join("\n") };
                sort_by_title(&mut games);
                if games == self.games {
                    return Task::none();
                }
                // Downloads that haven't finished yet get progress polling.
                self.tracked.extend(games.iter().filter(|g| !g.installed).map(Game::key));
                let (db, to_save) = (self.dirs.db.clone(), games.clone());
                let save = blocking(move || {
                    let saved = cache::db::open(&db).and_then(|mut conn| {
                        scanned.into_iter().try_for_each(|p| cache::db::replace(&mut conn, p, &to_save))
                    });
                    if let Err(e) = saved {
                        eprintln!("library cache save: {e}");
                    }
                });
                Task::batch([self.set_games(games), Task::future(save).discard()])
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
            Message::Search(query) => {
                self.query = query;
                self.refilter()
            }
            Message::SortBy(sort) => {
                self.sort = sort;
                self.refilter()
            }
            Message::Store(store) => {
                self.store = store;
                self.refilter()
            }
            Message::InstalledOnly(on) => {
                self.installed_only = on;
                self.refilter()
            }
            Message::Open(key) => {
                let Some(game) = self.game(&key).cloned() else { return Task::none() };
                self.detail = Some(Detail { key: key.clone(), hero: None, logo: None });
                let (http, dirs) = (self.http.clone(), self.dirs.clone());
                Task::perform(load_detail_art(game, http, dirs), move |(hero, logo)| {
                    Message::DetailArt(key.clone(), hero, logo)
                })
            }
            Message::Back => {
                self.detail = None;
                // The grid's scrollable was rebuilt at the top; put it back where the user was.
                operation::scroll_to(GRID, AbsoluteOffset { x: 0.0, y: self.offset_y })
            }
            Message::DetailArt(key, hero, logo) => {
                if let Some(detail) = self.detail.as_mut().filter(|d| d.key == key) {
                    detail.hero = hero;
                    detail.logo = logo;
                }
                Task::none()
            }
            Message::Act(action, key) => {
                let Some(game) = self.game(&key).cloned() else { return Task::none() };
                if action == Action::Install {
                    self.tracked.insert(key);
                }
                // Epic installs/uninstalls return once Legendary is done; Steam's only open a dialog.
                let rescan_epic = game.provider == ProviderKind::Epic && action != Action::Play;
                let request = blocking(move || {
                    let store = game.provider.provider();
                    match action {
                        Action::Play => store.launch(&game),
                        Action::Install => store.install(&game),
                        Action::Uninstall => store.uninstall(&game),
                    }
                    .map_err(|e| format!("{} request failed: {e}", game.provider.name()))
                });
                Task::perform(request, move |result| Message::Acted(result, rescan_epic))
            }
            Message::Acted(result, rescan_epic) => {
                self.error = result.err();
                if rescan_epic { Task::done(Message::Refresh { epic: true }) } else { Task::none() }
            }
            Message::Tick => {
                let running = blocking(|| {
                    ProviderKind::ALL.into_iter().find_map(|p| p.provider().running_game().map(|id| p.key(&id)))
                });
                let running = Task::perform(running, Message::Running);
                let tracked: Vec<Game> = self.games.iter().filter(|g| self.tracked.contains(&g.key())).cloned().collect();
                if tracked.is_empty() {
                    return running;
                }
                let poll = blocking(move || {
                    tracked.iter().map(|g| (g.key(), g.provider.provider().download_progress(g))).collect()
                });
                Task::batch([running, Task::perform(poll, Message::Progress)])
            }
            Message::Running(key) => {
                self.running = key;
                Task::none()
            }
            Message::Progress(results) => {
                let mut finished = false;
                for (key, progress) in results {
                    match progress {
                        Some(p) => _ = self.progress.insert(key, p),
                        None => {
                            finished |= self.progress.remove(&key).is_some();
                            // Not-installed games stay tracked: the user may still be in Steam's install dialog.
                            if self.game(&key).is_some_and(|g| g.installed) {
                                self.tracked.remove(&key);
                            }
                        }
                    }
                }
                // A download just completed: rescan so the game shows as installed.
                // Progress only comes from Steam, so only Steam needs the rescan.
                if finished { Task::done(Message::Refresh { epic: false }) } else { Task::none() }
            }
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            window::resize_events().map(|(_, size)| Message::Resized(size)),
            // Registry and one manifest per tracked download: cheap enough every 2 s.
            iced::time::every(Duration::from_secs(2)).map(|_| Message::Tick),
            // Picks up installs and uninstalls finished in Steam's own dialogs.
            iced::time::every(Duration::from_secs(60)).map(|_| Message::Refresh { epic: false }),
        ])
    }

    fn game(&self, key: &str) -> Option<&Game> {
        self.games.iter().find(|g| g.key() == key)
    }

    fn set_games(&mut self, games: Vec<Game>) -> Task<Message> {
        // An open page whose game vanished shows a notice; Back restores the grid's scroll.
        self.games = games;
        self.shown = arrange(&self.games, &self.query, self.store, self.installed_only, self.sort);
        self.request_visible()
    }

    /// Search, filter or sort changed: recompute and jump to the top.
    fn refilter(&mut self) -> Task<Message> {
        self.shown = arrange(&self.games, &self.query, self.store, self.installed_only, self.sort);
        self.offset_y = 0.0;
        Task::batch([operation::snap_to(GRID, RelativeOffset::START), self.request_visible()])
    }

    /// Queues thumbnails for visible games and drops handles far from the viewport.
    fn request_visible(&mut self) -> Task<Message> {
        let cols = columns(self.size.width);
        let len = self.shown.len();
        let rows = len.div_ceil(cols);
        let games = |r: Range<usize>| {
            self.shown[r.start * cols..(r.end * cols).min(len)].iter().map(|&i| &self.games[i])
        };
        let visible: Vec<&Game> = games(row_range(self.offset_y, self.size.height, rows, BUFFER_ROWS)).collect();
        let keep: HashSet<String> = games(row_range(self.offset_y, self.size.height, rows, KEEP_ROWS)).map(Game::key).collect();

        *self.wanted.lock().unwrap() = visible.iter().map(|g| g.key()).collect();
        let missing: Vec<Game> = visible.into_iter().filter(|g| !self.slots.contains_key(&g.key())).cloned().collect();
        self.slots.retain(|key, slot| !matches!(slot, Slot::Ready(_)) || keep.contains(key));

        Task::batch(missing.into_iter().map(|game| {
            let key = game.key();
            self.slots.insert(key.clone(), Slot::Pending);
            let (http, dirs, wanted) = (self.http.clone(), self.dirs.clone(), self.wanted.clone());
            Task::perform(load_thumb(game, http, dirs, wanted), move |l| Message::Thumb(key.clone(), l))
        }))
    }
}

fn scan(with_epic: bool) -> (ScanResults, Option<Login>) {
    let steam = ProviderKind::Steam.provider().list_games().map_err(|e| e.to_string());
    let mut results = vec![(ProviderKind::Steam, steam)];
    if !with_epic {
        return (results, None);
    }
    let login = epic::login_state();
    let games = match login {
        Login::LoggedIn => ProviderKind::Epic.provider().list_games().map_err(|e| e.to_string()),
        Login::LoggedOut | Login::NoLegendary => Ok(Vec::new()),
        // Offline or Epic down: an error keeps the last known Epic games.
        Login::Unreachable => Err("Legendary couldn't reach Epic".into()),
    };
    results.push((ProviderKind::Epic, games));
    (results, Some(login))
}

/// Opens our own login window as a child process, then hands its code to Legendary.
fn epic_log_in() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let output = std::process::Command::new(exe).arg(crate::EPIC_LOGIN_ARG).output().map_err(|e| e.to_string())?;
    let code = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if code.is_empty() {
        return Err("the login window was closed before logging in".into());
    }
    epic::log_in(&code).map_err(|e| e.to_string())
}

fn sort_by_title(games: &mut [Game]) {
    games.sort_by_cached_key(|g| g.title.to_lowercase());
}

/// Indices of `games` (already sorted by title) matching the search and filters, in `sort` order.
/// Stable sorts keep title order among ties; games without stats go last.
fn arrange(games: &[Game], query: &str, store: Store, installed_only: bool, sort: Sort) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut shown: Vec<usize> = (0..games.len())
        .filter(|&i| {
            let g = &games[i];
            store.matches(g.provider)
                && (g.installed || !installed_only)
                && (query.is_empty() || g.title.to_lowercase().contains(&query))
        })
        .collect();
    match sort {
        Sort::Title => {}
        Sort::LastPlayed => shown.sort_by_key(|&i| Reverse(games[i].last_played)),
        Sort::Playtime => shown.sort_by_key(|&i| Reverse(games[i].playtime_minutes)),
    }
    shown
}

async fn load_thumb(game: Game, http: reqwest::Client, dirs: Arc<Dirs>, wanted: Arc<Mutex<HashSet<String>>>) -> Loaded {
    let key = game.key();
    if !wanted.lock().unwrap().contains(&key) {
        return Loaded::Skipped;
    }
    // Cover first, landscape header as the fallback; each tries its local file, then its URL.
    let art_dir = dirs.art.join(&key);
    let mut found = None;
    for (slot, image) in [("cover", &game.art.cover), ("header", &game.art.header)] {
        if let Some(path) = cache::resolve(image, &http, art_dir.join(slot)).await {
            found = Some((slot, path));
            break;
        }
    }
    let Some((slot, src)) = found else { return Loaded::NoArt };
    // Slot in the key so a cover that shows up later doesn't reuse a header thumbnail.
    let thumb_key = format!("{key}-{slot}");
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

/// Full-size hero and logo for the detail page, decoded off the UI thread.
async fn load_detail_art(game: Game, http: reqwest::Client, dirs: Arc<Dirs>) -> (Option<image::Handle>, Option<image::Handle>) {
    let art_dir = dirs.art.join(game.key());
    let (hero, logo) = iced::futures::join!(
        cache::resolve(&game.art.hero, &http, art_dir.join("hero")),
        cache::resolve(&game.art.logo, &http, art_dir.join("logo")),
    );
    blocking(move || (hero.and_then(decode), logo.and_then(decode))).await
}

fn decode(path: PathBuf) -> Option<image::Handle> {
    match cache::open_image(&path) {
        Ok(img) => {
            let img = img.to_rgba8();
            Some(image::Handle::from_rgba(img.width(), img.height(), img.into_raw()))
        }
        Err(e) => {
            eprintln!("art {}: {e}", path.display());
            None
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
    use std::time::UNIX_EPOCH;

    use super::*;
    use crate::library::Art;

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

    fn game(title: &str, installed: bool, playtime: Option<u64>, last: Option<u64>) -> Game {
        Game {
            provider: ProviderKind::Steam,
            external_id: title.into(),
            title: title.into(),
            installed,
            install_dir: None,
            playtime_minutes: playtime,
            last_played: last.map(|s| UNIX_EPOCH + Duration::from_secs(s)),
            art: Art::default(),
        }
    }

    fn library() -> Vec<Game> {
        let mut games = vec![
            game("beta", true, Some(10), Some(300)),
            game("Alpha", false, None, None),
            game("Gamma Beta", true, Some(500), Some(100)),
            game("delta", true, Some(10), None),
        ];
        sort_by_title(&mut games);
        games
    }

    fn titles(games: &[Game], shown: Vec<usize>) -> Vec<&str> {
        shown.into_iter().map(|i| games[i].title.as_str()).collect()
    }

    #[test]
    fn refresh_keeps_games_of_stores_not_scanned_or_failed() {
        let (mut app, _) = App::new();
        let mut epic_game = game("Fortnite", true, None, None);
        epic_game.provider = ProviderKind::Epic;
        app.games = vec![epic_game.clone(), game("old steam", true, None, None)];

        // Timer refresh: Steam only. Epic's game stays, Steam's list is replaced.
        let fresh = game("new steam", true, None, None);
        let _ = app.update(Message::Refreshed(vec![(ProviderKind::Steam, Ok(vec![fresh.clone()]))], None));
        assert_eq!(app.games, vec![epic_game.clone(), fresh.clone()]);
        assert_eq!(app.epic_login, None);

        // Offline: Epic's scan fails, so its last known games stay.
        let results = vec![(ProviderKind::Steam, Ok(vec![fresh.clone()])), (ProviderKind::Epic, Err("offline".into()))];
        let _ = app.update(Message::Refreshed(results, Some(Login::Unreachable)));
        assert_eq!(app.games, vec![epic_game, fresh]);
        assert_eq!(app.epic_login, Some(Login::Unreachable));
    }

    #[test]
    fn progress_tracks_until_done_and_rescans_once() {
        let (mut app, _) = App::new();
        let downloading = game("dl", false, None, None);
        let installed = game("ok", true, None, None);
        let (dl, ok) = (downloading.key(), installed.key());
        app.games = vec![downloading, installed];
        app.tracked = HashSet::from([dl.clone(), ok.clone()]);
        let p = Progress { done: 1, total: 4 };

        // Nothing started yet: no rescan, and the not-installed game stays tracked
        // (the user may still be in Steam's dialog) while the installed one is dropped.
        let nothing = app.update(Message::Progress(vec![(dl.clone(), None), (ok.clone(), None)]));
        assert_eq!(nothing.units(), 0);
        assert_eq!(app.tracked, HashSet::from([dl.clone()]));

        let _ = app.update(Message::Progress(vec![(dl.clone(), Some(p))]));
        assert_eq!(app.progress.get(&dl), Some(&p));

        // Download finished: progress cleared and a rescan requested.
        let rescan = app.update(Message::Progress(vec![(dl.clone(), None)]));
        assert!(app.progress.is_empty());
        assert_eq!(rescan.units(), 1);
    }

    #[test]
    fn arrange_searches_case_insensitively() {
        let g = library();
        assert_eq!(titles(&g, arrange(&g, " BETA ", Store::All, false, Sort::Title)), ["beta", "Gamma Beta"]);
        assert!(arrange(&g, "zzz", Store::All, false, Sort::Title).is_empty());
    }

    #[test]
    fn arrange_filters_installed() {
        let g = library();
        assert_eq!(titles(&g, arrange(&g, "", Store::All, true, Sort::Title)), ["beta", "delta", "Gamma Beta"]);
        assert_eq!(arrange(&g, "", Store::Only(ProviderKind::Steam), false, Sort::Title).len(), 4);
    }

    #[test]
    fn arrange_sorts_with_missing_stats_last_and_title_ties() {
        let g = library();
        assert_eq!(
            titles(&g, arrange(&g, "", Store::All, false, Sort::LastPlayed)),
            ["beta", "Gamma Beta", "Alpha", "delta"]
        );
        // beta and delta tie on playtime: title order decides.
        assert_eq!(
            titles(&g, arrange(&g, "", Store::All, false, Sort::Playtime)),
            ["Gamma Beta", "beta", "delta", "Alpha"]
        );
    }
}
