//! The library list, scanning and merging the stores, covers, the featured game and the 2 s poll.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use super::{AppState, Process, Res, Shared, allow, blocking, open_db, regroup};
use crate::bench;
use crate::cache;
use crate::entries::{self, Entry};
use crate::epic_playtime;
use crate::library::epic;
use crate::library::steam::{self, owned_games, with_owned};
use crate::library::tool::StoreLogin;
use crate::library::{Game, Progress, ProviderKind};
use crate::library::{amazon, gog};
use crate::msg::Msg;
use crate::settings;
use crate::steam_owned::{self, OwnedGame, OwnedOutcome};
use crate::view::{self, Lists, Sort};

/// How long a launched Epic or added game may take to show up as a process before we stop looking.
const START_GRACE: Duration = Duration::from_secs(30);

#[derive(Serialize, specta::Type)]
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
    /// Summed over the copies; `None` when no store knows.
    playtime_minutes: Option<u64>,
    /// How long to beat ("normally") in shown hours, from the cache; `None` when unknown.
    ttb_hours: Option<u64>,
    /// Any copy is a favorite, hidden, or in the collection (ids).
    favorite: bool,
    hidden: bool,
    collections: Vec<i64>,
}

/// The key of the copy Play and Install use.
fn chosen(e: &Entry, games: &[Game]) -> String {
    games[entries::default_copy(e, games)].key()
}

fn card(e: &Entry, games: &[Game], ttb: &HashMap<String, u64>, lists: &Lists) -> Card {
    Card {
        favorite: lists.favorite(e, games),
        hidden: lists.hidden(e, games),
        collections: lists.collections(e, games),
        key: e.key.clone(),
        chosen: chosen(e, games),
        copy_keys: e.copies.iter().map(|&i| games[i].key()).collect(),
        title: games[e.copies[0]].title.clone(),
        stores: entries::stores(e, games),
        installed: entries::installed(e, games),
        update: games[entries::default_copy(e, games)].update_available,
        playtime_minutes: entries::playtime(e, games).0,
        ttb_hours: view::ttb_secs(e, games, ttb).map(view::ttb_hours),
    }
}

/// A run of `count` cards in `Library::games`; `group` is its id (`view::group`), which the UI names.
#[derive(Serialize, specta::Type)]
pub struct Section {
    group: &'static str,
    count: usize,
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    /// In order; grouped by the sort, group after group.
    games: Vec<Card>,
    /// The groups over `games`, in order; empty for Title without favorites.
    groups: Vec<Section>,
    /// Games in the library, hidden ones left out.
    total: usize,
    /// Shown when nothing is listed: one line each.
    status: Vec<Msg>,
    epic: Option<StoreLogin>,
    /// Epic was set up here and broke (`attention`).
    epic_attention: bool,
    /// Steam rejected the saved login: the settings button carries a dot too.
    steam_attention: bool,
    epic_notice: bool,
    gog: Option<StoreLogin>,
    /// The same for GOG.
    gog_attention: bool,
    amazon: Option<StoreLogin>,
    /// The same for Amazon.
    amazon_attention: bool,
    /// The stores the whole library has a game from, in toolbar order; games the owner added don't count.
    stores: Vec<ProviderKind>,
}

/// `lists` are the Lists filters that are on ("favorites", "hidden", "collection-<id>"); the UI applies
/// them, like its other filters. Here, Hidden brings hidden games back, and favorites go on top unless
/// Favorites is the only one.
#[tauri::command]
#[specta::specta]
pub fn library(
    state: State<'_, AppState>,
    query: String,
    store: Option<ProviderKind>,
    installed_only: bool,
    sort: Sort,
    lists: Vec<String>,
) -> Library {
    let s = state.lock();
    let mut shown = view::arrange(
        &s.games,
        &s.entries,
        &query,
        store,
        installed_only,
        sort,
        &s.ttb,
    );
    view::hide(
        &mut shown,
        &s.entries,
        &s.games,
        &s.lists,
        lists.iter().any(|f| f == "hidden"),
    );
    let mut groups = view::group(
        &shown,
        &s.entries,
        &s.games,
        &s.ttb,
        sort,
        SystemTime::now(),
        &jiff::tz::TimeZone::system(),
    );
    if lists != ["favorites"] {
        groups = view::favorites_first(&shown, groups, &s.entries, &s.games, &s.lists);
    }
    let (games, groups) = sections(&s.games, &s.entries, &s.ttb, &s.lists, shown, groups);
    let [epic_attention, gog_attention, amazon_attention] = attentions(
        [s.epic_login, s.gog_login, s.amazon_login],
        &s.games,
        s.signed_out,
    );
    Library {
        games,
        groups,
        total: s
            .entries
            .iter()
            .filter(|e| !s.lists.hidden(e, &s.games))
            .count(),
        status: s.status.clone(),
        epic: s.epic_login,
        epic_attention,
        steam_attention: s.steam_attention,
        epic_notice: s.epic_notice,
        gog: s.gog_login,
        gog_attention,
        amazon: s.amazon_login,
        amazon_attention,
        stores: stores_in_use(&s.games),
    }
}

/// The stores with at least one game in `games`, in `ProviderKind::ALL` order, without `Custom`.
fn stores_in_use(games: &[Game]) -> Vec<ProviderKind> {
    ProviderKind::ALL
        .into_iter()
        .filter(|&p| p != ProviderKind::Custom && games.iter().any(|g| g.provider == p))
        .collect()
}

/// Whether a store's state raises the Settings dot: only when it was set up here and broke. Unreachable means a login
/// exists that can't be used; logged out, or its tool gone, counts only while its games are cached, so it was used
/// before. A store never set up, or logged out on purpose with Log out (`signed_out`), raises nothing.
fn attention(logged_in: bool, unreachable: bool, has_games: bool, signed_out: bool) -> bool {
    !signed_out && !logged_in && (unreachable || has_games)
}

/// `attention` for Epic, GOG and Amazon, in that order; an unknown state (no online scan yet) raises nothing.
fn attentions(logins: [Option<StoreLogin>; 3], games: &[Game], signed_out: [bool; 3]) -> [bool; 3] {
    let stores = [ProviderKind::Epic, ProviderKind::Gog, ProviderKind::Amazon];
    std::array::from_fn(|i| {
        logins[i].is_some_and(|l| {
            attention(
                l == StoreLogin::LoggedIn,
                l == StoreLogin::Unreachable,
                games.iter().any(|g| g.provider == stores[i]),
                signed_out[i],
            )
        })
    })
}

