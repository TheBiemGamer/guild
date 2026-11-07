//! The library list, scanning and merging the stores, covers, the featured game and the 2 s poll.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use super::{allow, blocking, open_db, regroup, AppState, Res, Shared};
use crate::bench;
use crate::cache;
use crate::entries::{self, Entry};
use crate::library::epic::{self, Login};
use crate::library::steam::{owned_games, with_owned};
use crate::library::{Game, Progress, ProviderKind};
use crate::settings;
use crate::steam_owned::{self, OwnedGame, OwnedOutcome};
use crate::view::{self, Sort};

/// How long a launched Epic game may take to show up as a process before we stop looking.
const EPIC_START_GRACE: Duration = Duration::from_secs(30);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    key: String,
    /// The copy key Play and Install use (`entries::default_copy`).
    chosen: String,
    /// Every copy's key, for matching running state and install progress.
    copy_keys: Vec<String>,
    title: String,
    stores: Vec<ProviderKind>,
    installed: bool,
    /// The chosen copy has an update to install.
    update: bool,
}

/// The key of the copy Play and Install use.
fn chosen(e: &Entry, games: &[Game]) -> String {
    games[entries::default_copy(e, games)].key()
}

fn card(e: &Entry, games: &[Game]) -> Card {
    Card {
        key: e.key.clone(),
        chosen: chosen(e, games),
        copy_keys: e.copies.iter().map(|&i| games[i].key()).collect(),
        title: games[e.copies[0]].title.clone(),
        stores: entries::stores(e, games),
        installed: entries::installed(e, games),
        update: games[entries::default_copy(e, games)].update_available,
    }
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
    /// Steam rejected the saved login: the settings button carries a dot too.
    steam_attention: bool,
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
    let games = view::arrange(&s.games, &s.entries, &query, store, installed_only, sort)
        .into_iter()
        .map(|i| card(&s.entries[i], &s.games))
        .collect();
    Library {
        games,
        total: s.entries.len(),
        status: s.status.clone(),
        epic: s.epic_login,
        epic_attention: s.epic_login.is_some_and(|l| l != Login::LoggedIn),
        steam_attention: s.steam_attention,
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
        let twins = crate::gamesdb::all_cached(&conn, super::unix_now());
        let mut s = state.lock();
        // A scan that already finished wins over the cache.
        if s.games.is_empty() {
            s.games = games;
        }
        s.twins = twins;
        regroup(&mut s);
        drop(s);
        refresh_owned(app.clone(), false);
        fill_twins(app.clone());
        Ok(())
    })
    .await
}

/// Rescans Steam, and Epic too when `epic`: Legendary is slow and online, so the UI only asks on demand.
/// `owned` forces a fresh owned-games fetch; without it the 6 h rule applies. Returns whether the library changed.
#[tauri::command]
pub async fn refresh(app: AppHandle, epic: bool, owned: bool) -> bool {
    let again = app.clone();
    let changed = blocking(move || rescan(&app, epic)).await;
    if epic {
        fill_twins(again.clone());
    }
    refresh_owned(again, owned);
    changed
}

/// Marks a background job (the flag the second field picks) as running until dropped, even if the
/// task panics or is cancelled. A poisoned lock still gets its flag cleared.
struct Running(AppHandle, fn(&mut Shared) -> &mut bool);

impl Drop for Running {
    fn drop(&mut self) {
        let state = self.0.state::<AppState>();
        let mut s = state.shared.lock().unwrap_or_else(|e| e.into_inner());
        *(self.1)(&mut s) = false;
    }
}

/// Looks up the Steam twin of every Epic game without a fresh cached answer, one request per
/// `TWIN_GAP`, in the background. Answers are cached; errors are skipped and retried on a later run, and
/// an unreachable GamesDB (offline) ends the run.
/// Only one run at a time. Each new twin regroups the library and tells the next poll.
pub(crate) fn fill_twins(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        {
            let mut s = state.lock();
            if s.twins_running {
                return;
            }
            s.twins_running = true;
        }
        let _running = Running(app.clone(), |s: &mut Shared| &mut s.twins_running);
        let pending = {
            let epic: Vec<String> = state
                .lock()
                .games
                .iter()
                .filter(|g| g.provider == ProviderKind::Epic)
                .map(|g| g.external_id.clone())
                .collect();
            let Some(conn) = open_db(&state) else { return };
            let now = super::unix_now();
            // Cached answers are already in `twins` (load_cache), so only the unanswered remain.
            epic.into_iter()
                .filter(|app_name| crate::gamesdb::cached(&conn, &format!("epic-{app_name}"), now).is_none())
                .collect::<Vec<_>>()
        };
        for (i, app_name) in pending.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(crate::gamesdb::TWIN_GAP).await;
            }
            match crate::gamesdb::look_up(&state.http, app_name).await {
                Ok(twin) => {
                    let key = format!("epic-{app_name}");
                    if let Some(conn) = open_db(&state) {
                        crate::gamesdb::remember(&conn, &key, twin, super::unix_now());
                    }
                    if let Some(id) = twin {
                        let mut s = state.lock();
                        s.twins.insert(key, id);
                        regroup(&mut s);
                        // The next poll tells the UI to re-ask the library.
                        s.library_changed = true;
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
                    if e.starts_with(crate::gamesdb::UNREACHABLE) {
                        break;
                    }
                }
            }
        }
    });
}

