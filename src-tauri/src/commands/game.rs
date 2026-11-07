//! One game: its page, art, details, play/install/uninstall, download size, how long to beat.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, UNIX_EPOCH};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use super::library::rescan;
use super::{allow, blocking, open_db, queue_view, unix_now, AppState, Res, Shared, Watch};
use crate::cache;
use crate::downloads::{Job, Queue, QueueView, QueuedAction, DOWNLOAD_QUEUE};
use crate::entries::{self, Entry};
use crate::gamesdb;
use crate::igdb;
use crate::info::{self, GameInfo};
use crate::library::epic::{self, InstallEvent, LaunchOptions, SaveSync, SyncOutcome};
use crate::library::{Game, Image, ProviderKind};
use crate::links::{self, LinkKind};
use crate::settings;
use crate::steam_size;
use crate::view;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    /// The entry key.
    key: String,
    title: String,
    stores: Vec<ProviderKind>,
    /// Any copy.
    installed: bool,
    copies: Vec<CopyView>,
    /// The copy key Play and Install use.
    chosen: String,
    /// Playtime tile minutes, 0 being never played; `None` hides the tile (an Epic copy with no known playtime).
    playtime_minutes: Option<u64>,
    /// Both copies contributed to the playtime.
    playtime_both: bool,
    /// Unix seconds; `None` when never played.
    last_played: Option<u64>,
    /// The store played on last, only for an entry with several copies.
    last_played_store: Option<ProviderKind>,
    /// Store pages, then info sites when the Steam app id is known.
    links: Vec<LinkKind>,
    /// The Epic copies' DLC; a Steam copy's comes from `steam_dlc`.
    dlc: Vec<DlcView>,
    /// Any copy is a favorite, hidden, or in the collection (ids).
    favorite: bool,
    hidden: bool,
    collections: Vec<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyView {
    key: String,
    provider: ProviderKind,
    installed: bool,
    /// An installed Epic copy has a newer version.
    update_available: bool,
    /// Size on disk of an installed copy.
    size_bytes: Option<u64>,
    /// Only for an installed copy (Steam knows a folder for games it hasn't installed yet).
    install_dir: Option<String>,
    /// An installed Epic copy that keeps its saves in Epic's cloud.
    cloud_saves: bool,
}

#[tauri::command]
pub fn game(state: State<'_, AppState>, key: String) -> Option<Detail> {
    let s = state.lock();
    let entry = entries::find(&s.entries, &s.games, &key)?;
    Some(detail(entry, &s.games, &s.twins, &s.lists))
}

fn detail(entry: &Entry, games: &[Game], twins: &HashMap<String, u32>, lists: &view::Lists) -> Detail {
    let (minutes, both) = entries::playtime(entry, games);
    let chosen = &games[entries::default_copy(entry, games)];
    // Steam's zero is a fact, so a Steam copy decides the tile; Epic alone shows only known playtime.
    let tile_provider = entry
        .copies
        .iter()
        .map(|&i| games[i].provider)
        .find(|&p| p == ProviderKind::Steam)
        .unwrap_or(chosen.provider);
    let last = entries::last_played(entry, games);
    let stores = entries::stores(entry, games);
    Detail {
        key: entry.key.clone(),
        title: games[entry.copies[0]].title.clone(),
        links: links::kinds(&stores, entries::steam_appid(entry, games, twins)),
        stores,
        installed: entries::installed(entry, games),
        copies: entry
            .copies
            .iter()
            .map(|&i| {
                let g = &games[i];
                CopyView {
                    key: g.key(),
                    provider: g.provider,
                    installed: g.installed,
                    update_available: g.installed && g.update_available,
                    size_bytes: g.size_bytes.filter(|_| g.installed),
                    install_dir: installed_dir(g).map(|p| p.to_string_lossy().into_owned()),
                    cloud_saves: g.installed && g.cloud_saves,
                }
            })
            .collect(),
        chosen: chosen.key(),
        playtime_minutes: view::playtime_tile(tile_provider, minutes),
        playtime_both: both,
        last_played: last.map(|(t, _)| t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())),
        last_played_store: last.filter(|_| entry.copies.len() > 1).map(|(_, p)| p),
        dlc: epic_dlc(&entry.copies.iter().map(|&i| &games[i]).collect::<Vec<_>>()),
        favorite: lists.favorite(entry, games),
        hidden: lists.hidden(entry, games),
        collections: lists.collections(entry, games),
    }
}

fn installed_dir(g: &Game) -> Option<&PathBuf> {
    g.install_dir.as_ref().filter(|_| g.installed)
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

/// Opens one of the game page's links in the default browser. The URL is built here from the kind.
#[tauri::command]
pub async fn open_link(app: AppHandle, key: String, kind: LinkKind) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (appid, title, epic_key) = {
            let s = state.lock();
            let entry = entries::find(&s.entries, &s.games, &key).ok_or("This game is no longer in the library.")?;
            let epic = entry.copies.iter().map(|&i| &s.games[i]).find(|g| g.provider == ProviderKind::Epic);
            (entries::steam_appid(entry, &s.games, &s.twins), s.games[entry.copies[0]].title.clone(), epic.map(|g| g.key()))
        };
        let epic_url = epic_key.and_then(|k| igdb::epic_url(&open_db(&state)?, &k));
        let url = links::url(kind, appid, epic_url.as_deref(), &title).ok_or("This game has no page there.")?;
        crate::library::open_url(&url).map_err(|e| e.to_string())
    })
    .await
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
    /// Epic only: start an out-of-date game without updating it first.
    PlayOutdated,
    Install,
    /// Epic only; Steam updates games itself.
    Update,
    Uninstall,
    /// Epic verifies and repairs here; Steam validates in its own window.
    Verify,
    /// Epic moves the folder to `dest`; Steam opens its storage manager.
    Move,
}

/// The answer to a cloud save conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Keep {
    Cloud,
    Local,
}

/// Why an Epic Play didn't launch yet: the question to ask first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Conflict {
    LocalNewer,
    CloudNewer,
}

/// Steam requests return at once (Steam shows its own dialogs). Epic requests return when
/// Legendary is done; installs report progress through `poll` meanwhile, under the game's key.
/// `with_dlc` (Epic install, default on) installs the owned DLC too. `dlc` installs or uninstalls
/// one DLC of the Epic copy `key` instead of the game. `dest` is the folder an Epic Move goes to.
/// An Epic Play that finds a cloud save conflict returns it instead of launching; Play again with `keep`.
#[tauri::command]
pub async fn act(
    app: AppHandle,
    action: Action,
    key: String,
    with_dlc: Option<bool>,
    dlc: Option<String>,
    keep: Option<Keep>,
    dest: Option<String>,
) -> Res<Option<Conflict>> {
    let game = app.state::<AppState>().game(&key).ok_or("This game is no longer in the library.")?;
    if let Some(id) = &dlc
        && (matches!(action, Action::Play | Action::PlayOutdated) || !game.dlc.iter().any(|d| &d.id == id))
    {
        return Err("This DLC is no longer in the library.".into());
    }
    // A DLC install is just that DLC.
    let with_dlc = if dlc.is_some() { None } else { Some(with_dlc.unwrap_or(true)) };
    blocking(move || act_blocking(&app, action, &game, with_dlc, dlc.as_deref(), keep, dest.as_deref())).await
}

