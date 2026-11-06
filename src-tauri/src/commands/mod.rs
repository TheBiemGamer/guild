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
}

#[derive(Default)]
struct Shared {
    /// Sorted by title.
    games: Vec<Game>,
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
        }
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().expect("state lock poisoned")
    }

    fn game(&self, key: &str) -> Option<Game> {
        self.lock().games.iter().find(|g| g.key() == key).cloned()
    }
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