/// The cards in group order, and the groups over them; without groups (Title), in `shown` order.
fn sections(
    games: &[Game],
    entries: &[Entry],
    ttb: &HashMap<String, u64>,
    lists: &Lists,
    shown: Vec<usize>,
    groups: Vec<(&'static str, Vec<usize>)>,
) -> (Vec<Card>, Vec<Section>) {
    let card = |&i: &usize| card(&entries[i], games, ttb, lists);
    if groups.is_empty() {
        return (shown.iter().map(card).collect(), Vec::new());
    }
    let cards = groups
        .iter()
        .flat_map(|(_, ids)| ids.iter().map(card))
        .collect();
    (
        cards,
        groups
            .into_iter()
            .map(|(group, ids)| Section {
                group,
                count: ids.len(),
            })
            .collect(),
    )
}

/// The toolbar's sort as last saved, the version this start updated to (`settings::take_updated`), and the UI's
/// language (`msg::language`).
#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ViewPrefs {
    sort: Sort,
    updated_to: Option<String>,
    ui_language: &'static str,
}

/// Fills the library from SQLite so the UI renders before the stores are scanned. Answers the saved
/// sort, so the first `library` call already uses it.
#[tauri::command]
#[specta::specta]
pub async fn load_cache(app: AppHandle) -> Res<ViewPrefs> {
    blocking(move || {
        let state = app.state::<AppState>();
        let conn = cache::db::open(&state.dirs.db)?;
        let mut games = cache::db::load(&conn)?;
        view::merge_last_played(&mut games, &cache::db::launches(&conn)?);
        epic_playtime::merge(&conn, &mut games);
        view::sort_by_title(&mut games);
        let twin_lists = crate::gamesdb::all_cached(&conn);
        let ttb = crate::igdb::all_normally(&conn);
        let lists = cache::db::lists(&conn)?;
        let mut s = state.lock();
        s.ttb = ttb;
        s.lists = lists;
        // A scan that already finished wins over the cache.
        if s.games.is_empty() {
            s.games = games;
        }
        s.twin_lists = twin_lists;
        regroup(&mut s);
        drop(s);
        super::game::restore_queue(&app, &conn);
        refresh_owned(app.clone(), false);
        fill_twins(app.clone());
        let saved = settings::load(&conn);
        let updated_to = settings::take_updated(&conn, &app.package_info().version.to_string());
        Ok(ViewPrefs {
            sort: saved.sort,
            updated_to,
            ui_language: crate::msg::language(),
        })
    })
    .await
}

/// Rescans Steam, and Epic, GOG and Amazon too when `epic`: they are slow and online, so the UI only asks on demand.
/// `owned` forces a fresh owned-games fetch; without it the 6 h rule applies. Returns whether the library changed.
#[tauri::command]
#[specta::specta]
pub async fn refresh(app: AppHandle, epic: bool, owned: bool) -> bool {
    let again = app.clone();
    let changed = blocking(move || rescan(&app, epic)).await;
    if epic {
        fill_twins(again.clone());
        refresh_epic_playtime(again.clone());
        refresh_gog_playtime(again.clone(), None);
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

/// Looks up the Steam apps (and a GOG or Amazon game's cover) of every Epic, GOG and Amazon game without a fresh cached answer,
/// one request per `TWIN_GAP`, in the background. Answers are cached; errors are skipped and retried on a later
/// run, and an unreachable GamesDB (offline) ends the run.
/// Only one run at a time. Each answer with Steam apps or a cover updates the library and tells the next poll.
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
            let keys: Vec<String> = state
                .lock()
                .games
                .iter()
                .filter(|g| {
                    matches!(
                        g.provider,
                        ProviderKind::Epic | ProviderKind::Gog | ProviderKind::Amazon
                    )
                })
                .map(Game::key)
                .collect();
            let Some(conn) = open_db(&state) else { return };
            let now = super::unix_now();
            // Cached answers are already in `twin_lists` (load_cache), so only unanswered and stale ones remain.
            keys.into_iter()
                .filter(|key| crate::gamesdb::cached(&conn, key, now).is_none())
                .collect::<Vec<_>>()
        };
        for (i, key) in pending.into_iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(crate::gamesdb::TWIN_GAP).await;
            }
            match crate::gamesdb::look_up(&state.http, &key).await {
                Ok((apps, cover)) => {
                    if let Some(conn) = open_db(&state) {
                        crate::gamesdb::remember(
                            &conn,
                            &key,
                            &apps,
                            cover.as_deref(),
                            super::unix_now(),
                        );
                    }
                    let mut s = state.lock();
                    // A GOG or Amazon game's grid art: until the next scan of its store reads it from the cache.
                    // shortcut: a card whose header was already shown keeps it until the UI asks for its cover again.
                    let covered = s.games.iter_mut().find(|g| {
                        matches!(g.provider, ProviderKind::Gog | ProviderKind::Amazon)
                            && g.key() == key
                    });
                    if let (Some(game), Some(url)) = (covered, cover) {
                        game.art.cover.url = Some(url);
                        s.library_changed = true;
                    }
                    // A refetched stale answer is usually unchanged, and can also drop the Steam apps it had.
                    if s.twin_lists.get(&key).map_or(&[][..], Vec::as_slice) != apps {
                        s.twin_lists.insert(key, apps);
                        regroup(&mut s);
                        // The next poll tells the UI to re-ask the library.
                        s.library_changed = true;
                    }
                }
                Err(e) => {
                    tracing::warn!("{e}");
                    if e.starts_with(crate::gamesdb::UNREACHABLE) {
                        break;
                    }
                }
            }
        }
    });
}

/// Asks Epic's playtime service in the background after an Epic refresh, while logged in. Only one at a
/// time. Quiet: a failure keeps Epic's last answer; the sessions timed here count either way.
fn refresh_epic_playtime(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        {
            let mut s = state.lock();
            if s.playtime_running || s.epic_login != Some(StoreLogin::LoggedIn) {
                return;
            }
            s.playtime_running = true;
        }
        let _running = Running(app.clone(), |s: &mut Shared| &mut s.playtime_running);
        match epic_playtime::fetch(&state.http).await {
            Ok(minutes) => {
                if let Some(conn) = open_db(&state) {
                    epic_playtime::remember(&conn, &minutes, super::unix_now());
                    show_playtime(&state, &conn);
                }
            }
            Err(e) => tracing::warn!("{e}"),
        }
    });
}

