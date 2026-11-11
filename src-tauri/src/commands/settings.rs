//! The settings page and the Epic, GOG and Amazon login actions.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use steam_vent::auth::{
    AuthConfirmationHandler, ClientInfo, ConfirmationAction, ConfirmationMethod,
    ConfirmationMethodClass, GuardDataStore, GuardTokenType, NullGuardDataStore, SteamGuardToken,
};
use steam_vent::{
    Connection as Session, ConnectionError, EResult, LoginError, NetworkError, ServerList,
};
use tokio::sync::mpsc;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use super::library::{fill_twins, refresh_owned, rescan, rescan_local, rescan_store};
use super::{AppState, Res, blocking};
use crate::amazon_login;
use crate::cache;
use crate::epic_login;
use crate::gog_login;
use crate::library::ProviderKind;
use crate::library::epic;
use crate::library::tool::StoreLogin;
use crate::library::{amazon, gog};
use crate::msg::Msg;
use crate::settings;
use crate::steam_login::{self, PollStep, QrSession};
use crate::steam_owned;
use crate::tools;

/// Opens Epic's login window, hands the code to Legendary, then loads the Epic library.
#[tauri::command]
#[specta::specta]
pub async fn epic_log_in(app: AppHandle) -> Res<()> {
    blocking(move || {
        // Closing the window yourself is no error.
        let Some(code) = epic_login::run(&app)? else {
            return Ok(());
        };
        epic::log_in(&code)?;
        rescan_store(&app, ProviderKind::Epic);
        Ok(())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub fn dismiss_epic_notice(state: State<'_, AppState>) {
    state.lock().epic_notice = false;
}

/// Forgets the Epic login in Legendary, then rescans Epic. Cached Epic games stay.
#[tauri::command]
#[specta::specta]
pub async fn epic_log_out(app: AppHandle) -> Res<()> {
    blocking(move || {
        epic::log_out()?;
        note_signed_out(&app, "epic");
        rescan_store(&app, ProviderKind::Epic);
        // The user did this; it isn't news.
        app.state::<AppState>().lock().epic_notice = false;
        Ok(())
    })
    .await
}

/// Opens GOG's login window, hands the code to gogdl, then loads the GOG library.
#[tauri::command]
#[specta::specta]
pub async fn gog_log_in(app: AppHandle) -> Res<()> {
    blocking(move || {
        let Some(code) = gog_login::run(&app)? else {
            return Ok(());
        };
        gog::log_in(&code)?;
        rescan_store(&app, ProviderKind::Gog);
        fill_twins(app.clone());
        Ok(())
    })
    .await
}

/// Forgets the GOG login, then rescans. Cached GOG games stay.
#[tauri::command]
#[specta::specta]
pub async fn gog_log_out(app: AppHandle) -> Res<()> {
    blocking(move || {
        gog::log_out()?;
        note_signed_out(&app, "gog");
        rescan_store(&app, ProviderKind::Gog);
        Ok(())
    })
    .await
}

/// Starts Nile's device login, opens Amazon's login window on it, hands the code back to Nile, then loads the
/// Amazon library.
#[tauri::command]
#[specta::specta]
pub async fn amazon_log_in(app: AppHandle) -> Res<()> {
    blocking(move || {
        let sign_in = amazon::sign_in()?;
        // Starting may have logged a stale account out.
        app.state::<AppState>().lock().amazon_login = Some(amazon::login());
        let Some(code) = amazon_login::run(&app, &sign_in.url)? else {
            return Ok(());
        };
        amazon::register(&sign_in, &code)?;
        rescan_store(&app, ProviderKind::Amazon);
        fill_twins(app.clone());
        Ok(())
    })
    .await
}

/// Deregisters this PC from the Amazon account and forgets the login, then rescans. Cached Amazon games stay.
#[tauri::command]
#[specta::specta]
pub async fn amazon_log_out(app: AppHandle) -> Res<()> {
    blocking(move || {
        amazon::log_out()?;
        note_signed_out(&app, "amazon");
        rescan_store(&app, ProviderKind::Amazon);
        Ok(())
    })
    .await
}

/// Remembers a deliberate Log out, so the Settings dot stays down (`settings::signed_out`); the next scan that finds
/// the store logged in forgets it.
fn note_signed_out(app: &AppHandle, store: &str) {
    let saved =
        super::open_db(&app.state::<AppState>()).map(|c| settings::set_signed_out(&c, store, true));
    if let Some(Err(e)) = saved {
        tracing::warn!("{store} signed out: {e}");
    }
}

/// An owned Epic game that isn't installed, for "Import an existing install".
#[derive(Serialize, specta::Type)]
pub struct Importable {
    key: String,
    title: String,
}

/// Owned games of `provider` that aren't installed, by title.
fn importable(state: &AppState, provider: ProviderKind) -> Vec<Importable> {
    let mut games: Vec<Importable> = state
        .lock()
        .games
        .iter()
        .filter(|g| g.provider == provider && !g.installed)
        .map(|g| Importable {
            key: g.key(),
            title: g.title.clone(),
        })
        .collect();
    games.sort_by_key(|g| g.title.to_lowercase());
    games
}

/// Owned Epic games that aren't installed, by title.
#[tauri::command]
#[specta::specta]
pub fn epic_importable(state: State<'_, AppState>) -> Vec<Importable> {
    importable(&state, ProviderKind::Epic)
}

/// Owned Amazon games that aren't installed, by title.
#[tauri::command]
#[specta::specta]
pub fn amazon_importable(state: State<'_, AppState>) -> Vec<Importable> {
    importable(&state, ProviderKind::Amazon)
}

/// Makes Nile take an existing install of an owned Amazon game (`key`) at `path`, then rescans. Nile checks every
/// file first, so this can take minutes.
#[tauri::command]
#[specta::specta]
pub async fn amazon_import(app: AppHandle, key: String, path: String) -> Res<()> {
    let game = app
        .state::<AppState>()
        .game(&key)
        .filter(|g| g.provider == ProviderKind::Amazon && !g.installed)
        .ok_or(msg!(cantImport))?;
    blocking(move || {
        let conn = cache::db::open(&app.state::<AppState>().dirs.db).ok();
        let result = amazon::import(
            conn.as_ref(),
            &game.external_id,
            std::path::Path::new(&path),
        )
        .map_err(Msg::from);
        rescan_local(&app);
        result
    })
    .await
}

/// Makes Legendary take an existing install of an owned Epic game (`key`) at `path`, then rescans Epic.
#[tauri::command]
#[specta::specta]
pub async fn epic_import(app: AppHandle, key: String, path: String) -> Res<()> {
    let game = app
        .state::<AppState>()
        .game(&key)
        .filter(|g| g.provider == ProviderKind::Epic && !g.installed)
        .ok_or(msg!(cantImport))?;
    blocking(move || {
        let result = epic::import(&game.external_id, &path).map_err(Msg::from);
        rescan_local(&app);
        result
    })
    .await
}

/// Imports the games the Epic Games Launcher installed, then rescans Epic.
#[tauri::command]
#[specta::specta]
pub async fn epic_egl_sync(app: AppHandle) -> Res<()> {
    blocking(move || {
        let result = epic::egl_sync().map_err(Msg::from);
        rescan_local(&app);
        result
    })
    .await
}

/// Takes an existing install of an owned GOG game at `path` (gogdl reads which game it is), then rescans GOG.
/// Resolves to the game's title.
#[tauri::command]
#[specta::specta]
pub async fn gog_import(app: AppHandle, path: String) -> Res<String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let id = gog::import(std::path::Path::new(&path))?;
        let key = ProviderKind::Gog.key(&id);
        let title = state
            .game(&key)
            .map(|g| g.title)
            .ok_or(msg!(notInGogLibrary))?;
        let conn = cache::db::open(&state.dirs.db)?;
        gog::record_install(&conn, &id, std::path::Path::new(&path))?;
        rescan_local(&app);
        Ok(title)
    })
    .await
}

