//! Everything the UI can call. Store work runs on the blocking pool; shared state sits behind one mutex.
//! The UI polls `poll` every 2 s (progress, running game, download queue) and re-asks `library` when something changed.

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
    /// When the app started, for the "window shown after" line.
    started: Instant,
    /// The window has been shown (`library::ready`).
    shown: std::sync::atomic::AtomicBool,
    dirs: Dirs,
    http: reqwest::Client,
    shared: Mutex<Shared>,
    cover_jobs: tokio::sync::Semaphore,
    /// Held across a Steam download-size lookup, so page opens never stack Steam logons.
    steam_size: tokio::sync::Mutex<()>,
    /// Twitch app access token for IGDB. Never held across a network call.
    igdb: Mutex<igdb::TokenCache>,
    /// The Steam QR login in progress, if any. A cancel or newer start bumps `steam_gen` first, which is
    /// what makes a poll already talking to Steam save nothing.
    steam_qr: tokio::sync::Mutex<Option<crate::steam_login::QrSession>>,
    /// Bumped by every connect start and cancel; a poll that finishes under a newer one is discarded.
    steam_gen: std::sync::atomic::AtomicU64,
    /// The `steam_gen` the session in `steam_qr` was started under.
    steam_session_gen: std::sync::atomic::AtomicU64,
    /// Steam Guard codes for the password login in progress; dropping the sender cancels it.
    steam_pw_codes: Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>,
    /// What the password login asks next.
    steam_pw_steps: tokio::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<settings::LoginStep>>>,
}

#[derive(Default)]
struct Shared {
    /// Sorted by title.
    games: Vec<Game>,
    /// One per game across stores, in title order. Rebuilt by `regroup` whenever `games` or `twin_lists` change.
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
    /// Legendary's pid per running Epic job, for cancelling and pausing.
    install_pids: HashMap<String, u32>,
    /// Queued jobs the user cancelled while they ran; their failure isn't an error.
    cancelled: HashSet<String>,
    /// Epic installs, updates and verifies, run one at a time (`game::start_queue`).
    queue: crate::downloads::Queue,
    /// A runner is working through `queue`.
    queue_running: bool,
    /// The stored queue was read at start.
    queue_loaded: bool,
    /// Pause stopped the active job's Legendary; its failure isn't an error and it stays at the head.
    interrupted: bool,
    /// The `act` call waiting for a queued job's outcome, by copy key.
    waiters: HashMap<String, std::sync::mpsc::Sender<Res<()>>>,
    epic_watch: Option<EpicWatch>,
    /// Steam rejected the saved token; the Settings dot asks the owner to connect again.
    steam_attention: bool,
    /// An owned-games fetch is in flight.
    owned_running: bool,
    /// When the last owned-games fetch started; non-forced fetches wait `OWNED_RETRY_SECS` after it.
    owned_attempt: Option<Instant>,
    /// The library changed outside a UI call (owned games arrived); the next poll reports it.
    library_changed: bool,
    /// Every Steam app GamesDB lists for an Epic game, by Epic game key.
    twin_lists: HashMap<String, Vec<u32>>,
    /// Steam twin (app id) of an Epic game, by Epic game key. Resolved from `twin_lists` by `regroup`.
    twins: HashMap<String, u32>,
    /// A twin lookup run is in flight.
    twins_running: bool,
    /// An Epic playtime fetch is in flight.
    playtime_running: bool,
    /// Cached how-long-to-beat "normally" seconds by key (`igdb::all_normally`), for sort, group,
    /// filters and the card badge. Loaded once by `load_cache`; `time_to_beat` keeps it current.
    ttb: HashMap<String, u64>,
    /// Every owned Steam game as a `Game`, built once per owned list so a rescan does no art lookups.
    /// `None` until built from the cached list (or after Disconnect).
    owned: Option<Vec<Game>>,
}

/// An Epic game launched from here, watched through its process.
struct EpicWatch {
    key: String,
    exe: String,
    since: Instant,
    /// Its process was seen running, so its end times a play session.
    seen: bool,
    /// How its cloud saves are uploaded once it exits; `None` doesn't sync.
    upload: Option<crate::library::epic::SaveSync>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            shown: std::sync::atomic::AtomicBool::new(false),
            dirs: Dirs::new(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("default TLS backend is available"),
            shared: Mutex::new(Shared { status: "Loading library…".into(), ..Shared::default() }),
            cover_jobs: tokio::sync::Semaphore::new(COVER_JOBS),
            steam_size: tokio::sync::Mutex::new(()),
            igdb: Mutex::new(igdb::TokenCache::default()),
            steam_qr: tokio::sync::Mutex::new(None),
            steam_gen: std::sync::atomic::AtomicU64::new(0),
            steam_session_gen: std::sync::atomic::AtomicU64::new(0),
            steam_pw_codes: Mutex::new(None),
            steam_pw_steps: tokio::sync::Mutex::new(None),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().expect("state lock poisoned")
    }

    fn game(&self, key: &str) -> Option<Game> {
        self.lock().games.iter().find(|g| g.key() == key).cloned()
    }

    /// The first copy of the entry with this key or a copy with this key.
    fn first_copy(&self, key: &str) -> Option<Game> {
        let s = self.lock();
        let entry = entries::find(&s.entries, &s.games, key)?;
        Some(s.games[entry.copies[0]].clone())
    }
}

/// Resolves the twins against the Steam games in `games` and rebuilds the entries. Call after every
/// change to `games` or `twin_lists`.
fn regroup(s: &mut Shared) {
    s.twins = crate::gamesdb::resolve_all(&s.twin_lists, &crate::gamesdb::steam_ids(&s.games));
    s.entries = entries::group(&s.games, &s.twins);
}

type Res<T> = Result<T, String>;

/// The download queue as the UI shows it, with titles.
fn queue_view(s: &Shared) -> crate::downloads::QueueView {
    crate::downloads::view(&s.queue, |key| s.games.iter().find(|g| g.key() == key).map(|g| g.title.clone()))
}

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
