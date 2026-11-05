//! M0 spike: a manually virtualized cover grid with async, disk-cached thumbnails.
//!
//! `LAUNCHER_BENCH=<px/s>` auto-scrolls top to bottom at that speed, prints
//! frame-time stats and exits.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use iced::widget::scrollable::{AbsoluteOffset, Viewport};
use iced::widget::{Space, column, container, image, row, scrollable, text};
use iced::{ContentFit, Element, Length, Size, Subscription, Task, window};

use crate::cache;

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

enum Slot {
    Empty,
    Pending,
    Ready(image::Handle),
    Failed,
}

#[derive(Debug, Clone)]
pub enum Loaded {
    Ready(image::Handle),
    /// Scrolled out of view before the job started.
    Skipped,
    Failed,
}

#[derive(Debug, Clone)]
pub enum Message {
    Listed(Vec<PathBuf>),
    Scrolled(Viewport),
    Resized(Size),
    Thumb(usize, Loaded),
    Frame(Instant),
}

struct Bench {
    speed: f32,
    last: Option<Instant>,
    frame_ms: Vec<f32>,
}

pub struct Spike {
    covers: Vec<PathBuf>,
    slots: Vec<Slot>,
    thumbs_dir: Arc<PathBuf>,
    size: Size,
    offset_y: f32,
    /// Item range still wanted; queued jobs outside it skip decoding.
    wanted: Arc<Mutex<Range<usize>>>,
    bench: Option<Bench>,
}

impl Spike {
    pub fn new() -> (Self, Task<Message>) {
        let dir = std::env::args().nth(1).map_or_else(|| PathBuf::from("covers"), PathBuf::from);
        let bench = std::env::var("LAUNCHER_BENCH").ok().and_then(|s| s.parse().ok());
        let spike = Self {
            covers: Vec::new(),
            slots: Vec::new(),
            thumbs_dir: Arc::new(cache::thumbs_dir()),
            size: WINDOW,
            offset_y: 0.0,
            wanted: Arc::new(Mutex::new(0..0)),
            bench: bench.map(|speed| Bench { speed, last: None, frame_ms: Vec::new() }),
        };
        (spike, Task::perform(async move { list_covers(&dir) }, Message::Listed))
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Listed(covers) => {
                self.slots = covers.iter().map(|_| Slot::Empty).collect();
                self.covers = covers;
                self.request_visible()
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
            Message::Thumb(i, loaded) => {
                self.slots[i] = match loaded {
                    Loaded::Ready(handle) => Slot::Ready(handle),
                    Loaded::Failed => Slot::Failed,
                    // May have scrolled back into view while queued.
                    Loaded::Skipped => {
                        self.slots[i] = Slot::Empty;
                        return self.request_visible();
                    }
                };
                Task::none()
            }
            Message::Frame(at) => self.bench_frame(at),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let cols = columns(self.size.width);
        let rows = self.covers.len().div_ceil(cols);
        let range = row_range(self.offset_y, self.size.height, rows, BUFFER_ROWS);

        // Spacers stand in for unbuilt rows so the scrollbar and offsets stay correct.
        let mut grid = column![Space::new().height(range.start as f32 * ROW_H)];
        for r in range.clone() {
            let items = r * cols..((r + 1) * cols).min(self.covers.len());
            grid = grid.push(row(items.map(|i| self.card(i))).spacing(GAP).height(ROW_H));
        }
        grid = grid.push(Space::new().height((rows - range.end) as f32 * ROW_H));

        scrollable(grid.padding(GAP))
            .id(GRID)
            .on_scroll(Message::Scrolled)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let resized = window::resize_events().map(|(_, size)| Message::Resized(size));
        if self.bench.is_some() && !self.covers.is_empty() {
            Subscription::batch([resized, window::frames().map(Message::Frame)])
        } else {
            resized
        }
    }

    fn card(&self, i: usize) -> Element<'_, Message> {
        match &self.slots[i] {
            Slot::Ready(handle) => image(handle.clone())
                .width(CARD_W)
                .height(CARD_H)
                .content_fit(ContentFit::Cover)
                .into(),
            slot => container(text(if matches!(slot, Slot::Failed) { "no art" } else { "" }))
                .center(Length::Fill)
                .width(CARD_W)
                .height(CARD_H)
                .style(container::bordered_box)
                .into(),
        }
    }