/// Downloads and checks a store tool (`tools.rs`), sending each new whole percent. Then it reads just that store's
/// login, so its card moves on to Log in at once, and rescans in the background; the next poll picks that up.
#[tauri::command]
#[specta::specta]
pub async fn install_tool(app: AppHandle, name: String, on_percent: Channel<u8>) -> Res<()> {
    let tool = tools::find(&name).ok_or(msg!(unknownTool))?;
    let http = app.state::<AppState>().http.clone();
    tools::install(&http, tool, |percent| {
        let _ = on_percent.send(percent);
    })
    .await?;
    let quick = app.clone();
    let store = blocking(move || {
        let state = quick.state::<AppState>();
        if tool.name == tools::LEGENDARY.name {
            let login = epic::login_state();
            state.lock().epic_login = Some(login);
            ProviderKind::Epic
        } else if tool.name == tools::GOGDL.name {
            let login = gog::login().0;
            state.lock().gog_login = Some(login);
            ProviderKind::Gog
        } else {
            let login = amazon::login();
            state.lock().amazon_login = Some(login);
            ProviderKind::Amazon
        }
    })
    .await;
    tauri::async_runtime::spawn(blocking(move || {
        rescan_store(&app, store);
        app.state::<AppState>().lock().library_changed = true;
    }));
    Ok(())
}