fn act_blocking(
    app: &AppHandle,
    action: Action,
    game: &Game,
    with_dlc: Option<bool>,
    dlc: Option<&str>,
    keep: Option<Keep>,
    dest: Option<&str>,
) -> Res<Option<Conflict>> {
    let state = app.state::<AppState>();
    let key = game.key();
    let store = game.provider.provider();
    if game.provider == ProviderKind::Steam {
        if action == Action::Install {
            state.lock().tracked.insert(key.clone());
        }
        let result = match action {
            Action::Play | Action::PlayOutdated => store.launch(game).inspect(|()| record_play(&state, &key)),
            Action::Install => store.install(game),
            Action::Update => Err("Steam updates its games itself.".into()),
            Action::Uninstall => store.uninstall(game),
            Action::Verify => crate::library::open_uri(&format!("steam://validate/{}", game.external_id)),
            Action::Move => crate::library::open_uri("steam://open/storage"),
        };
        return result.map(|()| None).map_err(|e| format!("Steam: {e}"));
    }
    if game.provider == ProviderKind::Custom {
        return act_custom(&state, action, game).map(|()| None);
    }
    let queued = match action {
        Action::Install => Some(QueuedAction::Install),
        Action::Update => Some(QueuedAction::Update),
        Action::Verify => Some(QueuedAction::Verify),
        _ => None,
    };
    if let Some(queued) = queued {
        let dest = (queued == QueuedAction::Install)
            .then(|| open_db(&state).and_then(|c| settings::load(&c).epic_install_dir))
            .flatten()
            .map(|p| p.to_string_lossy().into_owned());
        let job = Job { dlc: dlc.map(str::to_string), with_dlc, dest, ..Job::new(&key, queued) };
        return enqueue(app, job).map(|()| None);
    }
    if matches!(action, Action::Move | Action::Uninstall) && state.lock().queue.contains(&key) {
        return Err("This game is in the download queue. Cancel its download first.".into());
    }
    let result = match action {
        Action::Play | Action::PlayOutdated => {
            let upload = match saves_before_play(&state, game, keep) {
                Ok(upload) => upload,
                Err(conflict) => return Ok(Some(conflict)),
            };
            let options = checked_launch_options(open_db(&state).as_ref(), game)?;
            epic::launch(&game.external_id, &options, action == Action::PlayOutdated).map(|()| {
                record_play(&state, &key);
                // The game started either way; without its exe name it just isn't shown as running.
                // shortcut: unwatched, its saves aren't uploaded either; the next Play asks about the newer local save.
                let alternative = options.exe.as_deref().and_then(|e| std::path::Path::new(e).file_name());
                let exe = match alternative {
                    Some(name) => Some(name.to_string_lossy().into_owned()),
                    None => epic::executable(game).unwrap_or_else(|e| {
                        log!("legendary list-installed {key}: {e}");
                        None
                    }),
                };
                state.lock().watch = exe.map(|exe| Watch { key: key.clone(), exe, since: Instant::now(), seen: false, upload });
            })
        }
        // Not cancellable: stopped halfway, Legendary's record and the folder disagree.
        Action::Move => {
            let on_event = |event| _ = record_event(&mut state.lock(), &key, event);
            let result = match dest {
                Some(dest) => epic::move_to(&game.external_id, game.install_dir.as_deref(), dest, on_event),
                None => Err("Pick a folder to move the game to.".into()),
            };
            let mut s = state.lock();
            s.install_pids.remove(&key);
            s.progress.remove(&key);
            result
        }
        Action::Uninstall => epic::uninstall(dlc.unwrap_or(&game.external_id)),
        Action::Install | Action::Update | Action::Verify => unreachable!("queued above"),
    };
    if !matches!(action, Action::Play | Action::PlayOutdated) {
        rescan(app, true);
    }
    result.map(|()| None).map_err(|e| format!("Epic Games: {e}"))
}

/// A game the owner added only plays: its program starts, watched like an Epic game to time the session.
fn act_custom(state: &AppState, action: Action, game: &Game) -> Res<()> {
    let store = game.provider.provider();
    let result = match action {
        Action::Play | Action::PlayOutdated => {
            let conn = open_db(state).ok_or("The settings database can't be opened.")?;
            let row = cache::db::custom_game(&conn, &game.external_id)
                .map_err(|e| e.to_string())?
                .ok_or("This game is no longer in the library.")?;
            let key = game.key();
            crate::library::custom::launch(&row).map(|exe| {
                record_play(state, &key);
                state.lock().watch = Some(Watch { key, exe, since: Instant::now(), seen: false, upload: None });
            })
        }
        Action::Install => store.install(game),
        Action::Uninstall => store.uninstall(game),
        Action::Update | Action::Verify | Action::Move => Err("Games you added can only be played.".into()),
    };
    result.map_err(|e| e.to_string())
}

/// Records a Legendary job's pid or progress under its copy key. Returns the pid of a queued job that
/// must stop at once: it was cancelled or the queue paused before its process started.
fn record_event(s: &mut Shared, key: &str, event: InstallEvent) -> Option<u32> {
    match event {
        InstallEvent::Started(pid) => {
            s.install_pids.insert(key.to_string(), pid);
            let stop = s.queue.is_head(key) && s.queue_running && (s.cancelled.contains(key) || s.queue.paused);
            s.interrupted |= stop && !s.cancelled.contains(key);
            stop.then_some(pid)
        }
        InstallEvent::Progress(p) => {
            s.progress.insert(key.to_string(), p);
            None
        }
    }
}

fn save_queue(state: &AppState, queue: &Queue) {
    let saved = cache::db::open(&state.dirs.db).and_then(|c| cache::db::set_setting(&c, DOWNLOAD_QUEUE, Some(&queue.to_json())));
    if let Err(e) = saved {
        log!("download queue: {e}");
    }
}

/// Gives the `act` waiting for `key` its outcome; a job nobody waits for (restored at start) only logs a failure.
fn report(s: &mut Shared, key: &str, outcome: Res<()>) {
    match s.waiters.remove(key) {
        Some(waiter) => _ = waiter.send(outcome),
        None => {
            if let Err(e) = outcome {
                log!("download {key}: {e}");
            }
        }
    }
}

/// Queues an Epic job and waits until it leaves the queue: done, failed, cancelled or removed.
fn enqueue(app: &AppHandle, job: Job) -> Res<()> {
    let state = app.state::<AppState>();
    let (tx, rx) = std::sync::mpsc::channel();
    {
        let mut s = state.lock();
        let key = job.key.clone();
        if !s.queue.push(job) {
            return Ok(());
        }
        s.waiters.insert(key, tx);
        save_queue(&state, &s.queue);
    }
    start_queue(app);
    // A waiter replaced by a later one for the same game gets no answer; the queue shows the rest.
    rx.recv().unwrap_or(Ok(()))
}