/// Fetches the owned Steam games in the background when there is a saved login and the cached list is
/// older than `OWNED_MAX_AGE_SECS` (or `force`), then rescans. A rejected login raises `steam_attention`
/// and stops further tries until the owner connects again or the app restarts.
pub(crate) fn refresh_owned(app: AppHandle, force: bool) {
    tauri::async_runtime::spawn(async move {
        let job = app.clone();
        let Some((token, _running)) = blocking(move || owned_job(&job, force)).await else { return };
        let result = steam_owned::fetch(&token).await;
        drop(token);
        if let Err(e) = &result {
            eprintln!("steam owned: {}", error_kind(e));
        }
        let done = app.clone();
        blocking(move || finish_owned(&done, steam_owned::outcome(result))).await;
    });
}

/// For the log: steam-vent's error text describes the failure, never the token.
fn error_kind(e: &steam_owned::OwnedError) -> String {
    match e {
        steam_owned::OwnedError::TokenRejected => "login rejected".into(),
        steam_owned::OwnedError::Network(why) => format!("network: {why}"),
    }
}

/// Non-forced fetches that fail aren't cached, so they wait this long before trying again.
const OWNED_RETRY_SECS: u64 = 15 * 60;

/// Whether a non-forced fetch may start: none was tried in the last `OWNED_RETRY_SECS`.
fn retry_due(last_attempt: Option<Instant>) -> bool {
    last_attempt.is_none_or(|t| t.elapsed().as_secs() >= OWNED_RETRY_SECS)
}

/// The saved token and a running mark when a fetch is due and none is running. The cheap checks come
/// first so Credential Manager is only read when a fetch will happen.
fn owned_job(app: &AppHandle, force: bool) -> Option<(String, Running)> {
    let state = app.state::<AppState>();
    {
        let s = state.lock();
        if s.owned_running || (!force && (s.steam_attention || !retry_due(s.owned_attempt))) {
            return None;
        }
    }
    let conn = open_db(&state)?;
    // Never connected: nothing to fetch, and no reason to touch Credential Manager.
    cache::db::setting(&conn, settings::STEAM_ACCOUNT).ok().flatten()?;
    if !force && steam_owned::fresh(&conn, super::unix_now()) {
        return None;
    }
    {
        let mut s = state.lock();
        if s.owned_running {
            return None;
        }
        s.owned_running = true;
        s.owned_attempt = Some(Instant::now());
    }
    let running = Running(app.clone(), |s: &mut Shared| &mut s.owned_running);
    let token = crate::settings::steam_token()?;
    Some((token, running))
}

/// Saves a fetched list only while an account is still connected: a Disconnect that ran during the
/// fetch must not be undone. Returns whether it was saved.
fn remember_if_connected(conn: &rusqlite::Connection, games: &[OwnedGame], now: i64) -> bool {
    let connected = steam_connected(conn);
    if connected {
        steam_owned::remember(conn, games, now);
    }
    connected
}

fn steam_connected(conn: &rusqlite::Connection) -> bool {
    cache::db::setting(conn, settings::STEAM_ACCOUNT).ok().flatten().is_some()
}

fn finish_owned(app: &AppHandle, outcome: OwnedOutcome) {
    let state = app.state::<AppState>();
    match outcome {
        OwnedOutcome::Store(games) => {
            let saved = open_db(&state).is_some_and(|conn| remember_if_connected(&conn, &games, super::unix_now()));
            if !saved {
                return;
            }
            // Built before the lock: it reads the Steam library cache folder.
            let built = build_owned(&games);
            {
                let mut s = state.lock();
                s.steam_attention = false;
                s.owned = built;
            }
            rescan(app, false);
            // The next poll tells the UI to re-ask the library.
            state.lock().library_changed = true;
        }
        // A Disconnect during the fetch must not raise the dot again.
        OwnedOutcome::KeepAndFlag => {
            let connected = open_db(&state).is_some_and(|c| steam_connected(&c));
            if connected {
                state.lock().steam_attention = true;
            }
        }
        OwnedOutcome::KeepQuiet => {}
    }
}

