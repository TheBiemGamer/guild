//! The settings page and the Epic login actions.

use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use super::library::{refresh_owned, rescan};
use super::{blocking, AppState, Res};
use crate::cache;
use crate::epic_login;
use crate::library::epic::{self, Login};
use crate::library::steam;
use crate::library::ProviderKind;
use crate::settings;
use crate::steam_login::{self, PollStep, QrSession};
use crate::steam_owned;
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

/// The Steam install found on this PC.
#[derive(Serialize)]
pub struct SteamLocal {
    name: Option<String>,
    path: String,
}

/// The owned-games login, separate from the local install.
#[derive(Serialize)]
pub struct SteamLibrary {
    /// The connected account; `None` when not connected.
    account: Option<String>,
    /// Steam rejected the saved login; connect again.
    attention: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    steam: Option<SteamLocal>,
    steam_library: SteamLibrary,
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
        let steam = steamlocate::SteamDir::locate().ok().map(|dir| SteamLocal {
            name: steam::persona_name(dir.path()),
            path: dir.path().to_string_lossy().into_owned(),
        });
        let steam_library = SteamLibrary {
            account: cache::db::setting(&conn, settings::STEAM_ACCOUNT).ok().flatten(),
            attention: state.lock().steam_attention,
        };
        Ok(SettingsView {
            steam,
            steam_library,
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

/// One answer to the dialog's poll. Never carries the refresh token.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(tag = "step", rename_all = "camelCase")]
pub enum ConnectStep {
    Waiting,
    Rotated { svg: String },
    Scanned,
    Connected { account: String },
    /// The login was approved but couldn't be saved; the dialog stops here.
    Failed { message: String },
}

/// Starts a QR login, replacing any running one, and returns the code as an SVG.
#[tauri::command]
pub async fn steam_connect_start(app: AppHandle) -> Res<String> {
    let state = app.state::<AppState>();
    let generation = state.steam_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let session = steam_login::begin(&state.http).await?;
    let svg = steam_login::qr_svg(&session.challenge_url)?;
    let mut slot = state.steam_qr.lock().await;
    // A cancel (or newer start) while Steam was answering wins.
    if !should_save(generation, state.steam_gen.load(Ordering::SeqCst)) {
        return Err("Not connecting.".into());
    }
    *slot = Some(session);
    state.steam_session_gen.store(generation, Ordering::SeqCst);
    Ok(svg)
}

/// Asks Steam how the login is going. When approved, saves the login and loads the owned games.
#[tauri::command]
pub async fn steam_connect_poll(app: AppHandle) -> Res<ConnectStep> {
    let state = app.state::<AppState>();
    let mut slot = state.steam_qr.lock().await;
    let generation = state.steam_session_gen.load(Ordering::SeqCst);
    let step = poll_session(&state.http, &mut slot).await?;
    let approved = match step {
        PollStep::Waiting => return Ok(ConnectStep::Waiting),
        PollStep::Scanned => return Ok(ConnectStep::Scanned),
        PollStep::Rotated => {
            let url = slot.as_ref().ok_or("Not connecting.")?.challenge_url.clone();
            return Ok(ConnectStep::Rotated { svg: steam_login::qr_svg(&url)? });
        }
        PollStep::Done(approved) => approved,
    };
    drop(slot);
    // Closed while Steam was answering: save nothing.
    if !should_save(generation, state.steam_gen.load(Ordering::SeqCst)) {
        return Err("Not connecting.".into());
    }
    let account = approved.account.clone();
    // The token goes straight from Steam's answer into Credential Manager.
    let saved = blocking({
        let app = app.clone();
        move || save_login(&app, approved)
    })
    .await;
    if let Err(message) = saved {
        return Ok(ConnectStep::Failed { message });
    }
    refresh_owned(app, true);
    Ok(ConnectStep::Connected { account })
}

/// Stores the login and clears the "connect again" flag. The token goes nowhere else.
fn save_login(app: &AppHandle, approved: steam_login::Approved) -> Res<()> {
    let state = app.state::<AppState>();
    if approved.account.trim().is_empty() {
        return Err("Steam sent no account name.".into());
    }
    // Account first, so there is never a token without an account.
    let conn = cache::db::open(&state.dirs.db).map_err(|e| e.to_string())?;
    cache::db::set_setting(&conn, settings::STEAM_ACCOUNT, Some(&approved.account)).map_err(|e| e.to_string())?;
    if let Err(e) = settings::set_steam_token(Some(&approved.refresh_token)) {
        _ = cache::db::set_setting(&conn, settings::STEAM_ACCOUNT, None);
        return Err(e);
    }
    state.lock().steam_attention = false;
    Ok(())
}

/// Polls the session in `slot`; an approved login ends it. An empty slot is an error.
async fn poll_session(http: &reqwest::Client, slot: &mut Option<QrSession>) -> Res<PollStep> {
    let session = slot.as_mut().ok_or("Not connecting.")?;
    let step = steam_login::poll(http, session).await?;
    if matches!(step, PollStep::Done(_)) {
        *slot = None;
    }
    Ok(step)
}

/// A login approved under `started` may be saved only if no cancel or newer start happened since.
fn should_save(started: u64, now: u64) -> bool {
    started == now
}

fn cancel_session(slot: &mut Option<QrSession>) {
    *slot = None;
}

/// Drops the running QR login, if any.
#[tauri::command]
pub async fn steam_connect_cancel(app: AppHandle) {
    let state = app.state::<AppState>();
    // Before waiting for the lock, so a poll already in flight sees it when Steam answers.
    state.steam_gen.fetch_add(1, Ordering::SeqCst);
    cancel_session(&mut *state.steam_qr.lock().await);
}

/// Forgets the Steam login and the owned list, then rescans so only installed Steam games remain.
#[tauri::command]
pub async fn steam_disconnect(app: AppHandle) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        settings::set_steam_token(None)?;
        let conn = cache::db::open(&state.dirs.db).map_err(|e| e.to_string())?;
        cache::db::set_setting(&conn, settings::STEAM_ACCOUNT, None).map_err(|e| e.to_string())?;
        steam_owned::forget(&conn);
        {
            let mut s = state.lock();
            s.steam_attention = false;
            s.owned = None;
        }
        rescan(&app, false);
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_drops_the_session() {
        let mut slot = Some(steam_login::test_session());
        cancel_session(&mut slot);
        assert!(slot.is_none());
        let http = reqwest::Client::new();
        let r = tauri::async_runtime::block_on(poll_session(&http, &mut slot));
        assert_eq!(r.err().as_deref(), Some("Not connecting."));
    }

    #[test]
    fn a_login_approved_after_cancel_is_not_saved() {
        assert!(should_save(3, 3));
        assert!(!should_save(3, 4));
    }

    #[test]
    fn connect_step_serializes_with_a_step_tag() {
        assert_eq!(
            serde_json::to_string(&ConnectStep::Connected { account: "a".into() }).unwrap(),
            r#"{"step":"connected","account":"a"}"#
        );
    }
}
