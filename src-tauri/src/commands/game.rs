//! One game: its page, art, details, play/install/uninstall, download size, how long to beat.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, UNIX_EPOCH};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use super::library::rescan;
use super::{
    AppState, Process, Res, Shared, Watch, allow, blocking, open_db, queue_view, unix_now,
};
use crate::cache;
use crate::downloads::{DOWNLOAD_QUEUE, Job, Queue, QueueView, QueuedAction};
use crate::entries::{self, Entry};
use crate::gamesdb;
use crate::igdb;
use crate::info::{self, GameInfo};
use crate::library::epic;
use crate::library::tool::{InstallEvent, LaunchOptions, SaveSync, SyncOutcome};
use crate::library::{Game, Image, ProgressAction, ProviderKind};
use crate::library::{amazon, ea, gog, steam};
use crate::links::{self, LinkKind};
use crate::msg::{Msg, Param};
use crate::platform::kill_tree;
use crate::settings;
use crate::steam_size;
use crate::view;

#[derive(Serialize, specta::Type)]
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
    /// The stores whose playtime is summed, only when more than one is.
    playtime_stores: Vec<ProviderKind>,
    /// Unix seconds; `None` when never played.
    last_played: Option<u64>,
    /// The store played on last, only for an entry with several copies.
    last_played_store: Option<ProviderKind>,
    /// Store pages, then info sites when the Steam app id is known.
    links: Vec<LinkKind>,
    /// The Epic and GOG copies' DLC; a Steam copy's comes from `steam_dlc`.
    dlc: Vec<DlcView>,
    /// Any copy is a favorite, hidden, or in the collection (ids).
    favorite: bool,
    hidden: bool,
    collections: Vec<i64>,
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CopyView {
    key: String,
    provider: ProviderKind,
    installed: bool,
    /// An installed Epic or GOG copy has a newer version.
    update_available: bool,
    /// Size on disk of an installed copy.
    size_bytes: Option<u64>,
    /// Only for an installed copy (Steam knows a folder for games it hasn't installed yet).
    install_dir: Option<String>,
    /// An installed Epic or GOG copy that keeps its saves in its store's cloud.
    cloud_saves: bool,
}

/// The game page. Opening it asks GOG for a GOG copy's playtime in the background, when that is due.
#[tauri::command]
#[specta::specta]
pub fn game(app: AppHandle, key: String) -> Option<Detail> {
    let state = app.state::<AppState>();
    let s = state.lock();
    let entry = entries::find(&s.entries, &s.games, &key)?;
    let gog = entry
        .copies
        .iter()
        .map(|&i| &s.games[i])
        .find(|g| g.provider == ProviderKind::Gog)
        .map(Game::key);
    let detail = detail(entry, &s.games, &s.twins, &s.lists);
    drop(s);
    if let Some(gog) = gog {
        super::library::refresh_gog_playtime(app.clone(), Some(gog));
    }
    Some(detail)
}

fn detail(
    entry: &Entry,
    games: &[Game],
    twins: &HashMap<String, u32>,
    lists: &view::Lists,
) -> Detail {
    let (minutes, mut playtime_stores) = entries::playtime(entry, games);
    if playtime_stores.len() < 2 {
        playtime_stores.clear();
    }
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
        playtime_stores,
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
#[specta::specta]
pub fn open_folder(state: State<'_, AppState>, key: String) -> Res<()> {
    let game = state.game(&key).ok_or(msg!(gameNotInLibrary))?;
    let dir = installed_dir(&game).ok_or(msg!(noInstallFolder))?;
    if !dir.is_dir() {
        return Err(msg!(installFolderMissing, path = dir.display().to_string()));
    }
    crate::platform::shell_open(dir).map_err(Msg::from)
}

/// Opens one of the game page's links in the default browser. The URL is built here from the kind.
#[tauri::command]
#[specta::specta]
pub async fn open_link(app: AppHandle, key: String, kind: LinkKind) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (appid, title, epic_key) = {
            let s = state.lock();
            let entry = entries::find(&s.entries, &s.games, &key).ok_or(msg!(gameNotInLibrary))?;
            let epic = entry
                .copies
                .iter()
                .map(|&i| &s.games[i])
                .find(|g| g.provider == ProviderKind::Epic);
            (
                entries::steam_appid(entry, &s.games, &s.twins),
                s.games[entry.copies[0]].title.clone(),
                epic.map(|g| g.key()),
            )
        };
        let epic_url = epic_key.and_then(|k| igdb::epic_url(&open_db(&state)?, &k));
        let url = links::url(kind, appid, epic_url.as_deref(), &title).ok_or(msg!(noPageThere))?;
        crate::platform::open_url(&url).map_err(Msg::from)
    })
    .await
}

