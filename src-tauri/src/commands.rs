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
use crate::library::{Game, Image, Progress, ProviderKind};
use crate::view::{self, Sort};
use crate::library::steam;
use crate::igdb;
use crate::info::{self, GameInfoView};
use crate::{bench, epic_login, settings};

/// How long a launched Epic game may take to show up as a process before we stop looking.
const EPIC_START_GRACE: Duration = Duration::from_secs(30);

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    key: String,
    title: String,
    provider: ProviderKind,
    installed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    games: Vec<Card>,
    total: usize,
    status: String,
    epic: Option<Login>,
    /// Epic state is known and isn't logged in.
    epic_attention: bool,
    epic_notice: bool,
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
        .map(|i| {
            let g = &s.games[i];
            Card { key: g.key(), title: g.title.clone(), provider: g.provider, installed: g.installed }
        })
        .collect();
    Library {
        games,
        total: s.games.len(),
        status: s.status.clone(),
        epic: s.epic_login,
        epic_attention: s.epic_login.is_some_and(|l| l != Login::LoggedIn),
        epic_notice: s.epic_notice,
    }
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
    if let Some(login) = login {
        s.epic_login = Some(login);
        note_epic_login(&state, &mut s, login);
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
    let games = epic_result(login, || ProviderKind::Epic.provider().list_games().map_err(|e| e.to_string()));
    results.push((ProviderKind::Epic, games));
    (results, Some(login))
}

/// Only a logged-in Epic is listed. Any other state is an `Err`, so `merge` keeps the cached Epic games.
fn epic_result(login: Login, list: impl FnOnce() -> Result<Vec<Game>, String>) -> Result<Vec<Game>, String> {
    match login {
        Login::LoggedIn => list(),
        Login::LoggedOut => Err(format!("{EPIC_QUIET}: not logged in")),
        Login::NoLegendary => Err(format!("{EPIC_QUIET}: Legendary isn't installed")),
        Login::Unreachable => Err(format!("{EPIC_QUIET}: Legendary couldn't reach Epic")),
    }
}

/// Prefix of the `Err`s that only keep the cache; `Library::epic_attention` reports them, so they stay out of `status`.
const EPIC_QUIET: &str = "Epic unavailable";

/// True only when Epic went from logged in to logged out between two scans.
fn logged_out_since_last_run(previous: Option<&str>, now: Login) -> bool {
    previous == Some("loggedIn") && now == Login::LoggedOut
}

const EPIC_LAST_LOGIN: &str = "epic_last_login";

/// Compares with the state stored by the last scan, raises the notice on a fresh logout and stores the new state.
/// `Unreachable` says nothing about the login, so it leaves the stored state alone.
fn note_epic_login(state: &AppState, s: &mut Shared, login: Login) {
    let name = match login {
        Login::LoggedIn => "loggedIn",
        Login::LoggedOut => "loggedOut",
        Login::NoLegendary => "noLegendary",
        Login::Unreachable => return,
    };
    let stored = cache::db::open(&state.dirs.db).and_then(|c| {
        let previous = cache::db::setting(&c, EPIC_LAST_LOGIN)?;
        cache::db::set_setting(&c, EPIC_LAST_LOGIN, Some(name))?;
        Ok(previous)
    });
    match stored {
        Ok(previous) => s.epic_notice = epic_notice(s.epic_notice, previous.as_deref(), login),
        Err(e) => eprintln!("epic login state: {e}"),
    }
}

/// Whether the "logged out of Epic" notice shows after a scan found `login`. Logging back in clears it.
fn epic_notice(showing: bool, previous: Option<&str>, login: Login) -> bool {
    login != Login::LoggedIn && (showing || logged_out_since_last_run(previous, login))
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
                if !(provider == ProviderKind::Epic && e.starts_with(EPIC_QUIET)) {
                    errors.push(format!("Couldn't read the {} library: {e}", provider.name()));
                }
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
    /// Tile text, like `15 h 3 min`; `None` hides the tile (Epic doesn't report playtime).
    playtime_tile: Option<String>,
    /// Tile text, like `12 days ago`; `None` when never played.
    last_played_tile: Option<String>,
    /// Size on disk of an installed game, like `27.3 GB`.
    size: Option<String>,
    /// Only for an installed game (Steam knows a folder for games it hasn't installed yet).
    install_dir: Option<String>,
}