/// Whether the next poll reports a change: a finished download, or a library update made in the background.
/// Reading the flag clears it.
fn take_changed(s: &mut Shared, finished: bool) -> bool {
    std::mem::take(&mut s.library_changed) | finished
}

type ScanResults = Vec<(ProviderKind, Result<Vec<Game>, String>)>;

/// The owned games as `Game`s, or `None` when Steam can't be found.
fn build_owned(owned: &[OwnedGame]) -> Option<Vec<Game>> {
    let steam = steamlocate::SteamDir::locate().ok()?;
    Some(owned_games(owned, &steam.path().join("appcache").join("librarycache")))
}

/// The built owned games; built from the cached list the first time.
fn owned(state: &AppState) -> Option<Vec<Game>> {
    if let Some(games) = &state.lock().owned {
        return Some(games.clone());
    }
    let (list, _) = open_db(state).and_then(|c| steam_owned::cached(&c))?;
    let games = build_owned(&list)?;
    // A Disconnect during the build already forgot the list: don't bring it back.
    if open_db(state).is_some_and(|c| steam_connected(&c)) {
        state.lock().owned = Some(games.clone());
    }
    Some(games)
}

/// Scans, merges and saves. Runs on a blocking thread. Returns whether the library changed.
pub(super) fn rescan(app: &AppHandle, with_epic: bool) -> bool {
    let state = app.state::<AppState>();
    let owned = owned(&state);
    let (results, login) = scan(with_epic, owned.as_deref());
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
        return false;
    }
    let unfinished: Vec<String> = merged
        .games
        .iter()
        // Owned-only games have no manifest to poll.
        .filter(|g| g.provider == ProviderKind::Steam && !g.installed && g.install_dir.is_some())
        .map(Game::key)
        .collect();
    s.tracked.extend(unfinished);
    s.games = merged.games.clone();
    regroup(&mut s);
    drop(s);
    let saved = cache::db::open(&state.dirs.db)
        .and_then(|mut conn| merged.scanned.iter().try_for_each(|&p| cache::db::replace(&mut conn, p, &merged.games)));
    if let Err(e) = saved {
        eprintln!("library cache save: {e}");
    }
    true
}

