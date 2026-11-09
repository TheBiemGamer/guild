//! The install panel of an Epic, GOG or Amazon copy: what the install takes, where it can go, and the install itself.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::game::{SIZE_MAX_AGE_SECS, enqueue, install_base};
use super::{AppState, Res, blocking, open_db, unix_now};
use crate::cache;
use crate::downloads::{Job, QueuedAction};
use crate::info;
use crate::library::{Game, ProviderKind, amazon, epic, gog};
use crate::msg::Msg;
use crate::settings;

/// `info` kind of an Epic game's `[download, disk]` bytes, from `legendary info`.
const EPIC_SIZES: &str = "epic-install-size";
/// Folders kept per store in the folder menu, newest first, besides its install folder.
const RECENT_FOLDERS: usize = 4;

/// Download and disk bytes, each `None` when unknown.
type Sizes = (Option<u64>, Option<u64>);

/// A GOG build, where both a native Linux one and a Windows one exist (Linux only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Build {
    Linux,
    Windows,
}

/// What an install takes. Sizes are `None` where the store tool doesn't say.
#[derive(Debug, Default, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    /// The game alone, without DLC.
    download_bytes: Option<u64>,
    disk_bytes: Option<u64>,
    /// The folder the game gets inside the chosen one, when the store tool says.
    folder_name: Option<String>,
    /// The owned DLC (Epic and GOG).
    dlc: Vec<DlcSize>,
    /// GOG with more than one language: its languages; `language` is the one the sizes are for.
    languages: Vec<String>,
    language: Option<String>,
    /// GOG on Linux with a native build: both builds; `build` is the one the sizes are for.
    builds: Vec<Build>,
    build: Option<Build>,
}

#[derive(Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DlcSize {
    id: String,
    title: String,
    download_bytes: Option<u64>,
    disk_bytes: Option<u64>,
}

/// A folder in the folder menu and the drive space around it.
#[derive(Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InstallFolder {
    path: String,
    free_bytes: Option<u64>,
    total_bytes: Option<u64>,
}

/// What the owner picked in the panel.
#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InstallChoice {
    /// The base folder; the game gets its own folder inside it.
    folder: String,
    /// The DLC to install with the game, by id; `None` takes all owned DLC (the panel's choice before it knew them).
    dlc: Option<Vec<String>>,
    /// GOG: one of the plan's `languages`; `None` picks one.
    language: Option<String>,
    /// GOG on Linux: one of the plan's `builds`; `None` is the native one.
    build: Option<Build>,
}

/// The panel's numbers for a copy that isn't installed, for `language` and `build` (GOG). Store tools are asked once
/// and their answers cached for a week, so a second open is instant. `None` for Steam, added games and installed copies.
#[tauri::command]
#[specta::specta]
pub async fn install_plan(
    app: AppHandle,
    key: String,
    language: Option<String>,
    build: Option<Build>,
) -> Option<InstallPlan> {
    let game = store_copy(&app, &key)?;
    blocking(move || {
        let conn = open_db(&app.state::<AppState>());
        Some(plan(conn.as_ref(), &game, language, build))
    })
    .await
}

