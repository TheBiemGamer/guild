//! The library list, scanning and merging the stores, covers, the featured game and the 2 s poll.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use super::{allow, blocking, open_db, AppState, Res, Shared};
use crate::bench;
use crate::cache;
use crate::library::epic::{self, Login};
use crate::library::steam::with_owned;
use crate::library::{Game, Progress, ProviderKind};
use crate::steam_owned::{self, OwnedGame, OwnedOutcome};
use crate::view::{self, Sort};

/// How long a launched Epic game may take to show up as a process before we stop looking.
const EPIC_START_GRACE: Duration = Duration::from_secs(30);

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
        let mut s = state.lock();
        // A scan that already finished wins over the cache.
        if s.games.is_empty() {
            s.games = games;
        }
        drop(s);
        refresh_owned(app.clone(), false);
        Ok(())
    })
    .await
}

/// Rescans Steam, and Epic too when `epic`: Legendary is slow and online, so the UI only asks on demand.
/// `owned` forces a fresh owned-games fetch; without it the 6 h rule applies.
#[tauri::command]
pub async fn refresh(app: AppHandle, epic: bool, owned: bool) {
    let again = app.clone();
    blocking(move || rescan(&app, epic)).await;
    refresh_owned(again, owned);
}

/// Fetches the owned Steam games in the background when there is a saved login and the cached list is
/// older than `OWNED_MAX_AGE_SECS` (or `force`), then rescans. A rejected login raises `steam_attention`
/// and stops further tries until the owner connects again or the app restarts.
pub(crate) fn refresh_owned(app: AppHandle, force: bool) {
    tauri::async_runtime::spawn(async move {
        let job = app.clone();
        let Some(token) = blocking(move || owned_job_token(&job, force)).await else { return };
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

/// The saved token when a fetch is due and none is running; marks one as running.
fn owned_job_token(app: &AppHandle, force: bool) -> Option<String> {
    let state = app.state::<AppState>();
    let token = crate::settings::steam_token()?;
    let mut s = state.lock();
    if s.owned_running || (s.steam_attention && !force) {
        return None;
    }
    if !force && cache::db::open(&state.dirs.db).is_ok_and(|c| steam_owned::fresh(&c, super::unix_now())) {
        return None;
    }
    s.owned_running = true;
    Some(token)
}

fn finish_owned(app: &AppHandle, outcome: OwnedOutcome) {
    let state = app.state::<AppState>();
    match outcome {
        OwnedOutcome::Store(games) => {
            if let Some(conn) = open_db(&state) {
                steam_owned::remember(&conn, &games, super::unix_now());
            }
            {
                let mut s = state.lock();
                s.steam_attention = false;
                s.owned_running = false;
            }
            rescan(app, false);
        }
        OwnedOutcome::KeepAndFlag => {
            let mut s = state.lock();
            s.steam_attention = true;
            s.owned_running = false;
        }
        OwnedOutcome::KeepQuiet => state.lock().owned_running = false,
    }
}

type ScanResults = Vec<(ProviderKind, Result<Vec<Game>, String>)>;

/// Scans, merges and saves. Runs on a blocking thread.
pub(super) fn rescan(app: &AppHandle, with_epic: bool) {
    let state = app.state::<AppState>();
    let owned = open_db(&state).and_then(|c| steam_owned::cached(&c)).map(|(games, _)| games);
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
        return;
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
    drop(s);
    let saved = cache::db::open(&state.dirs.db)
        .and_then(|mut conn| merged.scanned.iter().try_for_each(|&p| cache::db::replace(&mut conn, p, &merged.games)));
    if let Err(e) = saved {
        eprintln!("library cache save: {e}");
    }
}

/// Steam's installed games plus the cached owned list, if there is one.
fn merge_owned(installed: Vec<Game>, owned: Option<&[OwnedGame]>, librarycache: &Path) -> Vec<Game> {
    match owned {
        Some(owned) => with_owned(installed, owned, librarycache),
        None => installed,
    }
}

fn scan(with_epic: bool, owned: Option<&[OwnedGame]>) -> (ScanResults, Option<Login>) {
    let steam = ProviderKind::Steam.provider().list_games().map_err(|e| e.to_string()).map(|installed| {
        let cache = steamlocate::SteamDir::locate().map(|d| d.path().join("appcache").join("librarycache"));
        match cache {
            Ok(cache) => merge_owned(installed, owned, &cache),
            Err(_) => installed,
        }
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
    fn scan_merges_cached_owned_without_network() {
        let installed = vec![game("400", true, None, None)];
        let owned = [OwnedGame { appid: 70, name: "Half-Life".into(), playtime_minutes: 3 }];
        let merged = merge_owned(installed.clone(), Some(&owned), Path::new("cache"));
        assert!(merged.iter().any(|g| g.external_id == "70" && !g.installed));
        // No cached list: installed games only.
        assert_eq!(merge_owned(installed.clone(), None, Path::new("cache")), installed);
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