/// Asks GOG for its playtime of the GOG games due (no answer in the last day), one request per `gog::PLAYTIME_GAP`, in
/// the background while logged in: after a GOG refresh the games launched here, on opening a game page that copy
/// (`only`). Only one run at a time. Quiet: an unreachable GOG ends the run and keeps the last answers.
pub(super) fn refresh_gog_playtime(app: AppHandle, only: Option<String>) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let opened = only.is_some();
        let keys: Vec<String> = {
            let mut s = state.lock();
            if s.gog_playtime_running || s.gog_login != Some(StoreLogin::LoggedIn) {
                return;
            }
            s.gog_playtime_running = true;
            only.map_or_else(
                || {
                    s.games
                        .iter()
                        .filter(|g| g.provider == ProviderKind::Gog)
                        .map(Game::key)
                        .collect()
                },
                |k| vec![k],
            )
        };
        let _running = Running(app.clone(), |s: &mut Shared| &mut s.gog_playtime_running);
        let job = app.clone();
        let due = blocking(move || {
            let state = job.state::<AppState>();
            let conn = open_db(&state)?;
            let launched = cache::db::launches(&conn).ok()?;
            let now = super::unix_now();
            let due: Vec<String> = keys
                .into_iter()
                .filter(|k| {
                    (opened || launched.contains_key(k)) && gog::playtime_due(&conn, k, now)
                })
                .collect();
            // The login last: it may run gogdl to refresh the token.
            if due.is_empty() {
                None
            } else {
                Some((due, gog::login().1?))
            }
        })
        .await;
        let Some((due, creds)) = due else { return };
        for (i, key) in due.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(gog::PLAYTIME_GAP).await;
            }
            let Some(id) = key.strip_prefix("gog-") else {
                continue;
            };
            match gog::fetch_playtime(&state.http, &creds, id).await {
                Ok(minutes) => {
                    let json = minutes.map_or_else(|| "null".to_string(), |m| m.to_string());
                    if let Some(Err(e)) = open_db(&state).map(|c| {
                        cache::db::put_info(&c, key, gog::PLAYTIME, &json, super::unix_now())
                    }) {
                        tracing::warn!("gog playtime save: {e}");
                    }
                }
                // Reduced to its kind: reqwest's errors carry the URL, which holds the account id.
                Err(_) => {
                    tracing::warn!("gog playtime: GOG unreachable");
                    break;
                }
            }
        }
        if let Some(conn) = open_db(&state) {
            show_playtime(&state, &conn);
        }
    });
}

/// Applies the stored Epic and GOG playtime to the library; the next poll tells the UI when it changed.
fn show_playtime(state: &AppState, conn: &rusqlite::Connection) {
    let mut s = state.lock();
    if epic_playtime::merge(conn, &mut s.games) {
        s.library_changed = true;
    }
}

/// Adds a play session timed here, ending now, to the copy's own playtime.
fn record_session(state: &AppState, key: &str, played: Duration) {
    let seconds = played.as_secs() as i64;
    let Some(conn) = open_db(state) else { return };
    match cache::db::add_session(&conn, key, super::unix_now() - seconds, seconds) {
        Ok(()) => show_playtime(state, &conn),
        Err(e) => tracing::warn!("playtime: {e}"),
    }
}

/// Times the watched game's session up to now when the launcher quits while it runs.
// shortcut: its saves aren't uploaded at exit (network); the next Play asks about the newer local save.
pub(crate) fn end_session_on_exit(app: &AppHandle) {
    let state = app.state::<AppState>();
    let ended = state.lock().watch.take();
    if let Some(w) = ended.filter(|w| w.seen) {
        record_session(&state, &w.key, w.since.elapsed());
    }
}