/// Reads the stored queue once, after the cached library is loaded, without the jobs whose games are gone,
/// and starts its head unless it was paused.
pub(super) fn restore_queue(app: &AppHandle, conn: &Connection) {
    let state = app.state::<AppState>();
    {
        let mut s = state.lock();
        if std::mem::replace(&mut s.queue_loaded, true) {
            return;
        }
        let stored = cache::db::setting(conn, DOWNLOAD_QUEUE).ok().flatten();
        let keys: std::collections::HashSet<String> = s.games.iter().map(Game::key).collect();
        let restored = Queue::restore(stored.as_deref(), |k| keys.contains(k));
        // A job queued before this (none normally) keeps its place ahead of the stored ones.
        s.queue.paused |= restored.paused;
        for job in restored.jobs {
            s.queue.push(job);
        }
        save_queue(&state, &s.queue);
    }
    start_queue(app);
}

/// Works through the queue on a blocking thread unless a runner already is, the queue is paused or empty.
fn start_queue(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut s = state.lock();
        if s.queue_running || s.queue.next().is_none() {
            return;
        }
        s.queue_running = true;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || run_queue(&app));
}

/// Runs the head job, then the next, until the queue is empty or paused. A failed job leaves the queue;
/// one stopped by Pause stays at the head.
fn run_queue(app: &AppHandle) {
    let state = app.state::<AppState>();
    loop {
        let job = {
            let mut s = state.lock();
            match s.queue.next().cloned() {
                Some(job) => job,
                None => {
                    s.queue_running = false;
                    return;
                }
            }
        };
        let key = job.key.clone();
        let result = match state.game(&key) {
            Some(game) => run_job(&state, &game, &job),
            None => Err("This game is no longer in the library.".into()),
        };
        let paused = {
            let mut s = state.lock();
            s.install_pids.remove(&key);
            let paused = std::mem::take(&mut s.interrupted) && result.is_err() && !s.cancelled.contains(&key);
            if paused {
                save_queue(&state, &s.queue);
            }
            paused
        };
        // The progress stays for "Paused · 42%".
        if paused {
            continue;
        }
        rescan(app, true);
        let mut s = state.lock();
        s.progress.remove(&key);
        // A cancelled job fails by design; don't report it.
        let outcome = if s.cancelled.remove(&key) { Ok(()) } else { result.map_err(|e| format!("Epic Games: {e}")) };
        s.queue.finish(&key);
        save_queue(&state, &s.queue);
        report(&mut s, &key, outcome);
        s.library_changed = true;
    }
}

fn run_job(state: &AppState, game: &Game, job: &Job) -> Result<(), crate::library::Error> {
    let on_event = |event| {
        let stop = record_event(&mut state.lock(), &job.key, event);
        if let Some(pid) = stop
            && let Err(e) = epic::cancel(pid)
        {
            log!("stop download {}: {e}", job.key);
        }
    };
    match job.action {
        QueuedAction::Update => epic::update(&game.external_id, on_event),
        QueuedAction::Verify => epic::verify(&game.external_id, on_event),
        QueuedAction::Install => epic::install(
            job.dlc.as_deref().unwrap_or(&game.external_id),
            job.with_dlc,
            job.dest.as_deref().map(std::path::Path::new),
            on_event,
        ),
    }
}

/// `game_prefs` name for an Epic copy's launch options, as `LaunchOptions` JSON; none is all defaults.
const LAUNCH_OPTIONS: &str = "launch_options";

fn launch_options_of(conn: &Connection, key: &str) -> LaunchOptions {
    let json = cache::db::pref(conn, key, LAUNCH_OPTIONS).ok().flatten();
    json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}

fn store_launch_options(conn: &Connection, key: &str, options: &LaunchOptions) -> Res<()> {
    let json = (*options != LaunchOptions::default()).then(|| serde_json::to_string(options).unwrap_or_default());
    cache::db::set_pref(conn, key, LAUNCH_OPTIONS, json.as_deref()).map_err(|e| e.to_string())
}

/// An installed Epic copy's launch options, with the alternative executable checked again: the folder
/// may have changed since it was saved.
fn checked_launch_options(conn: Option<&Connection>, game: &Game) -> Res<LaunchOptions> {
    let mut options = conn.map(|c| launch_options_of(c, &game.key())).unwrap_or_default();
    if let Some(exe) = &options.exe {
        let dir = installed_dir(game).ok_or("This game has no install folder.")?;
        options.exe = Some(epic::override_exe(dir, exe).map_err(|e| format!("Alternative executable: {e}"))?);
    }
    Ok(options)
}

/// The Launch options tab of an installed Epic copy; `None` for others (Steam keeps its own).
#[tauri::command]
pub async fn launch_options(app: AppHandle, key: String) -> Option<LaunchOptions> {
    app.state::<AppState>().game(&key).filter(|g| g.installed && g.provider == ProviderKind::Epic)?;
    blocking(move || Some(launch_options_of(&open_db(&app.state::<AppState>())?, &key))).await
}

/// Saves an installed Epic copy's launch options. `exe` may be absolute (from the file picker); it is
/// stored relative to the install folder, and only when it is a file inside it. Returns what was stored.
#[tauri::command]
pub async fn set_launch_options(app: AppHandle, key: String, mut options: LaunchOptions) -> Res<LaunchOptions> {
    let game = app
        .state::<AppState>()
        .game(&key)
        .filter(|g| g.installed && g.provider == ProviderKind::Epic)
        .ok_or("This game isn't installed from Epic.")?;
    blocking(move || {
        if let Some(exe) = &options.exe {
            let dir = installed_dir(&game).ok_or("This game has no install folder.")?;
            options.exe = Some(epic::override_exe(dir, exe).map_err(|e| e.to_string())?);
        }
        let conn = open_db(&app.state::<AppState>()).ok_or("The settings database can't be opened.")?;
        store_launch_options(&conn, &key, &options)?;
        Ok(options)
    })
    .await
}

/// Opens a Steam copy's properties in Steam, where its launch options live.
#[tauri::command]
pub fn open_steam_properties(state: State<'_, AppState>, key: String) -> Res<()> {
    let game = state.game(&key).filter(|g| g.provider == ProviderKind::Steam).ok_or("This game isn't from Steam.")?;
    crate::library::open_uri(&format!("steam://gameproperties/{}", game.external_id)).map_err(|e| e.to_string())
}

/// `game_prefs` name for an Epic copy's own cloud save choice, `on` or `off`; none follows the setting.
const SAVES_CHOICE: &str = "cloud_saves";
/// Info cache kind for an Epic copy's last cloud save sync, as a `SyncRecord`.
const SAVES_SYNC: &str = "cloud-saves-sync";
/// Info rows are read with a maximum age; these never go stale.
const FOREVER: i64 = i64::MAX / 2;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct SyncRecord {
    /// Unix seconds.
    at: i64,
    /// What went wrong; nothing synced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// After the game, the cloud save was newer, so the local one wasn't uploaded. The next Play asks first.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    cloud_newer: bool,
}

