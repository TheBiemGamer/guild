//! Amazon Games through Nile (https://github.com/imLinguin/nile, 1.2.0). nile makes every Amazon call and keeps its
//! own files in `<data>/amazon/nile` (set through `NILE_CONFIG_PATH`, so a Heroic or standalone nile in `%APPDATA%\nile`
//! is never touched): the account in `current_user.json`, the tokens in `<md5(user_id)>.enc` (never read, copied or
//! logged here), the owned games in `library.json` and the installs in `installed.json`, which is our install record too.
//! This process holds no Amazon token and sends Amazon nothing. Every nile run but `--version` holds `NILE`: each one
//! rewrites `installed.json`, and a token refresh rewrites the `.enc` file non-atomically.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use super::{Art, Error, Game, Image, Progress, Provider, ProviderKind};
use crate::cache::{Dirs, db};
use crate::info::{self, GameInfo};
use crate::tools;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Login {
    /// `nile.exe` is in none of the places `tools::NILE.exe()` looks.
    NoNile,
    LoggedOut,
    LoggedIn,
    /// The last `library sync` failed for another reason than the login, e.g. offline: keep the last known games.
    Unreachable,
}

/// Held by every nile run but `--version`, so only one runs at a time. A scan only tries it.
static NILE: Mutex<()> = Mutex::new(());
/// What the last `library sync` said about the login, for a scan that skips its sync while nile is busy.
static SYNCED: Mutex<Option<Login>> = Mutex::new(None);

/// The first run of a nile build downloads Amazon's FuelSDK before anything else, so these are generous. nile sets no
/// HTTP timeouts of its own: a dead connection would hang it forever.
const SYNC_TIMEOUT: Duration = Duration::from_secs(120);
const LOGOUT_TIMEOUT: Duration = Duration::from_secs(60);

const UNREACHABLE: &str = "Can't reach Amazon. Try again later.";

fn lock() -> MutexGuard<'static, ()> {
    NILE.lock().unwrap_or_else(|e| e.into_inner())
}

fn try_lock() -> Option<MutexGuard<'static, ()>> {
    match NILE.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(e)) => Some(e.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

fn set_synced(login: Login) {
    *SYNCED.lock().unwrap_or_else(|e| e.into_inner()) = Some(login);
}

/// `<data>/amazon`, what `NILE_CONFIG_PATH` names. nile adds `nile` itself.
fn dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| Dirs::new().amazon)
}

/// nile's own folder: `<data>/amazon/nile`.
fn cfg() -> PathBuf {
    dir().join("nile")
}

fn nile(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    nile_in(dir(), args)
}

/// nile with its config under `config`. Never `-d`: its debug log prints requests and arguments.
fn nile_in(config: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    let mut command = Command::new(tools::NILE.exe());
    command.args(args).env("NILE_CONFIG_PATH", config).stdin(Stdio::null());
    crate::platform::no_window(&mut command);
    command
}

/// Runs `command` to its end, or kills it at `timeout` and returns `None`. Callers hold `NILE`.
fn run_short(mut command: Command, timeout: Duration) -> Option<Output> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .inspect_err(|e| log!("nile: {e}"))
        .ok()?;
    let read = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    };
    let stdout = read(Box::new(child.stdout.take()?));
    let stderr = read(Box::new(child.stderr.take()?));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(100)),
            other => {
                if let Err(e) = other {
                    log!("nile: {e}");
                } else {
                    log!("nile: no answer in {} s", timeout.as_secs());
                }
                // nile is a PyInstaller build: a bootloader and the real program as its child.
                if let Err(e) = crate::platform::kill_tree(child.id()) {
                    log!("stop nile: {e}");
                    _ = child.kill();
                }
                _ = child.wait();
                return None;
            }
        }
    };
    Some(Output { status, stdout: stdout.join().unwrap_or_default(), stderr: stderr.join().unwrap_or_default() })
}

/// The last error in a nile log: an `ERROR [NAME]:\t ` line's message, or a traceback's exception line.
fn last_error(log: &str) -> Option<&str> {
    log.lines().rev().find_map(|line| match line.split_once("]:\t") {
        Some((head, message)) if head.starts_with("ERROR [") => Some(message.trim()),
        _ => line.split(':').next().filter(|head| head.ends_with("Error") && !head.contains(' ')).map(|_| line.trim()),
    })
}

