//! The settings page and the Epic login actions.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use super::library::rescan;
use super::{blocking, AppState, Res};
use crate::cache;
use crate::epic_login;
use crate::library::epic::{self, Login};
use crate::library::steam;
use crate::library::ProviderKind;
use crate::settings;
use crate::view;

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
