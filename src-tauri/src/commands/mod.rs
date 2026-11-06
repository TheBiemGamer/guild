//! Everything the UI can call. Store work runs on the blocking pool; shared state sits behind one mutex.
//! The UI polls `poll` every 2 s and re-asks `library` when something changed.

pub mod game;
pub mod library;
pub mod settings;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};

use crate::cache::{self, Dirs};
use crate::entries::{self, Entry};
use crate::igdb;
use crate::library::epic::Login;
use crate::library::{Game, Progress};

/// Covers fetched and shrunk at once. A cold fast scroll asks for hundreds; each decode holds ~8 MB.
const COVER_JOBS: usize = 8;

pub struct AppState {
    dirs: Dirs,
    http: reqwest::Client,
    shared: Mutex<Shared>,
    cover_jobs: tokio::sync::Semaphore,
    /// Twitch app access token for IGDB. Never held across a network call.
    igdb: Mutex<igdb::TokenCache>,
    /// The Steam QR login in progress, if any. Held across the poll call so a cancel waits for it.
    steam_qr: tokio::sync::Mutex<Option<crate::steam_login::QrSession>>,
    /// Bumped by every connect start and cancel; a poll that finishes under a newer one is discarded.
    steam_gen: std::sync::atomic::AtomicU64,
    /// The `steam_gen` the session in `steam_qr` was started under.
    steam_session_gen: std::sync::atomic::AtomicU64,
}

#[derive(Default)]
struct Shared {
    /// Sorted by title.
    games: Vec<Game>,
    /// One per game across stores, in title order. Rebuilt by `regroup` whenever `games` or `twins` change.
    entries: Vec<Entry>,
    /// Shown when nothing is listed.
    status: String,
    epic_login: Option<Login>,
    /// Epic logged out since the last run; shown once until dismissed.
    epic_notice: bool,
    /// Steam games polled for download progress.
    tracked: HashSet<String>,
    /// Steam (polled) and Epic (streamed) download progress by game key.
    progress: HashMap<String, Progress>,
    /// Legendary's pid per running Epic install, for cancelling.
    install_pids: HashMap<String, u32>,
    /// Installs the user cancelled; their failure isn't an error.
    cancelled: HashSet<String>,
    epic_watch: Option<EpicWatch>,
    /// Steam rejected the saved token; the Settings dot asks the owner to connect again.
    steam_attention: bool,
    /// An owned-games fetch is in flight.
    owned_running: bool,
    /// When the last owned-games fetch started; non-forced fetches wait `OWNED_RETRY_SECS` after it.
    owned_attempt: Option<Instant>,
    /// The library changed outside a UI call (owned games arrived); the next poll reports it.
    library_changed: bool,
    /// Steam twin (app id) of an Epic game, by Epic game key.
    twins: HashMap<String, u32>,
    /// A twin lookup run is in flight.
    twins_running: bool,
    /// Every owned Steam game as a `Game`, built once per owned list so a rescan does no art lookups.
    /// `None` until built from the cached list (or after Disconnect).
    owned: Option<Vec<Game>>,
}

/// An Epic game launched from here, watched through its process.
struct EpicWatch {
    key: String,
    exe: String,
    since: Instant,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            dirs: Dirs::new(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("default TLS backend is available"),
            shared: Mutex::new(Shared { status: "Loading library…".into(), ..Shared::default() }),
            cover_jobs: tokio::sync::Semaphore::new(COVER_JOBS),
            igdb: Mutex::new(igdb::TokenCache::default()),
            steam_qr: tokio::sync::Mutex::new(None),
            steam_gen: std::sync::atomic::AtomicU64::new(0),
            steam_session_gen: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().expect("state lock poisoned")
    }

    fn game(&self, key: &str) -> Option<Game> {
        self.lock().games.iter().find(|g| g.key() == key).cloned()
    }

    /// The entry with this key or a copy with this key, and clones of its copies in copy order.
    fn entry(&self, key: &str) -> Option<(Entry, Vec<Game>)> {
        let s = self.lock();
        let entry = entries::find(&s.entries, &s.games, key)?.clone();
        let copies = entry.copies.iter().map(|&i| s.games[i].clone()).collect();
        Some((entry, copies))
    }
}

/// Rebuilds the entries. Call after every change to `games` or `twins`.
fn regroup(s: &mut Shared) {
    s.entries = entries::group(&s.games, &s.twins);
}

type Res<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tauri::async_runtime::spawn_blocking(f).await.expect("blocking task panicked")
}

/// Lets the webview load exactly this file through the asset protocol.
fn allow(app: &AppHandle, path: PathBuf) -> Option<String> {
    app.asset_protocol_scope().allow_file(&path).ok()?;
    Some(path.to_string_lossy().into_owned())
}

/// Seconds since the Unix epoch, as the database stores times; 0 if the clock is before 1970.
fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn open_db(state: &AppState) -> Option<rusqlite::Connection> {
    cache::db::open(&state.dirs.db).inspect_err(|e| eprintln!("settings db: {e}")).ok()
}