#[derive(Serialize, Default, specta::Type)]
pub struct DetailArt {
    hero: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn detail_art(app: AppHandle, key: String) -> DetailArt {
    let state = app.state::<AppState>();
    let Some(game) = state.game(&key) else {
        return DetailArt::default();
    };
    let dir = state.dirs.art.join(&key);
    // The game page shows a text title, so the logo art isn't fetched.
    let hero = cache::resolve(&game.art.hero, &state.http, dir.join("hero")).await;
    DetailArt {
        hero: hero.and_then(|p| allow(&app, p)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Play,
    /// Epic and GOG: start an out-of-date game without updating it first (a GOG Play never updates).
    PlayOutdated,
    Install,
    /// Epic and GOG; Steam updates games itself.
    Update,
    Uninstall,
    /// Epic and GOG verify and repair here; Steam validates in its own window.
    Verify,
    /// Epic and GOG move the folder to `dest`; Steam opens its storage manager.
    Move,
}

/// The answer to a cloud save conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Keep {
    Cloud,
    Local,
}

/// Why an Epic or GOG Play didn't launch yet: the question to ask first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Conflict {
    LocalNewer,
    CloudNewer,
}

/// Steam requests return at once (Steam shows its own dialogs). Epic and GOG requests return when
/// Legendary or gogdl is done; installs report progress through `poll` meanwhile, under the game's key.
/// An Epic or GOG install here takes the owned DLC too (the install panel uses `install::install`). `dlc` installs or
/// uninstalls one DLC of the copy `key` instead of the game. `dest` is the folder an Epic or GOG Move goes to.
/// An Epic or GOG Play that finds a cloud save conflict returns it instead of launching; Play again with `keep`.
#[tauri::command]
#[specta::specta]
pub async fn act(
    app: AppHandle,
    action: Action,
    key: String,
    dlc: Option<String>,
    keep: Option<Keep>,
    dest: Option<String>,
) -> Res<Option<Conflict>> {
    let game = app
        .state::<AppState>()
        .game(&key)
        .ok_or(msg!(gameNotInLibrary))?;
    if let Some(id) = &dlc
        && (matches!(action, Action::Play | Action::PlayOutdated)
            || !game.dlc.iter().any(|d| &d.id == id))
    {
        return Err(msg!(dlcNotInLibrary));
    }
    // A DLC install is just that DLC.
    let with_dlc = dlc.is_none().then_some(true);
    blocking(move || {
        act_blocking(
            &app,
            action,
            &game,
            with_dlc,
            dlc.as_deref(),
            keep,
            dest.as_deref(),
        )
    })
    .await
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
    let play = matches!(action, Action::Play | Action::PlayOutdated);
    if play
        && game.provider != ProviderKind::Steam
        && replaces_upload(state.lock().watch.as_ref(), &key)
    {
        return Err(msg!(closeGameFirst));
    }
    if action == Action::Uninstall {
        let steam = steam::running_game().map(|id| ProviderKind::Steam.key(&id));
        if card_running(&state.lock(), &key, steam.as_deref()) {
            return Err(msg!(closeGameFirst));
        }
    }
    if game.provider == ProviderKind::Steam {
        if action == Action::Install {
            state.lock().tracked.insert(key.clone());
        }
        let result = match action {
            Action::Play | Action::PlayOutdated => {
                steam::launch(game).inspect(|()| record_play(&state, &key))
            }
            Action::Install => steam::install(game),
            Action::Update => Err(msg!(steamUpdatesItself).into()),
            Action::Uninstall => steam::uninstall(game),
            Action::Verify => {
                crate::platform::shell_open(format!("steam://validate/{}", game.external_id))
            }
            Action::Move => crate::platform::shell_open("steam://open/storage"),
        };
        return result
            .map(|()| None)
            .map_err(|e| store_error(ProviderKind::Steam, e));
    }
    if game.provider == ProviderKind::Custom {
        return act_custom(&state, action, game).map(|()| None);
    }
    if game.provider == ProviderKind::Ea
        && let Some(reason) = ea_refusal(action)
    {
        return Err(store_error(ProviderKind::Ea, reason));
    }
    let queued = match action {
        Action::Install => Some(QueuedAction::Install),
        Action::Update => Some(QueuedAction::Update),
        Action::Verify => Some(QueuedAction::Verify),
        _ => None,
    };
    if matches!(
        game.provider,
        ProviderKind::Gog | ProviderKind::Amazon | ProviderKind::Ea
    ) && action == Action::Install
        && dlc.is_none()
        && game.installed
    {
        return Err(store_error(game.provider, msg!(alreadyInstalled)));
    }
    // The job checks again with a fresh answer.
    if game.provider == ProviderKind::Amazon && action == Action::Verify && game.update_available {
        return Err(store_error(game.provider, msg!(updateFirst)));
    }
    if let Some(queued) = queued {
        let dest = (queued == QueuedAction::Install)
            .then(|| install_base(&state, game.provider))
            .flatten();
        let job = Job {
            dlc: dlc.map(str::to_string),
            with_dlc,
            dest,
            ..Job::new(&key, queued)
        };
        return enqueue(app, job).map(|()| None);
    }
    if matches!(action, Action::Move | Action::Uninstall) && state.lock().queue.contains(&key) {
        return Err(msg!(cancelDownloadFirst));
    }
    // Only Play and Uninstall are left for EA: the rest was refused or queued above.
    if game.provider == ProviderKind::Ea {
        let result = act_ea(&state, action, game);
        if action == Action::Uninstall {
            rescan(app, true);
        }
        return result
            .map(|()| None)
            .map_err(|e| store_error(ProviderKind::Ea, e));
    }
    if game.provider == ProviderKind::Gog && matches!(action, Action::Play | Action::PlayOutdated) {
        let upload = match saves_before_play(&state, game, keep) {
            Ok(upload) => upload,
            Err(conflict) => return Ok(Some(conflict)),
        };
        let options = checked_launch_options(open_db(&state).as_ref(), game)?;
        let dir = installed_dir(game)
            .ok_or_else(|| store_error(ProviderKind::Gog, msg!(notInstalled)))?;
        let child = gog::launch(&game.external_id, dir, &options)
            .map_err(|e| store_error(ProviderKind::Gog, e))?;
        record_play(&state, &key);
        state.lock().watch = Some(Watch {
            key,
            process: Process::Ours(child),
            since: Instant::now(),
            seen: false,
            upload,
        });
        return Ok(None);
    }
    if game.provider == ProviderKind::Gog {
        let result = act_gog(&state, action, game, dlc, dest);
        rescan(app, true);
        return result
            .map(|()| None)
            .map_err(|e| store_error(ProviderKind::Gog, e));
    }
    if game.provider == ProviderKind::Amazon {
        let result = act_amazon(&state, action, game, dest);
        if !matches!(action, Action::Play | Action::PlayOutdated) {
            rescan(app, true);
        }
        return result
            .map(|()| None)
            .map_err(|e| store_error(ProviderKind::Amazon, e));
    }
    let result = match action {
        Action::Play | Action::PlayOutdated => {
            let upload = match saves_before_play(&state, game, keep) {
                Ok(upload) => upload,
                Err(conflict) => return Ok(Some(conflict)),
            };
            let options = checked_launch_options(open_db(&state).as_ref(), game)?;
            epic::launch(&game.external_id, &options, action == Action::PlayOutdated).map(|child| {
                record_play(&state, &key);
                // Linux starts the game itself; on Windows Legendary did, so it is found by its exe name.
                // The game started either way; without its exe name it just isn't shown as running.
                // shortcut: unwatched, its saves aren't uploaded either; the next Play asks about the newer local save.
                let process = child.map(Process::Ours).or_else(|| {
                    let alternative = options
                        .exe
                        .as_deref()
                        .and_then(|e| std::path::Path::new(e).file_name());
                    let exe = match alternative {
                        Some(name) => Some(name.to_string_lossy().into_owned()),
                        None => epic::executable(game).unwrap_or_else(|e| {
                            tracing::warn!("legendary list-installed {key}: {e}");
                            None
                        }),
                    };
                    exe.map(Process::Named)
                });
                state.lock().watch = process.map(|process| Watch {
                    key: key.clone(),
                    process,
                    since: Instant::now(),
                    seen: false,
                    upload,
                });
            })
        }
        // Not cancellable: stopped halfway, Legendary's record and the folder disagree.
        Action::Move => {
            let on_event = |event| _ = record_event(&mut state.lock(), &key, event);
            let result = match dest {
                Some(dest) => epic::move_to(
                    &game.external_id,
                    game.install_dir.as_deref(),
                    dest,
                    on_event,
                ),
                None => Err(msg!(pickMoveFolder).into()),
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
    result
        .map(|()| None)
        .map_err(|e| store_error(ProviderKind::Epic, e))
}

/// Why an EA copy can't do `action`, if it can't: Maxima has no verify or move.
fn ea_refusal(action: Action) -> Option<Msg> {
    match action {
        Action::Verify | Action::Move => Some(msg!(eaCantVerifyOrMove)),
        Action::Play
        | Action::PlayOutdated
        | Action::Install
        | Action::Update
        | Action::Uninstall => None,
    }
}

/// Whether a Play would replace the watch of another game whose saves still go up once it exits. One game is watched
/// at a time, and a replaced watch never uploads.
fn replaces_upload(watch: Option<&Watch>, key: &str) -> bool {
    watch.is_some_and(|w| w.key != key && w.upload.is_some())
}

/// Whether a copy of the card `key` belongs to runs: Steam's running game (`steam`, its key) or the watched one.
fn card_running(s: &Shared, key: &str, steam: Option<&str>) -> bool {
    let watched = s.watch.as_ref().map(|w| w.key.as_str());
    let runs = |k: &str| Some(k) == steam || Some(k) == watched;
    match crate::entries::find(&s.entries, &s.games, key) {
        Some(e) => e.copies.iter().any(|&i| runs(&s.games[i].key())),
        None => runs(key),
    }
}

/// `{store}: {reason}`, like "GOG: Close the game first."
fn store_error(store: ProviderKind, reason: impl Into<Msg>) -> Msg {
    msg!(storeError, store = store.name(), reason = reason.into())
}

/// The base folder a queued install goes into, fixed when it is queued so Pause and Resume use the same one. Epic:
/// the setting, else Legendary's own default (`epic::default_base`). GOG, Amazon and EA: the store's setting, else `%USERPROFILE%\Games`
/// (gogdl has no default, and nile's is `~/Games/nile`).
pub(super) fn install_base(state: &AppState, provider: ProviderKind) -> Option<String> {
    let saved = open_db(state).map(|c| settings::load(&c));
    let dir = match provider {
        ProviderKind::Gog => saved
            .and_then(|s| s.gog_install_dir)
            .or_else(gog::default_base),
        ProviderKind::Amazon => saved
            .and_then(|s| s.amazon_install_dir)
            .or_else(gog::default_base),
        ProviderKind::Ea => saved
            .and_then(|s| s.ea_install_dir)
            .or_else(gog::default_base),
        // Passed as `--base-path` from now on, so the folder shown is the folder used.
        ProviderKind::Epic | ProviderKind::Steam | ProviderKind::Custom => saved
            .and_then(|s| s.epic_install_dir)
            .or_else(epic::default_base),
    };
    dir.map(|p| p.to_string_lossy().into_owned())
}

/// A GOG copy's actions outside the queue, but Play. A DLC uninstall is an update without it, so it reports progress like Move.
fn act_gog(
    state: &AppState,
    action: Action,
    game: &Game,
    dlc: Option<&str>,
    dest: Option<&str>,
) -> Result<(), crate::library::Error> {
    let key = game.key();
    let dir = installed_dir(game).ok_or(msg!(notInstalled))?;
    let conn = cache::db::open(&state.dirs.db)?;
    match (action, dlc) {
        (Action::Play | Action::PlayOutdated, _) => unreachable!("played in act_blocking"),
        (Action::Move, _) => gog::move_to(
            &conn,
            &game.external_id,
            dir,
            std::path::Path::new(dest.ok_or(msg!(pickMoveFolder))?),
        ),
        (Action::Uninstall, None) => gog::uninstall(&conn, &game.external_id, dir),
        (Action::Uninstall, Some(dlc)) => {
            let on_event = |event| _ = record_event(&mut state.lock(), &key, event);
            let result = gog::change_dlc(&game.external_id, dir, dlc, false, on_event);
            let mut s = state.lock();
            s.install_pids.remove(&key);
            s.progress.remove(&key);
            result
        }
        (Action::Install | Action::Update | Action::Verify, _) => unreachable!("queued"),
    }
}

/// An Amazon copy's actions outside the queue. Play starts its `fuel.json` program and watches it to time the session:
/// Amazon keeps no playtime and has no cloud saves. Play without updating is Play.
fn act_amazon(
    state: &AppState,
    action: Action,
    game: &Game,
    dest: Option<&str>,
) -> Result<(), crate::library::Error> {
    let key = game.key();
    let dir = installed_dir(game).ok_or(msg!(notInstalled))?;
    match action {
        Action::Play | Action::PlayOutdated => {
            let options = checked_launch_options(open_db(state).as_ref(), game)?;
            let child = amazon::launch(&game.external_id, dir, &options)?;
            record_play(state, &key);
            state.lock().watch = Some(Watch {
                key,
                process: Process::Ours(child),
                since: Instant::now(),
                seen: false,
                upload: None,
            });
            Ok(())
        }
        Action::Move => amazon::move_to(
            &game.external_id,
            dir,
            std::path::Path::new(dest.ok_or(msg!(pickMoveFolder))?),
        ),
        Action::Uninstall => amazon::uninstall(open_db(state).as_ref(), &game.external_id),
        Action::Install | Action::Update | Action::Verify => unreachable!("queued"),
    }
}

/// An EA copy's Play and Uninstall. Play watches `guild launch`, which stays until the game stops and its saves are up.
fn act_ea(state: &AppState, action: Action, game: &Game) -> Result<(), crate::library::Error> {
    match action {
        Action::Play | Action::PlayOutdated => {
            let options = checked_launch_options(open_db(state).as_ref(), game)?;
            let child = ea::launch(&state.dirs, &game.external_id, &options)?;
            let key = game.key();
            record_play(state, &key);
            state.lock().watch = Some(Watch {
                key,
                process: Process::Ours(child),
                since: Instant::now(),
                seen: false,
                upload: None,
            });
            Ok(())
        }
        Action::Uninstall => ea::uninstall(&state.dirs, &game.external_id),
        Action::Install | Action::Update | Action::Verify | Action::Move => {
            unreachable!("queued or refused")
        }
    }
}

/// A game the owner added only plays: its program starts, watched like an Epic game to time the session.
fn act_custom(state: &AppState, action: Action, game: &Game) -> Res<()> {
    let result = match action {
        Action::Play | Action::PlayOutdated => {
            let conn = open_db(state).ok_or(msg!(noDb))?;
            let row =
                cache::db::custom_game(&conn, &game.external_id)?.ok_or(msg!(gameNotInLibrary))?;
            let key = game.key();
            let proton = launch_options_of(&conn, &game.key()).proton;
            crate::library::custom::launch(&row, proton.as_deref()).map(|child| {
                record_play(state, &key);
                state.lock().watch = Some(Watch {
                    key,
                    process: Process::Ours(child),
                    since: Instant::now(),
                    seen: false,
                    upload: None,
                });
            })
        }
        // A not-installed added game is one whose program is gone.
        Action::Install => Err(crate::library::custom::missing(&game.title)),
        Action::Uninstall => Err(msg!(customUninstall).into()),
        Action::Update | Action::Verify | Action::Move => Err(msg!(addedGamesOnlyPlay).into()),
    };
    result.map_err(Msg::from)
}

/// Records a Legendary or gogdl job's pid or progress under its copy key. Returns the pid of a queued job that
/// must stop at once: it was cancelled or the queue paused before its process started.
fn record_event(s: &mut Shared, key: &str, event: InstallEvent) -> Option<u32> {
    match event {
        InstallEvent::Started(pid) => {
            s.install_pids.insert(key.to_string(), pid);
            let stop = s.queue.is_head(key)
                && s.queue_running
                && (s.cancelled.contains(key) || s.queue.paused);
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
    let saved = cache::db::open(&state.dirs.db)
        .and_then(|c| cache::db::set_setting(&c, DOWNLOAD_QUEUE, Some(&queue.to_json())));
    if let Err(e) = saved {
        tracing::warn!("download queue: {e}");
    }
}

/// Gives the `act` waiting for `key` its outcome; a job nobody waits for (restored at start) only logs a failure.
fn report(s: &mut Shared, key: &str, outcome: Res<()>) {
    match s.waiters.remove(key) {
        Some(waiter) => _ = waiter.send(outcome),
        None => {
            if let Err(e) = outcome {
                tracing::warn!("download {key}: {e}");
            }
        }
    }
}

/// Queues an Epic or GOG job and waits until it leaves the queue: done, failed, cancelled or removed.
pub(super) fn enqueue(app: &AppHandle, job: Job) -> Res<()> {
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
        let game = state.game(&key);
        let result = match &game {
            Some(game) => run_job(&state, game, &job),
            None => Err(msg!(gameNotInLibrary).into()),
        };
        let store = game.map_or(ProviderKind::Epic, |g| g.provider);
        let paused = {
            let mut s = state.lock();
            let paused = job_paused(&mut s, &key, result.is_err());
            if paused {
                save_queue(&state, &s.queue);
            }
            paused
        };
        // The progress stays for "Paused · 42%".
        if paused {
            continue;
        }
        let mut s = state.lock();
        let (toast, repaired) = finish_job(&mut s, &key, result, store);
        save_queue(&state, &s.queue);
        drop(s);
        // In the background: the next job need not wait for a full online scan. Only that scan shows a store game
        // as installed, so the finished job's progress and its toast wait for it.
        let scan = app.clone();
        let action = job.action;
        tauri::async_runtime::spawn_blocking(move || {
            let changed = rescan(&scan, true);
            let state = scan.state::<AppState>();
            let mut s = state.lock();
            s.library_changed |= changed;
            let Some(title) = toast else { return };
            if !s.queue.jobs.iter().any(|j| j.key == key) {
                s.progress.remove(&key);
            }
            drop(s);
            super::notify_done(
                &scan,
                &title,
                crate::downloads::done_message(action, repaired),
            );
        });
    }
}

/// After a job's process ends: whether Pause stopped it, so it stays at the head and the runner goes on with it.
fn job_paused(s: &mut Shared, key: &str, failed: bool) -> bool {
    s.install_pids.remove(key);
    std::mem::take(&mut s.interrupted) && failed && !s.cancelled.contains(key)
}

/// Takes a job that ended out of the queue and gives its waiter the outcome. Returns the game's title for the
/// done toast, and whether a verify repaired files.
fn finish_job(
    s: &mut Shared,
    key: &str,
    result: Result<(), crate::library::Error>,
    store: ProviderKind,
) -> (Option<String>, bool) {
    // A verify that found bad files ends reporting the repair.
    let repaired = s
        .progress
        .get(key)
        .is_some_and(|p| p.action == ProgressAction::Repair);
    // A cancelled job fails by design; don't report it.
    let cancelled = s.cancelled.remove(key);
    let toast = (result.is_ok() && !cancelled)
        .then(|| {
            s.games
                .iter()
                .find(|g| g.key() == key)
                .map(|g| g.title.clone())
        })
        .flatten();
    // A job with a toast keeps its progress until the rescan after it (`run_queue`), so the card never shows
    // Install between the download ending and the game showing as installed.
    if toast.is_none() {
        s.progress.remove(key);
    }
    let outcome = if cancelled {
        Ok(())
    } else {
        result.map_err(|e| store_error(store, e))
    };
    s.queue.finish(key);
    report(s, key, outcome);
    s.library_changed = true;
    (toast, repaired)
}

fn run_job(state: &AppState, game: &Game, job: &Job) -> Result<(), crate::library::Error> {
    let on_event = |event| {
        let stop = record_event(&mut state.lock(), &job.key, event);
        if let Some(pid) = stop
            && let Err(e) = kill_tree(pid)
        {
            tracing::warn!("stop download {}: {e}", job.key);
        }
    };
    if game.provider == ProviderKind::Gog {
        return run_gog_job(state, game, job, on_event);
    }
    if game.provider == ProviderKind::Amazon {
        return run_amazon_job(state, game, job, on_event);
    }
    if game.provider == ProviderKind::Ea {
        return run_ea_job(state, game, job, on_event);
    }
    match job.action {
        QueuedAction::Update => epic::update(&game.external_id, on_event),
        QueuedAction::Verify => epic::verify(&game.external_id, on_event),
        QueuedAction::Install => {
            let dest = job.dest.as_deref().map(std::path::Path::new);
            epic::install(
                job.dlc.as_deref().unwrap_or(&game.external_id),
                job.with_dlc,
                dest,
                &on_event,
            )?;
            // Legendary takes all DLC or none, so a choice of some follows the game one by one.
            for dlc in job.dlcs.iter().flatten() {
                epic::install(dlc, None, dest, &on_event)?;
            }
            Ok(())
        }
    }
}

/// A queued GOG job. Each makes sure of the login first; an install writes the game's record once its files are in.
fn run_gog_job(
    state: &AppState,
    game: &Game,
    job: &Job,
    on_event: impl FnMut(InstallEvent),
) -> Result<(), crate::library::Error> {
    let id = &game.external_id;
    let dir = || installed_dir(game).ok_or(msg!(notInstalled));
    match (job.action, &job.dlc) {
        (QueuedAction::Install, None) => {
            let base = job
                .dest
                .as_deref()
                .map(PathBuf::from)
                .ok_or(msg!(noInstallBase))?;
            let dlcs = match &job.dlcs {
                Some(ids) => gog::Dlcs::Only(ids.clone()),
                None if job.with_dlc != Some(false) => gog::Dlcs::All,
                None => gog::Dlcs::Only(Vec::new()),
            };
            let dir = gog::install(
                open_db(state).as_ref(),
                id,
                &base,
                &dlcs,
                job.lang.as_deref(),
                job.windows,
                on_event,
            )?;
            gog::record_install(&cache::db::open(&state.dirs.db)?, id, &dir)
        }
        (QueuedAction::Install, Some(dlc)) => gog::change_dlc(id, dir()?, dlc, true, on_event),
        (QueuedAction::Update, _) => gog::update(id, dir()?, on_event),
        (QueuedAction::Verify, _) => gog::verify(id, dir()?, on_event),
    }
}

/// A queued Amazon job; each holds nile for its whole run.
fn run_amazon_job(
    state: &AppState,
    game: &Game,
    job: &Job,
    on_event: impl FnMut(InstallEvent),
) -> Result<(), crate::library::Error> {
    let id = &game.external_id;
    let dir = || installed_dir(game).ok_or(msg!(notInstalled));
    match job.action {
        QueuedAction::Install => {
            let base = job
                .dest
                .as_deref()
                .map(PathBuf::from)
                .ok_or(msg!(noInstallBase))?;
            amazon::install(id, &base, on_event).map(drop)
        }
        QueuedAction::Update => amazon::update(open_db(state).as_ref(), id, dir()?, on_event),
        QueuedAction::Verify => amazon::verify(open_db(state).as_ref(), id, dir()?, on_event),
    }
}

/// A queued EA job. An update is an install into the game's own folder.
fn run_ea_job(
    state: &AppState,
    game: &Game,
    job: &Job,
    on_event: impl FnMut(InstallEvent),
) -> Result<(), crate::library::Error> {
    let id = &game.external_id;
    match job.action {
        QueuedAction::Install => {
            let base = job
                .dest
                .as_deref()
                .map(PathBuf::from)
                .ok_or(msg!(noInstallBase))?;
            ea::install(&state.dirs, id, &game.title, &base, on_event).map(drop)
        }
        QueuedAction::Update => {
            let dir = installed_dir(game).ok_or(msg!(notInstalled))?;
            ea::update(&state.dirs, id, dir, on_event)
        }
        QueuedAction::Verify => Err(msg!(eaCantVerifyOrMove).into()),
    }
}

/// `game_prefs` name for an Epic, GOG or Amazon copy's launch options, as `LaunchOptions` JSON; none is all defaults.
/// GOG and Amazon use only the arguments and the alternative executable.
const LAUNCH_OPTIONS: &str = "launch_options";

pub(super) fn launch_options_of(conn: &Connection, key: &str) -> LaunchOptions {
    let json = cache::db::pref(conn, key, LAUNCH_OPTIONS).ok().flatten();
    json.and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default()
}

fn store_launch_options(conn: &Connection, key: &str, options: &LaunchOptions) -> Res<()> {
    let json = (*options != LaunchOptions::default())
        .then(|| serde_json::to_string(options).unwrap_or_default());
    cache::db::set_pref(conn, key, LAUNCH_OPTIONS, json.as_deref()).map_err(Msg::from)
}

/// An installed copy's launch options, with the alternative executable checked again: the folder
/// may have changed since it was saved.
fn checked_launch_options(conn: Option<&Connection>, game: &Game) -> Res<LaunchOptions> {
    let mut options = conn
        .map(|c| launch_options_of(c, &game.key()))
        .unwrap_or_default();
    if let Some(exe) = &options.exe {
        let dir = installed_dir(game).ok_or(msg!(noInstallFolder))?;
        options.exe = Some(
            epic::override_exe(dir, exe)
                .map_err(|e| msg!(alternativeExe, reason = Msg::from(e)))?,
        );
    }
    Ok(options)
}

/// Whether the copy has launch options here: an installed Epic, GOG, Amazon or EA copy (Steam keeps its own), or a game the
/// owner added, which only has its Proton choice (Linux).
fn has_launch_options(g: &Game) -> bool {
    g.installed
        && matches!(
            g.provider,
            ProviderKind::Epic
                | ProviderKind::Gog
                | ProviderKind::Amazon
                | ProviderKind::Ea
                | ProviderKind::Custom
        )
}

/// The Proton builds a copy's launch options may pick, newest first. None on Windows, none for a native Linux build
/// of a GOG game, which runs without one, and none for EA, whose Maxima picks its own.
#[tauri::command]
#[specta::specta]
pub async fn protons(app: AppHandle, key: String) -> Vec<String> {
    let game = app.state::<AppState>().game(&key);
    blocking(move || {
        let native = game
            .as_ref()
            .filter(|g| g.provider == ProviderKind::Gog)
            .and_then(installed_dir)
            .is_some_and(|dir| gog::native(dir));
        let ea = game
            .as_ref()
            .is_some_and(|g| g.provider == ProviderKind::Ea);
        if native || ea {
            Vec::new()
        } else {
            crate::platform::protons()
        }
    })
    .await
}

/// The Launch options tab of an installed Epic, GOG or Amazon copy; `None` for others.
#[tauri::command]
#[specta::specta]
pub async fn launch_options(app: AppHandle, key: String) -> Option<LaunchOptions> {
    app.state::<AppState>()
        .game(&key)
        .filter(has_launch_options)?;
    blocking(move || Some(launch_options_of(&open_db(&app.state::<AppState>())?, &key))).await
}

/// Saves an installed Epic, GOG or Amazon copy's launch options. `exe` may be absolute (from the file picker); it is
/// stored relative to the install folder, and only when it is a file inside it. Returns what was stored.
#[tauri::command]
#[specta::specta]
pub async fn set_launch_options(
    app: AppHandle,
    key: String,
    mut options: LaunchOptions,
) -> Res<LaunchOptions> {
    let game = app
        .state::<AppState>()
        .game(&key)
        .filter(has_launch_options)
        .ok_or(msg!(noLaunchOptionsHere))?;
    if game.provider == ProviderKind::Custom {
        // An added game keeps its program and arguments itself.
        options = LaunchOptions {
            proton: options.proton,
            ..LaunchOptions::default()
        };
    }
    blocking(move || {
        if let Some(exe) = &options.exe {
            let dir = installed_dir(&game).ok_or(msg!(noInstallFolder))?;
            options.exe = Some(epic::override_exe(dir, exe)?);
        }
        let conn = open_db(&app.state::<AppState>()).ok_or(msg!(noDb))?;
        store_launch_options(&conn, &key, &options)?;
        Ok(options)
    })
    .await
}

/// Opens a Steam copy's properties in Steam, where its launch options live.
#[tauri::command]
#[specta::specta]
pub fn open_steam_properties(state: State<'_, AppState>, key: String) -> Res<()> {
    let game = state
        .game(&key)
        .filter(|g| g.provider == ProviderKind::Steam)
        .ok_or(msg!(notFromSteam))?;
    crate::platform::shell_open(format!("steam://gameproperties/{}", game.external_id))
        .map_err(Msg::from)
}

/// `game_prefs` name for an Epic or GOG copy's own cloud save choice, `on` or `off`; none follows its store's setting.
const SAVES_CHOICE: &str = "cloud_saves";
/// `game_prefs` name for an Epic or GOG copy's last cloud save sync, as a `SyncRecord`.
const SAVES_SYNC: &str = "cloud_saves_sync";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct SyncRecord {
    /// Unix seconds.
    at: i64,
    /// What went wrong; nothing synced. A `Msg`, or plain English stored before message codes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<Param>,
    /// After the game, the cloud save was newer, so the local one wasn't uploaded. The next Play asks first.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    cloud_newer: bool,
}

fn saves_choice(conn: &Connection, key: &str) -> Option<bool> {
    match cache::db::pref(conn, key, SAVES_CHOICE)
        .ok()
        .flatten()?
        .as_str()
    {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

fn last_sync(conn: &Connection, key: &str) -> Option<SyncRecord> {
    let json = cache::db::pref(conn, key, SAVES_SYNC).ok().flatten()?;
    serde_json::from_str(&json).ok()
}

/// What a sync leaves for the Saves tab. A conflict found before launch leaves nothing: nothing ran.
fn sync_record(outcome: &SyncOutcome, at: i64) -> Option<SyncRecord> {
    Some(match outcome {
        SyncOutcome::Synced => SyncRecord {
            at,
            error: None,
            cloud_newer: false,
        },
        // A GOG conflict changed nothing either: the next Play asks.
        SyncOutcome::CloudNewer | SyncOutcome::Conflict => SyncRecord {
            at,
            error: None,
            cloud_newer: true,
        },
        SyncOutcome::Failed(e) => SyncRecord {
            at,
            error: Some(e.clone().into()),
            cloud_newer: false,
        },
        SyncOutcome::LocalNewer => return None,
    })
}

fn record_sync(conn: Option<&Connection>, key: &str, outcome: &SyncOutcome) {
    if let SyncOutcome::Failed(e) = outcome {
        tracing::warn!("sync saves {key}: {e}");
    }
    let at = unix_now();
    let (Some(conn), Some(mut record)) = (conn, sync_record(outcome, at)) else {
        return;
    };
    // A failure changed nothing, so a newer cloud save left by the last upload still waits to be asked about.
    record.cloud_newer |=
        record.error.is_some() && last_sync(conn, key).is_some_and(|r| r.cloud_newer);
    let json = serde_json::to_string(&record).unwrap_or_default();
    if let Err(e) = cache::db::set_pref(conn, key, SAVES_SYNC, Some(&json)) {
        tracing::warn!("record sync {key}: {e}");
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

/// Syncs a copy's cloud saves with its store's tool: Legendary for Epic, gogdl for GOG.
fn sync_copy(conn: Option<&Connection>, game: &Game, sync: SaveSync) -> SyncOutcome {
    match (game.provider, installed_dir(game)) {
        (ProviderKind::Gog, Some(dir)) => gog::sync_saves(conn, &game.external_id, dir, sync),
        (ProviderKind::Gog, None) => SyncOutcome::Failed(msg!(notInstalled)),
        (ProviderKind::Ea, _) => SyncOutcome::Failed(msg!(eaNotYet)),
        _ => epic::sync_saves(&game.external_id, sync),
    }
}

/// Syncs an Epic or GOG game's cloud saves before it launches, when it has them and syncing is on for it.
/// Returns how to upload after the game exits (`None`: no syncing), or the conflict to ask about instead
/// of launching. A failed sync is remembered and never stops the launch.
fn saves_before_play(
    state: &AppState,
    game: &Game,
    keep: Option<Keep>,
) -> Result<Option<SaveSync>, Conflict> {
    let key = game.key();
    let conn = open_db(state);
    let store_default = |c: &Connection| {
        let saved = settings::load(c);
        if game.provider == ProviderKind::Gog {
            saved.gog_cloud_saves
        } else {
            saved.cloud_saves
        }
    };
    let enabled = conn
        .as_ref()
        .is_none_or(|c| saves_choice(c, &key).unwrap_or_else(|| store_default(c)));
    if !game.cloud_saves || !enabled {
        return Ok(None);
    }
    let cloud_newer = conn
        .as_ref()
        .and_then(|c| last_sync(c, &key))
        .is_some_and(|r| r.cloud_newer);
    let Some(download) = pre_launch_sync(keep, cloud_newer)? else {
        // Keep local: the upload afterwards sends this PC's save whatever the cloud holds.
        return Ok(Some(SaveSync::KeepLocal));
    };
    match sync_copy(conn.as_ref(), game, download) {
        SyncOutcome::LocalNewer => Err(Conflict::LocalNewer),
        // GOG: both sides changed since the last sync; Keep cloud or Keep local settles it.
        SyncOutcome::Conflict => Err(Conflict::CloudNewer),
        outcome => {
            record_sync(conn.as_ref(), &key, &outcome);
            Ok(Some(SaveSync::Upload))
        }
    }
}

/// Uploads a watched Epic or GOG game's saves once it has exited, off the poll.
pub(super) fn upload_saves(app: &AppHandle, key: String, sync: SaveSync) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let Some(game) = state.game(&key) else { return };
        let conn = open_db(&state);
        let outcome = sync_copy(conn.as_ref(), &game, sync);
        record_sync(conn.as_ref(), &key, &outcome);
    });
}

#[derive(Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavesView {
    /// The game's own choice; `None` follows the setting.
    choice: Option<bool>,
    /// Unix seconds of the last sync that worked; `None` when never synced or `error` is set.
    synced_at: Option<i64>,
    /// What went wrong with the last sync; the Overview repeats it.
    error: Option<Msg>,
}

fn saves_view(choice: Option<bool>, last: Option<SyncRecord>) -> SavesView {
    let (synced_at, error) = match last {
        None => (None, None),
        Some(SyncRecord { error: Some(e), .. }) => (None, Some(msg!(savesDidntSync, reason = e))),
        Some(SyncRecord {
            cloud_newer: true, ..
        }) => (None, Some(msg!(cloudSaveNewer))),
        Some(r) => (Some(r.at), None),
    };
    SavesView {
        choice,
        synced_at,
        error,
    }
}

/// The Saves tab of an Epic or GOG copy (`key`); `None` for a copy without cloud saves.
#[tauri::command]
#[specta::specta]
pub async fn saves(app: AppHandle, key: String) -> Option<SavesView> {
    app.state::<AppState>()
        .game(&key)
        .filter(|g| g.installed && g.cloud_saves)?;
    blocking(move || {
        let conn = open_db(&app.state::<AppState>())?;
        Some(saves_view(
            saves_choice(&conn, &key),
            last_sync(&conn, &key),
        ))
    })
    .await
}

/// Sets an Epic or GOG copy's own cloud save choice; `None` follows its store's setting again.
#[tauri::command]
#[specta::specta]
pub async fn set_saves(app: AppHandle, key: String, choice: Option<bool>) -> Res<()> {
    app.state::<AppState>()
        .game(&key)
        .filter(|g| g.cloud_saves)
        .ok_or(msg!(noCloudSaves))?;
    blocking(move || {
        let conn = open_db(&app.state::<AppState>()).ok_or(msg!(noDb))?;
        let value = choice.map(|on| if on { "on" } else { "off" });
        cache::db::set_pref(&conn, &key, SAVES_CHOICE, value).map_err(Msg::from)
    })
    .await
}

/// Sync now: the newer side wins. The outcome shows in the Saves tab, so this fails only before Legendary or gogdl runs.
#[tauri::command]
#[specta::specta]
pub async fn sync_saves_now(app: AppHandle, key: String) -> Res<()> {
    let game = app
        .state::<AppState>()
        .game(&key)
        .filter(|g| g.installed && g.cloud_saves)
        .ok_or(msg!(noCloudSaves))?;
    blocking(move || {
        let conn = open_db(&app.state::<AppState>());
        let outcome = sync_copy(conn.as_ref(), &game, SaveSync::Both);
        record_sync(conn.as_ref(), &key, &outcome);
        Ok(())
    })
    .await
}

/// How long a download size stays good; sizes change only with game updates.
pub(super) const SIZE_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;
const INFO_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;

/// Download size of a game that isn't installed, like `27.3 GB`, for the install popup and game page.
/// Epic asks Legendary and Steam asks its servers (slow, seconds), so the bytes are cached for a week. GOG's comes
/// from `gogdl info`, also cached for a week, with the owned DLC an install takes by default; Amazon's from
/// `nile install --info`, also cached for a week.
/// None if unknown, or for Steam when no account is connected.
#[tauri::command]
#[specta::specta]
pub async fn download_size(app: AppHandle, key: String) -> Option<u64> {
    let game = app
        .state::<AppState>()
        .game(&key)
        .filter(|g| !g.installed)?;
    let appid = match game.provider {
        ProviderKind::Epic => {
            return blocking(move || {
                super::install::epic_sizes(open_db(&app.state::<AppState>()).as_ref(), &game).0
            })
            .await;
        }
        ProviderKind::Steam => game.external_id.parse::<u32>().ok()?,
        ProviderKind::Gog => {
            let dlcs: Vec<String> = game.dlc.iter().map(|d| d.id.clone()).collect();
            return blocking(move || {
                let info = gog::install_info(
                    open_db(&app.state::<AppState>()).as_ref(),
                    &game.external_id,
                )?;
                gog::download_size(&info, &dlcs)
            })
            .await;
        }
        ProviderKind::Amazon => {
            return blocking(move || {
                amazon::download_size(
                    open_db(&app.state::<AppState>()).as_ref(),
                    &game.external_id,
                )
            })
            .await;
        }
        // shortcut: unknown until Maxima's `guild info` is read.
        ProviderKind::Ea | ProviderKind::Custom => return None,
    };
    let cached = blocking({
        let (app, key) = (app.clone(), key.clone());
        move || {
            let conn = open_db(&app.state::<AppState>());
            let fresh = conn.as_ref().and_then(|c| cached_size(c, &key));
            let token = match (&fresh, conn.as_ref()) {
                (None, Some(c)) => steam_token(c),
                _ => None,
            };
            (fresh, token)
        }
    })
    .await;
    let bytes = match cached {
        (Some(bytes), _) => bytes,
        (None, token) => {
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
                        .inspect_err(|e| tracing::warn!("steam size {key}: {e}"))
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
    };
    bytes.parse().ok()
}

/// The cached download size in bytes, if fresh.
fn cached_size(conn: &Connection, key: &str) -> Option<String> {
    cache::db::cached_info(conn, key, "size", SIZE_MAX_AGE_SECS, unix_now())
        .ok()
        .flatten()
}

/// The saved Steam token. Never connected: no reason to touch Credential Manager.
fn steam_token(conn: &Connection) -> Option<String> {
    cache::db::setting(conn, settings::STEAM_ACCOUNT)
        .ok()
        .flatten()
        .and_then(|_| settings::steam_token())
}

#[derive(Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename = "Dlc")] // its TypeScript name
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

#[derive(Serialize, specta::Type)]
pub struct SteamDlc {
    id: String,
    title: String,
}

/// The entry's Steam copy's owned DLC for the game page. Asked from Steam, which logs on, so only
/// with a connected account and when the store page lists DLC; cached a week, a failure for 6 h.
/// Failures are quiet and give an empty list.
#[tauri::command]
#[specta::specta]
pub async fn steam_dlc(app: AppHandle, key: String) -> Vec<SteamDlc> {
    let steam_key = {
        let state = app.state::<AppState>();
        let s = state.lock();
        entries::find(&s.entries, &s.games, &key).and_then(|e| {
            e.copies
                .iter()
                .map(|&i| &s.games[i])
                .find(|g| g.provider == ProviderKind::Steam)
                .map(Game::key)
        })
    };
    let Some(steam_key) = steam_key else {
        return Vec::new();
    };
    steam_owned_dlc(&app, steam_key)
        .await
        .into_iter()
        .map(|(id, title)| SteamDlc {
            id: id.to_string(),
            title,
        })
        .collect()
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
    let cached = |kind, max_age| {
        cache::db::cached_info(conn, key, kind, max_age, now)
            .ok()
            .flatten()
    };
    (
        cached(DLC, INFO_MAX_AGE_SECS),
        cached(DLC_FAILED, DLC_RETRY_SECS).is_some(),
    )
}

/// A Steam copy's owned DLC as (app id, name). Answers from the cache when it can; otherwise logs on,
/// one logon at a time, and remembers the answer or the failure.
async fn steam_owned_dlc(app: &AppHandle, key: String) -> Vec<(u32, String)> {
    let Some(appid) = key
        .strip_prefix("steam-")
        .and_then(|id| id.parse::<u32>().ok())
    else {
        return Vec::new();
    };
    // Everything that can answer is checked before the lock, so this never waits behind another logon for nothing.
    let (step, token) = blocking({
        let (app, key) = (app.clone(), key.clone());
        move || {
            let state = app.state::<AppState>();
            let Some(conn) = open_db(&state) else {
                return (DlcStep::Answer(Vec::new()), None);
            };
            let now = unix_now();
            let (cached, failed) = cached_dlc(&conn, &key, now);
            let has_dlc = (cached.is_none() && !failed)
                .then(|| {
                    parse_details(steam_details(
                        Some(&conn),
                        &state.http,
                        &appid.to_string(),
                        now,
                    ))
                })
                .flatten()
                .and_then(|i| i.has_dlc);
            let step = dlc_step(cached.as_deref(), failed, has_dlc);
            let token = (step == DlcStep::LogOn)
                .then(|| steam_token(&conn))
                .flatten();
            (step, token)
        }
    })
    .await;
    if let DlcStep::Answer(list) = step {
        return list;
    }
    let Some(token) = token else {
        return Vec::new();
    };
    let state = app.state::<AppState>();
    let _one = state.steam_size.lock().await;
    // A caller that queued on the lock may find its answer cached by the one before it.
    let again = blocking({
        let (app, key) = (app.clone(), key.clone());
        move || {
            let Some(conn) = open_db(&app.state::<AppState>()) else {
                return DlcStep::Answer(Vec::new());
            };
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
        Ok(list) => (
            DLC,
            serde_json::to_string(list).unwrap_or_else(|_| "[]".into()),
        ),
        Err(e) => {
            tracing::warn!("steam dlc {key}: {e}");
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
fn steam_details(
    conn: Option<&Connection>,
    http: &reqwest::Client,
    appid: &str,
    now: i64,
) -> Option<String> {
    let key = format!("steam-{appid}");
    info::cached_or_fetch(conn, &key, info::DETAILS, INFO_MAX_AGE_SECS, now, || {
        let url = format!("https://store.steampowered.com/api/appdetails?appids={appid}&l=english");
        let body = tauri::async_runtime::block_on(async {
            http.get(url).send().await?.error_for_status()?.text().await
        })
        .inspect_err(|e| tracing::warn!("appdetails {key}: {e}"))
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
/// gaps filled from its Steam twin's page, else from GOG GamesDB (30 days, 7 for a miss). A GOG or EA game
/// starts from GamesDB and has its gaps filled from its Steam twin's page. An Amazon game starts from Nile's `library.json`
/// and has its gaps filled like an Epic game's.
/// Screenshots are remote URLs; `screenshots` turns them into local files.
#[tauri::command]
#[specta::specta]
pub async fn game_info(app: AppHandle, key: String) -> Option<GameInfo> {
    // A game the owner added has no store to ask.
    let game = app
        .state::<AppState>()
        .game(&key)
        .filter(|g| g.provider != ProviderKind::Custom)?;
    let twin = {
        let state = app.state::<AppState>();
        let s = state.lock();
        entries::find(&s.entries, &s.games, &key)
            .and_then(|e| entries::steam_appid(e, &s.games, &s.twins))
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
            let gamesdb = || {
                let json = info::gamesdb_cached_or_fetch(conn.as_ref(), &key, now, || {
                    let body = tauri::async_runtime::block_on(gamesdb::fetch(&http, &key))
                        .inspect_err(|e| tracing::warn!("gamesdb details {key}: {e}"))
                        .ok()?;
                    serde_json::to_string(&body.and_then(|b| info::parse_gamesdb_info(&b))).ok()
                });
                parse_details(json)
            };
            let steam_twin = || {
                twin.and_then(|id| {
                    parse_details(steam_details(conn.as_ref(), &http, &id.to_string(), now))
                })
            };
            if matches!(game.provider, ProviderKind::Gog | ProviderKind::Ea) {
                let all = match (gamesdb(), steam_twin()) {
                    (Some(base), Some(extra)) => info::fill_missing(base, extra),
                    (base, extra) => base.or(extra).unwrap_or_default(),
                };
                return (all != GameInfo::default()).then_some(all);
            }
            if game.provider == ProviderKind::Amazon {
                let base = amazon::details(&game.external_id).unwrap_or_default();
                let all = steam_twin()
                    .or_else(gamesdb)
                    .map_or(base.clone(), |extra| info::fill_missing(base, extra));
                return (all != GameInfo::default()).then_some(all);
            }
            // One list answers for every Epic game, so all of them are cached from it.
            let catalog = info::cached_or_run_tool(
                conn.as_ref(),
                &key,
                info::DETAILS,
                INFO_MAX_AGE_SECS,
                now,
                || {
                    let list = epic::catalog()
                        .inspect_err(|e| tracing::warn!("legendary list {key}: {e}"))
                        .ok()?;
                    let all = info::epic_details(&list);
                    if let Some(c) = &conn
                        && let Err(e) = info::remember_all(c, &all, now)
                    {
                        tracing::warn!("epic details cache: {e}");
                    }
                    Some(
                        all.into_iter()
                            .find(|(k, _)| *k == key)
                            .map_or_else(|| "null".into(), |(_, json)| json),
                    )
                },
            );
            let base = parse_details(catalog).unwrap_or_default();
            // A twin whose store page is gone still has GamesDB.
            let extra = steam_twin().or_else(gamesdb);
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
#[specta::specta]
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
            let image = Image {
                path: None,
                url: Some(url),
            };
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

#[derive(Serialize, specta::Type)]
#[serde(rename = "TimeToBeat")] // its TypeScript name
#[serde(rename_all = "camelCase")]
/// Seconds; `None` when unknown.
pub struct TimeToBeatView {
    normally: Option<u64>,
    hastily: Option<u64>,
    completely: Option<u64>,
}

fn time_to_beat_view(t: igdb::TimeToBeat) -> Option<TimeToBeatView> {
    let known = |secs: Option<u64>| secs.filter(|&s| s > 0);
    let shown = TimeToBeatView {
        normally: known(t.normally),
        hastily: known(t.hastily),
        completely: known(t.completely),
    };
    (shown.normally.is_some() || shown.hastily.is_some() || shown.completely.is_some())
        .then_some(shown)
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
                .inspect_err(|e| tracing::warn!("igdb epic url: {e}"))
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
#[specta::specta]
pub async fn time_to_beat(app: AppHandle, key: String) -> Option<TimeToBeatView> {
    let state = app.state::<AppState>();
    let (appid, title, epic_key) = {
        let s = state.lock();
        let entry = entries::find(&s.entries, &s.games, &key)?;
        let epic = entry
            .copies
            .iter()
            .map(|&i| &s.games[i])
            .find(|g| g.provider == ProviderKind::Epic);
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
    let answer = match look_up_time_to_beat(
        &app,
        &state,
        &client_id,
        &secret,
        appid,
        &title,
        epic_key.as_deref(),
    )
    .await
    {
        Ok(answer) => answer,
        Err(e) => {
            tracing::warn!("how long to beat {key}: {e}");
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
#[specta::specta]
pub async fn test_igdb(app: AppHandle) -> Res<()> {
    let state = app.state::<AppState>();
    let (client_id, secret) = {
        let app = app.clone();
        blocking(move || {
            let conn = cache::db::open(&app.state::<AppState>().dirs.db)?;
            let client_id = settings::load(&conn)
                .igdb_client_id
                .ok_or(msg!(enterClientId))?;
            let secret = settings::secret().ok_or(msg!(saveClientSecret))?;
            Ok::<_, Msg>((client_id, secret))
        })
        .await?
    };
    let token = igdb::token(&state.http, &client_id, &secret).await?;
    state
        .igdb
        .lock()
        .expect("token cache lock poisoned")
        .store(token.clone());
    igdb::game_id_for_steam(&state.http, &token, &client_id, 447040).await?;
    Ok(())
}

/// Remembers a launch from here, for every store, so last played survives rescans.
fn record_play(state: &AppState, key: &str) {
    let secs = unix_now();
    if let Some(g) = state.lock().games.iter_mut().find(|g| g.key() == key) {
        g.last_played = Some(UNIX_EPOCH + Duration::from_secs(secs as u64));
    }
    let saved =
        cache::db::open(&state.dirs.db).and_then(|c| cache::db::record_launch(&c, key, secs));
    if let Err(e) = saved {
        tracing::warn!("record launch: {e}");
    }
}

/// Cancels a game's queued job: the active one is stopped (Legendary and gogdl resume it next time) and
/// leaves the queue when its process ends; a waiting or paused one leaves the queue at once.
/// Returns the queue as it is now.
#[tauri::command]
#[specta::specta]
pub async fn cancel_install(app: AppHandle, key: String) -> Res<QueueView> {
    let pid = {
        let state = app.state::<AppState>();
        let mut s = state.lock();
        if !s.queue.contains(&key) {
            return Err(msg!(notInQueue));
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
        blocking(move || {
            kill_tree(pid).map_err(|e| msg!(cantCancelInstall, detail = e.to_string()))
        })
        .await?;
    }
    Ok(queue_view(&app.state::<AppState>().lock()))
}

/// Pauses the whole queue: the active job's Legendary or gogdl is stopped (both resume partial downloads) and
/// the job stays at the head.
#[tauri::command]
#[specta::specta]
pub async fn pause_downloads(app: AppHandle) -> Res<QueueView> {
    let pid = {
        let state = app.state::<AppState>();
        let mut s = state.lock();
        let pid = pause_head(&mut s);
        save_queue(&state, &s.queue);
        pid
    };
    if let Some(pid) = pid {
        blocking(move || {
            kill_tree(pid).map_err(|e| msg!(cantPauseDownload, detail = e.to_string()))
        })
        .await?;
    }
    Ok(queue_view(&app.state::<AppState>().lock()))
}

/// Pauses the queue. Returns the pid of the active job's process, for the caller to stop.
fn pause_head(s: &mut Shared) -> Option<u32> {
    s.queue.pause();
    let head = s
        .queue
        .jobs
        .first()
        .map(|j| j.key.clone())
        .filter(|_| s.queue_running);
    let pid = head.and_then(|k| s.install_pids.get(&k).copied());
    s.interrupted |= pid.is_some();
    pid
}

/// Stops the queue before the app exits for an update: the active job's process is stopped (it continues the
/// partial download next time) and the queue is stored with its own paused flag, so the next start's
/// `restore_queue` resumes it. Returns whether the queue was already paused.
pub(super) fn stop_queue_for_restart(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let (was_paused, pid) = {
        let mut s = state.lock();
        (s.queue.paused, pause_head(&mut s))
    };
    if let Some(pid) = pid
        && let Err(e) = kill_tree(pid)
    {
        tracing::warn!("stop download for update: {e}");
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

/// On a normal exit (tray Quit, or the window closed while close-to-tray is off): stops a running download as an
/// update restart does, so the next start resumes it rather than racing a tool still at work. Skipped without a
/// runner, which also keeps what `install_update` stored.
pub(crate) fn stop_queue_on_exit(app: &AppHandle) {
    if app.state::<AppState>().lock().queue_running {
        stop_queue_for_restart(app);
    }
}

/// The queue as stored across an update restart. The live queue stays paused, so the runner can't start
/// the next job before the app exits.
fn stored_for_restart(queue: &Queue, was_paused: bool) -> Queue {
    Queue {
        paused: was_paused,
        ..queue.clone()
    }
}

/// Starts the head job again with the same command.
#[tauri::command]
#[specta::specta]
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
#[specta::specta]
pub fn move_download(state: State<'_, AppState>, key: String, up: bool) -> QueueView {
    let mut s = state.lock();
    let moved = if up {
        s.queue.move_up(&key)
    } else {
        s.queue.move_down(&key)
    };
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
    fn verify_and_move_are_not_offered_for_ea() {
        for action in [Action::Verify, Action::Move] {
            assert_eq!(ea_refusal(action), Some(msg!(eaCantVerifyOrMove)));
        }
        for action in [
            Action::Play,
            Action::PlayOutdated,
            Action::Install,
            Action::Update,
            Action::Uninstall,
        ] {
            assert_eq!(ea_refusal(action), None);
        }
    }

    #[test]
    fn restart_keeps_live_queue_paused_and_stores_its_own_flag() {
        let mut s = Shared {
            queue_running: true,
            ..Shared::default()
        };
        s.queue.push(Job::new("epic-a", QueuedAction::Install));
        s.install_pids.insert("epic-a".into(), 42);
        assert_eq!(pause_head(&mut s), Some(42));
        assert!(s.queue.paused && s.interrupted);

        let stored = Queue::restore(Some(&stored_for_restart(&s.queue, false).to_json()), |_| {
            true
        });
        assert!(!stored.paused, "a running queue resumes after the restart");
        assert_eq!(stored.jobs, s.queue.jobs);
        assert!(
            s.queue.paused,
            "the live queue stays paused until the app exits"
        );
        assert!(
            Queue::restore(Some(&stored_for_restart(&s.queue, true).to_json()), |_| {
                true
            })
            .paused
        );
    }

    /// A runner working on `keys`, in order, with a waiter on each job.
    fn running(keys: &[&str]) -> (Shared, Vec<std::sync::mpsc::Receiver<Res<()>>>) {
        let mut s = Shared {
            queue_running: true,
            ..Shared::default()
        };
        let rxs = keys
            .iter()
            .map(|k| {
                s.queue.push(Job::new(k, QueuedAction::Install));
                let (tx, rx) = std::sync::mpsc::channel();
                s.waiters.insert(k.to_string(), tx);
                rx
            })
            .collect();
        (s, rxs)
    }

    fn failed() -> Result<(), crate::library::Error> {
        Err(crate::library::Error::Text("stopped".into()))
    }

    #[test]
    fn cancel_before_the_process_starts_stops_it_when_it_does() {
        let (mut s, rxs) = running(&["epic-a", "epic-b"]);
        s.cancelled.insert("epic-a".into());
        assert_eq!(
            record_event(&mut s, "epic-a", InstallEvent::Started(7)),
            Some(7)
        );
        assert!(!s.interrupted, "a cancelled job isn't interrupted");
        assert!(!job_paused(&mut s, "epic-a", true));
        finish_job(&mut s, "epic-a", failed(), ProviderKind::Epic);
        assert!(rxs[0].recv().unwrap().is_ok(), "a cancel isn't an error");
        assert!(s.cancelled.is_empty());
        assert_eq!(s.queue.next().map(|j| j.key.as_str()), Some("epic-b"));
    }

    #[test]
    fn a_running_job_starts_unhindered() {
        let (mut s, _rxs) = running(&["epic-a"]);
        assert_eq!(
            record_event(&mut s, "epic-a", InstallEvent::Started(7)),
            None
        );
        assert_eq!(s.install_pids.get("epic-a"), Some(&7));
        assert!(!s.interrupted);
    }

    #[test]
    fn pause_keeps_the_job_at_the_head_until_resume() {
        let (mut s, rxs) = running(&["epic-a", "epic-b"]);
        record_event(&mut s, "epic-a", InstallEvent::Started(7));
        assert_eq!(pause_head(&mut s), Some(7));
        assert!(s.interrupted);
        assert!(job_paused(&mut s, "epic-a", true));
        assert!(!s.interrupted && s.install_pids.is_empty());
        assert!(s.queue.is_head("epic-a") && s.queue.next().is_none());
        assert!(
            rxs[0].try_recv().is_err(),
            "a paused job has no outcome yet"
        );

        s.queue.resume();
        assert_eq!(s.queue.next().map(|j| j.key.as_str()), Some("epic-a"));
        assert_eq!(
            record_event(&mut s, "epic-a", InstallEvent::Started(8)),
            None
        );
        assert!(!job_paused(&mut s, "epic-a", false));
    }

    #[test]
    fn pause_before_the_process_starts_stops_it_when_it_does() {
        let (mut s, _rxs) = running(&["epic-a"]);
        assert_eq!(pause_head(&mut s), None);
        assert!(!s.interrupted, "nothing to stop yet");
        assert_eq!(
            record_event(&mut s, "epic-a", InstallEvent::Started(7)),
            Some(7)
        );
        assert!(s.interrupted);
        assert!(job_paused(&mut s, "epic-a", true));
        assert!(s.queue.is_head("epic-a"));
    }

    #[test]
    fn a_cancelled_job_is_never_paused() {
        let (mut s, _rxs) = running(&["epic-a"]);
        record_event(&mut s, "epic-a", InstallEvent::Started(7));
        s.cancelled.insert("epic-a".into());
        assert_eq!(pause_head(&mut s), Some(7));
        assert!(!job_paused(&mut s, "epic-a", true));
        finish_job(&mut s, "epic-a", failed(), ProviderKind::Epic);
        assert!(s.queue.jobs.is_empty());
    }

    #[test]
    fn a_failed_job_leaves_the_queue_and_the_next_starts() {
        let (mut s, rxs) = running(&["epic-a", "epic-b"]);
        record_event(&mut s, "epic-a", InstallEvent::Started(7));
        assert!(!job_paused(&mut s, "epic-a", true));
        let (toast, repaired) = finish_job(&mut s, "epic-a", failed(), ProviderKind::Epic);
        assert_eq!((toast, repaired), (None, false));
        assert!(rxs[0].recv().unwrap().is_err());
        assert!(s.library_changed);
        assert_eq!(s.queue.next().map(|j| j.key.as_str()), Some("epic-b"));
    }

    #[test]
    fn a_done_job_keeps_its_progress_for_the_rescan_and_a_failed_one_drops_it() {
        let (mut s, _rxs) = running(&["epic-a", "epic-b"]);
        for id in ["a", "b"] {
            s.games.push(Game {
                provider: ProviderKind::Epic,
                external_id: id.into(),
                ..game("June", false, None, None)
            });
        }
        let progress = crate::library::Progress {
            fraction: 1.0,
            action: ProgressAction::Install,
            done_bytes: None,
            total_bytes: None,
            secs_left: None,
            done_files: None,
            total_files: None,
        };
        s.progress.insert("epic-a".into(), progress.clone());
        s.progress.insert("epic-b".into(), progress);
        let (toast, _) = finish_job(&mut s, "epic-a", Ok(()), ProviderKind::Epic);
        assert_eq!(toast.as_deref(), Some("June"));
        assert!(s.progress.contains_key("epic-a"));
        finish_job(&mut s, "epic-b", failed(), ProviderKind::Epic);
        assert!(!s.progress.contains_key("epic-b"));
    }

    fn watch(key: &str, upload: Option<SaveSync>) -> Watch {
        Watch {
            key: key.into(),
            process: Process::Named("game.exe".into()),
            since: Instant::now(),
            seen: false,
            upload,
        }
    }

    #[test]
    fn play_never_drops_another_games_save_upload() {
        let uploading = watch("gog-1", Some(SaveSync::Upload));
        assert!(replaces_upload(Some(&uploading), "epic-2"));
        assert!(!replaces_upload(Some(&uploading), "gog-1"));
        assert!(!replaces_upload(Some(&watch("custom-a", None)), "epic-2"));
        assert!(!replaces_upload(None, "epic-2"));
    }

    #[test]
    fn uninstall_waits_while_any_copy_of_the_card_runs() {
        let steam = Game {
            external_id: "620".into(),
            ..game("Portal 2", true, None, None)
        };
        let epic = Game {
            provider: ProviderKind::Epic,
            external_id: "x".into(),
            ..game("Portal 2", true, None, None)
        };
        let other = Game {
            external_id: "70".into(),
            ..game("Half-Life", true, None, None)
        };
        let mut s = Shared {
            games: vec![other, steam, epic],
            twin_lists: HashMap::from([("epic-x".to_string(), vec![620])]),
            ..Shared::default()
        };
        super::super::regroup(&mut s);
        assert!(card_running(&s, "epic-x", Some("steam-620")));
        assert!(!card_running(&s, "steam-70", Some("steam-620")));
        assert!(!card_running(&s, "epic-x", None));
        s.watch = Some(watch("epic-x", None));
        assert!(card_running(&s, "steam-620", None));
        assert!(!card_running(&s, "steam-70", None));
    }

    #[test]
    fn launch_options_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let conn = cache::db::open(&dir.join("library.sqlite")).unwrap();
        assert_eq!(launch_options_of(&conn, "epic-A"), LaunchOptions::default());
        let options = LaunchOptions {
            args: "-dx11".into(),
            offline: true,
            skip_version_check: false,
            exe: Some(r"bin\game.exe".into()),
            proton: Some("GE-Proton11-12".into()),
        };
        store_launch_options(&conn, "epic-A", &options).unwrap();
        assert_eq!(launch_options_of(&conn, "epic-A"), options);
        assert_eq!(launch_options_of(&conn, "epic-B"), LaunchOptions::default());
        let json = cache::db::pref(&conn, "epic-A", LAUNCH_OPTIONS)
            .unwrap()
            .unwrap();
        assert_eq!(
            json,
            r#"{"args":"-dx11","offline":true,"skipVersionCheck":false,"exe":"bin\\game.exe","proton":"GE-Proton11-12"}"#
        );
        store_launch_options(&conn, "epic-A", &LaunchOptions::default()).unwrap();
        assert_eq!(
            cache::db::pref(&conn, "epic-A", LAUNCH_OPTIONS).unwrap(),
            None
        );
    }

    fn pumpkin(steam_last: Option<u64>, epic_last: Option<u64>) -> Vec<Game> {
        let steam = Game {
            external_id: "635320".into(),
            ..game("Pumpkin Jack", true, Some(120), steam_last)
        };
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
        games[0].dlc = vec![crate::library::Dlc {
            id: "PumpkinDlc".into(),
            title: "Pumpkin Hat".into(),
            installed: true,
        }];
        let rows = epic_dlc(&games.iter().collect::<Vec<_>>());
        assert_eq!(
            rows,
            [DlcView {
                id: "PumpkinDlc".into(),
                title: "Pumpkin Hat".into(),
                installed: true,
                copy: Some("epic-Pumpkin".into())
            }]
        );
    }

    #[test]
    fn steam_dlc_logs_on_only_when_nothing_answers() {
        let list = r#"[[10,"Hat"]]"#;
        assert_eq!(
            dlc_step(Some(list), true, Some(false)),
            DlcStep::Answer(vec![(10, "Hat".into())])
        );
        // A recent failure or a store page without DLC answers "none" without a logon.
        assert_eq!(
            dlc_step(None, true, Some(true)),
            DlcStep::Answer(Vec::new())
        );
        assert_eq!(
            dlc_step(None, false, Some(false)),
            DlcStep::Answer(Vec::new())
        );
        // Unknown or listed DLC, nothing cached: log on. A broken cache entry counts as nothing cached.
        assert_eq!(dlc_step(None, false, None), DlcStep::LogOn);
        assert_eq!(dlc_step(Some("junk"), false, Some(true)), DlcStep::LogOn);
    }

    #[test]
    fn steam_dlc_queued_caller_reads_what_the_first_cached() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.sqlite");
        let conn = cache::db::open(&path).unwrap();
        // Before the lock: nothing yet, so it would log on.
        let (cached, failed) = cached_dlc(&conn, "steam-1", 1_000);
        assert_eq!(dlc_step(cached.as_deref(), failed, None), DlcStep::LogOn);
        // After the lock: the first caller's answer is there.
        cache::db::put_info(&conn, "steam-1", DLC, r#"[[10,"Hat"]]"#, 1_000).unwrap();
        let (cached, failed) = cached_dlc(&conn, "steam-1", 1_001);
        assert_eq!(
            dlc_step(cached.as_deref(), failed, None),
            DlcStep::Answer(vec![(10, "Hat".into())])
        );
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
        assert_eq!(
            pre_launch_sync(Some(Keep::Cloud), true),
            Ok(Some(SaveSync::KeepCloud))
        );
        assert_eq!(pre_launch_sync(Some(Keep::Local), false), Ok(None));
    }

    #[test]
    fn last_sync_is_owner_data_and_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let conn = cache::db::open(&tmp.path().join("t.sqlite")).unwrap();
        record_sync(Some(&conn), "epic-a", &SyncOutcome::CloudNewer);
        // A failure keeps the newer cloud save waiting.
        record_sync(
            Some(&conn),
            "epic-a",
            &SyncOutcome::Failed(msg!(savesTimedOut)),
        );
        let last = last_sync(&conn, "epic-a").unwrap();
        assert!(last.cloud_newer && last.error.is_some());
        let in_info: i64 = conn
            .query_row("SELECT count(*) FROM info", [], |r| r.get(0))
            .unwrap();
        assert_eq!(in_info, 0);
    }

    #[test]
    fn sync_records_show_in_the_saves_tab() {
        let shown = |outcome: SyncOutcome| saves_view(None, sync_record(&outcome, 1_000));
        assert_eq!(
            shown(SyncOutcome::Synced),
            SavesView {
                choice: None,
                synced_at: Some(1_000),
                error: None
            }
        );
        let failed = shown(SyncOutcome::Failed(Msg::system("Login failed!"))).error;
        assert_eq!(
            failed,
            Some(msg!(savesDidntSync, reason = Msg::system("Login failed!")))
        );
        // Stored with its code, so it is shown in the app's language.
        let json = serde_json::to_string(
            &sync_record(&SyncOutcome::Failed(msg!(savesTimedOut)), 7).unwrap(),
        )
        .unwrap();
        assert_eq!(
            json,
            r#"{"at":7,"error":{"code":"savesTimedOut","params":{}}}"#
        );
        assert_eq!(
            serde_json::from_str::<SyncRecord>(&json).unwrap().error,
            Some(Param::Msg(msg!(savesTimedOut)))
        );
        // A row stored before message codes holds English; it is shown as is.
        let old: SyncRecord = serde_json::from_str(r#"{"at":7,"error":"Login failed!"}"#).unwrap();
        assert_eq!(
            saves_view(None, Some(old)).error,
            Some(msg!(savesDidntSync, reason = "Login failed!"))
        );
        assert!(shown(SyncOutcome::CloudNewer).error.is_some());
        assert_eq!(sync_record(&SyncOutcome::LocalNewer, 1_000), None);
        // A GOG conflict changed nothing: the next Play asks, as after a newer cloud save.
        assert!(
            sync_record(&SyncOutcome::Conflict, 1_000)
                .unwrap()
                .cloud_newer
        );
        assert_eq!(
            saves_view(Some(false), None),
            SavesView {
                choice: Some(false),
                synced_at: None,
                error: None
            }
        );
        // Stored as JSON in the info table and read back.
        let json =
            serde_json::to_string(&sync_record(&SyncOutcome::CloudNewer, 7).unwrap()).unwrap();
        assert_eq!(json, r#"{"at":7,"cloud_newer":true}"#);
        assert_eq!(
            serde_json::from_str::<SyncRecord>(r#"{"at":7}"#).unwrap(),
            SyncRecord {
                at: 7,
                error: None,
                cloud_newer: false
            }
        );
    }

    #[test]
    fn detail_for_a_merged_entry() {
        let games = pumpkin(None, None);
        let d = pumpkin_detail(
            &games,
            &HashMap::from([("epic-Pumpkin".to_string(), 635320)]),
        );
        assert_eq!(d.key, "steam-635320");
        assert_eq!(d.chosen, "steam-635320");
        assert_eq!(d.copies.len(), 2);
        assert_eq!(d.playtime_minutes, Some(120));
        assert!(d.playtime_stores.is_empty());
        assert_eq!(d.links.len(), 5);
    }

    #[test]
    fn last_played_store_only_for_merged_entries() {
        let games = pumpkin(Some(86_400), Some(2 * 86_400));
        let merged = pumpkin_detail(
            &games,
            &HashMap::from([("epic-Pumpkin".to_string(), 635320)]),
        );
        assert_eq!(merged.last_played_store, Some(ProviderKind::Epic));
        assert_eq!(merged.last_played, Some(2 * 86_400));
        let single = pumpkin_detail(&games, &HashMap::new());
        assert_eq!(single.last_played_store, None);
    }

    #[test]
    fn detail_payload_snapshot() {
        let mut games = pumpkin(Some(86_400), Some(2 * 86_400));
        games[1].install_dir = Some(r"C:\Games\Pumpkin Jack".into());
        games[1].size_bytes = Some(4 << 30);
        games[0].dlc = vec![crate::library::Dlc {
            id: "PumpkinDlc".into(),
            title: "Pumpkin Hat".into(),
            installed: false,
        }];
        let detail = pumpkin_detail(
            &games,
            &HashMap::from([("epic-Pumpkin".to_string(), 635320)]),
        );
        // Paths and times stand in as placeholders; a missing one stays null.
        let placeholder = |text: &'static str| {
            insta::dynamic_redaction(
                move |value, _| if value.is_nil() { value } else { text.into() },
            )
        };
        insta::assert_json_snapshot!(detail, { ".copies[].installDir" => placeholder("[path]"), ".lastPlayed" => placeholder("[time]") });
    }
}