/// What `nile library sync` says about the login. Only a refresh Amazon answered (`<Response [400]>`) is a logout; a
/// refresh that failed on the network, a traceback or no answer at all is `Unreachable`.
fn classify_sync(output: Option<&Output>) -> Login {
    let Some(output) = output else { return Login::Unreachable };
    let log = String::from_utf8_lossy(&output.stderr);
    if output.status.success() && log.contains("Successfully synced the library") {
        Login::LoggedIn
    } else if log.contains("User not logged in") || log.contains("Failed to refresh the token <Response [") {
        Login::LoggedOut
    } else {
        Login::Unreachable
    }
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// nile's `current_user.json`: `{"name", "user_id"}`, plain.
fn current_user(cfg: &Path) -> Option<Value> {
    read_json(&cfg.join("current_user.json"))
}

fn has_user(cfg: &Path) -> bool {
    current_user(cfg).is_some_and(|u| u["user_id"].as_str().is_some_and(|id| !id.trim().is_empty()))
}

fn name_of(user: &Value) -> Option<String> {
    user["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned)
}

/// The logged-in account's first name, from nile's file. Local, so it needs no network and no cache.
pub fn account_name() -> Option<String> {
    current_user(&cfg()).as_ref().and_then(name_of)
}

/// The login state: no Nile, no account in its file, else what the last sync said. Before any sync this run (a
/// download held nile through the first scan), nile was logged in when that download started.
pub fn login() -> Login {
    if tools::version(&tools::NILE).is_none() {
        Login::NoNile
    } else if !has_user(&cfg()) {
        Login::LoggedOut
    } else {
        SYNCED.lock().unwrap_or_else(|e| e.into_inner()).unwrap_or(Login::LoggedIn)
    }
}

/// What `nile auth --login --non-interactive` prints: Amazon's sign-in page for a new device and what `register`
/// needs after it. No `Debug`: the PKCE verifier must not reach a log.
#[derive(Deserialize)]
pub struct SignIn {
    client_id: String,
    code_verifier: String,
    serial: String,
    pub url: String,
}

/// Serde errors are dropped unread: they could quote the verifier.
fn parse_sign_in(stdout: &[u8]) -> Option<SignIn> {
    serde_json::from_slice(stdout).ok()
}

/// Starts a device login. nile won't start one over an account it already has, so a stale one (rejected or
/// unreachable) is logged out first.
pub fn sign_in() -> Result<SignIn, Error> {
    if has_user(&cfg()) {
        log_out()?;
    }
    let _nile = lock();
    let output = run_short(nile(["auth", "--login", "--non-interactive"]), SYNC_TIMEOUT).ok_or(UNREACHABLE)?;
    // stdout carries the verifier: parsed, never logged.
    parse_sign_in(&output.stdout).ok_or_else(|| {
        let log = String::from_utf8_lossy(&output.stderr);
        log!("nile auth --login: {}", last_error(&log).unwrap_or("no login page"));
        "Couldn't start the Amazon login. Try again.".into()
    })
}

/// Registers this device with the code from the login window. Nothing of it is logged: the arguments hold the code and
/// the verifier, stderr the device id and stdout Amazon's answer.
pub fn register(sign_in: &SignIn, code: &str) -> Result<(), Error> {
    let _nile = lock();
    let args = [
        "register",
        "--code",
        code,
        "--client-id",
        &sign_in.client_id,
        "--code-verifier",
        &sign_in.code_verifier,
        "--serial",
        &sign_in.serial,
    ];
    let output = run_short(nile(args), SYNC_TIMEOUT).ok_or(UNREACHABLE)?;
    // Sic: nile's own spelling.
    if String::from_utf8_lossy(&output.stderr).contains("Succesfully registered a device") {
        set_synced(Login::LoggedIn);
        Ok(())
    } else {
        log!("nile register: the device wasn't registered ({})", output.status);
        Err("Amazon didn't accept the login. Try again.".into())
    }
}

/// Deregisters this device and forgets the login. nile deletes its files only when Amazon answers, so whatever is left
/// goes here, online or not; the device then stays in the Amazon account's device list.
pub fn log_out() -> Result<(), Error> {
    let _nile = lock();
    if run_short(nile(["auth", "--logout"]), LOGOUT_TIMEOUT).is_none() {
        log!("nile auth --logout: no answer");
    }
    forget_login(&cfg()).map_err(|e| format!("Couldn't log out of Amazon Games: {e}"))?;
    set_synced(Login::LoggedOut);
    Ok(())
}

/// Deletes the account, its tokens, and the owned games with their sync point (syncs are incremental, so another
/// account would inherit the list). Installs, manifests and the SDK stay.
fn forget_login(cfg: &Path) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(cfg) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        entries => entries?,
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(&*name, "current_user.json" | "library.json" | "syncpoint.raw") || name.ends_with(".enc") {
            match std::fs::remove_file(entry.path()) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
    }
    Ok(())
}

pub struct Amazon;

/// Listed by `list`; installs and launches come through `commands::game`.
const NOT_HERE: &str = "Amazon games can't be installed or played here yet.";

impl Provider for Amazon {
    fn list_games(&self) -> Result<Vec<Game>, Error> {
        Err(NOT_HERE.into())
    }

    fn launch(&self, _game: &Game) -> Result<(), Error> {
        Err(NOT_HERE.into())
    }

    fn install(&self, _game: &Game) -> Result<(), Error> {
        Err(NOT_HERE.into())
    }

    fn uninstall(&self, _game: &Game) -> Result<(), Error> {
        Err(NOT_HERE.into())
    }

    fn running_game(&self) -> Option<String> {
        None
    }

    fn download_progress(&self, _game: &Game) -> Option<Progress> {
        None
    }
}

/// The owned Amazon games: `nile library sync`, then its `library.json`. Skipped while another nile run holds `NILE`
/// (a download), so the cached games and the login state stay. Blocks: call from a blocking thread.
pub fn list(dirs: &Dirs) -> Result<Vec<Game>, Error> {
    if tools::version(&tools::NILE).is_none() {
        return Err("Nile isn't installed".into());
    }
    let cfg = cfg();
    if !has_user(&cfg) {
        set_synced(Login::LoggedOut);
        return Err("not logged in".into());
    }
    let (library, installed) = {
        let _nile = try_lock().ok_or("busy")?;
        let output = run_short(nile(["library", "sync"]), SYNC_TIMEOUT);
        let login = classify_sync(output.as_ref());
        set_synced(login);
        if login != Login::LoggedIn {
            if let Some(error) = output.as_ref().and_then(|o| last_error(&String::from_utf8_lossy(&o.stderr)).map(str::to_owned)) {
                log!("nile library sync: {error}");
            }
            return Err(format!("{login:?}").into());
        }
        let library = read_json(&cfg.join("library.json")).ok_or("no library.json")?;
        // Under the lock too: every nile run rewrites it in place.
        (library, parse_installed(&std::fs::read(cfg.join("installed.json")).unwrap_or_default()))
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let covers = crate::gamesdb::covers(&db::open(&dirs.db)?, now);
    Ok(games(&library, &installed, &covers))
}

/// One `installed.json` entry; keyed by product id.
#[derive(Deserialize)]
struct Installed {
    id: String,
    path: PathBuf,
    /// The manifest's byte total.
    size: Option<u64>,
}

/// `installed.json` by product id. Unreadable entries are left out; a missing file is no installs.
fn parse_installed(bytes: &[u8]) -> HashMap<String, Installed> {
    let entries: Vec<Value> = serde_json::from_slice(bytes).unwrap_or_default();
    entries.into_iter().filter_map(|e| serde_json::from_value::<Installed>(e).ok()).map(|i| (i.id.clone(), i)).collect()
}

/// An https URL; anything else isn't fetched.
fn https(v: &Value) -> Option<String> {
    v.as_str().filter(|u| u.starts_with("https://")).map(str::to_owned)
}

/// The entitlements in `library.json` (already one per product) as games. An entry without a title is left out: nile
/// itself treats that as a broken sync. A game is installed when `installed` has its product id with an existing
/// folder. Art follows Heroic: header `backgroundUrl2`, else the icon; hero `backgroundUrl1`, else `backgroundUrl2`;
/// the portrait cover is GamesDB's (`covers`, by key).
fn games(library: &Value, installed: &HashMap<String, Installed>, covers: &HashMap<String, String>) -> Vec<Game> {
    library
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entitlement| {
            let product = &entitlement["product"];
            let id = product["id"].as_str().filter(|id| !id.is_empty())?;
            let title = product["title"].as_str().map(str::trim).filter(|t| !t.is_empty())?;
            let key = ProviderKind::Amazon.key(id);
            let install = installed.get(id).filter(|i| i.path.is_dir());
            let detail = &product["productDetail"];
            let details = &detail["details"];
            let image = |url: Option<String>| Image { path: None, url };
            Some(Game {
                provider: ProviderKind::Amazon,
                external_id: id.to_owned(),
                title: title.to_owned(),
                installed: install.is_some(),
                install_dir: install.map(|i| i.path.clone()),
                // Playtime comes from `epic_playtime::merge`: the sessions timed here.
                playtime_minutes: None,
                last_played: None,
                size_bytes: install.and_then(|i| i.size),
                art: Art {
                    cover: image(covers.get(&key).cloned()),
                    header: image(https(&details["backgroundUrl2"]).or_else(|| https(&detail["iconUrl"]))),
                    hero: image(https(&details["backgroundUrl1"]).or_else(|| https(&details["backgroundUrl2"]))),
                },
                dlc: Vec::new(),
                // shortcut: updates come with the Amazon install (`nile list-updates`).
                update_available: false,
                // Amazon has no cloud saves.
                cloud_saves: false,
            })
        })
        .collect()
}

/// The game page's details of an owned product, from `library.json`.
pub fn details(id: &str) -> Option<GameInfo> {
    let library = read_json(&cfg().join("library.json"))?;
    let entitlement = library.as_array()?.iter().find(|e| e["product"]["id"] == id)?;
    let parsed = parse_details(&entitlement["product"]["productDetail"]["details"]);
    (parsed != GameInfo::default()).then_some(parsed)
}

fn parse_details(details: &Value) -> GameInfo {
    let text = |field: &str| details[field].as_str().map(info::strip_html).filter(|t| !t.is_empty());
    let strings = |field: &str| details[field].as_array().into_iter().flatten().filter_map(Value::as_str);
    GameInfo {
        description: text("shortDescription").unwrap_or_default(),
        genres: strings("genres").map(str::to_owned).collect(),
        developer: text("developer"),
        publisher: text("publisher"),
        released: details["releaseDate"].as_str().and_then(|d| d.split('T').next()).filter(|d| !d.is_empty()).map(str::to_owned),
        screenshots: strings("screenshots").filter(|u| u.starts_with("https://")).take(info::MAX_SCREENSHOTS).map(str::to_owned).collect(),
        has_dlc: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/amazon").join(name)).unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("launcher-amazon-{name}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn output(code: u32, stderr: &[u8]) -> Output {
        use std::os::windows::process::ExitStatusExt;
        Output { status: std::process::ExitStatus::from_raw(code), stdout: Vec::new(), stderr: stderr.to_vec() }
    }

    #[test]
    fn sync_output_is_classified() {
        let sync = |code, name| classify_sync(Some(&output(code, &fixture(name))));
        assert_eq!(sync(0, "nile-sync-ok-stderr.txt"), Login::LoggedIn);
        assert_eq!(sync(1, "nile-sync-not-logged-in-stderr.txt"), Login::LoggedOut);
        assert_eq!(sync(1, "nile-sync-refresh-rejected-stderr.txt"), Login::LoggedOut);
        assert_eq!(sync(1, "nile-sync-offline-stderr.txt"), Login::Unreachable);
        // nile exits 0 after logged errors: only the success line counts.
        assert_eq!(classify_sync(Some(&output(0, b"INFO [LIBRARY]:\t Synchronizing library\n"))), Login::Unreachable);
        assert_eq!(classify_sync(Some(&output(1, b"Successfully synced the library"))), Login::Unreachable);
        // The deadline passed.
        assert_eq!(classify_sync(None), Login::Unreachable);
    }

    #[test]
    fn last_error_is_the_message_or_the_exception() {
        let text = |name| String::from_utf8(fixture(name)).unwrap();
        assert_eq!(last_error(&text("nile-sync-not-logged-in-stderr.txt")), Some("User not logged in"));
        assert_eq!(
            last_error(&text("nile-sync-offline-stderr.txt")),
            Some("requests.exceptions.ConnectionError: HTTPSConnectionPool(host='gaming.amazon.com', port=443): Max retries exceeded")
        );
        assert_eq!(last_error(&text("nile-sync-refresh-rejected-stderr.txt")), Some("KeyError: 'entitlements'"));
        assert_eq!(last_error(&text("nile-sync-ok-stderr.txt")), None);
    }

    #[test]
    fn sign_in_output_parses() {
        let sign_in = parse_sign_in(&fixture("sample-auth-login-stdout.txt")).unwrap();
        assert!(sign_in.url.starts_with("https://amazon.com/ap/signin?"));
        assert_eq!((sign_in.serial.as_str(), sign_in.code_verifier.as_str()), ("TESTSERIAL0000000000000000000000", "test-code-verifier"));
        assert!(sign_in.client_id.ends_with("234132554d56484f58375550345637"));
        // Already logged in, nile prints nothing.
        assert!(parse_sign_in(b"").is_none());
        assert!(parse_sign_in(b"{\"url\": \"https://amazon.com/ap/signin\"}").is_none());
    }

    #[test]
    fn current_user_needs_a_user_id() {
        let dir = temp_dir("user");
        assert!(!has_user(&dir));
        std::fs::write(dir.join("current_user.json"), fixture("sample-current_user.json")).unwrap();
        assert!(has_user(&dir));
        assert_eq!(current_user(&dir).as_ref().and_then(name_of).as_deref(), Some("Test"));
        std::fs::write(dir.join("current_user.json"), r#"{"name": " ", "user_id": " "}"#).unwrap();
        assert!(!has_user(&dir));
        assert_eq!(current_user(&dir).as_ref().and_then(name_of), None);
        std::fs::write(dir.join("current_user.json"), "{not json").unwrap();
        assert!(!has_user(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn logging_out_forgets_the_account_and_keeps_the_installs() {
        let dir = temp_dir("logout");
        assert!(forget_login(&dir.join("missing")).is_ok());
        for file in ["current_user.json", "0123abcd.enc", "library.json", "syncpoint.raw", "installed.json"] {
            std::fs::write(dir.join(file), "x").unwrap();
        }
        std::fs::create_dir_all(dir.join("manifests")).unwrap();
        std::fs::create_dir_all(dir.join("SDK").join("Amazon Games Services")).unwrap();
        forget_login(&dir).unwrap();
        let mut left: Vec<String> =
            std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["SDK", "installed.json", "manifests"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nile_runs_with_its_own_config_folder() {
        let command = nile(["library", "sync"]);
        let args: Vec<String> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["library", "sync"]);
        let config = command.get_envs().find(|(k, _)| *k == "NILE_CONFIG_PATH").and_then(|(_, v)| v).unwrap();
        assert_eq!(Path::new(config), dir());
        assert!(dir().ends_with("amazon"));
    }

    #[test]
    fn installed_json_is_read_by_product_id() {
        let installed = parse_installed(&fixture("sample-installed.json"));
        let game = &installed["amzn1.adg.product.4c6f3fb8-e68a-4706-a8eb-490905dfa633"];
        assert_eq!(game.path, PathBuf::from(r"C:\Users\Test\Games\Last Day of June"));
        assert_eq!(game.size, Some(4_200_000_000));
        assert!(parse_installed(b"").is_empty());
        assert!(parse_installed(b"[{\"id\": \"x\"}, {\"id\": \"y\", \"path\": \"C:\\\\y\"}]").keys().eq(["y"]));
    }

    #[test]
    fn library_json_becomes_games() {
        let dir = temp_dir("library");
        let library: Value = serde_json::from_slice(&fixture("sample-library.json")).unwrap();
        let ldoj = "amzn1.adg.product.4c6f3fb8-e68a-4706-a8eb-490905dfa633";
        let installed = HashMap::from([
            (ldoj.to_string(), Installed { id: ldoj.into(), path: dir.clone(), size: Some(4_200_000_000) }),
            // A folder that's gone isn't an install.
            (
                "amzn1.adg.product.e9b762cd-e130-4c5b-aaf5-070d785d1cd2".to_string(),
                Installed { id: "e9".into(), path: dir.join("gone"), size: Some(1) },
            ),
        ]);
        let covers = HashMap::from([(format!("amazon-{ldoj}"), "https://images.gog.com/c.jpg".to_string())]);
        let games = games(&library, &installed, &covers);
        // The untitled third entitlement is left out.
        assert_eq!(games.len(), 2);
        let (ldoj_game, dote) = (&games[0], &games[1]);
        assert_eq!(ldoj_game.key(), format!("amazon-{ldoj}"));
        assert_eq!(ldoj_game.title, "Last Day of June");
        assert!(ldoj_game.installed);
        assert_eq!((ldoj_game.install_dir.as_deref(), ldoj_game.size_bytes), (Some(dir.as_path()), Some(4_200_000_000)));
        assert_eq!(ldoj_game.art.cover.url.as_deref(), Some("https://images.gog.com/c.jpg"));
        assert_eq!(ldoj_game.art.header.url.as_deref(), Some("https://example.invalid/TEST-SKU-1/bg2.jpg"));
        assert_eq!(ldoj_game.art.hero.url.as_deref(), Some("https://example.invalid/TEST-SKU-1/bg1.jpg"));
        assert!(!ldoj_game.update_available && !ldoj_game.cloud_saves && ldoj_game.dlc.is_empty());
        assert_eq!(dote.title, "Dungeon of the ENDLESS - Definitive Edition");
        assert!(!dote.installed && dote.install_dir.is_none() && dote.size_bytes.is_none());
        assert_eq!(dote.art.cover.url, None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn art_falls_back_to_the_icon_and_the_other_background() {
        let library = serde_json::json!([{"product": {"id": "p", "title": " T ", "productDetail": {
            "iconUrl": "https://example.invalid/icon.png",
            "details": {"backgroundUrl2": "https://example.invalid/bg2.jpg", "backgroundUrl1": "http://example.invalid/bg1.jpg"}
        }}}]);
        let game = &games(&library, &HashMap::new(), &HashMap::new())[0];
        assert_eq!(game.title, "T");
        assert_eq!(game.art.header.url.as_deref(), Some("https://example.invalid/bg2.jpg"));
        assert_eq!(game.art.hero.url.as_deref(), Some("https://example.invalid/bg2.jpg"));
        let library = serde_json::json!([{"product": {"id": "p", "title": "T", "productDetail": {"iconUrl": "https://example.invalid/icon.png"}}}]);
        let game = &games(&library, &HashMap::new(), &HashMap::new())[0];
        assert_eq!(game.art.header.url.as_deref(), Some("https://example.invalid/icon.png"));
        assert_eq!(game.art.hero.url, None);
    }

    #[test]
    fn details_map_onto_the_game_page() {
        let library: Value = serde_json::from_slice(&fixture("sample-library.json")).unwrap();
        let parsed = parse_details(&library[0]["product"]["productDetail"]["details"]);
        assert_eq!(parsed.description, "Sample description.");
        assert_eq!(parsed.genres, ["Adventure"]);
        assert_eq!((parsed.developer.as_deref(), parsed.publisher.as_deref()), (Some("Sample Developer"), Some("Sample Publisher")));
        assert_eq!(parsed.released.as_deref(), Some("2017-08-31"));
        assert_eq!(parsed.screenshots, ["https://example.invalid/TEST-SKU-1/shot1.jpg"]);
        assert_eq!(parse_details(&Value::Null), GameInfo::default());
    }

    /// Needs Nile installed from Settings; runs it against a temp config folder, never the app's.
    /// `cargo test real_nile_logged_out -- --ignored`
    #[test]
    #[ignore]
    fn real_nile_logged_out() {
        let config = temp_dir("real");
        let version = Command::new(tools::NILE.exe()).arg("--version").output().unwrap();
        assert!(String::from_utf8_lossy(&version.stdout).starts_with(tools::NILE.version));
        let status = run_short(nile_in(&config, ["auth", "--status"]), SYNC_TIMEOUT).unwrap();
        assert!(String::from_utf8_lossy(&status.stdout).contains("\"LoggedIn\": false"));
        assert!(config.join("nile").join("SDK").join("Amazon Games Services").is_dir());
        let sync = run_short(nile_in(&config, ["library", "sync"]), SYNC_TIMEOUT);
        assert_eq!(classify_sync(sync.as_ref()), Login::LoggedOut);
        let login = run_short(nile_in(&config, ["auth", "--login", "--non-interactive"]), SYNC_TIMEOUT).unwrap();
        let sign_in = parse_sign_in(&login.stdout).expect("a sign-in answer");
        assert!(crate::amazon_login::sign_in_page(&sign_in.url).is_some());
        std::fs::remove_dir_all(&config).unwrap();
    }
}