fn saves_choice(conn: &Connection, key: &str) -> Option<bool> {
    match cache::db::pref(conn, key, SAVES_CHOICE).ok().flatten()?.as_str() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

fn last_sync(conn: &Connection, key: &str) -> Option<SyncRecord> {
    let json = cache::db::cached_info(conn, key, SAVES_SYNC, FOREVER, unix_now()).ok().flatten()?;
    serde_json::from_str(&json).ok()
}

/// What a sync leaves for the Saves tab. A conflict found before launch leaves nothing: nothing ran.
fn sync_record(outcome: &SyncOutcome, at: i64) -> Option<SyncRecord> {
    Some(match outcome {
        SyncOutcome::Synced => SyncRecord { at, error: None, cloud_newer: false },
        SyncOutcome::CloudNewer => SyncRecord { at, error: None, cloud_newer: true },
        SyncOutcome::Failed(e) => SyncRecord { at, error: Some(e.clone()), cloud_newer: false },
        SyncOutcome::LocalNewer => return None,
    })
}

fn record_sync(conn: Option<&Connection>, key: &str, outcome: &SyncOutcome) {
    if let SyncOutcome::Failed(e) = outcome {
        log!("legendary sync-saves {key}: {e}");
    }
    let at = unix_now();
    let (Some(conn), Some(mut record)) = (conn, sync_record(outcome, at)) else { return };
    // A failure changed nothing, so a newer cloud save left by the last upload still waits to be asked about.
    record.cloud_newer |= record.error.is_some() && last_sync(conn, key).is_some_and(|r| r.cloud_newer);
    let json = serde_json::to_string(&record).unwrap_or_default();
    if let Err(e) = cache::db::put_info(conn, key, SAVES_SYNC, &json, at) {
        log!("record sync {key}: {e}");
    }
}

/// Whether to download before launch, and how. `Err` is the conflict to ask about instead.
/// `cloud_newer`: the last upload found the cloud save newer, which no plain download may overwrite.
fn pre_launch_sync(keep: Option<Keep>, cloud_newer: bool) -> Result<Option<SaveSync>, Conflict> {
    match keep {
        Some(Keep::Local) => Ok(None),
        Some(Keep::Cloud) => Ok(Some(SaveSync::KeepCloud)),
        None if cloud_newer => Err(Conflict::CloudNewer),
        None => Ok(Some(SaveSync::Download)),
    }
}

/// Syncs an Epic game's cloud saves before it launches, when it has them and syncing is on for it.
/// Returns how to upload after the game exits (`None`: no syncing), or the conflict to ask about instead
/// of launching. A failed sync is remembered and never stops the launch.
fn saves_before_play(state: &AppState, game: &Game, keep: Option<Keep>) -> Result<Option<SaveSync>, Conflict> {
    let key = game.key();
    let conn = open_db(state);
    let enabled = conn.as_ref().is_none_or(|c| saves_choice(c, &key).unwrap_or_else(|| settings::load(c).cloud_saves));
    if !game.cloud_saves || !enabled {
        return Ok(None);
    }
    let cloud_newer = conn.as_ref().and_then(|c| last_sync(c, &key)).is_some_and(|r| r.cloud_newer);
    let Some(download) = pre_launch_sync(keep, cloud_newer)? else {
        // Keep local: the upload afterwards sends this PC's save whatever the cloud holds.
        return Ok(Some(SaveSync::KeepLocal));
    };
    let outcome = epic::sync_saves(&game.external_id, download);
    if outcome == SyncOutcome::LocalNewer {
        return Err(Conflict::LocalNewer);
    }
    record_sync(conn.as_ref(), &key, &outcome);
    Ok(Some(SaveSync::Upload))
}

/// Uploads a watched Epic game's saves once it has exited, off the poll.
pub(super) fn upload_saves(app: &AppHandle, key: String, sync: SaveSync) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(app_name) = key.strip_prefix("epic-") else { return };
        let outcome = epic::sync_saves(app_name, sync);
        record_sync(open_db(&app.state::<AppState>()).as_ref(), &key, &outcome);
    });
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavesView {
    /// The game's own choice; `None` follows the setting.
    choice: Option<bool>,
    /// Unix seconds of the last sync that worked; `None` when never synced or `error` is set.
    synced_at: Option<i64>,
    /// What went wrong with the last sync; the Overview repeats it.
    error: Option<String>,
}

fn saves_view(choice: Option<bool>, last: Option<SyncRecord>) -> SavesView {
    let (synced_at, error) = match last {
        None => (None, None),
        Some(SyncRecord { error: Some(e), .. }) => (None, Some(format!("Cloud saves didn't sync: {e}"))),
        Some(SyncRecord { cloud_newer: true, .. }) => {
            (None, Some("The cloud save is newer, so this PC's save wasn't uploaded.".to_string()))
        }
        Some(r) => (Some(r.at), None),
    };
    SavesView { choice, synced_at, error }
}

/// The Saves tab of an Epic copy (`key`); `None` for a copy without cloud saves.
#[tauri::command]
pub async fn saves(app: AppHandle, key: String) -> Option<SavesView> {
    app.state::<AppState>().game(&key).filter(|g| g.installed && g.cloud_saves)?;
    blocking(move || {
        let conn = open_db(&app.state::<AppState>())?;
        Some(saves_view(saves_choice(&conn, &key), last_sync(&conn, &key)))
    })
    .await
}

/// Sets an Epic copy's own cloud save choice; `None` follows the setting again.
#[tauri::command]
pub async fn set_saves(app: AppHandle, key: String, choice: Option<bool>) -> Res<()> {
    app.state::<AppState>().game(&key).filter(|g| g.cloud_saves).ok_or("This game has no cloud saves.")?;
    blocking(move || {
        let conn = open_db(&app.state::<AppState>()).ok_or("The settings database can't be opened.")?;
        let value = choice.map(|on| if on { "on" } else { "off" });
        cache::db::set_pref(&conn, &key, SAVES_CHOICE, value).map_err(|e| e.to_string())
    })
    .await
}

/// Sync now: the newer side wins. The outcome shows in the Saves tab, so this fails only before Legendary runs.
#[tauri::command]
pub async fn sync_saves_now(app: AppHandle, key: String) -> Res<()> {
    let game = app
        .state::<AppState>()
        .game(&key)
        .filter(|g| g.installed && g.cloud_saves)
        .ok_or("This game has no cloud saves.")?;
    blocking(move || {
        let outcome = epic::sync_saves(&game.external_id, SaveSync::Both);
        record_sync(open_db(&app.state::<AppState>()).as_ref(), &key, &outcome);
        Ok(())
    })
    .await
}

/// How long an Epic download size stays good; sizes change only with game updates.
const SIZE_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;
const INFO_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;