fn scan(with_epic: bool, owned: Option<&[Game]>) -> (ScanResults, Option<Login>) {
    let steam = ProviderKind::Steam.provider().list_games().map_err(|e| e.to_string()).map(|installed| match owned {
        Some(owned) => with_owned(installed, owned),
        None => installed,
    });
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

/// Shows the window, which starts hidden. The UI calls this once the first screen has its covers;
/// `main` calls it after `SHOW_BY` as a safety net. Only the first call does anything.
#[tauri::command]
pub fn ready(app: AppHandle) {
    show_window(&app, "ui");
}

/// The window is shown by then even if the UI never says it's ready.
pub const SHOW_BY: Duration = Duration::from_secs(2);

pub fn show_window(app: &AppHandle, by: &str) {
    let state = app.state::<AppState>();
    if !first_show(&state.shown) {
        return;
    }
    // Read by the benchmark; a GUI build has no stdout, so it goes nowhere otherwise.
    println!("window shown after {} ms by {by}", state.started.elapsed().as_millis());
    if let Some(window) = app.get_webview_window("main")
        && let Err(e) = window.show()
    {
        eprintln!("show window: {e}");
    }
}

/// True for the first call only.
fn first_show(shown: &AtomicBool) -> bool {
    !shown.swap(true, Ordering::SeqCst)
}

#[derive(Serialize)]
pub struct Cover {
    src: String,
    /// Average colour, `#rrggbb`, painted until the image has loaded.
    color: Option<String>,
}

/// Thumbnail file for a card: cover first, landscape header as the fallback, each from its
/// local file or URL. `None` means show a title card. At most `COVER_JOBS` run at once.
// shortcut: requests for cards already scrolled away still run (queued); add cancellation if cold first scrolls feel slow.
#[tauri::command]
pub async fn cover(app: AppHandle, key: String) -> Option<Cover> {
    let state = app.state::<AppState>();
    let _job = state.cover_jobs.acquire().await.ok()?;
    // The cover comes from the first copy, cached under that copy's key.
    let game = state.entry(&key)?.1.swap_remove(0);
    let key = game.key();
    let art_dir = state.dirs.art.join(&key);
    for (slot, image) in [("cover", &game.art.cover), ("header", &game.art.header)] {
        let Some(src) = cache::resolve(image, &state.http, art_dir.join(slot)).await else { continue };
        let (thumbs, thumb_key) = (state.dirs.thumbs.clone(), format!("{key}-{slot}"));
        match blocking(move || cache::thumbnail(&src, &thumbs, &thumb_key)).await {
            Ok((path, color)) => return allow(&app, path).map(|src| Cover { src, color }),
            Err(e) => eprintln!("thumbnail {key}: {e}"),
        }
    }
    None
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Featured {
    key: String,
    /// The copy key Play uses.
    chosen: String,
    copy_keys: Vec<String>,
    title: String,
    hero: Option<String>,
}

/// The game played last, with its hero art (cover as the fallback) for the library backdrop.
#[tauri::command]
pub async fn featured(app: AppHandle) -> Option<Featured> {
    let state = app.state::<AppState>();
    let (card, game) = {
        let s = state.lock();
        featured_pick(&s.games, &s.entries)?
    };
    let dir = state.dirs.art.join(game.key());
    let mut hero = None;
    for (slot, image) in [("hero", &game.art.hero), ("cover", &game.art.cover)] {
        if let Some(path) = cache::resolve(image, &state.http, dir.join(slot)).await {
            hero = allow(&app, path);
            break;
        }
    }
    Some(Featured { key: card.key, chosen: card.chosen, copy_keys: card.copy_keys, title: card.title, hero })
}

/// The featured entry as a card, and its first copy (for the art).
fn featured_pick(games: &[Game], entries: &[Entry]) -> Option<(Card, Game)> {
    let e = view::featured(games, entries)?;
    Some((card(e, games), games[e.copies[0]].clone()))
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
    let (progress, changed) = {
        let mut s = state.lock();
        (s.progress.clone(), take_changed(&mut s, finished))
    };
    Poll { running: steam_running.or(epic_running), progress, changed }
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
    use std::collections::HashSet;

    use crate::view::tests::game;

    #[test]
    fn a_fetch_finishing_after_disconnect_is_not_saved() {
        let path = std::env::temp_dir().join(format!("launcher-owned-test-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = cache::db::open(&path).unwrap();
        let owned = [OwnedGame { appid: 70, name: "Half-Life".into(), playtime_minutes: 3, last_played: None }];
        assert!(!steam_connected(&conn));
        assert!(!remember_if_connected(&conn, &owned, 100));
        assert!(steam_owned::cached(&conn).is_none());
        cache::db::set_setting(&conn, settings::STEAM_ACCOUNT, Some("noah")).unwrap();
        assert!(remember_if_connected(&conn, &owned, 100));
        assert!(steam_connected(&conn));
        assert!(steam_owned::cached(&conn).is_some());
    }

    #[test]
    fn card_and_featured_play_the_only_installed_copy() {
        let steam = Game { external_id: "635320".into(), ..game("Pumpkin Jack", false, None, Some(100)) };
        let epic = Game {
            provider: ProviderKind::Epic,
            external_id: "Pumpkin".into(),
            ..game("Pumpkin Jack", true, None, Some(50))
        };
        let games = vec![epic, steam];
        let entries = entries::group(&games, &HashMap::from([("epic-Pumpkin".to_string(), 635320)]));
        let c = card(&entries[0], &games);
        assert_eq!(c.key, "steam-635320");
        assert_eq!(c.chosen, "epic-Pumpkin");
        assert_eq!(c.copy_keys, ["steam-635320", "epic-Pumpkin"]);
        let (f, _) = featured_pick(&games, &entries).unwrap();
        assert_eq!(f.key, "steam-635320");
        assert_eq!(f.chosen, "epic-Pumpkin");
    }

    #[test]
    fn ready_is_idempotent() {
        let shown = AtomicBool::new(false);
        assert!(first_show(&shown));
        assert!(!first_show(&shown));
        assert!(!first_show(&shown));
    }

    #[test]
    fn failed_owned_fetches_wait_before_retrying() {
        assert!(retry_due(None));
        assert!(!retry_due(Some(Instant::now())));
    }

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
    fn background_library_change_is_reported_once() {
        let mut s = Shared { library_changed: true, ..Shared::default() };
        assert!(take_changed(&mut s, false));
        assert!(!take_changed(&mut s, false));
        assert!(take_changed(&mut s, true));
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
