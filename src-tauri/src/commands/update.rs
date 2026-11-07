//! App updates from GitHub releases, through the updater plugin. The installer is signed with the key in
//! `tauri.conf.json` (`plugins.updater.pubkey`); a download that doesn't match it is never installed.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::UpdaterExt;

use super::{blocking, AppState, Res};
use crate::library::ProviderKind;

#[derive(Serialize)]
pub struct UpdateInfo {
    version: String,
    /// The release notes, Markdown from CHANGELOG.md.
    notes: String,
}

/// Asks GitHub for a newer release and keeps it for `install_update`.
#[tauri::command]
pub async fn check_update(app: AppHandle, state: State<'_, AppState>) -> Res<Option<UpdateInfo>> {
    let mut pending = state.update.lock().await;
    let checked = async { app.updater_builder().timeout(Duration::from_secs(30)).build()?.check().await };
    let update = checked.await.map_err(|e: tauri_plugin_updater::Error| {
        log!("update check: {e}");
        "Couldn't check for updates.".to_string()
    })?;
    let info = update.as_ref().map(|u| UpdateInfo { version: u.version.clone(), notes: u.body.clone().unwrap_or_default() });
    *pending = update;
    Ok(info)
}

/// Downloads the update found by `check_update`, stops the download queue and runs the installer, which
/// exits the app and starts the new version.
#[tauri::command]
pub async fn install_update(app: AppHandle, state: State<'_, AppState>) -> Res<()> {
    let mut update = state.update.lock().await.clone().ok_or("No update to install.")?;
    // The check's 30 s limit would cut off a slow download.
    update.timeout = None;
    if game_running(&app).await {
        return Err("Close the game first.".into());
    }
    // Verifies the signature.
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| {
        log!("update download: {e}");
        "Couldn't download the update.".to_string()
    })?;
    if game_running(&app).await {
        return Err("Close the game first.".into());
    }
    let was_paused = blocking({
        let app = app.clone();
        move || super::game::stop_queue_for_restart(&app)
    })
    .await;
    // The exit skips the tray's cleanup and would leave a dead icon behind until hovered.
    let tray = app.tray_by_id("main");
    let show_tray = |on: bool| {
        if let Some(Err(e)) = tray.as_ref().map(|t| t.set_visible(on)) {
            log!("tray: {e}");
        }
    };
    show_tray(false);
    // On Windows this starts the installer and exits; it only returns on failure.
    if let Err(e) = update.install(bytes) {
        log!("update install: {e}");
        show_tray(true);
        if !was_paused {
            super::game::resume_downloads(app);
        }
        return Err("Couldn't install the update.".into());
    }
    Ok(())
}

/// A Steam game is running, or an Epic or added game launched from here hasn't been seen to end yet
/// (`library::poll` ends the watch and uploads its saves).
async fn game_running(app: &AppHandle) -> bool {
    let app = app.clone();
    blocking(move || {
        ProviderKind::Steam.provider().running_game().is_some() || app.state::<AppState>().lock().watch.is_some()
    })
    .await
}