/// Download size of a game that isn't installed, like `27.3 GB`, for the install popup and game page.
/// Epic asks Legendary and Steam asks its servers (slow, seconds), so the bytes are cached for a week.
/// None if unknown, or for Steam when no account is connected.
#[tauri::command]
pub async fn download_size(app: AppHandle, key: String) -> Option<u64> {
    let game = app
        .state::<AppState>()
        .game(&key)
        .filter(|g| !g.installed)?;
    let steam_id = match game.provider {
        ProviderKind::Epic => None,
        ProviderKind::Steam => Some(game.external_id.parse::<u32>().ok()?),
        ProviderKind::Custom => return None,
    };
    let cached = blocking({
        let (app, key) = (app.clone(), key.clone());
        move || {
            let conn = open_db(&app.state::<AppState>());
            let fresh = conn.as_ref().and_then(|c| cached_size(c, &key));
            let token = match (&fresh, steam_id, conn.as_ref()) {
                (None, Some(_), Some(c)) => steam_token(c),
                _ => None,
            };
            (fresh, token)
        }
    })
    .await;
    let bytes = match (cached, steam_id) {
        ((Some(bytes), _), _) => bytes,
        ((None, token), Some(appid)) => {
            let token = token?;
            // One Steam logon at a time; Steam rate-limits them and the owned-games fetch logs on too.
            let state = app.state::<AppState>();
            let _one = state.steam_size.lock().await;
            // A caller that queued on the lock may find the size cached by the one before it.
            let again = blocking({
                let (app, key) = (app.clone(), key.clone());
                move || open_db(&app.state::<AppState>()).and_then(|c| cached_size(&c, &key))
            })
            .await;
            match again {
                Some(bytes) => bytes,
                None => {
                    let vdf = steam_size::fetch_appinfo(&token, appid)
                        .await
                        .inspect_err(|e| log!("steam size {key}: {e}"))
                        .ok()?;
                    let bytes = steam_size::parse_download_size(&vdf)?.to_string();
                    let app = app.clone();
                    blocking(move || {
                        if let Some(c) = open_db(&app.state::<AppState>()) {
                            _ = cache::db::put_info(&c, &key, "size", &bytes, unix_now());
                        }
                        bytes
                    })
                    .await
                }
            }
        }
        _ => {
            blocking(move || {
                let conn = open_db(&app.state::<AppState>());
                info::cached_or_fetch(
                    conn.as_ref(),
                    &key,
                    "size",
                    SIZE_MAX_AGE_SECS,
                    unix_now(),
                    || {
                        let info = epic::info_json(&game)
                            .inspect_err(|e| log!("legendary info {key}: {e}"))
                            .ok()?;
                        epic::download_size_from_info(&info).map(|size| size.to_string())
                    },
                )
            })
            .await?
        }
    };
    bytes.parse().ok()
}

/// The cached download size in bytes, if fresh.
fn cached_size(conn: &Connection, key: &str) -> Option<String> {
    cache::db::cached_info(conn, key, "size", SIZE_MAX_AGE_SECS, unix_now()).ok().flatten()
}