/// The owned-games login, separate from the local install.
#[derive(Serialize, specta::Type)]
pub struct SteamLibrary {
    /// The connected account; `None` when not connected.
    account: Option<String>,
    /// Steam rejected the saved login; connect again.
    attention: bool,
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    /// The Language setting as saved: "system", "en" or "nl".
    language: String,
    /// The language the UI shows (`msg::language`).
    ui_language: &'static str,
    /// Whether Steam is installed on this PC.
    steam: bool,
    steam_library: SteamLibrary,
    epic: Option<StoreLogin>,
    /// The logged-in Epic account's display name, when Legendary's session file has one.
    epic_account: Option<String>,
    epic_install_dir: Option<String>,
    gog: Option<StoreLogin>,
    /// The logged-in GOG account's name, once a GOG scan has fetched it.
    gog_account: Option<String>,
    gog_install_dir: Option<String>,
    amazon: Option<StoreLogin>,
    /// The logged-in Amazon account's first name, from Nile's file.
    amazon_account: Option<String>,
    amazon_install_dir: Option<String>,
    /// Where Epic, GOG and Amazon install without an install folder setting: `%USERPROFILE%\Games`, Legendary's
    /// default and the one Guild passes to gogdl and Nile.
    default_install_dir: Option<String>,
    igdb_client_id: Option<String>,
    /// Whether a secret is saved; its value never leaves Credential Manager.
    igdb_secret_set: bool,
    /// Whether a SteamGridDB API key is saved; like the secret, it never leaves Credential Manager.
    steamgriddb_key_set: bool,
    show_banner: bool,
    ambient: bool,
    cloud_saves: bool,
    gog_cloud_saves: bool,
    /// Read from the autostart entry itself, so it can't drift from what Windows runs.
    start_with_windows: bool,
    /// Picks the Linux wording where the Windows one names Windows.
    linux: bool,
    close_to_tray: bool,
    notify_downloads: bool,
    app_version: String,
    tools: Vec<tools::ToolView>,
    /// Linux: what Windows games run with; `None` on Windows.
    runner: Option<crate::platform::RunnerView>,
    cache_bytes: u64,
}

#[tauri::command]
#[specta::specta]
pub async fn settings_view(app: AppHandle) -> Res<SettingsView> {
    blocking(move || {
        let state = app.state::<AppState>();
        let conn = cache::db::open(&state.dirs.db)?;
        let saved = settings::load(&conn);
        let epic = state.lock().epic_login;
        let epic_account = if epic == Some(StoreLogin::LoggedIn) {
            crate::epic_playtime::account_name()
        } else {
            None
        };
        let gog = state.lock().gog_login;
        let gog_account = if gog == Some(StoreLogin::LoggedIn) {
            gog::account_name(&conn)
        } else {
            None
        };
        let amazon = state.lock().amazon_login;
        let amazon_account = if amazon == Some(StoreLogin::LoggedIn) {
            amazon::account_name()
        } else {
            None
        };
        let cache_bytes = dir_size(&state.dirs.thumbs) + dir_size(&state.dirs.art);
        let steam = steamlocate::SteamDir::locate().is_ok();
        let steam_library = SteamLibrary {
            account: cache::db::setting(&conn, settings::STEAM_ACCOUNT)
                .ok()
                .flatten(),
            attention: state.lock().steam_attention,
        };
        Ok(SettingsView {
            language: saved.language,
            ui_language: crate::msg::language(),
            steam,
            steam_library,
            epic,
            epic_account,
            epic_install_dir: saved
                .epic_install_dir
                .map(|p| p.to_string_lossy().into_owned()),
            gog,
            gog_account,
            gog_install_dir: saved
                .gog_install_dir
                .map(|p| p.to_string_lossy().into_owned()),
            amazon,
            amazon_account,
            amazon_install_dir: saved
                .amazon_install_dir
                .map(|p| p.to_string_lossy().into_owned()),
            default_install_dir: gog::default_base().map(|p| p.to_string_lossy().into_owned()),
            igdb_client_id: saved.igdb_client_id,
            igdb_secret_set: settings::secret().is_some(),
            steamgriddb_key_set: settings::grid_key().is_some(),
            show_banner: saved.show_banner,
            ambient: saved.ambient,
            cloud_saves: saved.cloud_saves,
            gog_cloud_saves: saved.gog_cloud_saves,
            start_with_windows: app.autolaunch().is_enabled().unwrap_or(false),
            linux: cfg!(target_os = "linux"),
            close_to_tray: saved.close_to_tray,
            notify_downloads: saved.notify_downloads,
            app_version: app.package_info().version.to_string(),
            tools: tools::view(),
            runner: crate::platform::runner(),
            cache_bytes,
        })
    })
    .await
}

