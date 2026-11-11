//! Settings' Wine section on Linux: umu-launcher, the Proton builds and the games' Wine prefixes (`crate::wine`, behind
//! `platform`). On Windows every list is empty and no action is offered.

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use super::{AppState, Res, blocking, open_db};
use crate::platform::{self, ProtonKind, ReleaseView};

/// Downloads the pinned umu-launcher in the background, sending each new whole percent.
#[tauri::command]
#[specta::specta]
pub async fn install_umu(on_percent: Channel<u8>) -> Res<()> {
    platform::install_umu(|percent| {
        let _ = on_percent.send(percent);
    })
    .await
}

/// A Proton source's recent releases; `None` when GitHub couldn't be asked (the reason is logged).
#[tauri::command]
#[specta::specta]
pub async fn proton_releases(kind: ProtonKind) -> Option<Vec<ReleaseView>> {
    blocking(move || platform::proton_releases(kind)).await
}

/// Downloads and checks one Proton release in the background, sending each new whole percent. One at a time.
#[tauri::command]
#[specta::specta]
pub async fn install_proton(kind: ProtonKind, tag: String, on_percent: Channel<u8>) -> Res<()> {
    platform::install_proton(kind, tag, |percent| {
        let _ = on_percent.send(percent);
    })
    .await
}

/// Deletes a Proton build Guild installed.
#[tauri::command]
#[specta::specta]
pub async fn remove_proton(name: String) -> Res<()> {
    blocking(move || platform::remove_proton(&name)).await
}

#[derive(Serialize, specta::Type)]
pub struct PrefixView {
    key: String,
    /// The game's name; `None` when it is no longer in the library.
    title: Option<String>,
}

/// The games' Wine prefixes, by copy key. Their sizes come from `prefix_sizes`, which takes longer.
#[tauri::command]
#[specta::specta]
pub fn wine_prefixes(app: AppHandle) -> Vec<PrefixView> {
    let state = app.state::<AppState>();
    platform::prefixes()
        .into_iter()
        .map(|(key, _)| PrefixView {
            title: state.game(&key).map(|g| g.title),
            key,
        })
        .collect()
}

#[derive(Serialize, specta::Type)]
pub struct DirSize {
    /// A prefix's copy key, or a Proton build's name.
    key: String,
    bytes: u64,
}

#[derive(Serialize, specta::Type)]
pub struct WineSizes {
    prefixes: Vec<DirSize>,
    protons: Vec<DirSize>,
}

/// Each prefix's and Proton build's size on disk, walked in the background.
#[tauri::command]
#[specta::specta]
pub async fn wine_sizes() -> WineSizes {
    blocking(|| {
        let sizes = |dirs: Vec<(String, std::path::PathBuf)>| {
            dirs.into_iter()
                .map(|(key, path)| DirSize {
                    key,
                    bytes: super::settings::dir_size(&path),
                })
                .collect()
        };
        WineSizes {
            prefixes: sizes(platform::prefixes()),
            protons: sizes(platform::proton_dirs()),
        }
    })
    .await
}

#[derive(Serialize, specta::Type)]
pub struct GamePrefix {
    /// Whether winetricks can run in it: the system's, or one a Proton build here bundles.
    winetricks: bool,
}

/// The Wine prefix of a copy, for its game page: only on Linux, once its game has run, and never for a native Linux
/// build.
#[tauri::command]
#[specta::specta]
pub async fn game_prefix(app: AppHandle, key: String) -> Option<GamePrefix> {
    let game = app.state::<AppState>().game(&key);
    blocking(move || {
        if game.as_ref().is_some_and(super::game::native_build) {
            return None;
        }
        platform::prefixes().iter().find(|(k, _)| *k == key)?;
        Some(GamePrefix {
            winetricks: platform::runner()?.winetricks,
        })
    })
    .await
}

/// Whether the copy `key` is the game watched as running.
fn runs(app: &AppHandle, key: &str) -> bool {
    let state = app.state::<AppState>();
    let s = state.lock();
    s.watch.as_ref().is_some_and(|w| w.key == key)
}

/// Deletes a copy's prefix, saves and all; not while its game runs.
#[tauri::command]
#[specta::specta]
pub async fn remove_prefix(app: AppHandle, key: String) -> Res<()> {
    if runs(&app, &key) {
        return Err(msg!(closeGameFirst));
    }
    blocking(move || platform::remove_prefix(&key)).await
}

/// Opens a copy's prefix folder.
#[tauri::command]
#[specta::specta]
pub fn open_prefix(key: String) -> Res<()> {
    let (_, path) = platform::prefixes()
        .into_iter()
        .find(|(k, _)| *k == key)
        .ok_or(msg!(gameNotInLibrary))?;
    platform::shell_open(path).map_err(Into::into)
}

#[derive(Debug, Clone, Copy, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PrefixTool {
    Winecfg,
    Winetricks,
}

/// Starts winecfg or winetricks in a copy's prefix, with the Proton build its game runs with.
#[tauri::command]
#[specta::specta]
pub async fn run_in_prefix(app: AppHandle, key: String, tool: PrefixTool) -> Res<()> {
    blocking(move || {
        let proton = open_db(&app.state::<AppState>())
            .and_then(|conn| super::game::launch_options_of(&conn, &key).proton);
        let program = match tool {
            PrefixTool::Winecfg => "winecfg",
            PrefixTool::Winetricks => "winetricks",
        };
        platform::run_in_prefix(&key, program, proton.as_deref())
    })
    .await
}