#[tauri::command]
pub fn game(state: State<'_, AppState>, key: String) -> Option<Detail> {
    let g = state.game(&key)?;
    Some(Detail {
        key,
        size: g.size_bytes.filter(|_| g.installed).map(view::size_label),
        install_dir: installed_dir(&g).map(|p| p.to_string_lossy().into_owned()),
        title: g.title,
        provider: g.provider,
        installed: g.installed,
        playtime_tile: view::playtime_tile(g.provider, g.playtime_minutes),
        last_played_tile: view::last_played_tile(g.last_played, SystemTime::now()),
    })
}

fn installed_dir(g: &Game) -> Option<&PathBuf> {
    g.install_dir.as_ref().filter(|_| g.installed)
}

/// The game's store page. The Legendary catalog has no product slug, so Epic gets a store search by title.
fn store_url(g: &Game) -> String {
    match g.provider {
        ProviderKind::Steam => format!("https://store.steampowered.com/app/{}", g.external_id),
        ProviderKind::Epic => {
            format!("https://store.epicgames.com/browse?q={}", crate::library::percent_encode(&g.title))
        }
    }
}

/// Opens an installed game's folder in File Explorer.
#[tauri::command]
pub fn open_folder(state: State<'_, AppState>, key: String) -> Res<()> {
    let game = state.game(&key).ok_or("This game is no longer in the library.")?;
    let dir = installed_dir(&game).ok_or("This game has no install folder.")?;
    if !dir.is_dir() {
        return Err(format!("The install folder is missing: {}", dir.display()));
    }
    std::process::Command::new("explorer.exe").arg(dir).spawn().map(drop).map_err(|e| e.to_string())
}

/// Opens the game's store page in the default browser.
#[tauri::command]
pub fn open_store_page(state: State<'_, AppState>, key: String) -> Res<()> {
    let game = state.game(&key).ok_or("This game is no longer in the library.")?;
    crate::library::open_uri(&store_url(&game)).map_err(|e| e.to_string())
}

#[derive(Serialize, Default)]
pub struct DetailArt {
    hero: Option<String>,
}