/// The saved Steam token. Never connected: no reason to touch Credential Manager.
fn steam_token(conn: &Connection) -> Option<String> {
    cache::db::setting(conn, settings::STEAM_ACCOUNT).ok().flatten().and_then(|_| settings::steam_token())
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DlcView {
    id: String,
    title: String,
    installed: bool,
    /// The Epic copy's key, for Install and Uninstall.
    copy: Option<String>,
}

/// Info cache kind for a Steam copy's owned DLC, as JSON `[[appid, name], ...]`; kept a week.
const DLC: &str = "dlc";
/// Info cache kind marking a failed Steam DLC lookup, so a failing game isn't logged on for again soon.
const DLC_FAILED: &str = "dlc-failed";
const DLC_RETRY_SECS: i64 = 6 * 60 * 60;

fn epic_dlc(copies: &[&Game]) -> Vec<DlcView> {
    copies
        .iter()
        .flat_map(|g| {
            g.dlc.iter().map(|d| DlcView {
                id: d.id.clone(),
                title: d.title.clone(),
                installed: d.installed,
                copy: Some(g.key()),
            })
        })
        .collect()
}

#[derive(Serialize)]
pub struct SteamDlc {
    id: String,
    title: String,
}

/// The entry's Steam copy's owned DLC for the game page. Asked from Steam, which logs on, so only
/// with a connected account and when the store page lists DLC; cached a week, a failure for 6 h.
/// Failures are quiet and give an empty list.
#[tauri::command]
pub async fn steam_dlc(app: AppHandle, key: String) -> Vec<SteamDlc> {
    let steam_key = {
        let state = app.state::<AppState>();
        let s = state.lock();
        entries::find(&s.entries, &s.games, &key)
            .and_then(|e| e.copies.iter().map(|&i| &s.games[i]).find(|g| g.provider == ProviderKind::Steam).map(Game::key))
    };
    let Some(steam_key) = steam_key else { return Vec::new() };
    steam_owned_dlc(&app, steam_key).await.into_iter().map(|(id, title)| SteamDlc { id: id.to_string(), title }).collect()
}

#[derive(Debug, PartialEq)]
enum DlcStep {
    Answer(Vec<(u32, String)>),
    LogOn,
}

/// Whether the Steam DLC lookup can answer without a logon: from the cached list, after a recent
/// failure (nothing), or when the store page lists no DLC (`has_dlc`, `None` when unknown).
fn dlc_step(cached: Option<&str>, failed_recently: bool, has_dlc: Option<bool>) -> DlcStep {
    if let Some(list) = cached.and_then(|json| serde_json::from_str(json).ok()) {
        return DlcStep::Answer(list);
    }
    if failed_recently || has_dlc == Some(false) {
        return DlcStep::Answer(Vec::new());
    }
    DlcStep::LogOn
}

/// The cached list and whether a lookup failed recently.
fn cached_dlc(conn: &Connection, key: &str, now: i64) -> (Option<String>, bool) {
    let cached = |kind, max_age| cache::db::cached_info(conn, key, kind, max_age, now).ok().flatten();
    (cached(DLC, INFO_MAX_AGE_SECS), cached(DLC_FAILED, DLC_RETRY_SECS).is_some())
}

/// A Steam copy's owned DLC as (app id, name). Answers from the cache when it can; otherwise logs on,
/// one logon at a time, and remembers the answer or the failure.
async fn steam_owned_dlc(app: &AppHandle, key: String) -> Vec<(u32, String)> {
    let Some(appid) = key.strip_prefix("steam-").and_then(|id| id.parse::<u32>().ok()) else { return Vec::new() };
    // Everything that can answer is checked before the lock, so this never waits behind another logon for nothing.
    let (step, token) = blocking({
        let (app, key) = (app.clone(), key.clone());
        move || {
            let state = app.state::<AppState>();
            let Some(conn) = open_db(&state) else { return (DlcStep::Answer(Vec::new()), None) };
            let now = unix_now();
            let (cached, failed) = cached_dlc(&conn, &key, now);
            let has_dlc = (cached.is_none() && !failed)
                .then(|| parse_details(steam_details(Some(&conn), &state.http, &appid.to_string(), now)))
                .flatten()
                .and_then(|i| i.has_dlc);
            let step = dlc_step(cached.as_deref(), failed, has_dlc);
            let token = (step == DlcStep::LogOn).then(|| steam_token(&conn)).flatten();
            (step, token)
        }
    })
    .await;
    if let DlcStep::Answer(list) = step {
        return list;
    }
    let Some(token) = token else { return Vec::new() };
    let state = app.state::<AppState>();
    let _one = state.steam_size.lock().await;
    // A caller that queued on the lock may find its answer cached by the one before it.
    let again = blocking({
        let (app, key) = (app.clone(), key.clone());
        move || {
            let Some(conn) = open_db(&app.state::<AppState>()) else { return DlcStep::Answer(Vec::new()) };
            let (cached, failed) = cached_dlc(&conn, &key, unix_now());
            dlc_step(cached.as_deref(), failed, None)
        }
    })
    .await;
    if let DlcStep::Answer(list) = again {
        return list;
    }
    let result = steam_size::fetch_owned_dlc(&token, appid).await;
    let (kind, json) = match &result {
        Ok(list) => (DLC, serde_json::to_string(list).unwrap_or_else(|_| "[]".into())),
        Err(e) => {
            log!("steam dlc {key}: {e}");
            (DLC_FAILED, "null".into())
        }
    };
    let app = app.clone();
    blocking(move || {
        if let Some(c) = open_db(&app.state::<AppState>()) {
            _ = cache::db::put_info(&c, &key, kind, &json, unix_now());
        }
    })
    .await;
    result.unwrap_or_default()
}

/// Steam `appdetails` JSON for an app, cached as `steam-<appid>` (the same entry a Steam copy uses).
fn steam_details(conn: Option<&Connection>, http: &reqwest::Client, appid: &str, now: i64) -> Option<String> {
    let key = format!("steam-{appid}");
    info::cached_or_fetch(conn, &key, info::DETAILS, INFO_MAX_AGE_SECS, now, || {
        let url = format!("https://store.steampowered.com/api/appdetails?appids={appid}&l=english");
        let body = tauri::async_runtime::block_on(async { http.get(url).send().await?.error_for_status()?.text().await })
            .inspect_err(|e| log!("appdetails {key}: {e}"))
            .ok()?;
        serde_json::to_string(&info::parse_steam_appdetails(&body, appid)?).ok()
    })
}

/// Cached details JSON as a game's info; `null`, a miss or bad JSON is `None`.
fn parse_details(json: Option<String>) -> Option<GameInfo> {
    serde_json::from_str::<Option<GameInfo>>(&json?).ok()?
}

/// Store details for the game page (description, genres, developer, screenshots).
/// Fetched once a week per game; a failed fetch is not cached, so the next visit retries.
/// An Epic game starts from Legendary's catalog (cached as `null` when it says nothing) and has its
/// gaps filled from its Steam twin's page, else from GOG GamesDB (30 days, 7 for a miss).
/// Screenshots are remote URLs; `screenshots` turns them into local files.
#[tauri::command]
pub async fn game_info(app: AppHandle, key: String) -> Option<GameInfo> {
    // A game the owner added has no store to ask.
    let game = app.state::<AppState>().game(&key).filter(|g| g.provider != ProviderKind::Custom)?;
    let twin = {
        let state = app.state::<AppState>();
        let s = state.lock();
        entries::find(&s.entries, &s.games, &key).and_then(|e| entries::steam_appid(e, &s.games, &s.twins))
    };
    let details = {
        let (app, key, game) = (app.clone(), key.clone(), game.clone());
        blocking(move || {
            let state = app.state::<AppState>();
            let conn = open_db(&state);
            let now = unix_now();
            let http = state.http.clone();
            if game.provider == ProviderKind::Steam {
                return parse_details(steam_details(conn.as_ref(), &http, &game.external_id, now));
            }
            // One list answers for every Epic game, so all of them are cached from it.
            let catalog = info::cached_or_fetch(conn.as_ref(), &key, info::DETAILS, INFO_MAX_AGE_SECS, now, || {
                let list = epic::catalog().inspect_err(|e| log!("legendary list {key}: {e}")).ok()?;
                let all = info::epic_details(&list);
                if let Some(c) = &conn
                    && let Err(e) = info::remember_all(c, &all, now)
                {
                    log!("epic details cache: {e}");
                }
                Some(all.into_iter().find(|(k, _)| *k == key).map_or_else(|| "null".into(), |(_, json)| json))
            });
            let base = parse_details(catalog).unwrap_or_default();
            let gamesdb = || {
                let json = info::gamesdb_cached_or_fetch(conn.as_ref(), &key, now, || {
                    let body = tauri::async_runtime::block_on(gamesdb::fetch(&http, &game.external_id))
                        .inspect_err(|e| log!("gamesdb details {key}: {e}"))
                        .ok()?;
                    serde_json::to_string(&body.and_then(|b| info::parse_gamesdb_info(&b))).ok()
                });
                parse_details(json)
            };
            let extra = match twin {
                // A twin whose store page is gone still has GamesDB.
                Some(id) => parse_details(steam_details(conn.as_ref(), &http, &id.to_string(), now)).or_else(gamesdb),
                None => gamesdb(),
            };
            let all = extra.map_or(base.clone(), |extra| info::fill_missing(base, extra));
            (all != GameInfo::default()).then_some(all)
        })
        .await?
    };
    Some(details)
}

/// Local files for `game_info`'s screenshot URLs, downloaded on first use; the ones that fail are left out.
/// Separate from `game_info` so the page's text never waits for images.
#[tauri::command]
pub async fn screenshots(app: AppHandle, key: String, urls: Vec<String>) -> Vec<String> {
    let state = app.state::<AppState>();
    // The key names the art folder, so it must be a real game.
    if state.game(&key).is_none() {
        return Vec::new();
    }
    let dir = state.dirs.art.join(&key);
    // Downloaded side by side; awaited in order, so the screenshots keep theirs.
    let downloads: Vec<_> = urls
        .into_iter()
        .filter(|u| u.starts_with("https://"))
        .take(info::MAX_SCREENSHOTS)
        .enumerate()
        .map(|(n, url)| {
            let (http, dest) = (state.http.clone(), dir.join(format!("shot{n}")));
            let image = Image { path: None, url: Some(url) };
            tauri::async_runtime::spawn(async move { cache::resolve(&image, &http, dest).await })
        })
        .collect();
    let mut shots = Vec::new();
    for download in downloads {
        if let Ok(Some(path)) = download.await {
            shots.extend(allow(&app, path));
        }
    }
    shots
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
/// Seconds; `None` when unknown.
pub struct TimeToBeatView {
    normally: Option<u64>,
    hastily: Option<u64>,
    completely: Option<u64>,
}

fn time_to_beat_view(t: igdb::TimeToBeat) -> Option<TimeToBeatView> {
    let known = |secs: Option<u64>| secs.filter(|&s| s > 0);
    let shown = TimeToBeatView { normally: known(t.normally), hastily: known(t.hastily), completely: known(t.completely) };
    (shown.normally.is_some() || shown.hastily.is_some() || shown.completely.is_some()).then_some(shown)
}

/// A Steam app id matches by id; otherwise an Epic copy matches by exact title. The Epic page URL
/// IGDB reports (either way) is remembered for the copy.
async fn look_up_time_to_beat(
    app: &AppHandle,
    state: &AppState,
    client_id: &str,
    secret: &str,
    appid: Option<u32>,
    title: &str,
    epic_key: Option<&str>,
) -> Res<Option<igdb::TimeToBeat>> {
    let token = igdb::cached_token(&state.igdb, &state.http, client_id, secret).await?;
    let mut id = None;
    let mut url = None;
    if let Some(appid) = appid {
        id = igdb::game_id_for_steam(&state.http, &token, client_id, appid).await?;
        // Found by Steam id: the Epic page comes from the game's Epic rows. Without it the time still shows.
        if let (Some(game), Some(_)) = (id, epic_key) {
            url = igdb::epic_url_for_game(&state.http, &token, client_id, game)
                .await
                .inspect_err(|e| log!("igdb epic url: {e}"))
                .ok()
                .flatten();
        }
    }
    if let (None, Some(_)) = (id, epic_key)
        && let Some(m) = igdb::epic_by_title(&state.http, &token, client_id, title).await?
    {
        id = Some(m.game);
        url = m.url;
    }
    if let (Some(url), Some(epic_key)) = (url, epic_key) {
        let epic_key = epic_key.to_string();
        let app = app.clone();
        blocking(move || {
            if let Some(conn) = open_db(&app.state::<AppState>()) {
                igdb::remember_epic_url(&conn, &epic_key, &url);
            }
        })
        .await;
    }
    let Some(id) = id else { return Ok(None) };
    igdb::time_to_beat(&state.http, &token, client_id, id).await
}

/// How long to beat, from IGDB, matched by Steam app ID (an Epic-only game by exact title, and
/// only when IGDB has one game for it). `None` too without saved credentials or when IGDB doesn't know the game.
/// Cached 30 days when found, 7 when not; a failed request is not cached.
// Attribution: IGDB asks for visible credit where its data is shown; the game page puts a small
// "Data from IGDB" link (`open_igdb`) under the tile.
#[tauri::command]
pub async fn time_to_beat(app: AppHandle, key: String) -> Option<TimeToBeatView> {
    let state = app.state::<AppState>();
    let (appid, title, epic_key) = {
        let s = state.lock();
        let entry = entries::find(&s.entries, &s.games, &key)?;
        let epic = entry.copies.iter().map(|&i| &s.games[i]).find(|g| g.provider == ProviderKind::Epic);
        (
            entries::steam_appid(entry, &s.games, &s.twins),
            epic.map_or_else(String::new, |g| g.title.clone()),
            epic.map(|g| g.key()),
        )
    };
    // Nothing to match on: a game the owner added.
    if appid.is_none() && epic_key.is_none() {
        return None;
    }
    let now = unix_now();
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
    let answer = match look_up_time_to_beat(&app, &state, &client_id, &secret, appid, &title, epic_key.as_deref()).await {
        Ok(answer) => answer,
        Err(e) => {
            log!("how long to beat {key}: {e}");
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
    {
        let normally = answer.and_then(|t| t.normally).filter(|&s| s > 0);
        let mut s = state.lock();
        let before = match normally {
            Some(secs) => s.ttb.insert(key, secs),
            None => s.ttb.remove(&key),
        };
        // The library sorts, groups and badges on it: the next poll re-asks.
        if before != normally {
            s.library_changed = true;
        }
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
    igdb::game_id_for_steam(&state.http, &token, &client_id, 447040).await?;
    Ok(())
}

/// Remembers a launch from here, for every store, so last played survives rescans.
fn record_play(state: &AppState, key: &str) {
    let secs = unix_now();
    if let Some(g) = state.lock().games.iter_mut().find(|g| g.key() == key) {
        g.last_played = Some(UNIX_EPOCH + Duration::from_secs(secs as u64));
    }
    let saved = cache::db::open(&state.dirs.db).and_then(|c| cache::db::record_launch(&c, key, secs));
    if let Err(e) = saved {
        log!("record launch: {e}");
    }
}

/// Cancels a game's queued Epic job: the active one is stopped (Legendary resumes it next time) and
/// leaves the queue when its process ends; a waiting or paused one leaves the queue at once.
/// Returns the queue as it is now.
#[tauri::command]
pub async fn cancel_install(app: AppHandle, key: String) -> Res<QueueView> {
    let pid = {
        let state = app.state::<AppState>();
        let mut s = state.lock();
        if !s.queue.contains(&key) {
            return Err("This game isn't in the download queue.".into());
        }
        if s.queue.is_head(&key) && s.queue_running {
            s.cancelled.insert(key.clone());
            // None: its process hasn't started yet, and is stopped when it does.
            s.install_pids.get(&key).copied()
        } else {
            s.queue.remove(&key);
            s.progress.remove(&key);
            save_queue(&state, &s.queue);
            report(&mut s, &key, Ok(()));
            None
        }
    };
    start_queue(&app);
    if let Some(pid) = pid {
        blocking(move || epic::cancel(pid).map_err(|e| format!("Couldn't cancel the install: {e}"))).await?;
    }
    Ok(queue_view(&app.state::<AppState>().lock()))
}

/// Pauses the whole queue: the active job's Legendary is stopped (it resumes partial downloads) and the
/// job stays at the head.
#[tauri::command]
pub async fn pause_downloads(app: AppHandle) -> Res<QueueView> {
    let pid = {
        let state = app.state::<AppState>();
        let mut s = state.lock();
        let pid = pause_head(&mut s);
        save_queue(&state, &s.queue);
        pid
    };
    if let Some(pid) = pid {
        blocking(move || epic::cancel(pid).map_err(|e| format!("Couldn't pause the download: {e}"))).await?;
    }
    Ok(queue_view(&app.state::<AppState>().lock()))
}

/// Pauses the queue. Returns the pid of the active job's Legendary, for the caller to stop.
fn pause_head(s: &mut Shared) -> Option<u32> {
    s.queue.pause();
    let head = s.queue.jobs.first().map(|j| j.key.clone()).filter(|_| s.queue_running);
    let pid = head.and_then(|k| s.install_pids.get(&k).copied());
    s.interrupted |= pid.is_some();
    pid
}

/// Stops the queue before the app exits for an update: the active Legendary is stopped (it continues the
/// partial download next time) and the queue is stored with its own paused flag, so the next start's
/// `restore_queue` resumes it. Returns whether the queue was already paused.
pub(super) fn stop_queue_for_restart(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let (was_paused, pid) = {
        let mut s = state.lock();
        (s.queue.paused, pause_head(&mut s))
    };
    if let Some(pid) = pid
        && let Err(e) = epic::cancel(pid)
    {
        log!("stop download for update: {e}");
    }
    for _ in 0..100 {
        if !state.lock().queue_running {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    save_queue(&state, &stored_for_restart(&state.lock().queue, was_paused));
    was_paused
}

/// The queue as stored across an update restart. The live queue stays paused, so the runner can't start
/// the next job before the app exits.
fn stored_for_restart(queue: &Queue, was_paused: bool) -> Queue {
    Queue { paused: was_paused, ..queue.clone() }
}

/// Starts the head job again with the same command.
#[tauri::command]
pub fn resume_downloads(app: AppHandle) -> QueueView {
    {
        let state = app.state::<AppState>();
        let mut s = state.lock();
        s.queue.resume();
        save_queue(&state, &s.queue);
    }
    start_queue(&app);
    queue_view(&app.state::<AppState>().lock())
}

/// Moves a waiting job one place up or down; the active job stays first.
#[tauri::command]
pub fn move_download(state: State<'_, AppState>, key: String, up: bool) -> QueueView {
    let mut s = state.lock();
    let moved = if up { s.queue.move_up(&key) } else { s.queue.move_down(&key) };
    if moved {
        save_queue(&state, &s.queue);
    }
    queue_view(&s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::tests::game;

    #[test]
    fn restart_keeps_live_queue_paused_and_stores_its_own_flag() {
        let mut s = Shared { queue_running: true, ..Shared::default() };
        s.queue.push(Job::new("epic-a", QueuedAction::Install));
        s.install_pids.insert("epic-a".into(), 42);
        assert_eq!(pause_head(&mut s), Some(42));
        assert!(s.queue.paused && s.interrupted);

        let stored = Queue::restore(Some(&stored_for_restart(&s.queue, false).to_json()), |_| true);
        assert!(!stored.paused, "a running queue resumes after the restart");
        assert_eq!(stored.jobs, s.queue.jobs);
        assert!(s.queue.paused, "the live queue stays paused until the app exits");
        assert!(Queue::restore(Some(&stored_for_restart(&s.queue, true).to_json()), |_| true).paused);
    }

    #[test]
    fn launch_options_round_trip() {
        let dir = std::env::temp_dir().join(format!("launcher-launch-options-{}", std::process::id()));
        let conn = cache::db::open(&dir.join("library.sqlite")).unwrap();
        assert_eq!(launch_options_of(&conn, "epic-A"), LaunchOptions::default());
        let options = LaunchOptions { args: "-dx11".into(), offline: true, skip_version_check: false, exe: Some(r"bin\game.exe".into()) };
        store_launch_options(&conn, "epic-A", &options).unwrap();
        assert_eq!(launch_options_of(&conn, "epic-A"), options);
        assert_eq!(launch_options_of(&conn, "epic-B"), LaunchOptions::default());
        let json = cache::db::pref(&conn, "epic-A", LAUNCH_OPTIONS).unwrap().unwrap();
        assert_eq!(json, r#"{"args":"-dx11","offline":true,"skipVersionCheck":false,"exe":"bin\\game.exe"}"#);
        store_launch_options(&conn, "epic-A", &LaunchOptions::default()).unwrap();
        assert_eq!(cache::db::pref(&conn, "epic-A", LAUNCH_OPTIONS).unwrap(), None);
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn pumpkin(steam_last: Option<u64>, epic_last: Option<u64>) -> Vec<Game> {
        let steam = Game { external_id: "635320".into(), ..game("Pumpkin Jack", true, Some(120), steam_last) };
        let epic = Game {
            provider: ProviderKind::Epic,
            external_id: "Pumpkin".into(),
            ..game("Pumpkin Jack", false, None, epic_last)
        };
        vec![epic, steam]
    }

    fn pumpkin_detail(games: &[Game], twins: &HashMap<String, u32>) -> Detail {
        let entries = entries::group(games, twins);
        detail(&entries[0], games, twins, &view::Lists::default())
    }

    #[test]
    fn epic_dlc_rows_carry_their_copy_key() {
        let mut games = pumpkin(None, None);
        games[0].dlc = vec![crate::library::Dlc { id: "PumpkinDlc".into(), title: "Pumpkin Hat".into(), installed: true }];
        let rows = epic_dlc(&games.iter().collect::<Vec<_>>());
        assert_eq!(
            rows,
            [DlcView { id: "PumpkinDlc".into(), title: "Pumpkin Hat".into(), installed: true, copy: Some("epic-Pumpkin".into()) }]
        );
    }

    #[test]
    fn steam_dlc_logs_on_only_when_nothing_answers() {
        let list = r#"[[10,"Hat"]]"#;
        assert_eq!(dlc_step(Some(list), true, Some(false)), DlcStep::Answer(vec![(10, "Hat".into())]));
        // A recent failure or a store page without DLC answers "none" without a logon.
        assert_eq!(dlc_step(None, true, Some(true)), DlcStep::Answer(Vec::new()));
        assert_eq!(dlc_step(None, false, Some(false)), DlcStep::Answer(Vec::new()));
        // Unknown or listed DLC, nothing cached: log on. A broken cache entry counts as nothing cached.
        assert_eq!(dlc_step(None, false, None), DlcStep::LogOn);
        assert_eq!(dlc_step(Some("junk"), false, Some(true)), DlcStep::LogOn);
    }

    #[test]
    fn steam_dlc_queued_caller_reads_what_the_first_cached() {
        let path = std::env::temp_dir().join(format!("launcher-dlc-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = cache::db::open(&path).unwrap();
        // Before the lock: nothing yet, so it would log on.
        let (cached, failed) = cached_dlc(&conn, "steam-1", 1_000);
        assert_eq!(dlc_step(cached.as_deref(), failed, None), DlcStep::LogOn);
        // After the lock: the first caller's answer is there.
        cache::db::put_info(&conn, "steam-1", DLC, r#"[[10,"Hat"]]"#, 1_000).unwrap();
        let (cached, failed) = cached_dlc(&conn, "steam-1", 1_001);
        assert_eq!(dlc_step(cached.as_deref(), failed, None), DlcStep::Answer(vec![(10, "Hat".into())]));
        // A failure is remembered for 6 h only.
        cache::db::put_info(&conn, "steam-2", DLC_FAILED, "null", 1_000).unwrap();
        assert!(cached_dlc(&conn, "steam-2", 1_000 + DLC_RETRY_SECS).1);
        assert!(!cached_dlc(&conn, "steam-2", 1_001 + DLC_RETRY_SECS).1);
    }

    #[test]
    fn conflict_decision_before_launch() {
        assert_eq!(pre_launch_sync(None, false), Ok(Some(SaveSync::Download)));
        // A newer cloud save left over from the last upload is never downloaded without asking.
        assert_eq!(pre_launch_sync(None, true), Err(Conflict::CloudNewer));
        assert_eq!(pre_launch_sync(Some(Keep::Cloud), true), Ok(Some(SaveSync::KeepCloud)));
        assert_eq!(pre_launch_sync(Some(Keep::Local), false), Ok(None));
    }

    #[test]
    fn sync_records_show_in_the_saves_tab() {
        let shown = |outcome: SyncOutcome| saves_view(None, sync_record(&outcome, 1_000));
        assert_eq!(shown(SyncOutcome::Synced), SavesView { choice: None, synced_at: Some(1_000), error: None });
        assert_eq!(shown(SyncOutcome::Failed("Login failed!".into())).error.as_deref(), Some("Cloud saves didn't sync: Login failed!"));
        assert!(shown(SyncOutcome::CloudNewer).error.is_some());
        assert_eq!(sync_record(&SyncOutcome::LocalNewer, 1_000), None);
        assert_eq!(saves_view(Some(false), None), SavesView { choice: Some(false), synced_at: None, error: None });
        // Stored as JSON in the info table and read back.
        let json = serde_json::to_string(&sync_record(&SyncOutcome::CloudNewer, 7).unwrap()).unwrap();
        assert_eq!(json, r#"{"at":7,"cloud_newer":true}"#);
        assert_eq!(serde_json::from_str::<SyncRecord>(r#"{"at":7}"#).unwrap(), SyncRecord { at: 7, error: None, cloud_newer: false });
    }

    #[test]
    fn detail_for_a_merged_entry() {
        let games = pumpkin(None, None);
        let d = pumpkin_detail(&games, &HashMap::from([("epic-Pumpkin".to_string(), 635320)]));
        assert_eq!(d.key, "steam-635320");
        assert_eq!(d.chosen, "steam-635320");
        assert_eq!(d.copies.len(), 2);
        assert_eq!(d.playtime_minutes, Some(120));
        assert!(!d.playtime_both);
        assert_eq!(d.links.len(), 5);
    }

    #[test]
    fn last_played_store_only_for_merged_entries() {
        let games = pumpkin(Some(86_400), Some(2 * 86_400));
        let merged = pumpkin_detail(&games, &HashMap::from([("epic-Pumpkin".to_string(), 635320)]));
        assert_eq!(merged.last_played_store, Some(ProviderKind::Epic));
        assert_eq!(merged.last_played, Some(2 * 86_400));
        let single = pumpkin_detail(&games, &HashMap::new());
        assert_eq!(single.last_played_store, None);
    }
}