    /// Queues thumbnails for visible rows and drops handles far from the viewport.
    fn request_visible(&mut self) -> Task<Message> {
        let cols = columns(self.size.width);
        let len = self.covers.len();
        let rows = len.div_ceil(cols);
        let to_items = |r: Range<usize>| r.start * cols..(r.end * cols).min(len);
        let visible = to_items(row_range(self.offset_y, self.size.height, rows, BUFFER_ROWS));
        let keep = to_items(row_range(self.offset_y, self.size.height, rows, KEEP_ROWS));

        *self.wanted.lock().unwrap() = visible.clone();
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if matches!(slot, Slot::Ready(_)) && !keep.contains(&i) {
                *slot = Slot::Empty;
            }
        }

        let missing: Vec<usize> = visible.filter(|&i| matches!(self.slots[i], Slot::Empty)).collect();
        Task::batch(missing.into_iter().map(|i| {
            self.slots[i] = Slot::Pending;
            let (src, dir, wanted) = (self.covers[i].clone(), self.thumbs_dir.clone(), self.wanted.clone());
            // shortcut: decodes block the iced thread-pool executor; give them their own pool if M1's network tasks starve.
            Task::perform(async move { load(i, &src, &dir, &wanted) }, move |l| Message::Thumb(i, l))
        }))
    }

    fn bench_frame(&mut self, at: Instant) -> Task<Message> {
        let rows = self.covers.len().div_ceil(columns(self.size.width));
        let bottom = (rows as f32 * ROW_H + 2.0 * GAP - self.size.height).max(0.0);
        let Some(bench) = &mut self.bench else { return Task::none() };

        let dt = bench.last.map_or(0.0, |last| (at - last).as_secs_f32());
        if bench.last.is_some() {
            bench.frame_ms.push(dt * 1000.0);
        }
        bench.last = Some(at);

        if self.offset_y >= bottom - 1.0 && !bench.frame_ms.is_empty() {
            print_stats(&mut bench.frame_ms);
            return iced::exit();
        }
        // At least 1px so the first frame still triggers a redraw.
        iced::widget::operation::scroll_by(GRID, AbsoluteOffset { x: 0.0, y: (bench.speed * dt).max(1.0) })
    }
}

fn load(i: usize, src: &Path, dir: &Path, wanted: &Mutex<Range<usize>>) -> Loaded {
    if !wanted.lock().unwrap().contains(&i) {
        return Loaded::Skipped;
    }
    let key = src.file_stem().unwrap_or_default().to_string_lossy();
    match cache::thumbnail(src, dir, &key) {
        Ok(img) => {
            let (w, h) = img.dimensions();
            Loaded::Ready(image::Handle::from_rgba(w, h, img.into_raw()))
        }
        Err(e) => {
            eprintln!("thumbnail {}: {e}", src.display());
            Loaded::Failed
        }
    }
}

fn list_covers(dir: &Path) -> Vec<PathBuf> {
    let mut covers: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| Some(e.ok()?.path()))
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("jpg")))
            .collect(),
        Err(e) => {
            eprintln!("covers dir {}: {e}", dir.display());
            Vec::new()
        }
    };
    covers.sort();
    covers
}

fn print_stats(frame_ms: &mut [f32]) {
    frame_ms.sort_by(f32::total_cmp);
    let pct = |p: f32| frame_ms[((frame_ms.len() - 1) as f32 * p) as usize];
    let median = pct(0.5);
    let total: f32 = frame_ms.iter().sum();
    let slow = frame_ms.iter().filter(|&&ms| ms > median * 1.5).count();
    println!(
        "frames {} | avg fps {:.1} | p50 {:.2}ms p95 {:.2}ms p99 {:.2}ms max {:.2}ms | >1.5x median: {slow}",
        frame_ms.len(),
        frame_ms.len() as f32 / total * 1000.0,
        median,
        pct(0.95),
        pct(0.99),
        pct(1.0),
    );
}

fn columns(width: f32) -> usize {
    // 2*GAP padding plus room for the scrollbar.
    (((width - 2.0 * GAP - 12.0 + GAP) / (CARD_W + GAP)).floor() as usize).max(1)
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
