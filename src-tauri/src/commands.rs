//! Everything the UI can call. Store work runs on the blocking pool; shared state sits behind one mutex.
//! The UI polls `poll` every 2 s and re-asks `library` when something changed.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::cache::{self, Dirs};
use crate::library::epic::{self, InstallEvent, Login};
use crate::library::{Game, Progress, ProviderKind};
use crate::view::{self, Sort};
use crate::{bench, epic_login};

/// How long a launched Epic game may take to show up as a process before we stop looking.
const EPIC_START_GRACE: Duration = Duration::from_secs(30);

/// Covers fetched and shrunk at once. A cold fast scroll asks for hundreds; each decode holds ~8 MB.
const COVER_JOBS: usize = 8;

pub struct AppState {
    dirs: Dirs,
    http: reqwest::Client,
    shared: Mutex<Shared>,
    cover_jobs: tokio::sync::Semaphore,
}

#[derive(Default)]
struct Shared {
    /// Sorted by title.
    games: Vec<Game>,
    /// Shown when nothing is listed.
    status: String,
    epic_login: Option<Login>,
    /// Steam games polled for download progress.
    tracked: HashSet<String>,
    /// Steam (polled) and Epic (streamed) download progress by game key.
    progress: HashMap<String, Progress>,
    /// Legendary's pid per running Epic install, for cancelling.
    install_pids: HashMap<String, u32>,
    /// Installs the user cancelled; their failure isn't an error.
    cancelled: HashSet<String>,
    epic_watch: Option<EpicWatch>,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    key: String,
    title: String,
    installed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    games: Vec<Card>,
    total: usize,
    status: String,
    epic: Option<Login>,
}

#[tauri::command]
pub fn library(
    state: State<'_, AppState>,
    query: String,
    store: Option<ProviderKind>,
    installed_only: bool,
    sort: Sort,
) -> Library {
    let s = state.lock();
    let games = view::arrange(&s.games, &query, store, installed_only, sort)
        .into_iter()
        .map(|i| Card { key: s.games[i].key(), title: s.games[i].title.clone(), installed: s.games[i].installed })
        .collect();
    Library { games, total: s.games.len(), status: s.status.clone(), epic: s.epic_login }
}

/// Fills the library from SQLite so the UI renders before the stores are scanned.
#[tauri::command]
pub async fn load_cache(app: AppHandle) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let conn = cache::db::open(&state.dirs.db).map_err(|e| e.to_string())?;
        let mut games = cache::db::load(&conn).map_err(|e| e.to_string())?;
        view::merge_last_played(&mut games, &cache::db::launches(&conn).map_err(|e| e.to_string())?);
        view::sort_by_title(&mut games);
        let mut s = state.lock();
        // A scan that already finished wins over the cache.
        if s.games.is_empty() {
            s.games = games;
        }
        Ok(())
    })
    .await
}

/// Rescans Steam, and Epic too when `epic`: Legendary is slow and online, so the UI only asks on demand.
#[tauri::command]
pub async fn refresh(app: AppHandle, epic: bool) {
    blocking(move || rescan(&app, epic)).await;
}

type ScanResults = Vec<(ProviderKind, Result<Vec<Game>, String>)>;