/// Total size of the files under `dir`; a missing folder counts as empty.
pub(super) fn dir_size(dir: &std::path::Path) -> u64 {
    fs_err::read_dir(dir)
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
#[specta::specta]
pub async fn update_setting(app: AppHandle, name: String, value: Option<String>) -> Res<()> {
    blocking(move || {
        if name == settings::START_WITH_WINDOWS {
            settings::validate(&name, value.as_deref())?;
            let autostart = app.autolaunch();
            let done = if value.as_deref() == Some("true") {
                autostart.enable()
            } else {
                autostart.disable()
            };
            return done.map_err(|e| {
                if cfg!(windows) {
                    msg!(cantChangeStartup, detail = e.to_string())
                } else {
                    msg!(cantChangeAutostart, detail = e.to_string())
                }
            });
        }
        let state = app.state::<AppState>();
        let conn = cache::db::open(&state.dirs.db)?;
        settings::update(&conn, &name, value.as_deref())?;
        if name == settings::IGDB_CLIENT_ID {
            state
                .igdb
                .lock()
                .expect("token cache lock poisoned")
                .reset();
        }
        if name == settings::LANGUAGE {
            crate::msg::set_language(&settings::load(&conn).language);
            crate::tray::relabel(&app);
        }
        Ok(())
    })
    .await
}

/// Saves the IGDB Client Secret in Credential Manager, or removes it for `None`.
#[tauri::command]
#[specta::specta]
pub async fn set_igdb_secret(app: AppHandle, value: Option<String>) -> Res<()> {
    blocking(move || {
        let value = value.as_deref().map(str::trim).filter(|v| !v.is_empty());
        settings::set_secret(value)?;
        // A new secret deserves a fresh token request, even right after a failed one.
        app.state::<AppState>()
            .igdb
            .lock()
            .expect("token cache lock poisoned")
            .reset();
        Ok(())
    })
    .await
}

/// Saves the SteamGridDB API key in Credential Manager, or removes it for `None`.
#[tauri::command]
#[specta::specta]
pub async fn set_steamgriddb_key(value: Option<String>) -> Res<()> {
    blocking(move || {
        settings::set_grid_key(value.as_deref().map(str::trim).filter(|v| !v.is_empty()))
    })
    .await
}

/// SteamGridDB's API key page, where the owner gets a free key.
#[tauri::command]
#[specta::specta]
pub fn open_steamgriddb() -> Res<()> {
    crate::platform::open_url("https://www.steamgriddb.com/profile/preferences/api")
        .map_err(Msg::from)
}

/// Deletes downloaded art and thumbnails; they come back on demand. The database stays.
#[tauri::command]
#[specta::specta]
pub async fn clear_cache(app: AppHandle) -> Res<()> {
    blocking(move || {
        let dirs = &app.state::<AppState>().dirs;
        for dir in [&dirs.thumbs, &dirs.art] {
            match fs_err::remove_dir_all(dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(msg!(
                        cantClearFolder,
                        path = dir.display().to_string(),
                        detail = e.to_string()
                    ));
                }
            }
        }
        Ok(())
    })
    .await
}

