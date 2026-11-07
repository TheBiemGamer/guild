//! One game: its page, art, details, play/install/uninstall, download size, how long to beat.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use super::library::rescan;
use super::{allow, blocking, open_db, unix_now, AppState, EpicWatch, Res};
use crate::cache;
use crate::entries::{self, Entry};
use crate::gamesdb;
use crate::igdb;
use crate::info::{self, GameInfo};
use crate::library::epic::{self, InstallEvent, SaveSync, SyncOutcome};
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
    /// Tile text, like `15 h 3 min`; `None` hides the tile (Epic doesn't report playtime).
    playtime_tile: Option<String>,
    /// `Steam + Epic` when both copies contributed to the playtime.
    playtime_note: Option<String>,
    /// Tile text, like `12 days ago`; `None` when never played.
    last_played_tile: Option<String>,
    /// `on Steam` / `on Epic Games`, only for an entry with several copies.
    last_played_store: Option<String>,
    /// Store pages, then info sites when the Steam app id is known.
    links: Vec<LinkKind>,
    /// The Epic copies' DLC; a Steam copy's comes from `steam_dlc`.
    dlc: Vec<DlcView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyView {
    key: String,
    provider: ProviderKind,
    installed: bool,
    /// An installed Epic copy has a newer version.
    update_available: bool,
    /// Size on disk of an installed copy, like `27.3 GB`.
    size: Option<String>,
    /// Only for an installed copy (Steam knows a folder for games it hasn't installed yet).
    install_dir: Option<String>,
    /// An installed Epic copy that keeps its saves in Epic's cloud.
    cloud_saves: bool,
}

#[tauri::command]
pub fn game(state: State<'_, AppState>, key: String) -> Option<Detail> {
    let s = state.lock();
    let entry = entries::find(&s.entries, &s.games, &key)?;
    Some(detail(entry, &s.games, &s.twins, SystemTime::now()))
}

fn detail(entry: &Entry, games: &[Game], twins: &HashMap<String, u32>, now: SystemTime) -> Detail {
    let (minutes, both) = entries::playtime(entry, games);
    // Steam's zero is a fact, so a Steam copy decides the tile; Epic alone has no playtime to show.
    let tile_provider = entry
        .copies
        .iter()
        .map(|&i| games[i].provider)
        .find(|&p| p == ProviderKind::Steam)
        .unwrap_or(games[entries::default_copy(entry, games)].provider);
    let last = entries::last_played(entry, games);
    let stores = entries::stores(entry, games);
    Detail {
        key: entry.key.clone(),
        title: games[entry.copies[0]].title.clone(),
        stores: stores.clone(),
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
                    size: g.size_bytes.filter(|_| g.installed).map(view::size_label),
                    install_dir: installed_dir(g).map(|p| p.to_string_lossy().into_owned()),
                    cloud_saves: g.installed && g.cloud_saves,
                }
            })
            .collect(),
        chosen: games[entries::default_copy(entry, games)].key(),
        playtime_tile: view::playtime_tile(tile_provider, minutes),
        playtime_note: both.then(|| "Steam + Epic".to_string()),
        last_played_tile: view::last_played_tile(last.map(|(t, _)| t), now),
        last_played_store: last.filter(|_| entry.copies.len() > 1).map(|(_, p)| format!("on {}", p.name())),
        links: links::kinds(&stores, entries::steam_appid(entry, games, twins)),
        dlc: epic_dlc(&entry.copies.iter().map(|&i| &games[i]).collect::<Vec<_>>()),
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
    let result = match action {
        Action::Play | Action::PlayOutdated => {
            let upload = match saves_before_play(&state, game, keep) {
                Ok(upload) => upload,
                Err(conflict) => return Ok(Some(conflict)),
            };
            epic::launch(&game.external_id, action == Action::PlayOutdated).map(|()| {
                record_play(&state, &key);
                // The game started either way; without its exe name it just isn't shown as running.
                // shortcut: unwatched, its saves aren't uploaded either; the next Play asks about the newer local save.
                let exe = epic::executable(game).unwrap_or_else(|e| {
                    eprintln!("legendary list-installed {key}: {e}");
                    None
                });
                state.lock().epic_watch = exe.map(|exe| EpicWatch { key: key.clone(), exe, since: Instant::now(), upload });
            })
        }
        Action::Install | Action::Update | Action::Verify | Action::Move => {
            let on_event = |event| {
                let mut s = state.lock();
                match event {
                    InstallEvent::Started(pid) => _ = s.install_pids.insert(key.clone(), pid),
                    InstallEvent::Progress(p) => _ = s.progress.insert(key.clone(), p),
                }
            };
            let result = match action {
                Action::Update => epic::update(&game.external_id, on_event),
                Action::Verify => epic::verify(&game.external_id, on_event),
                Action::Move => match dest {
                    Some(dest) => epic::move_to(&game.external_id, dest, on_event),
                    None => Err("Pick a folder to move the game to.".into()),
                },
                _ => {
                    let base = open_db(&state).map(|c| settings::load(&c).epic_install_dir).unwrap_or_default();
                    epic::install(dlc.unwrap_or(&game.external_id), with_dlc, base.as_deref(), on_event)
                }
            };
            let mut s = state.lock();
            s.install_pids.remove(&key);
            s.progress.remove(&key);
            // A cancelled install fails by design; don't report it.
            if s.cancelled.remove(&key) { Ok(()) } else { result }
        }
        Action::Uninstall => epic::uninstall(dlc.unwrap_or(&game.external_id)),
    };
    if !matches!(action, Action::Play | Action::PlayOutdated) {
        rescan(app, true);
    }
    result.map(|()| None).map_err(|e| format!("Epic Games: {e}"))
}