/// Fetches the owned Steam games in the background when there is a saved login and the cached list is
/// older than `OWNED_MAX_AGE_SECS` (or `force`), then rescans. A rejected login raises `steam_attention`
/// and stops further tries until the owner connects again or the app restarts.
pub(crate) fn refresh_owned(app: AppHandle, force: bool) {
    tauri::async_runtime::spawn(async move {
        let job = app.clone();
        let Some((token, _running)) = blocking(move || owned_job(&job, force)).await else {
            return;
        };
        // One Steam logon at a time, shared with the download-size and DLC lookups.
        let state = app.state::<AppState>();
        let one = state.steam_size.lock().await;
        let result = steam_owned::fetch(&token).await;
        drop(one);
        drop(token);
        if let Err(e) = &result {
            tracing::warn!("steam owned: {}", error_kind(e));
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
    cache::db::setting(&conn, settings::STEAM_ACCOUNT)
        .ok()
        .flatten()?;
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
    cache::db::setting(conn, settings::STEAM_ACCOUNT)
        .ok()
        .flatten()
        .is_some()
}

fn finish_owned(app: &AppHandle, outcome: OwnedOutcome) {
    let state = app.state::<AppState>();
    match outcome {
        OwnedOutcome::Store(games) => {
            let saved = open_db(&state)
                .is_some_and(|conn| remember_if_connected(&conn, &games, super::unix_now()));
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
                let mut s = state.lock();
                s.steam_attention = true;
                // The next poll re-reads the library and an open Settings, so the dot shows.
                s.library_changed = true;
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

type ScanResults = Vec<(ProviderKind, Result<Vec<Game>, ScanError>)>;

/// Why a store has no fresh list. Either way its cached games stay; only `Shown` adds a status line, since a `Quiet`
/// store's login state reports the problem (`Library::*_attention`).
#[derive(Debug)]
enum ScanError {
    Shown(Msg),
    Quiet(Msg),
}

/// The owned games as `Game`s, or `None` when Steam can't be found.
fn build_owned(owned: &[OwnedGame]) -> Option<Vec<Game>> {
    let steam = steamlocate::SteamDir::locate().ok()?;
    Some(owned_games(
        owned,
        &steam.path().join("appcache").join("librarycache"),
    ))
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

/// Scans, merges and saves; Epic, GOG and Amazon only when `online`. Runs on a blocking thread and waits for a rescan
/// already running (`ScanTurns`). Returns whether the library changed since the call.
pub(super) fn rescan(app: &AppHandle, online: bool) -> bool {
    let scans = &app.state::<AppState>().scans;
    scans.run(scans.ask(), online, || rescan_now(app, online))
}

/// Rescans one at a time, so a slow older scan never overwrites a newer result. A rescan asked for while one runs
/// waits, then skips when a scan that started after it asked (and online, if it asked for online) already ran: any
/// number of callers waiting behind one scan make one more.
#[derive(Default)]
pub(super) struct ScanTurns {
    turn: Mutex<()>,
    /// Scans started.
    started: AtomicU64,
    /// `started` as it was after the last online scan started.
    online_started: AtomicU64,
    /// Scans that changed the library.
    changes: AtomicU64,
}

impl ScanTurns {
    /// The scans started and changes made so far, read without waiting.
    fn ask(&self) -> (u64, u64) {
        (
            self.started.load(Ordering::SeqCst),
            self.changes.load(Ordering::SeqCst),
        )
    }

    /// Runs `scan` (which returns whether it changed the library) in turn, unless a scan started after `asked`
    /// covered it. Returns whether the library changed since `asked`.
    fn run(&self, asked: (u64, u64), online: bool, scan: impl FnOnce() -> bool) -> bool {
        let _turn = self.turn.lock().unwrap_or_else(|e| e.into_inner());
        let latest = if online {
            &self.online_started
        } else {
            &self.started
        };
        if latest.load(Ordering::SeqCst) <= asked.0 {
            let started = self.started.fetch_add(1, Ordering::SeqCst) + 1;
            if online {
                self.online_started.store(started, Ordering::SeqCst);
            }
            if scan() {
                self.changes.fetch_add(1, Ordering::SeqCst);
            }
        }
        self.changes.load(Ordering::SeqCst) != asked.1
    }
}

fn rescan_now(app: &AppHandle, online: bool) -> bool {
    let state = app.state::<AppState>();
    let owned = owned(&state);
    let (results, logins) = scan(online, owned.as_deref(), &state.dirs, &state.http);
    let signed_out = logins.map(|(e, g, a)| {
        signed_out(
            &state,
            [
                e == StoreLogin::LoggedIn,
                g == StoreLogin::LoggedIn,
                a == StoreLogin::LoggedIn,
            ],
        )
    });
    let mut s = state.lock();
    if let Some((login, gog_login, amazon_login)) = logins {
        s.epic_login = Some(login);
        s.gog_login = Some(gog_login);
        s.amazon_login = Some(amazon_login);
        s.signed_out = signed_out.unwrap_or_default();
        note_epic_login(&state, &mut s, login);
    }
    let mut merged = merge(&s.games, results);
    match cache::db::open(&state.dirs.db) {
        Ok(conn) => {
            match cache::db::launches(&conn) {
                Ok(launches) => view::merge_last_played(&mut merged.games, &launches),
                Err(e) => tracing::warn!("launch history: {e}"),
            }
            epic_playtime::merge(&conn, &mut merged.games);
        }
        Err(e) => tracing::warn!("launch history: {e}"),
    }
    // Only a finished scan may claim the library is empty.
    s.status = if merged.errors.is_empty() {
        vec![msg!(noGamesFound)]
    } else {
        merged.errors
    };
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
    let saved = cache::db::open(&state.dirs.db).and_then(|mut conn| {
        merged
            .scanned
            .iter()
            .try_for_each(|&p| cache::db::replace(&mut conn, p, &merged.games))
    });
    if let Err(e) = saved {
        tracing::warn!("library cache save: {e}");
    }
    true
}

/// The Epic, GOG and Amazon "logged out on purpose" flags; a store found logged in clears its flag.
fn signed_out(state: &AppState, logged_in: [bool; 3]) -> [bool; 3] {
    let Some(conn) = open_db(state) else {
        return [false; 3];
    };
    let mut flags = [false; 3];
    for ((flag, store), logged_in) in flags
        .iter_mut()
        .zip(["epic", "gog", "amazon"])
        .zip(logged_in)
    {
        *flag = settings::signed_out(&conn, store);
        if *flag && logged_in {
            *flag = false;
            if let Err(e) = settings::set_signed_out(&conn, store, false) {
                tracing::warn!("{store} signed out: {e}");
            }
        }
    }
    flags
}

fn scan(
    online: bool,
    owned: Option<&[Game]>,
    dirs: &cache::Dirs,
    http: &reqwest::Client,
) -> (ScanResults, Option<(StoreLogin, StoreLogin, StoreLogin)>) {
    let steam = steam::list_games()
        .map_err(|e| ScanError::Shown(e.into()))
        .map(|installed| match owned {
            Some(owned) => with_owned(installed, owned),
            None => installed,
        });
    let custom = crate::library::custom::list(dirs).map_err(|e| ScanError::Shown(e.into()));
    let mut results = vec![(ProviderKind::Steam, steam), (ProviderKind::Custom, custom)];
    if !online {
        return (results, None);
    }
    let login = epic::login_state();
    let games = epic_result(login, || epic::list_games().map_err(Msg::from));
    results.push((ProviderKind::Epic, games));
    let (gog_login, creds) = gog::login();
    let games = match creds {
        Some(creds) => gog::list(http, dirs, &creds).map_err(|e| ScanError::Quiet(e.into())),
        None => Err(ScanError::Quiet(Msg::system(format!("{gog_login:?}")))),
    };
    results.push((ProviderKind::Gog, games));
    let games = amazon::list(dirs).map_err(|e| ScanError::Quiet(e.into()));
    // After the list: its sync is what says whether the login still works.
    let amazon_login = amazon::login();
    results.push((ProviderKind::Amazon, games));
    (results, Some((login, gog_login, amazon_login)))
}

/// Only a logged-in Epic is listed. Any other state is a quiet error, so `merge` keeps the cached Epic games.
fn epic_result(
    login: StoreLogin,
    list: impl FnOnce() -> Result<Vec<Game>, Msg>,
) -> Result<Vec<Game>, ScanError> {
    let quiet = |why: &str| Err(ScanError::Quiet(Msg::system(why)));
    match login {
        StoreLogin::LoggedIn => list().map_err(ScanError::Shown),
        StoreLogin::LoggedOut => quiet("not logged in"),
        StoreLogin::NoTool => quiet("Legendary isn't installed"),
        StoreLogin::Unreachable => quiet("Legendary couldn't reach Epic"),
    }
}

/// True only when Epic went from logged in to logged out between two scans.
fn logged_out_since_last_run(previous: Option<&str>, now: StoreLogin) -> bool {
    previous == Some("loggedIn") && now == StoreLogin::LoggedOut
}

const EPIC_LAST_LOGIN: &str = "epic_last_login";

/// Compares with the state stored by the last scan, raises the notice on a fresh logout and stores the new state.
/// `Unreachable` says nothing about the login, so it leaves the stored state alone.
fn note_epic_login(state: &AppState, s: &mut Shared, login: StoreLogin) {
    let name = match login {
        StoreLogin::LoggedIn => "loggedIn",
        StoreLogin::LoggedOut => "loggedOut",
        StoreLogin::NoTool => "noLegendary",
        StoreLogin::Unreachable => return,
    };
    let stored = cache::db::open(&state.dirs.db).and_then(|c| {
        let previous = cache::db::setting(&c, EPIC_LAST_LOGIN)?;
        cache::db::set_setting(&c, EPIC_LAST_LOGIN, Some(name))?;
        Ok(previous)
    });
    match stored {
        Ok(previous) => s.epic_notice = epic_notice(s.epic_notice, previous.as_deref(), login),
        Err(e) => tracing::warn!("epic login state: {e}"),
    }
}

/// Whether the "logged out of Epic" notice shows after a scan found `login`. Logging back in clears it.
fn epic_notice(showing: bool, previous: Option<&str>, login: StoreLogin) -> bool {
    login != StoreLogin::LoggedIn && (showing || logged_out_since_last_run(previous, login))
}

struct Merged {
    games: Vec<Game>,
    scanned: Vec<ProviderKind>,
    errors: Vec<Msg>,
}

/// Stores not scanned, or whose scan failed, keep what we last knew.
fn merge(current: &[Game], results: ScanResults) -> Merged {
    let mut games: Vec<Game> = current
        .iter()
        .filter(|g| results.iter().all(|(p, _)| *p != g.provider))
        .cloned()
        .collect();
    let (mut scanned, mut errors) = (Vec::new(), Vec::new());
    for (provider, result) in results {
        match result {
            Ok(list) => {
                scanned.push(provider);
                games.extend(list);
            }
            Err(e) => {
                let (ScanError::Shown(reason) | ScanError::Quiet(reason)) = &e;
                tracing::warn!("{}: {reason}", provider.name());
                match e {
                    ScanError::Shown(e) if provider == ProviderKind::Custom => {
                        errors.push(msg!(cantReadAddedGames, reason = e))
                    }
                    ScanError::Shown(e) => {
                        errors.push(msg!(cantReadLibrary, store = provider.name(), reason = e))
                    }
                    ScanError::Quiet(_) => {}
                }
                games.extend(current.iter().filter(|g| g.provider == provider).cloned());
            }
        }
    }
    view::sort_by_title(&mut games);
    Merged {
        games,
        scanned,
        errors,
    }
}

/// Shows the window, which starts hidden. The UI calls this once the first screen has its covers;
/// `main` calls it after `SHOW_BY` as a safety net. Only the first call does anything.
#[tauri::command]
#[specta::specta]
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
    println!(
        "window shown after {} ms by {by}",
        state.started.elapsed().as_millis()
    );
    if let Some(window) = app.get_webview_window("main")
        && let Err(e) = window.show()
    {
        tracing::warn!("show window: {e}");
    }
}

/// True for the first call only.
fn first_show(shown: &AtomicBool) -> bool {
    !shown.swap(true, Ordering::SeqCst)
}

#[derive(Serialize, specta::Type)]
pub struct Cover {
    src: String,
    /// Average colour, `#rrggbb`, painted until the image has loaded.
    color: Option<String>,
}

/// Thumbnail file for a card: cover first, landscape header as the fallback, each from its
/// local file or URL. `None` means show a title card. At most `COVER_JOBS` run at once.
// shortcut: requests for cards already scrolled away still run (queued); add cancellation if cold first scrolls feel slow.
#[tauri::command]
#[specta::specta]
pub async fn cover(app: AppHandle, key: String) -> Option<Cover> {
    let state = app.state::<AppState>();
    let _job = state.cover_jobs.acquire().await.ok()?;
    // The cover comes from the first copy, cached under that copy's key.
    let game = state.first_copy(&key)?;
    let key = game.key();
    let art_dir = state.dirs.art.join(&key);
    for (slot, image) in [("cover", &game.art.cover), ("header", &game.art.header)] {
        let Some(src) = cache::resolve(image, &state.http, art_dir.join(slot)).await else {
            continue;
        };
        let (thumbs, thumb_key) = (state.dirs.thumbs.clone(), thumb_key(&game, slot, &src));
        match blocking(move || cache::thumbnail(&src, &thumbs, &thumb_key)).await {
            Ok((path, color)) => return allow(&app, path).map(|src| Cover { src, color }),
            Err(e) => tracing::warn!("thumbnail {key}: {e}"),
        }
    }
    None
}

/// The thumbnail's cache key. An added game's cover can be replaced under the same path, so its key carries
/// the file's modified time; store art never changes under its key.
fn thumb_key(game: &Game, slot: &str, src: &std::path::Path) -> String {
    let modified = || {
        src.metadata()
            .and_then(|m| m.modified())
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
    };
    match modified().filter(|_| game.provider == ProviderKind::Custom) {
        Some(at) => format!("{}-{slot}-{}", game.key(), at.as_millis()),
        None => format!("{}-{slot}", game.key()),
    }
}

#[derive(Serialize, specta::Type)]
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
#[specta::specta]
pub async fn featured(app: AppHandle) -> Option<Featured> {
    let state = app.state::<AppState>();
    let (card, game) = {
        let s = state.lock();
        featured_pick(&s.games, &s.entries, &s.lists)?
    };
    let dir = state.dirs.art.join(game.key());
    let mut hero = None;
    for (slot, image) in [("hero", &game.art.hero), ("cover", &game.art.cover)] {
        if let Some(path) = cache::resolve(image, &state.http, dir.join(slot)).await {
            hero = allow(&app, path);
            break;
        }
    }
    Some(Featured {
        key: card.key,
        chosen: card.chosen,
        copy_keys: card.copy_keys,
        title: card.title,
        hero,
    })
}

/// The featured entry as a card, and its first copy (for the art).
fn featured_pick(games: &[Game], entries: &[Entry], lists: &Lists) -> Option<(Card, Game)> {
    let e = view::featured(games, entries, lists)?;
    Some((
        card(e, games, &HashMap::new(), lists),
        games[e.copies[0]].clone(),
    ))
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Poll {
    /// Key of the game running: Steam's, or the Epic or added game watched since its launch here.
    running: Option<String>,
    /// The running game is a Windows game on Linux whose first launch still sets up (umu fetching the Steam Linux
    /// Runtime or making its prefix); it ends once the game's program runs.
    setting_up: bool,
    progress: HashMap<String, Progress>,
    /// The library changed (a Steam download finished), so the UI should re-ask `library`.
    changed: bool,
    /// The Epic download queue.
    queue: crate::downloads::QueueView,
    /// A resumed download failed; shown like the failure of one started now.
    failed: Option<crate::msg::Msg>,
}

#[tauri::command]
#[specta::specta]
pub async fn poll(app: AppHandle) -> Poll {
    blocking(move || poll_blocking(&app)).await
}

fn poll_blocking(app: &AppHandle) -> Poll {
    let state = app.state::<AppState>();
    let steam_running = steam::running_game().map(|id| ProviderKind::Steam.key(&id));
    // Our own process answers at once; an Epic game's name is looked up after the lock is let go.
    let watch = state.lock().watch.as_mut().map(|w| {
        let running = match &mut w.process {
            Process::Ours(child) => Ok(matches!(child.try_wait(), Ok(None))),
            Process::Named(exe) => Err(exe.clone()),
        };
        (w.key.clone(), running, w.since)
    });
    let watched = watch.and_then(|(key, running, since)| {
        if running.unwrap_or_else(|exe| epic::is_running(&exe)) {
            if let Some(w) = state.lock().watch.as_mut().filter(|w| w.since == since) {
                w.seen = true;
            }
            return Some(key);
        }
        // Gone, or never showed up: stop watching once it had time to start, time the session if it ran,
        // and upload its saves.
        // shortcut: one that never showed up (a bootstrapper exe) has its saves uploaded while it may still run.
        if since.elapsed() > START_GRACE {
            let ended = state.lock().watch.take();
            if let Some(w) = ended {
                if w.seen {
                    record_session(&state, &w.key, w.since.elapsed());
                }
                if let Some(sync) = w.upload {
                    super::game::upload_saves(app, w.key, sync);
                }
            }
        }
        None
    });
    let tracked: Vec<Game> = {
        let s = state.lock();
        s.games
            .iter()
            .filter(|g| s.tracked.contains(&g.key()))
            .cloned()
            .collect()
    };
    let results = tracked
        .iter()
        .map(|g| {
            (
                g.key(),
                (g.provider == ProviderKind::Steam)
                    .then(|| steam::download_progress(g))
                    .flatten(),
                g.installed,
            )
        })
        .collect();
    let finished = apply_steam_progress(&mut state.lock(), results);
    if !finished.is_empty() {
        rescan(app, false);
    }
    let (progress, changed, queue, installed, failed) = {
        let mut s = state.lock();
        let queue = super::queue_view(&s);
        // A download that stopped without installing the game (cancelled in Steam) gets no toast.
        let installed: Vec<String> = finished
            .iter()
            .filter_map(|key| {
                s.games
                    .iter()
                    .find(|g| &g.key() == key && g.installed)
                    .map(|g| g.title.clone())
            })
            .collect();
        (
            s.progress.clone(),
            take_changed(&mut s, !finished.is_empty()),
            queue,
            installed,
            s.failed.take(),
        )
    };
    for title in installed {
        super::notify_done(app, &title, crate::msg::native("readyToPlay"));
    }
    let setting_up =
        steam_running.is_none() && watched.as_deref().is_some_and(crate::platform::setting_up);
    if setting_up && let Some(w) = state.lock().watch.as_mut() {
        // The session starts when the game does, not while umu sets it up.
        w.since = Instant::now();
    }
    Poll {
        setting_up,
        running: steam_running.or(watched),
        progress,
        changed,
        queue,
        failed,
    }
}

/// Records polled Steam progress. Returns the keys of the downloads that were showing progress and just ended.
/// Not-installed games stay tracked: the user may still be in Steam's install dialog.
fn apply_steam_progress(
    s: &mut Shared,
    results: Vec<(String, Option<Progress>, bool)>,
) -> Vec<String> {
    let mut finished = Vec::new();
    for (key, progress, installed) in results {
        match progress {
            Some(p) => _ = s.progress.insert(key, p),
            None => {
                if s.progress.remove(&key).is_some() {
                    finished.push(key.clone());
                }
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
#[specta::specta]
pub fn bench_speed() -> Option<f32> {
    std::env::var("LAUNCHER_BENCH").ok()?.parse().ok()
}

#[tauri::command]
#[specta::specta]
pub fn bench_done(app: AppHandle, mut frame_ms: Vec<f32>, games: usize) {
    println!("{}", bench::stats(&mut frame_ms, games));
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use crate::library::ProgressAction;
    use crate::view::tests::game;

    #[test]
    fn rescans_asked_during_a_scan_make_one_more() {
        use std::cell::Cell;
        let turns = ScanTurns::default();
        let runs = Cell::new(0);
        let mut waiting = Vec::new();
        turns.run(turns.ask(), false, || {
            runs.set(runs.get() + 1);
            waiting.extend([
                (turns.ask(), false),
                (turns.ask(), false),
                (turns.ask(), true),
                (turns.ask(), true),
            ]);
            true
        });
        let changed: Vec<bool> = waiting
            .into_iter()
            .map(|(asked, online)| {
                turns.run(asked, online, || {
                    runs.set(runs.get() + 1);
                    false
                })
            })
            .collect();
        // One offline run covers the second offline ask but not the online ones; one online run covers both.
        assert_eq!(runs.get(), 3);
        // Each waiter hears of the change the scan it waited behind made.
        assert_eq!(changed, [true; 4]);
        // An online scan covers an offline ask made before it started.
        let asked = turns.ask();
        turns.run(turns.ask(), true, || false);
        turns.run(asked, false, || panic!("covered by the online scan"));
        // A new ask after everything finished runs.
        assert!(!turns.run(turns.ask(), false, || {
            runs.set(runs.get() + 1);
            false
        }));
        assert_eq!(runs.get(), 4);
    }

    #[test]
    fn a_fetch_finishing_after_disconnect_is_not_saved() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.sqlite");
        let conn = cache::db::open(&path).unwrap();
        let owned = [OwnedGame {
            appid: 70,
            name: "Half-Life".into(),
            playtime_minutes: 3,
            last_played: None,
        }];
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
        let steam = Game {
            external_id: "635320".into(),
            ..game("Pumpkin Jack", false, None, Some(100))
        };
        let epic = Game {
            provider: ProviderKind::Epic,
            external_id: "Pumpkin".into(),
            ..game("Pumpkin Jack", true, None, Some(50))
        };
        let games = vec![epic, steam];
        let entries = entries::group(
            &games,
            &HashMap::from([("epic-Pumpkin".to_string(), 635320)]),
        );
        let c = card(&entries[0], &games, &HashMap::new(), &Lists::default());
        assert_eq!(c.key, "steam-635320");
        assert_eq!(c.chosen, "epic-Pumpkin");
        assert_eq!(c.copy_keys, ["steam-635320", "epic-Pumpkin"]);
        let (f, _) = featured_pick(&games, &entries, &Lists::default()).unwrap();
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
        let merged = merge(
            &current,
            vec![(ProviderKind::Steam, Ok(vec![fresh.clone()]))],
        );
        assert_eq!(merged.games, vec![epic_game.clone(), fresh.clone()]);
        assert_eq!(merged.scanned, vec![ProviderKind::Steam]);

        // Offline: Epic's scan fails, so its last known games stay and it isn't saved over.
        let results = vec![
            (ProviderKind::Steam, Ok(vec![fresh.clone()])),
            (
                ProviderKind::Epic,
                Err(ScanError::Shown(Msg::system("offline"))),
            ),
        ];
        let merged = merge(&current, results);
        assert_eq!(merged.games, vec![epic_game, fresh]);
        assert_eq!(merged.scanned, vec![ProviderKind::Steam]);
        assert_eq!(merged.errors.len(), 1);
    }

    #[test]
    fn epic_result_keeps_cache_when_not_logged_in() {
        for login in [
            StoreLogin::LoggedOut,
            StoreLogin::NoTool,
            StoreLogin::Unreachable,
        ] {
            assert!(epic_result(login, || panic!("must not list")).is_err());
        }
        assert_eq!(
            epic_result(StoreLogin::LoggedIn, || Ok(vec![]))
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn quiet_epic_states_add_no_status_error() {
        let mut epic_game = game("Fortnite", true, None, None);
        epic_game.provider = ProviderKind::Epic;
        for login in [
            StoreLogin::LoggedOut,
            StoreLogin::NoTool,
            StoreLogin::Unreachable,
        ] {
            let results = vec![(
                ProviderKind::Epic,
                epic_result(login, || panic!("must not list")),
            )];
            let merged = merge(&[epic_game.clone()], results);
            assert!(merged.errors.is_empty(), "{login:?}");
            assert_eq!(merged.games, vec![epic_game.clone()]);
            assert!(merged.scanned.is_empty());
        }
    }

    #[test]
    fn stores_in_use_follow_toolbar_order_without_custom() {
        let copy = |provider| Game {
            provider,
            ..game("A", false, None, None)
        };
        assert_eq!(stores_in_use(&[]), []);
        let games = [
            copy(ProviderKind::Custom),
            copy(ProviderKind::Amazon),
            copy(ProviderKind::Steam),
            copy(ProviderKind::Amazon),
        ];
        assert_eq!(
            stores_in_use(&games),
            [ProviderKind::Steam, ProviderKind::Amazon]
        );
    }

    #[test]
    fn only_a_store_that_was_set_up_and_broke_raises_the_dot() {
        let copy = |provider| Game {
            provider,
            ..game("A", false, None, None)
        };
        let none: [Game; 0] = [];
        let all = [
            copy(ProviderKind::Epic),
            copy(ProviderKind::Gog),
            copy(ProviderKind::Amazon),
        ];
        // Never set up: no tool, or never logged in, with no games cached.
        assert_eq!(
            attentions([Some(StoreLogin::NoTool); 3], &none, [false; 3]),
            [false; 3]
        );
        assert_eq!(
            attentions([Some(StoreLogin::LoggedOut); 3], &none, [false; 3]),
            [false; 3]
        );
        // Used before (games cached), then logged out or the tool went missing.
        assert_eq!(
            attentions([Some(StoreLogin::LoggedOut); 3], &all, [false; 3]),
            [true; 3]
        );
        assert_eq!(
            attentions([Some(StoreLogin::NoTool); 3], &all, [false; 3]),
            [true; 3]
        );
        // A login that can't be checked is always a problem.
        let unreachable = [Some(StoreLogin::Unreachable); 3];
        assert_eq!(attentions(unreachable, &none, [false; 3]), [true; 3]);
        // Logged in, or not known yet: no dot.
        assert_eq!(
            attentions([Some(StoreLogin::LoggedIn); 3], &all, [false; 3]),
            [false; 3]
        );
        assert_eq!(attentions([None; 3], &all, [false; 3]), [false; 3]);
        // Each store counts only its own games.
        let gog_only = [copy(ProviderKind::Gog)];
        assert_eq!(
            attentions([Some(StoreLogin::LoggedOut); 3], &gog_only, [false; 3]),
            [false, true, false]
        );
        // Logged out on purpose: no dot, cached games or not, and an unreachable store isn't expected to answer.
        let logged_out = [Some(StoreLogin::LoggedOut); 3];
        assert_eq!(attentions(logged_out, &all, [true; 3]), [false; 3]);
        assert_eq!(attentions(unreachable, &all, [true; 3]), [false; 3]);
        // Only the store logged out of on purpose stays quiet.
        assert_eq!(
            attentions(logged_out, &all, [false, true, false]),
            [true, false, true]
        );
    }

    #[test]
    fn failed_gog_scans_keep_the_cache_quietly() {
        let mut gog_game = game("Last Day of June", false, None, None);
        gog_game.provider = ProviderKind::Gog;
        let results = vec![(
            ProviderKind::Gog,
            Err(ScanError::Quiet(Msg::system("offline"))),
        )];
        let merged = merge(&[gog_game.clone()], results);
        assert!(merged.errors.is_empty());
        assert_eq!(merged.games, vec![gog_game.clone()]);
        assert!(merged.scanned.is_empty());
        // A list replaces the cached GOG games.
        let merged = merge(&[gog_game], vec![(ProviderKind::Gog, Ok(vec![]))]);
        assert!(merged.games.is_empty());
        assert_eq!(merged.scanned, vec![ProviderKind::Gog]);
    }

    #[test]
    fn failed_or_skipped_amazon_scans_keep_the_cache_quietly() {
        let mut amazon_game = game("Last Day of June", false, None, None);
        amazon_game.provider = ProviderKind::Amazon;
        for e in ["busy", "Unreachable", "not logged in"] {
            let merged = merge(
                &[amazon_game.clone()],
                vec![(ProviderKind::Amazon, Err(ScanError::Quiet(Msg::system(e))))],
            );
            assert!(merged.errors.is_empty());
            assert_eq!(merged.games, vec![amazon_game.clone()]);
            assert!(merged.scanned.is_empty());
        }
        let merged = merge(&[amazon_game], vec![(ProviderKind::Amazon, Ok(vec![]))]);
        assert!(merged.games.is_empty());
        assert_eq!(merged.scanned, vec![ProviderKind::Amazon]);
    }

    #[test]
    fn logged_out_notice_only_on_change() {
        assert!(logged_out_since_last_run(
            Some("loggedIn"),
            StoreLogin::LoggedOut
        ));
        assert!(!logged_out_since_last_run(
            Some("loggedOut"),
            StoreLogin::LoggedOut
        ));
        assert!(!logged_out_since_last_run(None, StoreLogin::LoggedOut));
        assert!(!logged_out_since_last_run(
            Some("loggedIn"),
            StoreLogin::Unreachable
        ));
    }

    #[test]
    fn logging_back_in_clears_the_notice() {
        assert!(epic_notice(false, Some("loggedIn"), StoreLogin::LoggedOut));
        assert!(
            epic_notice(true, Some("loggedOut"), StoreLogin::LoggedOut),
            "stays until dismissed"
        );
        assert!(!epic_notice(true, Some("loggedOut"), StoreLogin::LoggedIn));
        assert!(!epic_notice(true, Some("loggedIn"), StoreLogin::LoggedIn));
    }

    #[test]
    fn background_library_change_is_reported_once() {
        let mut s = Shared {
            library_changed: true,
            ..Shared::default()
        };
        assert!(take_changed(&mut s, false));
        assert!(!take_changed(&mut s, false));
        assert!(take_changed(&mut s, true));
    }

    #[test]
    fn steam_progress_tracks_until_done_and_reports_finish_once() {
        let mut s = Shared {
            tracked: HashSet::from(["dl".to_string(), "ok".to_string()]),
            ..Shared::default()
        };
        let p = Progress {
            fraction: 0.25,
            action: ProgressAction::Download,
            done_bytes: Some(1),
            total_bytes: Some(4),
            secs_left: None,
            done_files: None,
            total_files: None,
        };

        // Nothing started yet: the not-installed game stays tracked, the installed one is dropped.
        assert!(
            apply_steam_progress(
                &mut s,
                vec![("dl".into(), None, false), ("ok".into(), None, true)]
            )
            .is_empty()
        );
        assert_eq!(s.tracked, HashSet::from(["dl".to_string()]));

        assert!(
            apply_steam_progress(&mut s, vec![("dl".into(), Some(p.clone()), false)]).is_empty()
        );
        assert_eq!(s.progress.get("dl"), Some(&p));

        // Download finished: progress cleared and reported once.
        assert_eq!(
            apply_steam_progress(&mut s, vec![("dl".into(), None, false)]),
            ["dl"]
        );
        assert!(s.progress.is_empty());
        assert!(apply_steam_progress(&mut s, vec![("dl".into(), None, false)]).is_empty());
    }

    /// Pumpkin Jack on Steam and Epic (one card, a favorite in a collection), and Last Day of June on GOG.
    fn payload_games() -> (Vec<Game>, Vec<Entry>, Lists) {
        let games = vec![
            Game {
                external_id: "1425609977".into(),
                provider: ProviderKind::Gog,
                ..game("Last Day of June", false, None, None)
            },
            Game {
                external_id: "635320".into(),
                ..game("Pumpkin Jack", true, Some(120), Some(100))
            },
            Game {
                provider: ProviderKind::Epic,
                external_id: "Pumpkin".into(),
                update_available: true,
                ..game("Pumpkin Jack", true, Some(30), Some(50))
            },
        ];
        let entries = entries::group(
            &games,
            &HashMap::from([("epic-Pumpkin".to_string(), 635320)]),
        );
        let lists = Lists {
            favorite: HashSet::from(["epic-Pumpkin".to_string()]),
            collections: HashMap::from([("steam-635320".to_string(), vec![3])]),
            ..Lists::default()
        };
        (games, entries, lists)
    }

    #[test]
    fn library_payload_snapshot() {
        let (games, entries, lists) = payload_games();
        let ttb = HashMap::from([("steam-635320".to_string(), 6 * 3600)]);
        let shown = view::arrange(&games, &entries, "", None, false, Sort::MostPlayed, &ttb);
        let groups = view::group(
            &shown,
            &entries,
            &games,
            &ttb,
            Sort::MostPlayed,
            SystemTime::UNIX_EPOCH,
            &jiff::tz::TimeZone::UTC,
        );
        let groups = view::favorites_first(&shown, groups, &entries, &games, &lists);
        let (cards, groups) = sections(&games, &entries, &ttb, &lists, shown, groups);
        let library = Library {
            games: cards,
            groups,
            total: entries.len(),
            status: vec![msg!(storeError, store = "GOG", reason = msg!(cantReachGog))],
            epic: Some(StoreLogin::LoggedIn),
            epic_attention: false,
            steam_attention: false,
            epic_notice: false,
            gog: Some(StoreLogin::Unreachable),
            gog_attention: true,
            amazon: None,
            amazon_attention: false,
            stores: stores_in_use(&games),
        };
        insta::assert_json_snapshot!(library);
    }

    #[test]
    fn poll_payload_snapshot() {
        let progress = Progress {
            fraction: 0.5,
            action: ProgressAction::Update,
            done_bytes: Some(1 << 30),
            total_bytes: None,
            secs_left: Some(65),
            done_files: None,
            total_files: None,
        };
        let mut queue = crate::downloads::Queue::default();
        queue.push(crate::downloads::Job::new(
            "epic-Pumpkin",
            crate::downloads::QueuedAction::Update,
        ));
        queue.push(crate::downloads::Job::new(
            "gog-1425609977",
            crate::downloads::QueuedAction::Install,
        ));
        let titles = HashMap::from([
            ("epic-Pumpkin", "Pumpkin Jack"),
            ("gog-1425609977", "Last Day of June"),
        ]);
        let poll = Poll {
            running: Some("steam-635320".into()),
            setting_up: false,
            progress: HashMap::from([("epic-Pumpkin".to_string(), progress)]),
            changed: false,
            queue: crate::downloads::view(&queue, |key| titles.get(key).map(|t| t.to_string())),
            failed: None,
        };
        insta::assert_json_snapshot!(poll);
    }
}