/// The page where the owner creates the Twitch application IGDB needs.
#[tauri::command]
#[specta::specta]
pub fn open_twitch_console() -> Res<()> {
    crate::platform::open_url("https://dev.twitch.tv/console/apps").map_err(Msg::from)
}

/// IGDB's site, for the "Data from IGDB" credit next to how long to beat.
#[tauri::command]
#[specta::specta]
pub fn open_igdb() -> Res<()> {
    crate::platform::open_url("https://www.igdb.com").map_err(Msg::from)
}

/// Guild's repository, for "Source code" in About.
#[tauri::command]
#[specta::specta]
pub fn open_source() -> Res<()> {
    crate::platform::open_url("https://github.com/TheBiemGamer/guild").map_err(Msg::from)
}

/// The README's Thanks section, for "Credits" in About.
#[tauri::command]
#[specta::specta]
pub fn open_credits() -> Res<()> {
    crate::platform::open_url("https://github.com/TheBiemGamer/guild#thanks").map_err(Msg::from)
}

/// Opens the folder with launcher.log in File Explorer.
#[tauri::command]
#[specta::specta]
pub fn open_log_folder(state: State<'_, AppState>) -> Res<()> {
    let dir = &state.dirs.logs;
    fs_err::create_dir_all(dir).map_err(Msg::system)?;
    crate::platform::shell_open(dir).map_err(Msg::from)
}

/// Opens a new GitHub bug report with the app and Windows versions filled in.
#[tauri::command]
#[specta::specta]
pub fn report_problem(app: AppHandle) -> Res<()> {
    use crate::library::percent_encode;
    let version = app.package_info().version.to_string();
    let os = crate::platform::os_version();
    let url = format!(
        "https://github.com/TheBiemGamer/guild/issues/new?template=bug_report.yml&version={}&os={}",
        percent_encode(&version),
        percent_encode(&os)
    );
    crate::platform::open_url(&url).map_err(Msg::from)
}

/// One answer to the dialog's poll. Never carries the refresh token.
#[derive(Serialize, Debug, PartialEq, Eq, specta::Type)]
#[serde(tag = "step", rename_all = "camelCase")]
pub enum ConnectStep {
    Waiting,
    Rotated {
        svg: String,
    },
    Scanned,
    Connected {
        account: String,
    },
    /// The login was approved but couldn't be saved; the dialog stops here.
    Failed {
        message: Msg,
    },
}

/// Starts a QR login, replacing any running one, and returns the code as an SVG.
#[tauri::command]
#[specta::specta]
pub async fn steam_connect_start(app: AppHandle) -> Res<String> {
    let state = app.state::<AppState>();
    let generation = state.steam_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let session = steam_login::begin(&state.http).await?;
    let svg = steam_login::qr_svg(&session.challenge_url)?;
    let mut slot = state.steam_qr.lock().await;
    // A cancel (or newer start) while Steam was answering wins.
    if !should_save(generation, state.steam_gen.load(Ordering::SeqCst)) {
        return Err(msg!(notConnecting));
    }
    *slot = Some(session);
    state.steam_session_gen.store(generation, Ordering::SeqCst);
    Ok(svg)
}

/// Asks Steam how the login is going. When approved, saves the login and loads the owned games.
#[tauri::command]
#[specta::specta]
pub async fn steam_connect_poll(app: AppHandle) -> Res<ConnectStep> {
    let state = app.state::<AppState>();
    let mut slot = state.steam_qr.lock().await;
    let generation = state.steam_session_gen.load(Ordering::SeqCst);
    let step = poll_session(&state.http, &mut slot).await?;
    let approved = match step {
        PollStep::Waiting => return Ok(ConnectStep::Waiting),
        PollStep::Scanned => return Ok(ConnectStep::Scanned),
        PollStep::Rotated => {
            let url = slot
                .as_ref()
                .ok_or(msg!(notConnecting))?
                .challenge_url
                .clone();
            return Ok(ConnectStep::Rotated {
                svg: steam_login::qr_svg(&url)?,
            });
        }
        PollStep::Done(approved) => approved,
    };
    drop(slot);
    // Closed while Steam was answering: save nothing.
    if !should_save(generation, state.steam_gen.load(Ordering::SeqCst)) {
        return Err(msg!(notConnecting));
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
        return Err(msg!(steamNoAccount));
    }
    // Account first, so there is never a token without an account.
    let conn = cache::db::open(&state.dirs.db)?;
    cache::db::set_setting(&conn, settings::STEAM_ACCOUNT, Some(&approved.account))?;
    if let Err(e) = settings::set_steam_token(Some(&approved.refresh_token)) {
        _ = cache::db::set_setting(&conn, settings::STEAM_ACCOUNT, None);
        return Err(e);
    }
    state.lock().steam_attention = false;
    Ok(())
}