/// Info cache kind for an Epic copy's own cloud save choice, `on` or `off`; anything else follows the setting.
const SAVES_CHOICE: &str = "cloud-saves";
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
    match cache::db::cached_info(conn, key, SAVES_CHOICE, FOREVER, unix_now()).ok().flatten()?.as_str() {
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
        eprintln!("legendary sync-saves {key}: {e}");
    }
    let at = unix_now();
    let (Some(conn), Some(mut record)) = (conn, sync_record(outcome, at)) else { return };
    // A failure changed nothing, so a newer cloud save left by the last upload still waits to be asked about.
    record.cloud_newer |= record.error.is_some() && last_sync(conn, key).is_some_and(|r| r.cloud_newer);
    let json = serde_json::to_string(&record).unwrap_or_default();
    if let Err(e) = cache::db::put_info(conn, key, SAVES_SYNC, &json, at) {
        eprintln!("record sync {key}: {e}");
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
    /// `Synced 5 min ago`, `Not synced yet`, or what went wrong.
    last: String,
    /// `last` says what went wrong; the Overview repeats it.
    failed: bool,
}

fn saves_view(choice: Option<bool>, last: Option<SyncRecord>, now: i64) -> SavesView {
    let (last, failed) = match last {
        None => ("Not synced yet".to_string(), false),
        Some(SyncRecord { error: Some(e), .. }) => (format!("Cloud saves didn't sync: {e}"), true),
        Some(SyncRecord { cloud_newer: true, .. }) => {
            ("The cloud save is newer, so this PC's save wasn't uploaded.".to_string(), true)
        }
        Some(r) => (format!("Synced {}", view::ago_label(now - r.at)), false),
    };
    SavesView { choice, last, failed }
}

/// The Saves tab of an Epic copy (`key`); `None` for a copy without cloud saves.
#[tauri::command]
pub async fn saves(app: AppHandle, key: String) -> Option<SavesView> {
    app.state::<AppState>().game(&key).filter(|g| g.installed && g.cloud_saves)?;
    blocking(move || {
        let conn = open_db(&app.state::<AppState>())?;
        Some(saves_view(saves_choice(&conn, &key), last_sync(&conn, &key), unix_now()))
    })
    .await
}