#[tauri::command]
pub async fn detail_art(app: AppHandle, key: String) -> DetailArt {
    let state = app.state::<AppState>();
    let Some(game) = state.game(&key) else { return DetailArt::default() };
    let dir = state.dirs.art.join(&key);
    // The game page shows a text title, so the logo art isn't fetched.
    let hero = cache::resolve(&game.art.hero, &state.http, dir.join("hero")).await;
    DetailArt { hero: hero.and_then(|p| allow(&app, p)) }
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
        Action::Play => store.launch(game).map(|()| {
            record_play(&state, &key);
            // The game started either way; without its exe name it just isn't shown as running.
            let exe = epic::executable(game).unwrap_or_else(|e| {
                eprintln!("legendary list-installed {key}: {e}");
                None
            });
            state.lock().epic_watch = exe.map(|exe| EpicWatch { key: key.clone(), exe, since: Instant::now() });
        }),
        Action::Install => {
            let base = open_db(&state).map(|c| settings::load(&c).epic_install_dir).unwrap_or_default();
            let result = epic::install(game, base.as_deref(), |event| {
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

/// How long an Epic download size stays good; sizes change only with game updates.
const SIZE_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;
const INFO_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;

/// Download size of an Epic game that isn't installed, like `27.3 GB`, for the install popup and game page.
/// Asks Legendary online (slow, seconds), so the bytes are cached for a week. None if unknown.
#[tauri::command]
pub async fn download_size(app: AppHandle, key: String) -> Option<String> {
    let game = app.state::<AppState>().game(&key).filter(|g| g.provider == ProviderKind::Epic)?;
    blocking(move || {
        let conn = open_db(&app.state::<AppState>());
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        let cached = conn
            .as_ref()
            .and_then(|c| cache::db::cached_info(c, &key, "size", SIZE_MAX_AGE_SECS, now).ok().flatten())
            .and_then(|json| json.parse::<u64>().ok());
        if cached.is_some() {
            return cached;
        }
        let size = epic::info_json(&game)
            .inspect_err(|e| eprintln!("legendary info {key}: {e}"))
            .ok()
            .and_then(|info| epic::download_size_from_info(&info))?;
        if let Some(c) = &conn {
            _ = cache::db::put_info(c, &key, "size", &size.to_string(), now);
        }
        Some(size)
    })
    .await
    .map(view::size_label)
}

/// Store details for the game page (description, genres, developer, screenshots).
/// Fetched once a week per game; a failed fetch is not cached, so the next visit retries.
/// An Epic game the catalog says nothing about is cached as `null` like the rest of its list.
/// Screenshots come back as local files, downloaded on first use.
#[tauri::command]
pub async fn game_info(app: AppHandle, key: String) -> Option<GameInfoView> {
    let game = app.state::<AppState>().game(&key)?;
    let json = {
        let (app, key, game) = (app.clone(), key.clone(), game.clone());
        blocking(move || {
            let state = app.state::<AppState>();
            let conn = open_db(&state);
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
            let http = state.http.clone();
            info::cached_or_fetch(conn.as_ref(), &key, INFO_MAX_AGE_SECS, now, || match game.provider {
                ProviderKind::Steam => {
                    let url = format!(
                        "https://store.steampowered.com/api/appdetails?appids={}&l=english",
                        game.external_id
                    );
                    let body = tauri::async_runtime::block_on(async {
                        http.get(url).send().await?.error_for_status()?.text().await
                    })
                    .inspect_err(|e| eprintln!("appdetails {key}: {e}"))
                    .ok()?;
                    serde_json::to_string(&info::parse_steam_appdetails(&body, &game.external_id)?).ok()
                }
                // One list answers for every Epic game, so all of them are cached from it.
                ProviderKind::Epic => {
                    let list = epic::catalog().inspect_err(|e| eprintln!("legendary list {key}: {e}")).ok()?;
                    let all = info::epic_details(&list);
                    if let Some(c) = &conn
                        && let Err(e) = info::remember_all(c, &all, now)
                    {
                        eprintln!("epic details cache: {e}");
                    }
                    Some(all.into_iter().find(|(k, _)| *k == key).map_or_else(|| "null".into(), |(_, json)| json))
                }
            })
        })
        .await?
    };
    // `null` is a cached "nothing to show".
    let mut details: GameInfoView = serde_json::from_str::<Option<GameInfoView>>(&json).ok()??;
    let state = app.state::<AppState>();
    let dir = state.dirs.art.join(&key);
    // Downloaded side by side; awaited in order, so the screenshots keep theirs.
    let downloads: Vec<_> = details
        .screenshots
        .iter()
        .enumerate()
        .map(|(n, shot)| {
            let (http, dest) = (state.http.clone(), dir.join(format!("shot{n}")));
            let image = Image { path: None, url: Some(shot.clone()) };
            tauri::async_runtime::spawn(async move { cache::resolve(&image, &http, dest).await })
        })
        .collect();
    let mut shots = Vec::new();
    for download in downloads {
        if let Ok(Some(path)) = download.await {
            shots.extend(allow(&app, path));
        }
    }
    details.screenshots = shots;
    Some(details)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeToBeatView {
    normally: Option<String>,
    hastily: Option<String>,
    completely: Option<String>,
}

fn time_to_beat_view(t: igdb::TimeToBeat) -> Option<TimeToBeatView> {
    let label = |secs: Option<u64>| secs.filter(|&s| s > 0).map(view::hours_label);
    let shown = TimeToBeatView { normally: label(t.normally), hastily: label(t.hastily), completely: label(t.completely) };
    (shown.normally.is_some() || shown.hastily.is_some() || shown.completely.is_some()).then_some(shown)
}

async fn look_up_time_to_beat(
    state: &AppState,
    client_id: &str,
    secret: &str,
    game: &Game,
) -> Res<Option<igdb::TimeToBeat>> {
    let token = igdb::cached_token(&state.igdb, &state.http, client_id, secret).await?;
    let Some(id) = igdb::game_id(&state.http, &token, client_id, game.provider, &game.external_id).await? else {
        return Ok(None);
    };
    igdb::time_to_beat(&state.http, &token, client_id, id).await
}

/// How long to beat, from IGDB, matched by Steam app ID (Epic games have no usable IGDB ID, so
/// they get `None`). `None` too without saved credentials or when IGDB doesn't know the game.
/// Cached 30 days when found, 7 when not; a failed request is not cached.
// Attribution: IGDB asks for visible credit where its data is shown; the game page puts a small
// "Data from IGDB" link (`open_igdb`) under the tile.
#[tauri::command]
pub async fn time_to_beat(app: AppHandle, key: String) -> Option<TimeToBeatView> {
    let state = app.state::<AppState>();
    let game = state.game(&key).filter(|g| igdb::can_match(g.provider))?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let (cached, credentials) = {
        let (app, key) = (app.clone(), key.clone());
        blocking(move || {
            let conn = open_db(&app.state::<AppState>());
            let cached = conn.as_ref().and_then(|c| igdb::cached(c, &key, now));
            if cached.is_some() {
                return (cached, None);
            }
            let client_id = conn.and_then(|c| settings::load(&c).igdb_client_id);
            (None, client_id.zip(settings::secret()))
        })
        .await
    };
    if let Some(answer) = cached {
        return answer.and_then(time_to_beat_view);
    }
    let (client_id, secret) = credentials?;
    let answer = match look_up_time_to_beat(&state, &client_id, &secret, &game).await {
        Ok(answer) => answer,
        Err(e) => {
            eprintln!("how long to beat {key}: {e}");
            return None;
        }
    };
    {
        let (app, key) = (app.clone(), key.clone());
        blocking(move || {
            if let Some(conn) = open_db(&app.state::<AppState>()) {
                igdb::remember(&conn, &key, answer, now);
            }
        })
        .await;
    }
    answer.and_then(time_to_beat_view)
}

/// Settings "Test": gets a token with the saved credentials and looks up Steam app 447040.
/// Errors say what Twitch or IGDB answered and never contain the secret.
#[tauri::command]
pub async fn test_igdb(app: AppHandle) -> Res<()> {
    let state = app.state::<AppState>();
    let (client_id, secret) = {
        let app = app.clone();
        blocking(move || {
            let conn = cache::db::open(&app.state::<AppState>().dirs.db).map_err(|e| e.to_string())?;
            let client_id = settings::load(&conn).igdb_client_id.ok_or("Enter the Client ID first.")?;
            let secret = settings::secret().ok_or("Save the Client Secret first.")?;
            Ok::<_, String>((client_id, secret))
        })
        .await?
    };
    let token = igdb::token(&state.http, &client_id, &secret).await?;
    state.igdb.lock().expect("token cache lock poisoned").store(token.clone());
    igdb::game_id(&state.http, &token, &client_id, ProviderKind::Steam, "447040").await?;
    Ok(())
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

#[tauri::command]
pub fn dismiss_epic_notice(state: State<'_, AppState>) {
    state.lock().epic_notice = false;
}

/// Forgets the Epic login in Legendary, then rescans Epic. Cached Epic games stay.
#[tauri::command]
pub async fn epic_log_out(app: AppHandle) -> Res<()> {
    blocking(move || {
        epic::log_out().map_err(|e| e.to_string())?;
        rescan(&app, true);
        // The user did this; it isn't news.
        app.state::<AppState>().lock().epic_notice = false;
        Ok(())
    })
    .await
}

fn open_db(state: &AppState) -> Option<rusqlite::Connection> {
    cache::db::open(&state.dirs.db).inspect_err(|e| eprintln!("settings db: {e}")).ok()
}

#[derive(Serialize)]
pub struct SteamAccount {
    name: Option<String>,
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    steam: Option<SteamAccount>,
    epic: Option<Login>,
    epic_games: usize,
    epic_install_dir: Option<String>,
    igdb_client_id: Option<String>,
    /// Whether a secret is saved; its value never leaves Credential Manager.
    igdb_secret_set: bool,
    show_banner: bool,
    ambient: bool,
    app_version: String,
    legendary_version: Option<String>,
    cache_bytes: u64,
    /// `cache_bytes` as shown, like `812 MB`.
    cache_size: String,
}

#[tauri::command]
pub async fn settings_view(app: AppHandle) -> Res<SettingsView> {
    blocking(move || {
        let state = app.state::<AppState>();
        let conn = cache::db::open(&state.dirs.db).map_err(|e| e.to_string())?;
        let saved = settings::load(&conn);
        let (epic, epic_games) = {
            let s = state.lock();
            (s.epic_login, s.games.iter().filter(|g| g.provider == ProviderKind::Epic).count())
        };
        let cache_bytes = dir_size(&state.dirs.thumbs) + dir_size(&state.dirs.art);
        let steam = steamlocate::SteamDir::locate().ok().map(|dir| SteamAccount {
            name: steam::persona_name(dir.path()),
            path: dir.path().to_string_lossy().into_owned(),
        });
        Ok(SettingsView {
            steam,
            epic,
            epic_games,
            epic_install_dir: saved.epic_install_dir.map(|p| p.to_string_lossy().into_owned()),
            igdb_client_id: saved.igdb_client_id,
            igdb_secret_set: settings::secret().is_some(),
            show_banner: saved.show_banner,
            ambient: saved.ambient,
            app_version: app.package_info().version.to_string(),
            legendary_version: epic::legendary_version(),
            cache_bytes,
            cache_size: view::size_label(cache_bytes),
        })
    })
    .await
}

/// Total size of the files under `dir`; a missing folder counts as empty.
fn dir_size(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Sets one plain setting (`None` resets it). The IGDB secret goes through `set_igdb_secret`.
#[tauri::command]
pub async fn update_setting(app: AppHandle, name: String, value: Option<String>) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let conn = cache::db::open(&state.dirs.db).map_err(|e| e.to_string())?;
        settings::update(&conn, &name, value.as_deref())?;
        if name == settings::IGDB_CLIENT_ID {
            state.igdb.lock().expect("token cache lock poisoned").reset();
        }
        Ok(())
    })
    .await
}

/// Saves the IGDB Client Secret in Credential Manager, or removes it for `None`.
#[tauri::command]
pub async fn set_igdb_secret(app: AppHandle, value: Option<String>) -> Res<()> {
    blocking(move || {
        let value = value.as_deref().map(str::trim).filter(|v| !v.is_empty());
        settings::set_secret(value)?;
        // A new secret deserves a fresh token request, even right after a failed one.
        app.state::<AppState>().igdb.lock().expect("token cache lock poisoned").reset();
        Ok(())
    })
    .await
}

/// Deletes downloaded art and thumbnails; they come back on demand. The database stays.
#[tauri::command]
pub async fn clear_cache(app: AppHandle) -> Res<()> {
    blocking(move || {
        let dirs = &app.state::<AppState>().dirs;
        for dir in [&dirs.thumbs, &dirs.art] {
            match std::fs::remove_dir_all(dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("Couldn't clear {}: {e}", dir.display())),
            }
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn open_steam() -> Res<()> {
    crate::library::open_uri("steam://open/main").map_err(|e| e.to_string())
}

/// The page where the owner creates the Twitch application IGDB needs.
#[tauri::command]
pub fn open_twitch_console() -> Res<()> {
    crate::library::open_uri("https://dev.twitch.tv/console/apps").map_err(|e| e.to_string())
}

/// IGDB's site, for the "Data from IGDB" credit next to how long to beat.
#[tauri::command]
pub fn open_igdb() -> Res<()> {
    crate::library::open_uri("https://www.igdb.com").map_err(|e| e.to_string())
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
    fn epic_result_keeps_cache_when_not_logged_in() {
        for login in [Login::LoggedOut, Login::NoLegendary, Login::Unreachable] {
            assert!(epic_result(login, || panic!("must not list")).is_err());
        }
        assert_eq!(epic_result(Login::LoggedIn, || Ok(vec![])).unwrap().len(), 0);
    }

    #[test]
    fn quiet_epic_states_add_no_status_error() {
        let mut epic_game = game("Fortnite", true, None, None);
        epic_game.provider = ProviderKind::Epic;
        for login in [Login::LoggedOut, Login::NoLegendary, Login::Unreachable] {
            let results = vec![(ProviderKind::Epic, epic_result(login, || panic!("must not list")))];
            let merged = merge(&[epic_game.clone()], results);
            assert!(merged.errors.is_empty(), "{login:?}");
            assert_eq!(merged.games, vec![epic_game.clone()]);
            assert!(merged.scanned.is_empty());
        }
    }

    #[test]
    fn logged_out_notice_only_on_change() {
        assert!(logged_out_since_last_run(Some("loggedIn"), Login::LoggedOut));
        assert!(!logged_out_since_last_run(Some("loggedOut"), Login::LoggedOut));
        assert!(!logged_out_since_last_run(None, Login::LoggedOut));
        assert!(!logged_out_since_last_run(Some("loggedIn"), Login::Unreachable));
    }

    #[test]
    fn logging_back_in_clears_the_notice() {
        assert!(epic_notice(false, Some("loggedIn"), Login::LoggedOut));
        assert!(epic_notice(true, Some("loggedOut"), Login::LoggedOut), "stays until dismissed");
        assert!(!epic_notice(true, Some("loggedOut"), Login::LoggedIn));
        assert!(!epic_notice(true, Some("loggedIn"), Login::LoggedIn));
    }

    #[test]
    fn store_url_links_steam_app_page() {
        assert_eq!(store_url(&game("447040", true, None, None)), "https://store.steampowered.com/app/447040");
    }

    #[test]
    fn store_url_searches_epic_store_by_encoded_title() {
        // The Legendary catalog carries no product slug, so Epic gets a store search.
        let mut epic_game = game("Assassin's Creed® Unity & more", false, None, None);
        epic_game.provider = ProviderKind::Epic;
        epic_game.external_id = "Flour".into();
        assert_eq!(
            store_url(&epic_game),
            "https://store.epicgames.com/browse?q=Assassin%27s%20Creed%C2%AE%20Unity%20%26%20more"
        );
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