/// Polls the session in `slot`; an approved login ends it. An empty slot is an error.
async fn poll_session(http: &reqwest::Client, slot: &mut Option<QrSession>) -> Res<PollStep> {
    let session = slot.as_mut().ok_or(msg!(notConnecting))?;
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
#[specta::specta]
pub async fn steam_connect_cancel(app: AppHandle) {
    let state = app.state::<AppState>();
    // Before waiting for the lock, so a poll already in flight sees it when Steam answers.
    state.steam_gen.fetch_add(1, Ordering::SeqCst);
    cancel_session(&mut *state.steam_qr.lock().await);
}

/// Forgets the Steam login and the owned list, then rescans so only installed Steam games remain.
#[tauri::command]
#[specta::specta]
pub async fn steam_disconnect(app: AppHandle) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        settings::set_steam_token(None)?;
        let conn = cache::db::open(&state.dirs.db)?;
        cache::db::set_setting(&conn, settings::STEAM_ACCOUNT, None)?;
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

/// One answer to the password login. Never carries the password, the code or the token.
#[derive(Serialize, specta::Type)]
#[serde(tag = "step", rename_all = "camelCase")]
pub enum LoginStep {
    /// Approve the login: in the Steam app, or (`email`) with the link Steam emailed.
    Approve {
        email: bool,
    },
    /// Type the Steam Guard code sent by `kind`: "email" or "app".
    Code {
        kind: String,
    },
    Connected {
        account: String,
    },
    Failed {
        message: Msg,
    },
}

/// Logs in with account name and password. Returns the first step: Approve, Code, or the end.
#[tauri::command]
#[specta::specta]
pub async fn steam_password_login(
    app: AppHandle,
    account: String,
    password: String,
) -> Res<LoginStep> {
    let account = account.trim().to_owned();
    if account.is_empty() || password.is_empty() {
        return Ok(LoginStep::Failed {
            message: msg!(typeAccountAndPassword),
        });
    }
    let state = app.state::<AppState>();
    // Read, not bumped: the QR code in the same window keeps working.
    let generation = state.steam_gen.load(Ordering::SeqCst);
    let (code_tx, codes) = mpsc::unbounded_channel();
    let (steps, step_rx) = mpsc::unbounded_channel();
    // Replacing the sender aborts an older attempt.
    *state.steam_pw_codes.lock().expect("codes lock poisoned") = Some(code_tx);
    *state.steam_pw_steps.lock().await = Some(step_rx);
    let task_app = app.clone();
    // steam-vent's confirmation future isn't Send, so the login runs on its own thread.
    tauri::async_runtime::spawn_blocking(move || {
        let handler = UiConfirmation {
            steps: steps.clone(),
            codes,
        };
        let step = tauri::async_runtime::block_on(password_login(
            &task_app, generation, &account, &password, handler,
        ));
        _ = steps.send(step);
    });
    next_step(&app).await
}

/// Sends the Steam Guard code and returns what happened next.
#[tauri::command]
#[specta::specta]
pub async fn steam_password_code(app: AppHandle, code: String) -> Res<LoginStep> {
    let state = app.state::<AppState>();
    let sent = {
        let codes = state.steam_pw_codes.lock().expect("codes lock poisoned");
        codes
            .as_ref()
            .is_some_and(|tx| tx.send(code.trim().to_owned()).is_ok())
    };
    if !sent {
        return Err(msg!(notLoggingIn));
    }
    next_step(&app).await
}

/// Waits for the next step, like the end of an Approve.
#[tauri::command]
#[specta::specta]
pub async fn steam_password_next(app: AppHandle) -> Res<LoginStep> {
    next_step(&app).await
}

/// Stops the password login; one that completes anyway is not saved.
#[tauri::command]
#[specta::specta]
pub fn steam_login_cancel(app: AppHandle) {
    let state = app.state::<AppState>();
    cancel_password(&state.steam_gen, &state.steam_pw_codes);
}

async fn next_step(app: &AppHandle) -> Res<LoginStep> {
    let state = app.state::<AppState>();
    let mut steps = state.steam_pw_steps.lock().await;
    steps
        .as_mut()
        .ok_or(msg!(notLoggingIn))?
        .recv()
        .await
        .ok_or_else(|| msg!(notLoggingIn))
}

/// Bumps the generation, then drops the code sender so a waiting confirmation aborts.
fn cancel_password(generation: &AtomicU64, codes: &Mutex<Option<mpsc::UnboundedSender<String>>>) {
    generation.fetch_add(1, Ordering::SeqCst);
    *codes.lock().expect("codes lock poisoned") = None;
}

/// Machine tokens are never kept: nothing about the login is written to disk.
fn guard_store() -> impl GuardDataStore {
    NullGuardDataStore
}

async fn password_login(
    app: &AppHandle,
    generation: u64,
    account: &str,
    password: &str,
    handler: UiConfirmation,
) -> LoginStep {
    let login = async {
        let servers = ServerList::discover()
            .await
            .map_err(|_| msg!(cantReachSteam))?;
        let client = ClientInfo::new("Guild".into());
        Session::login(&servers, account, password, guard_store(), handler, &client)
            .await
            .map_err(|e| login_message(&e))
    };
    let connection = match login.await {
        Ok(c) => c,
        Err(message) => return LoginStep::Failed { message },
    };
    if !should_save(
        generation,
        app.state::<AppState>().steam_gen.load(Ordering::SeqCst),
    ) {
        return LoginStep::Failed {
            message: msg!(notLoggingIn),
        };
    }
    let approved = steam_login::Approved {
        account: account.to_owned(),
        refresh_token: connection.refresh_token().token().to_owned(),
    };
    drop(connection);
    if let Err(message) = save_login(app, approved) {
        return LoginStep::Failed { message };
    }
    refresh_owned(app.clone(), true);
    LoginStep::Connected {
        account: account.to_owned(),
    }
}

/// Steam's answer as a short message. Never includes what the user typed.
fn login_message(e: &ConnectionError) -> Msg {
    use ConnectionError::{LoginError as StoreLogin, Network};
    match e {
        StoreLogin(LoginError::InvalidCredentials)
        | Network(NetworkError::ApiError(EResult::InvalidPassword)) => {
            msg!(wrongPassword)
        }
        StoreLogin(LoginError::RateLimited)
        | Network(NetworkError::ApiError(EResult::RateLimitExceeded)) => {
            msg!(tooManyTries)
        }
        Network(NetworkError::ApiError(EResult::TwoFactorCodeMismatch)) => msg!(codeDidntWork),
        ConnectionError::Aborted => msg!(notLoggingIn),
        _ => msg!(cantLogInSteam),
    }
}

/// Tells the window what Steam Guard wants, then waits for its code. A cancel drops the sender: abort.
struct UiConfirmation {
    steps: mpsc::UnboundedSender<LoginStep>,
    codes: mpsc::UnboundedReceiver<String>,
}

impl AuthConfirmationHandler for UiConfirmation {
    fn handle_confirmation<'a>(
        &'a mut self,
        allowed: &[ConfirmationMethod],
    ) -> Box<dyn Future<Output = Option<ConfirmationAction>> + 'a> {
        let code = allowed.iter().find_map(ConfirmationMethod::token_type);
        let approve = allowed
            .iter()
            .any(|m| m.class() == ConfirmationMethodClass::Confirmation);
        // The app wins when Steam offers both.
        let by_email = approve
            && !allowed
                .iter()
                .any(|m| m.confirmation_type() == "device confirmation");
        let step = match &code {
            // Steam finishes on its own once approved in the app; a code still works meanwhile.
            _ if approve => LoginStep::Approve { email: by_email },
            Some(GuardTokenType::Email) => LoginStep::Code {
                kind: "email".into(),
            },
            Some(_) => LoginStep::Code { kind: "app".into() },
            None => return Box::new(std::future::ready(None)),
        };
        _ = self.steps.send(step);
        Box::new(async move {
            match (self.codes.recv().await, code) {
                (Some(c), Some(kind)) => Some(ConfirmationAction::GuardToken(
                    SteamGuardToken::new(c),
                    kind,
                )),
                _ => Some(ConfirmationAction::Abort),
            }
        })
    }
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
        assert_eq!(r.err(), Some(msg!(notConnecting)));
    }

    #[test]
    fn a_login_approved_after_cancel_is_not_saved() {
        assert!(should_save(3, 3));
        assert!(!should_save(3, 4));
    }

    #[test]
    fn connect_step_serializes_with_a_step_tag() {
        assert_eq!(
            serde_json::to_string(&ConnectStep::Connected {
                account: "a".into()
            })
            .unwrap(),
            r#"{"step":"connected","account":"a"}"#
        );
    }

    #[test]
    fn login_step_serializes_with_a_step_tag() {
        let json = |s: &LoginStep| serde_json::to_string(s).unwrap();
        assert_eq!(
            json(&LoginStep::Approve { email: true }),
            r#"{"step":"approve","email":true}"#
        );
        assert_eq!(
            json(&LoginStep::Code {
                kind: "email".into()
            }),
            r#"{"step":"code","kind":"email"}"#
        );
        assert_eq!(
            json(&LoginStep::Connected {
                account: "a".into()
            }),
            r#"{"step":"connected","account":"a"}"#
        );
    }

    #[test]
    fn a_password_login_approved_after_cancel_is_not_saved() {
        let generation = AtomicU64::new(7);
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let codes = Mutex::new(Some(tx));
        let started = generation.load(Ordering::SeqCst);
        cancel_password(&generation, &codes);
        assert!(!should_save(started, generation.load(Ordering::SeqCst)));
        // The waiting confirmation hears the cancel and aborts the login.
        assert!(codes.lock().unwrap().is_none());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn the_guard_store_stores_nothing() {
        use steam_vent::auth::GuardDataStore;
        let mut store = guard_store();
        tauri::async_runtime::block_on(async {
            store
                .store("someone", "machine-token".into())
                .await
                .unwrap();
            assert_eq!(store.load("someone").await.unwrap(), None);
        });
    }

    #[test]
    fn steam_errors_map_to_short_messages() {
        use steam_vent::{EResult, LoginError, NetworkError};
        let msg = |e: ConnectionError| login_message(&e);
        assert_eq!(
            msg(LoginError::InvalidCredentials.into()),
            msg!(wrongPassword)
        );
        assert_eq!(
            msg(NetworkError::ApiError(EResult::InvalidPassword).into()),
            msg!(wrongPassword)
        );
        assert_eq!(msg(LoginError::RateLimited.into()), msg!(tooManyTries));
        assert_eq!(
            msg(NetworkError::ApiError(EResult::TwoFactorCodeMismatch).into()),
            msg!(codeDidntWork)
        );
    }

    #[test]
    fn settings_payload_snapshot() {
        // `ToolView`'s shape is snapshotted in `tools`. Secrets only ever show as `…Set` flags.
        let view = SettingsView {
            language: "system".into(),
            ui_language: "en",
            steam: true,
            steam_library: SteamLibrary {
                account: Some("test-account".into()),
                attention: false,
            },
            epic: Some(StoreLogin::LoggedIn),
            epic_account: Some("Test".into()),
            epic_install_dir: Some(r"D:\Games\Epic".into()),
            gog: Some(StoreLogin::LoggedOut),
            gog_account: None,
            gog_install_dir: None,
            amazon: None,
            amazon_account: None,
            amazon_install_dir: None,
            default_install_dir: Some(r"C:\Users\Test\Games".into()),
            igdb_client_id: Some("test-client-id".into()),
            igdb_secret_set: true,
            steamgriddb_key_set: false,
            show_banner: true,
            ambient: true,
            cloud_saves: true,
            gog_cloud_saves: false,
            start_with_windows: false,
            linux: false,
            close_to_tray: true,
            notify_downloads: true,
            app_version: env!("CARGO_PKG_VERSION").into(),
            tools: Vec::new(),
            runner: None,
            cache_bytes: 1 << 20,
        };
        insta::assert_json_snapshot!(view, { ".appVersion" => "[version]", ".epicInstallDir" => "[path]" });
    }
}
