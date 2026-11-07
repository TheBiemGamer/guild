//! The settings page and the Epic login actions.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use steam_vent::auth::{
    AuthConfirmationHandler, ClientInfo, ConfirmationAction, ConfirmationMethod, ConfirmationMethodClass,
    GuardDataStore, GuardTokenType, NullGuardDataStore, SteamGuardToken,
};
use steam_vent::{Connection as Session, ConnectionError, EResult, LoginError, NetworkError, ServerList};
use tokio::sync::mpsc;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use super::library::{refresh_owned, rescan};
use super::{blocking, AppState, Res};
use crate::cache;
use crate::epic_login;
use crate::library::epic::{self, Login};
use crate::library::ProviderKind;
use crate::settings;
use crate::steam_login::{self, PollStep, QrSession};
use crate::steam_owned;
use crate::tools;

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

/// An owned Epic game that isn't installed, for "Import an existing install".
#[derive(Serialize)]
pub struct Importable {
    key: String,
    title: String,
}

/// Owned Epic games that aren't installed, by title.
#[tauri::command]
pub fn epic_importable(state: State<'_, AppState>) -> Vec<Importable> {
    let mut games: Vec<Importable> = state
        .lock()
        .games
        .iter()
        .filter(|g| g.provider == ProviderKind::Epic && !g.installed)
        .map(|g| Importable { key: g.key(), title: g.title.clone() })
        .collect();
    games.sort_by_key(|g| g.title.to_lowercase());
    games
}

/// Makes Legendary take an existing install of an owned Epic game (`key`) at `path`, then rescans Epic.
#[tauri::command]
pub async fn epic_import(app: AppHandle, key: String, path: String) -> Res<()> {
    let game = app
        .state::<AppState>()
        .game(&key)
        .filter(|g| g.provider == ProviderKind::Epic && !g.installed)
        .ok_or("This game can't be imported.")?;
    blocking(move || {
        let result = epic::import(&game.external_id, &path).map_err(|e| e.to_string());
        rescan(&app, true);
        result
    })
    .await
}

/// Imports the games the Epic Games Launcher installed, then rescans Epic.
#[tauri::command]
pub async fn epic_egl_sync(app: AppHandle) -> Res<()> {
    blocking(move || {
        let result = epic::egl_sync().map_err(|e| e.to_string());
        rescan(&app, true);
        result
    })
    .await
}

/// Downloads and checks a store tool (`tools.rs`), sending each new whole percent. Legendary then rescans Epic,
/// so the card moves on to Log in.
#[tauri::command]
pub async fn install_tool(app: AppHandle, name: String, on_percent: Channel<u8>) -> Res<()> {
    let tool = tools::find(&name).ok_or("Unknown tool.")?;
    let http = app.state::<AppState>().http.clone();
    tools::install(&http, tool, |percent| {
        let _ = on_percent.send(percent);
    })
    .await?;
    if tool.name == tools::LEGENDARY.name {
        blocking(move || rescan(&app, true)).await;
    }
    Ok(())
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
    /// Whether Steam is installed on this PC.
    steam: bool,
    steam_library: SteamLibrary,
    epic: Option<Login>,
    /// The logged-in Epic account's display name, when Legendary's session file has one.
    epic_account: Option<String>,
    epic_install_dir: Option<String>,
    igdb_client_id: Option<String>,
    /// Whether a secret is saved; its value never leaves Credential Manager.
    igdb_secret_set: bool,
    /// Whether a SteamGridDB API key is saved; like the secret, it never leaves Credential Manager.
    steamgriddb_key_set: bool,
    show_banner: bool,
    ambient: bool,
    cloud_saves: bool,
    /// Read from the autostart entry itself, so it can't drift from what Windows runs.
    start_with_windows: bool,
    close_to_tray: bool,
    notify_downloads: bool,
    app_version: String,
    legendary_version: Option<String>,
    tools: Vec<tools::ToolView>,
    cache_bytes: u64,
}