/// Sets an Epic copy's own cloud save choice; `None` follows the setting again.
#[tauri::command]
pub async fn set_saves(app: AppHandle, key: String, choice: Option<bool>) -> Res<()> {
    app.state::<AppState>().game(&key).filter(|g| g.cloud_saves).ok_or("This game has no cloud saves.")?;
    blocking(move || {
        let conn = open_db(&app.state::<AppState>()).ok_or("The settings database can't be opened.")?;
        let value = match choice {
            Some(true) => "on",
            Some(false) => "off",
            None => "default",
        };
        cache::db::put_info(&conn, &key, SAVES_CHOICE, value, unix_now()).map_err(|e| e.to_string())
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
pub async fn download_size(app: AppHandle, key: String) -> Option<String> {
    let game = app
        .state::<AppState>()
        .game(&key)
        .filter(|g| !g.installed)?;
    let steam_id = match game.provider {
        ProviderKind::Epic => None,
        ProviderKind::Steam => Some(game.external_id.parse::<u32>().ok()?),
    };
    let cached = blocking({
        let (app, key) = (app.clone(), key.clone());
        move || {
            let conn = open_db(&app.state::<AppState>());
            let fresh = conn.as_ref().and_then(|c| {
                cache::db::cached_info(c, &key, "size", SIZE_MAX_AGE_SECS, unix_now())
                    .ok()
                    .flatten()
            });
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
            let vdf = {
                // One Steam logon at a time; Steam rate-limits them and the owned-games fetch logs on too.
                let state = app.state::<AppState>();
                let _one = state.steam_size.lock().await;
                steam_size::fetch_appinfo(&token?, appid).await
            }
            .inspect_err(|e| eprintln!("steam size {key}: {e}"))
            .ok()?;
            let bytes = steam_size::parse_download_size(&vdf)?.to_string();
            blocking(move || {
                if let Some(c) = open_db(&app.state::<AppState>()) {
                    _ = cache::db::put_info(&c, &key, "size", &bytes, unix_now());
                }
                bytes
            })
            .await
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
                            .inspect_err(|e| eprintln!("legendary info {key}: {e}"))
                            .ok()?;
                        epic::download_size_from_info(&info).map(|size| size.to_string())
                    },
                )
            })
            .await?
        }
    };
    bytes.parse().ok().map(view::size_label)
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
            eprintln!("steam dlc {key}: {e}");
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
            .inspect_err(|e| eprintln!("appdetails {key}: {e}"))
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
    let game = app.state::<AppState>().game(&key)?;
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
                let list = epic::catalog().inspect_err(|e| eprintln!("legendary list {key}: {e}")).ok()?;
                let all = info::epic_details(&list);
                if let Some(c) = &conn
                    && let Err(e) = info::remember_all(c, &all, now)
                {
                    eprintln!("epic details cache: {e}");
                }
                Some(all.into_iter().find(|(k, _)| *k == key).map_or_else(|| "null".into(), |(_, json)| json))
            });
            let base = parse_details(catalog).unwrap_or_default();
            let gamesdb = || {
                let json = info::gamesdb_cached_or_fetch(conn.as_ref(), &key, now, || {
                    let body = tauri::async_runtime::block_on(gamesdb::fetch(&http, &game.external_id))
                        .inspect_err(|e| eprintln!("gamesdb details {key}: {e}"))
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
                .inspect_err(|e| eprintln!("igdb epic url: {e}"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::tests::game;

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
        detail(&entries[0], games, twins, UNIX_EPOCH + Duration::from_secs(1_000 * 86_400))
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
        let shown = |outcome: SyncOutcome| saves_view(None, sync_record(&outcome, 1_000), 1_300).last;
        assert_eq!(shown(SyncOutcome::Synced), "Synced 5 min ago");
        assert_eq!(shown(SyncOutcome::Failed("Login failed!".into())), "Cloud saves didn't sync: Login failed!");
        assert!(saves_view(None, sync_record(&SyncOutcome::CloudNewer, 1_000), 1_300).failed);
        assert_eq!(sync_record(&SyncOutcome::LocalNewer, 1_000), None);
        assert_eq!(saves_view(Some(false), None, 0), SavesView { choice: Some(false), last: "Not synced yet".into(), failed: false });
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
        assert_eq!(d.playtime_tile.as_deref(), Some("2 h 0 min"));
        assert_eq!(d.playtime_note, None);
        assert_eq!(d.links.len(), 5);
    }

    #[test]
    fn last_played_store_only_for_merged_entries() {
        let games = pumpkin(Some(86_400), Some(2 * 86_400));
        let merged = pumpkin_detail(&games, &HashMap::from([("epic-Pumpkin".to_string(), 635320)]));
        assert_eq!(merged.last_played_store.as_deref(), Some("on Epic Games"));
        let single = pumpkin_detail(&games, &HashMap::new());
        assert_eq!(single.last_played_store, None);
    }
}
