//! Everything the UI can call. Store work runs on the blocking pool; shared state sits behind one mutex.
//! The UI polls `poll` every 2 s (progress, running game, download queue) and re-asks `library` when something changed.

pub mod custom;
pub mod game;
pub mod library;
pub mod lists;
pub mod settings;
pub mod update;

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
    /// Takes turns for `library::rescan`.
    scans: library::ScanTurns,
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
    steam_pw_steps:
        tokio::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<settings::LoginStep>>>,
    /// The update found by the last check. Held across a check, so checks never stack.
    update: tokio::sync::Mutex<Option<tauri_plugin_updater::Update>>,
}

#[derive(Default)]
struct Shared {
    /// Sorted by title.
    games: Vec<Game>,
    /// One per game across stores, in title order. Rebuilt by `regroup` whenever `games` or `twin_lists` change.
    entries: Vec<Entry>,
    /// Shown when nothing is listed.
    status: Vec<crate::msg::Msg>,
    epic_login: Option<Login>,
    gog_login: Option<crate::library::gog::Login>,
    amazon_login: Option<crate::library::amazon::Login>,
    /// Epic, GOG and Amazon logged out on purpose (`settings::signed_out`), read with the logins.
    signed_out: [bool; 3],
    /// Epic logged out since the last run; shown once until dismissed.
    epic_notice: bool,
    /// Steam games polled for download progress.
    tracked: HashSet<String>,
    /// Steam (polled), Epic and GOG (streamed) download progress by game key.
    progress: HashMap<String, Progress>,
    /// The Legendary or gogdl pid per running job, for cancelling and pausing.
    install_pids: HashMap<String, u32>,
    /// Queued jobs the user cancelled while they ran; their failure isn't an error.
    cancelled: HashSet<String>,
    /// Epic and GOG installs, updates and verifies, run one at a time (`game::start_queue`).
    queue: crate::downloads::Queue,
    /// A runner is working through `queue`.
    queue_running: bool,
    /// The stored queue was read at start.
    queue_loaded: bool,
    /// Pause stopped the active job's process; its failure isn't an error and it stays at the head.
    interrupted: bool,
    /// The `act` call waiting for a queued job's outcome, by copy key.
    waiters: HashMap<String, std::sync::mpsc::Sender<Res<()>>>,
    watch: Option<Watch>,
    /// Steam rejected the saved token; the Settings dot asks the owner to connect again.
    steam_attention: bool,
    /// An owned-games fetch is in flight.
    owned_running: bool,
    /// When the last owned-games fetch started; non-forced fetches wait `OWNED_RETRY_SECS` after it.
    owned_attempt: Option<Instant>,
    /// The library changed outside a UI call (owned games arrived); the next poll reports it.
    library_changed: bool,
    /// Every Steam app GamesDB lists for an Epic, GOG or Amazon game, by its key.
    twin_lists: HashMap<String, Vec<u32>>,
    /// Steam twin (app id) of an Epic, GOG or Amazon game, by its key. Resolved from `twin_lists` by `regroup`.
    twins: HashMap<String, u32>,
    /// A twin lookup run is in flight.
    twins_running: bool,
    /// An Epic playtime fetch is in flight.
    playtime_running: bool,
    /// A GOG playtime fetch is in flight.
    gog_playtime_running: bool,
    /// Cached how-long-to-beat "normally" seconds by key (`igdb::all_normally`), for sort, group,
    /// filters and the card badge. Loaded once by `load_cache`; `time_to_beat` keeps it current.
    ttb: HashMap<String, u64>,
    /// Every owned Steam game as a `Game`, built once per owned list so a rescan does no art lookups.
    /// `None` until built from the cached list (or after Disconnect).
    owned: Option<Vec<Game>>,
    /// The owner's favorites, hidden games and collections. Loaded by `load_cache`; the `lists` commands keep it current.
    lists: crate::view::Lists,
}

/// An Epic, GOG or added game launched from here, watched through its process to time the session.
struct Watch {
    key: String,
    exe: String,
    since: Instant,
    /// Its process was seen running, so its end times a play session.
    seen: bool,
    /// How its cloud saves are uploaded once it exits; `None` doesn't sync (always for an added game).
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
            shared: Mutex::new(Shared {
                status: vec![msg!(loadingLibrary)],
                ..Shared::default()
            }),
            cover_jobs: tokio::sync::Semaphore::new(COVER_JOBS),
            steam_size: tokio::sync::Mutex::new(()),
            scans: library::ScanTurns::default(),
            igdb: Mutex::new(igdb::TokenCache::default()),
            steam_qr: tokio::sync::Mutex::new(None),
            steam_gen: std::sync::atomic::AtomicU64::new(0),
            steam_session_gen: std::sync::atomic::AtomicU64::new(0),
            steam_pw_codes: Mutex::new(None),
            steam_pw_steps: tokio::sync::Mutex::new(None),
            update: tokio::sync::Mutex::new(None),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().expect("state lock poisoned")
    }

    /// The window is out (or must stay in the tray): the startup show does nothing.
    pub fn mark_shown(&self) {
        self.shown.store(true, std::sync::atomic::Ordering::SeqCst);
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

type Res<T> = Result<T, crate::msg::Msg>;

/// The download queue as the UI shows it, with titles.
fn queue_view(s: &Shared) -> crate::downloads::QueueView {
    crate::downloads::view(&s.queue, |key| {
        s.games
            .iter()
            .find(|g| g.key() == key)
            .map(|g| g.title.clone())
    })
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .expect("blocking task panicked")
}

/// Lets the webview load exactly this file through the asset protocol.
fn allow(app: &AppHandle, path: PathBuf) -> Option<String> {
    app.asset_protocol_scope().allow_file(&path).ok()?;
    Some(path.to_string_lossy().into_owned())
}

/// Seconds since the Unix epoch, as the database stores times; 0 if the clock is before 1970.
fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Puts native text (tray, toasts, login windows) in the Language setting's language.
pub fn apply_language(app: &AppHandle) {
    if let Some(conn) = open_db(&app.state::<AppState>()) {
        crate::msg::set_language(&crate::settings::load(&conn).language);
    }
}

/// Whether closing the main window hides it to the tray (the default) instead of quitting.
pub fn close_to_tray(app: &AppHandle) -> bool {
    open_db(&app.state::<AppState>()).is_none_or(|conn| crate::settings::load(&conn).close_to_tray)
}

/// A Windows toast that a download finished, unless the owner turned them off.
fn notify_done(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if !open_db(&app.state::<AppState>())
        .is_none_or(|conn| crate::settings::load(&conn).notify_downloads)
    {
        return;
    }
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!("notification: {e}");
    }
}

fn open_db(state: &AppState) -> Option<rusqlite::Connection> {
    cache::db::open(&state.dirs.db)
        .inspect_err(|e| tracing::error!("settings db: {e}"))
        .ok()
}