/// Scans, merges and saves. Runs on a blocking thread.
fn rescan(app: &AppHandle, with_epic: bool) {
    let (results, login) = scan(with_epic);
    let state = app.state::<AppState>();
    let mut s = state.lock();
    if login.is_some() {
        s.epic_login = login;
    }
    let mut merged = merge(&s.games, results);
    match cache::db::open(&state.dirs.db).and_then(|c| cache::db::launches(&c)) {
        Ok(launches) => view::merge_last_played(&mut merged.games, &launches),
        Err(e) => eprintln!("launch history: {e}"),
    }
    // Only a finished scan may claim the library is empty.
    s.status = if merged.errors.is_empty() { "No games found.".into() } else { merged.errors.join("\n") };
    if merged.games == s.games {
        return;
    }
    let unfinished: Vec<String> = merged
        .games
        .iter()
        .filter(|g| g.provider == ProviderKind::Steam && !g.installed)
        .map(Game::key)
        .collect();
    s.tracked.extend(unfinished);
    s.games = merged.games.clone();
    drop(s);
    let saved = cache::db::open(&state.dirs.db)
        .and_then(|mut conn| merged.scanned.iter().try_for_each(|&p| cache::db::replace(&mut conn, p, &merged.games)));
    if let Err(e) = saved {
        eprintln!("library cache save: {e}");
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

struct Merged {
    games: Vec<Game>,
    scanned: Vec<ProviderKind>,
    errors: Vec<String>,
}

/// Stores not scanned, or whose scan failed, keep what we last knew.
fn merge(current: &[Game], results: ScanResults) -> Merged {
    let mut games: Vec<Game> =
        current.iter().filter(|g| results.iter().all(|(p, _)| *p != g.provider)).cloned().collect();
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
                games.extend(current.iter().filter(|g| g.provider == provider).cloned());
            }
        }
    }
    view::sort_by_title(&mut games);
    Merged { games, scanned, errors }
}