#[tauri::command]
pub async fn settings_view(app: AppHandle) -> Res<SettingsView> {
    blocking(move || {
        let state = app.state::<AppState>();
        let conn = cache::db::open(&state.dirs.db).map_err(|e| e.to_string())?;
        let saved = settings::load(&conn);
        let epic = state.lock().epic_login;
        let epic_account = if epic == Some(Login::LoggedIn) { crate::epic_playtime::account_name() } else { None };
        let cache_bytes = dir_size(&state.dirs.thumbs) + dir_size(&state.dirs.art);
        let steam = steamlocate::SteamDir::locate().is_ok();
        let steam_library = SteamLibrary {
            account: cache::db::setting(&conn, settings::STEAM_ACCOUNT).ok().flatten(),
            attention: state.lock().steam_attention,
        };
        Ok(SettingsView {
            steam,
            steam_library,
            epic,
            epic_account,
            epic_install_dir: saved.epic_install_dir.map(|p| p.to_string_lossy().into_owned()),
            igdb_client_id: saved.igdb_client_id,
            igdb_secret_set: settings::secret().is_some(),
            steamgriddb_key_set: settings::grid_key().is_some(),
            show_banner: saved.show_banner,
            ambient: saved.ambient,
            cloud_saves: saved.cloud_saves,
            start_with_windows: app.autolaunch().is_enabled().unwrap_or(false),
            close_to_tray: saved.close_to_tray,
            notify_downloads: saved.notify_downloads,
            app_version: app.package_info().version.to_string(),
            legendary_version: tools::version(&tools::LEGENDARY),
            tools: tools::view(),
            cache_bytes,
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
        if name == settings::START_WITH_WINDOWS {
            settings::validate(&name, value.as_deref())?;
            let autostart = app.autolaunch();
            let done = if value.as_deref() == Some("true") { autostart.enable() } else { autostart.disable() };
            return done.map_err(|e| format!("Couldn't change the Windows startup entry: {e}"));
        }
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

/// Saves the SteamGridDB API key in Credential Manager, or removes it for `None`.
#[tauri::command]
pub async fn set_steamgriddb_key(value: Option<String>) -> Res<()> {
    blocking(move || settings::set_grid_key(value.as_deref().map(str::trim).filter(|v| !v.is_empty()))).await
}

/// SteamGridDB's API key page, where the owner gets a free key.
#[tauri::command]
pub fn open_steamgriddb() -> Res<()> {
    crate::library::open_url("https://www.steamgriddb.com/profile/preferences/api").map_err(|e| e.to_string())
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

/// The page where the owner creates the Twitch application IGDB needs.
#[tauri::command]
pub fn open_twitch_console() -> Res<()> {
    crate::library::open_url("https://dev.twitch.tv/console/apps").map_err(|e| e.to_string())
}

/// IGDB's site, for the "Data from IGDB" credit next to how long to beat.
#[tauri::command]
pub fn open_igdb() -> Res<()> {
    crate::library::open_url("https://www.igdb.com").map_err(|e| e.to_string())
}

/// Opens the folder with launcher.log in File Explorer.
#[tauri::command]
pub fn open_log_folder(state: State<'_, AppState>) -> Res<()> {
    let dir = &state.dirs.logs;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer.exe").arg(dir).spawn().map(drop).map_err(|e| e.to_string())
}

/// Opens a new GitHub bug report with the app and Windows versions filled in.
#[tauri::command]
pub fn report_problem(app: AppHandle) -> Res<()> {
    use crate::library::percent_encode;
    let version = app.package_info().version.to_string();
    let os = crate::platform::windows_version();
    let url = format!(
        "https://github.com/TheBiemGamer/game-launcher/issues/new?template=bug_report.yml&version={}&os={}",
        percent_encode(&version),
        percent_encode(&os)
    );
    crate::library::open_url(&url).map_err(|e| e.to_string())
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

/// One answer to the password login. Never carries the password, the code or the token.
#[derive(Serialize)]
#[serde(tag = "step", rename_all = "camelCase")]
pub enum LoginStep {
    /// Approve the login: in the Steam app, or (`email`) with the link Steam emailed.
    Approve { email: bool },
    /// Type the Steam Guard code sent by `kind`: "email" or "app".
    Code { kind: String },
    Connected { account: String },
    Failed { message: String },
}

/// Logs in with account name and password. Returns the first step: Approve, Code, or the end.
#[tauri::command]
pub async fn steam_password_login(app: AppHandle, account: String, password: String) -> Res<LoginStep> {
    let account = account.trim().to_owned();
    if account.is_empty() || password.is_empty() {
        return Ok(LoginStep::Failed { message: "Type your account name and password.".into() });
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
        let handler = UiConfirmation { steps: steps.clone(), codes };
        let step = tauri::async_runtime::block_on(password_login(&task_app, generation, &account, &password, handler));
        _ = steps.send(step);
    });
    next_step(&app).await
}

/// Sends the Steam Guard code and returns what happened next.
#[tauri::command]
pub async fn steam_password_code(app: AppHandle, code: String) -> Res<LoginStep> {
    let state = app.state::<AppState>();
    let sent = {
        let codes = state.steam_pw_codes.lock().expect("codes lock poisoned");
        codes.as_ref().is_some_and(|tx| tx.send(code.trim().to_owned()).is_ok())
    };
    if !sent {
        return Err("Not logging in.".into());
    }
    next_step(&app).await
}

/// Waits for the next step, like the end of an Approve.
#[tauri::command]
pub async fn steam_password_next(app: AppHandle) -> Res<LoginStep> {
    next_step(&app).await
}

/// Stops the password login; one that completes anyway is not saved.
#[tauri::command]
pub fn steam_login_cancel(app: AppHandle) {
    let state = app.state::<AppState>();
    cancel_password(&state.steam_gen, &state.steam_pw_codes);
}

async fn next_step(app: &AppHandle) -> Res<LoginStep> {
    let state = app.state::<AppState>();
    let mut steps = state.steam_pw_steps.lock().await;
    steps.as_mut().ok_or("Not logging in.")?.recv().await.ok_or_else(|| "Not logging in.".into())
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

async fn password_login(app: &AppHandle, generation: u64, account: &str, password: &str, handler: UiConfirmation) -> LoginStep {
    let login = async {
        let servers = ServerList::discover().await.map_err(|_| "Couldn't reach Steam.".to_owned())?;
        let client = ClientInfo::new("Game Launcher".into());
        Session::login(&servers, account, password, guard_store(), handler, &client).await.map_err(|e| login_message(&e))
    };
    let connection = match login.await {
        Ok(c) => c,
        Err(message) => return LoginStep::Failed { message },
    };
    if !should_save(generation, app.state::<AppState>().steam_gen.load(Ordering::SeqCst)) {
        return LoginStep::Failed { message: "Not logging in.".into() };
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
    LoginStep::Connected { account: account.to_owned() }
}

/// Steam's answer as a short message. Never includes what the user typed.
fn login_message(e: &ConnectionError) -> String {
    use ConnectionError::{LoginError as Login, Network};
    match e {
        Login(LoginError::InvalidCredentials) | Network(NetworkError::ApiError(EResult::InvalidPassword)) => {
            "Wrong account name or password."
        }
        Login(LoginError::RateLimited) | Network(NetworkError::ApiError(EResult::RateLimitExceeded)) => {
            "Too many tries. Wait a while, then try again."
        }
        Network(NetworkError::ApiError(EResult::TwoFactorCodeMismatch)) => "That code didn't work.",
        ConnectionError::Aborted => "Not logging in.",
        _ => "Couldn't log in to Steam. Try again, or scan the QR code.",
    }
    .into()
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
        let approve = allowed.iter().any(|m| m.class() == ConfirmationMethodClass::Confirmation);
        // The app wins when Steam offers both.
        let by_email = approve && !allowed.iter().any(|m| m.confirmation_type() == "device confirmation");
        let step = match &code {
            // Steam finishes on its own once approved in the app; a code still works meanwhile.
            _ if approve => LoginStep::Approve { email: by_email },
            Some(GuardTokenType::Email) => LoginStep::Code { kind: "email".into() },
            Some(_) => LoginStep::Code { kind: "app".into() },
            None => return Box::new(std::future::ready(None)),
        };
        _ = self.steps.send(step);
        Box::new(async move {
            match (self.codes.recv().await, code) {
                (Some(c), Some(kind)) => Some(ConfirmationAction::GuardToken(SteamGuardToken::new(c), kind)),
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

    #[test]
    fn login_step_serializes_with_a_step_tag() {
        let json = |s: &LoginStep| serde_json::to_string(s).unwrap();
        assert_eq!(json(&LoginStep::Approve { email: true }), r#"{"step":"approve","email":true}"#);
        assert_eq!(json(&LoginStep::Code { kind: "email".into() }), r#"{"step":"code","kind":"email"}"#);
        assert_eq!(json(&LoginStep::Connected { account: "a".into() }), r#"{"step":"connected","account":"a"}"#);
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
            store.store("someone", "machine-token".into()).await.unwrap();
            assert_eq!(store.load("someone").await.unwrap(), None);
        });
    }

    #[test]
    fn steam_errors_map_to_short_messages() {
        use steam_vent::{EResult, LoginError, NetworkError};
        let msg = |e: ConnectionError| login_message(&e);
        assert_eq!(msg(LoginError::InvalidCredentials.into()), "Wrong account name or password.");
        assert_eq!(msg(NetworkError::ApiError(EResult::InvalidPassword).into()), "Wrong account name or password.");
        assert_eq!(msg(LoginError::RateLimited.into()), "Too many tries. Wait a while, then try again.");
        assert_eq!(msg(NetworkError::ApiError(EResult::TwoFactorCodeMismatch).into()), "That code didn't work.");
    }
}