/// The folder menu of `store`: its install folder first, then the folders used before for it, then a `Games` folder
/// on each other drive.
#[tauri::command]
#[specta::specta]
pub async fn install_folders(app: AppHandle, store: ProviderKind) -> Vec<InstallFolder> {
    blocking(move || {
        let state = app.state::<AppState>();
        let mut paths: Vec<PathBuf> = install_base(&state, store)
            .map(PathBuf::from)
            .into_iter()
            .collect();
        let recent = open_db(&state).map_or_else(Vec::new, |c| recent(&c, store));
        for path in recent {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        for root in crate::platform::drives() {
            if !paths.iter().any(|p| p.starts_with(&root)) {
                paths.push(root.join("Games"));
            }
        }
        paths.into_iter().map(folder).collect()
    })
    .await
}

/// A folder picked with "Another folder…", with its drive's space.
#[tauri::command]
#[specta::specta]
pub async fn folder_space(path: String) -> InstallFolder {
    blocking(move || folder(PathBuf::from(path))).await
}

/// Queues the install as picked in the panel and returns when it is done, like `act`. The folder becomes the store's
/// install folder. Refuses before the download when the drive hasn't room for it.
#[tauri::command]
#[specta::specta]
pub async fn install(app: AppHandle, key: String, choice: InstallChoice) -> Res<()> {
    let game = store_copy(&app, &key).ok_or(msg!(alreadyInstalled))?;
    let folder = PathBuf::from(choice.folder.trim());
    if !folder.is_absolute() {
        return Err(msg!(noInstallBase));
    }
    let owned: Vec<String> = game.dlc.iter().map(|d| d.id.clone()).collect();
    let chosen = choice.dlc.clone().unwrap_or_else(|| owned.clone());
    if chosen.iter().any(|id| !owned.contains(id)) {
        return Err(msg!(dlcNotInLibrary));
    }
    blocking(move || {
        let state = app.state::<AppState>();
        let conn = open_db(&state);
        let plan = plan(conn.as_ref(), &game, choice.language.clone(), choice.build);
        let free = crate::platform::disk_space(&folder).map(|(free, _)| free);
        if free.is_some_and(|free| needed(&plan, &chosen) > free) {
            return Err(msg!(notEnoughSpace));
        }
        if let Some(conn) = &conn {
            remember(conn, game.provider, &folder);
        }
        let (with_dlc, dlcs) = dlc_job(&owned, chosen);
        let job = Job {
            with_dlc: Some(with_dlc),
            dlcs,
            dest: Some(folder.to_string_lossy().into_owned()),
            lang: choice
                .language
                .filter(|_| game.provider == ProviderKind::Gog),
            windows: choice.build == Some(Build::Windows),
            ..Job::new(&key, QueuedAction::Install)
        };
        enqueue(&app, job)
    })
    .await
}

/// The job's `with_dlc` and `dlcs` for the `chosen` of the `owned` DLC: all of them, none, or just some.
fn dlc_job(owned: &[String], chosen: Vec<String>) -> (bool, Option<Vec<String>>) {
    if owned.iter().all(|id| chosen.contains(id)) {
        (true, None)
    } else if chosen.is_empty() {
        (false, None)
    } else {
        (false, Some(chosen))
    }
}

/// An Epic, GOG or Amazon copy that isn't installed.
fn store_copy(app: &AppHandle, key: &str) -> Option<Game> {
    app.state::<AppState>().game(key).filter(|g| {
        !g.installed
            && matches!(
                g.provider,
                ProviderKind::Epic | ProviderKind::Gog | ProviderKind::Amazon
            )
    })
}

fn plan(
    conn: Option<&Connection>,
    game: &Game,
    language: Option<String>,
    build: Option<Build>,
) -> InstallPlan {
    let dlc = |sizes: &dyn Fn(&str) -> Sizes| {
        game.dlc
            .iter()
            .map(|d| {
                let (download_bytes, disk_bytes) = sizes(&d.id);
                DlcSize {
                    id: d.id.clone(),
                    title: d.title.clone(),
                    download_bytes,
                    disk_bytes,
                }
            })
            .collect()
    };
    let id = &game.external_id;
    match game.provider {
        ProviderKind::Epic => {
            let (download_bytes, disk_bytes) = epic_sizes(conn, game);
            InstallPlan {
                download_bytes,
                disk_bytes,
                folder_name: epic::folder_name(id),
                // shortcut: no DLC sizes; Legendary has them only per DLC, one online call each.
                dlc: dlc(&|_| (None, None)),
                ..InstallPlan::default()
            }
        }
        ProviderKind::Amazon => {
            // Files download whole, so the download is the size on disk too.
            let size = amazon::download_size(conn, id);
            InstallPlan {
                download_bytes: size,
                disk_bytes: size,
                folder_name: Some(amazon::install_folder(id)),
                ..InstallPlan::default()
            }
        }
        ProviderKind::Gog => {
            let windows = build == Some(Build::Windows);
            let Some(info) = gog::build_info(conn, id, windows) else {
                return InstallPlan {
                    dlc: dlc(&|_| (None, None)),
                    ..InstallPlan::default()
                };
            };
            let native = gog::install_info(conn, id).is_some_and(|i| i["os"] == "linux");
            let languages = gog::languages(&info);
            let lang = language
                .filter(|l| languages.contains(l))
                .unwrap_or_else(|| gog::pick_lang(&info));
            let (download_bytes, disk_bytes) = gog::sizes(&info["size"], &lang);
            InstallPlan {
                download_bytes,
                disk_bytes,
                folder_name: info["folder_name"]
                    .as_str()
                    .filter(|f| !f.is_empty())
                    .map(str::to_owned),
                dlc: dlc(&|id| {
                    gog::dlc_size(&info, id).map_or((None, None), |s| gog::sizes(s, &lang))
                }),
                languages: if languages.len() > 1 {
                    languages
                } else {
                    Vec::new()
                },
                language: Some(lang),
                builds: if native {
                    vec![Build::Linux, Build::Windows]
                } else {
                    Vec::new()
                },
                build: native.then_some(if windows {
                    Build::Windows
                } else {
                    Build::Linux
                }),
            }
        }
        ProviderKind::Steam | ProviderKind::Custom => InstallPlan::default(),
    }
}

/// Bytes the install takes on disk: the game and the `dlc` chosen, each by its disk size, else its download size,
/// else nothing (unknown).
fn needed(plan: &InstallPlan, dlc: &[String]) -> u64 {
    let chosen = plan
        .dlc
        .iter()
        .filter(|d| dlc.contains(&d.id))
        .map(|d| d.disk_bytes.or(d.download_bytes));
    std::iter::once(plan.disk_bytes.or(plan.download_bytes))
        .chain(chosen)
        .flatten()
        .sum()
}

/// An Epic game's download and disk bytes, cached for a week. `(None, None)` when Legendary couldn't tell (logged).
pub(super) fn epic_sizes(conn: Option<&Connection>, game: &Game) -> Sizes {
    let key = game.key();
    let json = info::cached_or_run_tool(
        conn,
        &key,
        EPIC_SIZES,
        SIZE_MAX_AGE_SECS,
        unix_now(),
        || {
            let info = epic::info_json(game)
                .inspect_err(|e| tracing::warn!("legendary info {key}: {e}"))
                .ok()?;
            let sizes = epic::sizes_from_info(&info);
            sizes.0?;
            serde_json::to_string(&sizes).ok()
        },
    );
    json.and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default()
}

fn folder(path: PathBuf) -> InstallFolder {
    let space = crate::platform::disk_space(&path);
    InstallFolder {
        path: path.to_string_lossy().into_owned(),
        free_bytes: space.map(|(free, _)| free),
        total_bytes: space.map(|(_, total)| total),
    }
}

/// `settings` name of the folders used before for `store`, as a JSON list, newest first.
fn recent_name(store: ProviderKind) -> String {
    format!("install_folders_{}", store.as_str())
}

fn recent(conn: &Connection, store: ProviderKind) -> Vec<PathBuf> {
    cache::db::setting(conn, &recent_name(store))
        .ok()
        .flatten()
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default()
}

/// Makes `folder` the store's install folder and the newest of its recent ones. Saved as is: an install creates it,
/// so it needn't exist yet (`settings::update` would refuse that).
fn remember(conn: &Connection, store: ProviderKind, folder: &Path) {
    let setting = match store {
        ProviderKind::Gog => settings::GOG_INSTALL_DIR,
        ProviderKind::Amazon => settings::AMAZON_INSTALL_DIR,
        _ => settings::EPIC_INSTALL_DIR,
    };
    let mut folders = recent(conn, store);
    folders.retain(|f| f != folder);
    folders.insert(0, folder.to_path_buf());
    folders.truncate(RECENT_FOLDERS);
    let saved =
        cache::db::set_setting(conn, setting, Some(&folder.to_string_lossy())).and_then(|()| {
            let json = serde_json::to_string(&folders).unwrap_or_default();
            cache::db::set_setting(conn, &recent_name(store), Some(&json))
        });
    if let Err(e) = saved {
        tracing::warn!("saving the install folder: {}", Msg::from(e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dlc(id: &str, download: Option<u64>, disk: Option<u64>) -> DlcSize {
        DlcSize {
            id: id.into(),
            title: id.into(),
            download_bytes: download,
            disk_bytes: disk,
        }
    }

    #[test]
    fn needed_counts_the_chosen_dlc_by_disk_size_else_download_size() {
        let plan = InstallPlan {
            download_bytes: Some(10),
            disk_bytes: Some(30),
            dlc: vec![
                dlc("a", Some(1), Some(2)),
                dlc("b", Some(4), None),
                dlc("c", None, None),
                dlc("d", Some(100), Some(200)),
            ],
            ..InstallPlan::default()
        };
        let chosen = ["a", "b", "c"].map(String::from);
        assert_eq!(needed(&plan, &chosen), 30 + 2 + 4);
        assert_eq!(needed(&InstallPlan::default(), &[]), 0);
    }

    #[test]
    fn dlc_choice_becomes_all_none_or_a_list() {
        let owned = ["a", "b"].map(String::from);
        let ids = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(dlc_job(&owned, ids(&["b", "a"])), (true, None));
        assert_eq!(dlc_job(&owned, ids(&[])), (false, None));
        assert_eq!(dlc_job(&owned, ids(&["b"])), (false, Some(ids(&["b"]))));
        assert_eq!(dlc_job(&[], ids(&[])), (true, None));
    }

    #[test]
    fn the_folder_used_last_leads_the_recent_ones() {
        let dir = tempfile::tempdir().unwrap();
        let conn = cache::db::open(&dir.path().join("t.sqlite")).unwrap();
        for f in ["/a", "/b", "/c", "/d", "/e", "/b"] {
            remember(&conn, ProviderKind::Gog, Path::new(f));
        }
        assert_eq!(
            recent(&conn, ProviderKind::Gog),
            ["/b", "/e", "/d", "/c"].map(PathBuf::from)
        );
        assert_eq!(
            settings::load(&conn).gog_install_dir,
            Some(PathBuf::from("/b"))
        );
        assert!(recent(&conn, ProviderKind::Epic).is_empty());
    }
}