/// Thumbnail file for a card: cover first, landscape header as the fallback, each from its
/// local file or URL. `None` means show a title card. At most `COVER_JOBS` run at once.
// shortcut: requests for cards already scrolled away still run (queued); add cancellation if cold first scrolls feel slow.
#[tauri::command]
pub async fn cover(app: AppHandle, key: String) -> Option<String> {
    let state = app.state::<AppState>();
    let _job = state.cover_jobs.acquire().await.ok()?;
    let game = state.game(&key)?;
    let art_dir = state.dirs.art.join(&key);
    for (slot, image) in [("cover", &game.art.cover), ("header", &game.art.header)] {
        let Some(src) = cache::resolve(image, &state.http, art_dir.join(slot)).await else { continue };
        let (thumbs, thumb_key) = (state.dirs.thumbs.clone(), format!("{key}-{slot}"));
        match blocking(move || cache::thumbnail(&src, &thumbs, &thumb_key)).await {
            Ok(path) => return allow(&app, path),
            Err(e) => eprintln!("thumbnail {key}: {e}"),
        }
    }
    None
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Featured {
    key: String,
    title: String,
    hero: Option<String>,
}

/// The game played last, with its hero art (cover as the fallback) for the library backdrop.
#[tauri::command]
pub async fn featured(app: AppHandle) -> Option<Featured> {
    let state = app.state::<AppState>();
    let game = view::featured(&state.lock().games)?.clone();
    let key = game.key();
    let dir = state.dirs.art.join(&key);
    let mut hero = None;
    for (slot, image) in [("hero", &game.art.hero), ("cover", &game.art.cover)] {
        if let Some(path) = cache::resolve(image, &state.http, dir.join(slot)).await {
            hero = allow(&app, path);
            break;
        }
    }
    Some(Featured { key, title: game.title, hero })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    key: String,
    title: String,
    provider: ProviderKind,
    installed: bool,
    playtime: String,
    last_played: String,
}

#[tauri::command]
pub fn game(state: State<'_, AppState>, key: String) -> Option<Detail> {
    let g = state.game(&key)?;
    Some(Detail {
        key,
        title: g.title,
        provider: g.provider,
        installed: g.installed,
        playtime: view::playtime_label(g.playtime_minutes),
        last_played: view::last_played_label(g.last_played, SystemTime::now()),
    })
}

#[derive(Serialize, Default)]
pub struct DetailArt {
    hero: Option<String>,
    logo: Option<String>,
}

#[tauri::command]
pub async fn detail_art(app: AppHandle, key: String) -> DetailArt {
    let state = app.state::<AppState>();
    let Some(game) = state.game(&key) else { return DetailArt::default() };
    let dir = state.dirs.art.join(&key);
    let hero = cache::resolve(&game.art.hero, &state.http, dir.join("hero")).await;
    let logo = cache::resolve(&game.art.logo, &state.http, dir.join("logo")).await;
    DetailArt { hero: hero.and_then(|p| allow(&app, p)), logo: logo.and_then(|p| allow(&app, p)) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Play,
    Install,
    Uninstall,
}

/// Steam requests return at once (Steam shows its own dialogs). Epic requests return when
/// Legendary is done; installs report progress through `poll` meanwhile.
#[tauri::command]
pub async fn act(app: AppHandle, action: Action, key: String) -> Res<()> {
    let game = app.state::<AppState>().game(&key).ok_or("This game is no longer in the library.")?;
    blocking(move || act_blocking(&app, action, &game)).await
}

fn act_blocking(app: &AppHandle, action: Action, game: &Game) -> Res<()> {
    let state = app.state::<AppState>();
    let key = game.key();
    let store = game.provider.provider();
    if game.provider == ProviderKind::Steam {
        if action == Action::Install {
            state.lock().tracked.insert(key.clone());
        }
        let result = match action {
            Action::Play => store.launch(game).inspect(|()| record_play(&state, &key)),
            Action::Install => store.install(game),
            Action::Uninstall => store.uninstall(game),
        };
        return result.map_err(|e| format!("Steam: {e}"));
    }
    let result = match action {
        Action::Play => store.launch(game).and_then(|()| epic::executable(game)).map(|exe| {
            state.lock().epic_watch = exe.map(|exe| EpicWatch { key: key.clone(), exe, since: Instant::now() });
            record_play(&state, &key);
        }),
        Action::Install => {
            let result = epic::install(game, |event| {
                let mut s = state.lock();
                match event {
                    InstallEvent::Started(pid) => _ = s.install_pids.insert(key.clone(), pid),
                    InstallEvent::Progress(p) => _ = s.progress.insert(key.clone(), p),
                }
            });
            let mut s = state.lock();
            s.install_pids.remove(&key);
            s.progress.remove(&key);
            // A cancelled install fails by design; don't report it.
            if s.cancelled.remove(&key) { Ok(()) } else { result }
        }
        Action::Uninstall => store.uninstall(game),
    };
    if action != Action::Play {
        rescan(app, true);
    }
    result.map_err(|e| format!("Epic Games: {e}"))
}

/// Remembers a launch from here, for every store, so last played survives rescans.
fn record_play(state: &AppState, key: &str) {
    let now = SystemTime::now();
    let secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    if let Some(g) = state.lock().games.iter_mut().find(|g| g.key() == key) {
        g.last_played = Some(now);
    }
    let saved = cache::db::open(&state.dirs.db).and_then(|c| cache::db::record_launch(&c, key, secs as i64));
    if let Err(e) = saved {
        eprintln!("record launch: {e}");
    }
}

/// Stops a running Epic install; Legendary resumes it next time.
#[tauri::command]
pub async fn cancel_install(app: AppHandle, key: String) -> Res<()> {
    let pid = {
        let state = app.state::<AppState>();
        let mut s = state.lock();
        let pid = *s.install_pids.get(&key).ok_or("No install is running for this game.")?;
        s.cancelled.insert(key);
        pid
    };
    blocking(move || epic::cancel(pid).map_err(|e| format!("Couldn't cancel the install: {e}"))).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Poll {
    /// Key of the game running in either store.
    running: Option<String>,
    progress: HashMap<String, Progress>,
    /// The library changed (a Steam download finished), so the UI should re-ask `library`.
    changed: bool,
}

#[tauri::command]
pub async fn poll(app: AppHandle) -> Poll {
    blocking(move || poll_blocking(&app)).await
}

fn poll_blocking(app: &AppHandle) -> Poll {
    let state = app.state::<AppState>();
    let steam_running = ProviderKind::Steam.provider().running_game().map(|id| ProviderKind::Steam.key(&id));
    let watch = state.lock().epic_watch.as_ref().map(|w| (w.key.clone(), w.exe.clone(), w.since));
    let epic_running = watch.and_then(|(key, exe, since)| {
        if epic::is_running(&exe) {
            return Some(key);
        }
        // Gone, or never showed up: stop watching once it had time to start.
        if since.elapsed() > EPIC_START_GRACE {
            state.lock().epic_watch = None;
        }
        None
    });
    let tracked: Vec<Game> = {
        let s = state.lock();
        s.games.iter().filter(|g| s.tracked.contains(&g.key())).cloned().collect()
    };
    let results = tracked.iter().map(|g| (g.key(), g.provider.provider().download_progress(g), g.installed)).collect();
    let finished = apply_steam_progress(&mut state.lock(), results);
    if finished {
        rescan(app, false);
    }
    Poll { running: steam_running.or(epic_running), progress: state.lock().progress.clone(), changed: finished }
}

/// Records polled Steam progress. Returns whether a download that was showing progress just ended.
/// Not-installed games stay tracked: the user may still be in Steam's install dialog.
fn apply_steam_progress(s: &mut Shared, results: Vec<(String, Option<Progress>, bool)>) -> bool {
    let mut finished = false;
    for (key, progress, installed) in results {
        match progress {
            Some(p) => _ = s.progress.insert(key, p),
            None => {
                finished |= s.progress.remove(&key).is_some();
                if installed {
                    s.tracked.remove(&key);
                }
            }
        }
    }
    finished
}

/// Opens Epic's login window, hands the code to Legendary, then loads the Epic library.
#[tauri::command]
pub async fn epic_log_in(app: AppHandle) -> Res<()> {
    blocking(move || {
        let code = epic_login::run(&app)?.ok_or("The login window was closed before logging in.")?;
        epic::log_in(&code).map_err(|e| e.to_string())?;
        rescan(&app, true);
        Ok(())
    })
    .await
}

/// `LAUNCHER_BENCH=<px/s>` makes the UI scroll the full library at that speed, then report.
#[tauri::command]
pub fn bench_speed() -> Option<f32> {
    std::env::var("LAUNCHER_BENCH").ok()?.parse().ok()
}

#[tauri::command]
pub fn bench_done(app: AppHandle, mut frame_ms: Vec<f32>, games: usize) {
    println!("{}", bench::stats(&mut frame_ms, games));
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::tests::game;

    #[test]
    fn merge_keeps_games_of_stores_not_scanned_or_failed() {
        let mut epic_game = game("Fortnite", true, None, None);
        epic_game.provider = ProviderKind::Epic;
        let current = vec![epic_game.clone(), game("old steam", true, None, None)];
        let fresh = game("new steam", true, None, None);

        // Steam-only rescan: Epic's game stays, Steam's list is replaced.
        let merged = merge(&current, vec![(ProviderKind::Steam, Ok(vec![fresh.clone()]))]);
        assert_eq!(merged.games, vec![epic_game.clone(), fresh.clone()]);
        assert_eq!(merged.scanned, vec![ProviderKind::Steam]);

        // Offline: Epic's scan fails, so its last known games stay and it isn't saved over.
        let results = vec![(ProviderKind::Steam, Ok(vec![fresh.clone()])), (ProviderKind::Epic, Err("offline".into()))];
        let merged = merge(&current, results);
        assert_eq!(merged.games, vec![epic_game, fresh]);
        assert_eq!(merged.scanned, vec![ProviderKind::Steam]);
        assert_eq!(merged.errors.len(), 1);
    }

    #[test]
    fn steam_progress_tracks_until_done_and_reports_finish_once() {
        let mut s = Shared { tracked: HashSet::from(["dl".to_string(), "ok".to_string()]), ..Shared::default() };
        let p = Progress { fraction: 0.25, text: "Downloading 1 of 4 GB (25%)".into() };

        // Nothing started yet: the not-installed game stays tracked, the installed one is dropped.
        assert!(!apply_steam_progress(&mut s, vec![("dl".into(), None, false), ("ok".into(), None, true)]));
        assert_eq!(s.tracked, HashSet::from(["dl".to_string()]));

        assert!(!apply_steam_progress(&mut s, vec![("dl".into(), Some(p.clone()), false)]));
        assert_eq!(s.progress.get("dl"), Some(&p));

        // Download finished: progress cleared and reported once.
        assert!(apply_steam_progress(&mut s, vec![("dl".into(), None, false)]));
        assert!(s.progress.is_empty());
        assert!(!apply_steam_progress(&mut s, vec![("dl".into(), None, false)]));
    }
}
