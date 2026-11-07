//! Amazon Games through Nile (https://github.com/imLinguin/nile, 1.2.0). nile makes every Amazon call and keeps its
//! own files in `<data>/amazon/nile` (set through `NILE_CONFIG_PATH`, so a Heroic or standalone nile in `%APPDATA%\nile`
//! is never touched): the account in `current_user.json`, the tokens in `<md5(user_id)>.enc` (never read, copied or
//! logged here), the owned games in `library.json` and the installs in `installed.json`, which is our install record too.
//! This process holds no Amazon token and sends Amazon nothing. Every nile run but `--version` holds `NILE`: each one
//! rewrites `installed.json`, and a token refresh rewrites the `.enc` file non-atomically. Installs, updates and
//! verifies are queue jobs (`commands::game`) that hold `NILE` while they run. A game starts from its `fuel.json`
//! directly, and only the sessions timed here count as its playtime: Amazon keeps none, and has no cloud saves.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use super::epic::{InstallEvent, LaunchOptions, split_args};
use super::gog::{Fed, feed, inside, parse_progress};
use super::{Art, Error, Game, Image, Progress, ProgressAction, Provider, ProviderKind};
use crate::cache::{Dirs, db};
use crate::msg::Msg;
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
/// A queue job holds `NILE` for its whole download: other nile runs refuse instead of waiting for it.
static DOWNLOADING: AtomicBool = AtomicBool::new(false);
/// What the last `library sync` said about the login, for a scan that skips its sync while nile is busy.
static SYNCED: Mutex<Option<Login>> = Mutex::new(None);

/// The first run of a nile build downloads Amazon's FuelSDK before anything else, so these are generous. nile sets no
/// HTTP timeouts of its own: a dead connection would hang it forever.
const SYNC_TIMEOUT: Duration = Duration::from_secs(120);
const LOGOUT_TIMEOUT: Duration = Duration::from_secs(60);
const INFO_TIMEOUT: Duration = Duration::from_secs(60);
/// Uninstall only deletes files; import hashes every file of the game first, which takes minutes for a large one.
const UNINSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const IMPORT_TIMEOUT: Duration = Duration::from_secs(60 * 60);


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

/// `NILE` for a short run: waits out a scan's sync, but refuses while a download holds it, which can take hours.
fn lock_between_downloads() -> Result<MutexGuard<'static, ()>, Error> {
    loop {
        if let Some(guard) = try_lock() {
            return Ok(guard);
        }
        if DOWNLOADING.load(Ordering::SeqCst) {
            return Err(msg!(amazonBusy).into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// `NILE` held by a queue job, which other nile runs don't wait for.
struct Downloading {
    _nile: MutexGuard<'static, ()>,
}

impl Downloading {
    fn start() -> Self {
        let nile = lock();
        DOWNLOADING.store(true, Ordering::SeqCst);
        Self { _nile: nile }
    }
}

impl Drop for Downloading {
    fn drop(&mut self) {
        DOWNLOADING.store(false, Ordering::SeqCst);
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
    let _nile = lock_between_downloads()?;
    let output = run_short(nile(["auth", "--login", "--non-interactive"]), SYNC_TIMEOUT).ok_or(msg!(cantReachAmazon))?;
    // stdout carries the verifier: parsed, never logged.
    parse_sign_in(&output.stdout).ok_or_else(|| {
        let log = String::from_utf8_lossy(&output.stderr);
        log!("nile auth --login: {}", last_error(&log).unwrap_or("no login page"));
        msg!(cantStartAmazonLogin).into()
    })
}

/// Registers this device with the code from the login window. Nothing of it is logged: the arguments hold the code and
/// the verifier, stderr the device id and stdout Amazon's answer.
pub fn register(sign_in: &SignIn, code: &str) -> Result<(), Error> {
    let _nile = lock_between_downloads()?;
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
    let output = run_short(nile(args), SYNC_TIMEOUT).ok_or(msg!(cantReachAmazon))?;
    // Sic: nile's own spelling.
    if String::from_utf8_lossy(&output.stderr).contains("Succesfully registered a device") {
        set_synced(Login::LoggedIn);
        Ok(())
    } else {
        log!("nile register: the device wasn't registered ({})", output.status);
        Err(msg!(amazonLoginRejected).into())
    }
}

/// Deregisters this device and forgets the login. nile deletes its files only when Amazon answers, so whatever is left
/// goes here, online or not; the device then stays in the Amazon account's device list.
pub fn log_out() -> Result<(), Error> {
    let _nile = lock_between_downloads()?;
    if run_short(nile(["auth", "--logout"]), LOGOUT_TIMEOUT).is_none() {
        log!("nile auth --logout: no answer");
    }
    forget_login(&cfg()).map_err(|e| msg!(cantLogOutAmazon, detail = e.to_string()))?;
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
fn not_here() -> Error {
    msg!(amazonNotHere).into()
}

impl Provider for Amazon {
    fn list_games(&self) -> Result<Vec<Game>, Error> {
        Err(not_here())
    }

    fn launch(&self, _game: &Game) -> Result<(), Error> {
        Err(not_here())
    }

    fn install(&self, _game: &Game) -> Result<(), Error> {
        Err(not_here())
    }

    fn uninstall(&self, _game: &Game) -> Result<(), Error> {
        Err(not_here())
    }

    fn running_game(&self) -> Option<String> {
        None
    }

    fn download_progress(&self, _game: &Game) -> Option<Progress> {
        None
    }
}

/// The owned Amazon games: `nile library sync`, then its `library.json`. Skipped while another nile run holds `NILE`
/// (a download), so the cached games and the login state stay. With games installed, it also asks which have an
/// update, at most every `UPDATES_MAX_AGE_SECS`. Blocks: call from a blocking thread.
pub fn list(dirs: &Dirs) -> Result<Vec<Game>, Error> {
    if tools::version(&tools::NILE).is_none() {
        return Err("Nile isn't installed".into());
    }
    let cfg = cfg();
    if !has_user(&cfg) {
        set_synced(Login::LoggedOut);
        return Err("not logged in".into());
    }
    let conn = db::open(&dirs.db)?;
    let now = now_secs();
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
        let installed = parse_installed(&std::fs::read(cfg.join("installed.json")).unwrap_or_default());
        if !installed.is_empty() && cached(&conn, UPDATES_MAX_AGE_SECS, now).is_none() {
            check_updates(Some(&conn));
        }
        (library, installed)
    };
    let updates: HashSet<String> = cached(&conn, FOREVER, now).unwrap_or_default().into_iter().collect();
    let covers = crate::gamesdb::covers(&conn, now);
    Ok(games(&library, &installed, &covers, &updates))
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
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
/// the portrait cover is GamesDB's (`covers`, by key). An installed game in `updates` has an update.
fn games(
    library: &Value,
    installed: &HashMap<String, Installed>,
    covers: &HashMap<String, String>,
    updates: &HashSet<String>,
) -> Vec<Game> {
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
                update_available: install.is_some() && updates.contains(id),
                // Amazon has no cloud saves.
                cloud_saves: false,
            })
        })
        .collect()
}

/// The game page's details of an owned product, from `library.json`.
pub fn details(id: &str) -> Option<GameInfo> {
    let library = read_json(&cfg().join("library.json"))?;
    let entitlement = entitlement(&library, id)?;
    let parsed = parse_details(&entitlement["product"]["productDetail"]["details"]);
    (parsed != GameInfo::default()).then_some(parsed)
}

/// The entitlement of the product `id` in `library.json`.
fn entitlement<'a>(library: &'a Value, id: &str) -> Option<&'a Value> {
    library.as_array()?.iter().find(|e| e["product"]["id"] == id)
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

// Installing and managing games. Commands, flags and the lines read from nile's output are from nile 1.2.0's source.
// nile exits 0 after most failures, so a job's outcome is read from what it printed and left behind (`outcome`).

/// `info` kind of `nile install --info`'s download size for a game, kept a week.
const INSTALL_INFO: &str = "amazon-install-info";
const INSTALL_INFO_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;
/// `info` kind of `nile list-updates`' answer, the product ids with an update, in one row under `UPDATES_KEY`.
const UPDATES: &str = "amazon-updates";
const UPDATES_KEY: &str = "amazon";
const UPDATES_MAX_AGE_SECS: i64 = 6 * 60 * 60;
/// Info rows are read with a maximum age; the last update answer stands until a newer one.
const FOREVER: i64 = i64::MAX / 2;
/// nile prints progress every second even when nothing moves, so a stall is no new bytes for this long.
const STALL: Duration = Duration::from_secs(10 * 60);
const NO_SPACE_LINE: &str = "Not enough space available";

/// A job needs Nile and an account; nile would fail on its own without them, but less clearly.
fn ensure_login() -> Result<(), Error> {
    if tools::version(&tools::NILE).is_none() {
        return Err(msg!(installNileFirst).into());
    }
    if !has_user(&cfg()) {
        return Err(msg!(logInAmazonFirst).into());
    }
    Ok(())
}

/// Bytes an install downloads (files download whole, so also its size on disk), from the cache while under a week
/// old. `None` when nile is busy or couldn't tell (logged).
pub fn download_size(conn: Option<&Connection>, id: &str) -> Option<u64> {
    let key = ProviderKind::Amazon.key(id);
    let json = info::cached_or_fetch(conn, &key, INSTALL_INFO, INSTALL_INFO_MAX_AGE_SECS, now_secs(), || {
        if !has_user(&cfg()) {
            return None;
        }
        let _nile = try_lock()?;
        let output = run_short(nile(["install", "--info", "--json", id]), INFO_TIMEOUT)?;
        let size = parse_download_size(&output.stdout);
        if size.is_none() {
            log!("nile install --info {id}: {}", last_error(&String::from_utf8_lossy(&output.stderr)).unwrap_or("no size"));
        }
        Some(size?.to_string())
    })?;
    json.parse().ok()
}

/// `{"download_size": N}`.
fn parse_download_size(stdout: &[u8]) -> Option<u64> {
    serde_json::from_slice::<Value>(stdout).ok()?["download_size"].as_u64()
}

/// `nile list-updates --json`: a JSON array of product ids (`[]`, and an error logged, with nothing installed).
fn parse_updates(stdout: &[u8]) -> Option<Vec<String>> {
    serde_json::from_slice(stdout).ok()
}

/// The last update answer, when it is under `max_age` seconds old.
fn cached(conn: &Connection, max_age: i64, now: i64) -> Option<Vec<String>> {
    let json = db::cached_info(conn, UPDATES_KEY, UPDATES, max_age, now).ok().flatten()?;
    serde_json::from_str(&json).ok()
}

/// Asks nile which installed games have an update, and keeps the answer. A failed check keeps the last one. Callers
/// hold `NILE`.
fn check_updates(conn: Option<&Connection>) -> Option<Vec<String>> {
    let output = run_short(nile(["list-updates", "--json"]), INFO_TIMEOUT)?;
    let Some(ids) = parse_updates(&output.stdout) else {
        log!("nile list-updates: {}", last_error(&String::from_utf8_lossy(&output.stderr)).unwrap_or("no answer"));
        return None;
    };
    if let Some(conn) = conn {
        _ = db::put_info(conn, UPDATES_KEY, UPDATES, &serde_json::to_string(&ids).unwrap_or_default(), now_secs());
    }
    Some(ids)
}

/// Forgets the last update answer after a change to the installs, so the next scan asks again.
fn forget_updates(conn: Option<&Connection>) {
    if let Some(Err(e)) = conn.map(|c| db::delete_info(c, UPDATES_KEY, UPDATES)) {
        log!("amazon updates: {e}");
    }
}

/// The folder nile installs a game into inside its base folder: the title without `: / * ? < > \ | ™ " ®`, or the
/// product id when that leaves nothing.
fn folder_name(library: &Value, id: &str) -> String {
    let title = entitlement(library, id).and_then(|e| e["product"]["title"].as_str()).unwrap_or_default();
    let name: String = title.chars().filter(|c| !":/*?<>\\|™\"®".contains(*c)).collect();
    if name.is_empty() { id.to_owned() } else { name }
}

/// The folder `installed.json` records for the game, when it exists. Callers hold `NILE`.
fn installed_path(id: &str) -> Option<PathBuf> {
    let mut installed = parse_installed(&std::fs::read(cfg().join("installed.json")).unwrap_or_default());
    installed.remove(id).map(|i| i.path).filter(|p| p.is_dir())
}

/// Whether `dir` holds a `*.patch` file written since `since`: nile's temp file for a download, left behind when the
/// download failed (nile swallows the error and finishes).
fn leftover_patch(dir: &Path, since: SystemTime) -> bool {
    std::fs::read_dir(dir).into_iter().flatten().flatten().any(|entry| {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => leftover_patch(&path, since),
            Ok(kind) if kind.is_file() => {
                path.extension().is_some_and(|x| x.eq_ignore_ascii_case("patch"))
                    && entry.metadata().and_then(|m| m.modified()).is_ok_and(|m| m >= since)
            }
            _ => false,
        }
    })
}

/// How a nile job ended.
struct Run {
    success: bool,
    stdout: String,
    /// stderr, where nile logs, without its progress lines.
    log: String,
}

/// Follows a job's progress lines. Progress counts the bytes this run downloads.
struct Tracker {
    action: ProgressAction,
    /// The most bytes seen so far.
    most: u64,
}

impl Tracker {
    /// The progress to report for a line, and whether it showed new bytes. A `Verify` reports nothing while it
    /// hashes (its lines stay at `0/N`), then `Repair` once it downloads bad files.
    fn line(&mut self, line: &str) -> (Option<Progress>, bool) {
        let Some(progress) = parse_progress(line, self.action) else { return (None, false) };
        let done = progress.done_bytes.unwrap_or(0);
        if self.action == ProgressAction::Verify && done > 0 {
            self.action = ProgressAction::Repair;
        }
        let grew = done > self.most;
        self.most = self.most.max(done);
        let shown = (self.action != ProgressAction::Verify).then_some(Progress { action: self.action, ..progress });
        (shown, grew)
    }
}

/// Runs a nile job, reporting its pid and progress (`Tracker`). It is stopped after `STALL` without new bytes, counted
/// from the first (a resumed install first re-hashes its files without progress). Callers hold `NILE`.
fn run(args: Vec<OsString>, action: ProgressAction, mut on_event: impl FnMut(InstallEvent)) -> Result<Run, Error> {
    let mut child = nile(args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let pid = child.id();
    on_event(InstallEvent::Started(pid));
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        _ = stdout.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let stderr = child.stderr.take().expect("stderr is piped");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // In the locale's code page, not UTF-8: read as bytes, so an odd byte can't stop the reading.
        let mut reader = BufReader::new(stderr);
        let mut line = Vec::new();
        while reader.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
            if tx.send(String::from_utf8_lossy(&line).trim_end().to_owned()).is_err() {
                break;
            }
            line.clear();
        }
    });
    let mut log = String::new();
    let mut tracker = Tracker { action, most: 0 };
    let fed = feed(&rx, STALL, false, |line| {
        if !line.contains("[PROGRESS]") {
            log.push_str(line);
            log.push('\n');
        }
        let (progress, grew) = tracker.line(line);
        progress.into_iter().for_each(|p| on_event(InstallEvent::Progress(p)));
        grew
    });
    if fed == Fed::Stalled {
        if let Err(e) = crate::platform::kill_tree(pid) {
            log!("stop stalled nile: {e}");
            _ = child.kill();
        }
        _ = child.wait();
        return Err(msg!(amazonStalled).into());
    }
    let status = child.wait()?;
    Ok(Run { success: status.success(), stdout: out.join().unwrap_or_default(), log })
}

fn traceback(log: &str) -> bool {
    log.contains("Traceback (most recent call last)") || log.contains("unhandled exception")
}

/// What a failed nile run says: a crash on the network is `cantReachAmazon`, else the last error.
fn failure(log: &str) -> Msg {
    let offline = ["ConnectionError", "Max retries", "NameResolution", "Timeout", "Failed to refresh the token"];
    if traceback(log) && offline.iter().any(|w| log.contains(w)) {
        return msg!(cantReachAmazon);
    }
    last_error(log).map_or(msg!(nileNoReason), Msg::system)
}

/// Whether a job worked, from what nile printed (`run`), whether a download left a `.patch` file behind (`leftovers`)
/// and whether `installed.json` has the game in its folder (`recorded`). A killed job (Cancel, Pause) exits 1.
fn outcome(run: &Run, action: ProgressAction, leftovers: bool, recorded: bool) -> Result<(), Error> {
    if run.stdout.contains("Checksum error for") {
        return Err(msg!(amazonIncomplete).into());
    }
    if run.log.contains(NO_SPACE_LINE) {
        return Err(if action == ProgressAction::Verify { msg!(noDiskSpaceVerify) } else { msg!(noDiskSpace) }.into());
    }
    let error = run.log.lines().rev().find_map(|l| l.split_once("]:\t").filter(|(head, _)| head.starts_with("ERROR [")));
    if traceback(&run.log) || error.is_some() || !run.success {
        return Err(failure(&run.log).into());
    }
    if leftovers {
        return Err(msg!(amazonIncomplete).into());
    }
    if !recorded {
        return Err(msg!(nileNotRecorded).into());
    }
    Ok(())
}

fn job_args(verb: &str, path_flag: &str, path: &Path, id: &str) -> Vec<OsString> {
    vec![verb.into(), path_flag.into(), path.into(), id.into()]
}

/// Downloads a game into a folder named after it inside `base`. Returns that folder, as nile recorded it. Its setup
/// steps (`PostInstall` redistributables) don't run here; a game that has some is logged.
pub fn install(id: &str, base: &Path, on_event: impl FnMut(InstallEvent)) -> Result<PathBuf, Error> {
    ensure_login()?;
    std::fs::create_dir_all(base)?;
    let folder = base.join(folder_name(&read_json(&cfg().join("library.json")).unwrap_or(Value::Null), id));
    let _job = Downloading::start();
    let start = SystemTime::now();
    let run = run(job_args("install", "--base-path", base, id), ProgressAction::Install, on_event)?;
    let recorded = installed_path(id);
    let result = outcome(&run, ProgressAction::Install, leftover_patch(recorded.as_deref().unwrap_or(&folder), start), recorded.is_some());
    if run.log.contains(NO_SPACE_LINE) {
        // nile made the game's folder before it checked; only an empty one goes.
        _ = std::fs::remove_dir(&folder);
    }
    result?;
    let dir = recorded.ok_or(msg!(nileNotRecorded))?;
    let steps = read_fuel(&dir).and_then(|f| f["PostInstall"].as_array().map(Vec::len)).unwrap_or(0);
    if steps > 0 {
        log!("amazon {id}: {steps} setup step(s) skipped");
    }
    Ok(dir)
}

/// Brings an installed game to Amazon's current version. Always with `--path`: nile 1.2.0 looks the saved folder up by
/// the wrong id and would otherwise update into `~/Games/nile`.
pub fn update(conn: Option<&Connection>, id: &str, dir: &Path, on_event: impl FnMut(InstallEvent)) -> Result<(), Error> {
    ensure_login()?;
    let _job = Downloading::start();
    let start = SystemTime::now();
    let run = run(job_args("update", "--path", dir, id), ProgressAction::Update, on_event)?;
    outcome(&run, ProgressAction::Update, leftover_patch(dir, start), installed_path(id).is_some())?;
    forget_updates(conn);
    Ok(())
}

/// Checks an installed game's files and downloads the bad ones. Refused while the game has an update: nile would check
/// the old files against the new version and record that version as installed.
pub fn verify(conn: Option<&Connection>, id: &str, dir: &Path, on_event: impl FnMut(InstallEvent)) -> Result<(), Error> {
    ensure_login()?;
    let _job = Downloading::start();
    up_to_date(check_updates(conn), id)?;
    let start = SystemTime::now();
    let run = run(job_args("verify", "--path", dir, id), ProgressAction::Verify, on_event)?;
    outcome(&run, ProgressAction::Verify, leftover_patch(dir, start), installed_path(id).is_some())
}

/// Whether a fresh update answer lets the game be verified.
fn up_to_date(updates: Option<Vec<String>>, id: &str) -> Result<(), Error> {
    match updates {
        None => Err(msg!(cantReachAmazon).into()),
        Some(ids) if ids.iter().any(|u| u == id) => Err(msg!(updateFirst).into()),
        Some(_) => Ok(()),
    }
}

/// A short nile run worked when its log has `done`, nile's success line.
fn said(log: &[u8], done: &str) -> Result<(), Error> {
    let log = String::from_utf8_lossy(log);
    if log.contains(done) { Ok(()) } else { Err(failure(&log).into()) }
}

/// Uninstalls a game with nile, which deletes the files it installed and its record. Files the game wrote itself (saves)
/// stay, and so does its folder then.
pub fn uninstall(conn: Option<&Connection>, id: &str) -> Result<(), Error> {
    let _nile = lock_between_downloads()?;
    let output = run_short(nile(["uninstall", id]), UNINSTALL_TIMEOUT).ok_or(msg!(nileNoReason))?;
    said(&output.stderr, "Game removed successfully")?;
    forget_updates(conn);
    Ok(())
}

/// Makes nile take an existing install of the owned game `id` at `path`. nile hashes every file first.
pub fn import(conn: Option<&Connection>, id: &str, path: &Path) -> Result<(), Error> {
    ensure_login()?;
    if !path.is_dir() {
        return Err(msg!(notAFolder, path = path.display().to_string()).into());
    }
    let _nile = lock_between_downloads()?;
    let output = run_short(nile(job_args("import", "--path", path, id)), IMPORT_TIMEOUT).ok_or(msg!(cantReachAmazon))?;
    said(&output.stderr, "[IMPORT]:\t Imported ")?;
    forget_updates(conn);
    Ok(())
}

/// Moves an installed game's folder into `dest` and points nile's record at it. A rename, so only on the same drive.
pub fn move_to(id: &str, from: &Path, dest: &Path) -> Result<(), Error> {
    if !super::epic::same_drive(from, dest) {
        return Err(msg!(otherDriveMove).into());
    }
    let to = dest.join(from.file_name().ok_or(msg!(folderHasNoName))?);
    if to.exists() {
        return Err(msg!(alreadyExists, path = to.display().to_string()).into());
    }
    let _nile = lock_between_downloads()?;
    std::fs::rename(from, &to)?;
    if let Err(e) = set_installed_path(&cfg().join("installed.json"), id, &to) {
        _ = std::fs::rename(&to, from);
        return Err(msg!(cantUpdateNileRecord, reason = Msg::from(e)).into());
    }
    Ok(())
}

/// Sets the game's `path` in `installed.json`, keeping every other field, through a temp file renamed into place.
fn set_installed_path(file: &Path, id: &str, path: &Path) -> Result<(), Error> {
    let mut entries: Vec<Value> = serde_json::from_slice(&std::fs::read(file)?)?;
    let entry = entries.iter_mut().find(|e| e["id"] == id).ok_or(msg!(nileNoRecord))?;
    entry["path"] = Value::from(path.to_string_lossy().into_owned());
    let temp = file.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec(&entries)?)?;
    std::fs::rename(&temp, file).inspect_err(|_| _ = std::fs::remove_file(&temp))?;
    Ok(())
}

// Playing. Play runs `fuel.json`'s `Main` directly with the variables nile's `launch` sets: `nile launch` would wait
// for the whole session, and it rewrites `installed.json` at start without `NILE`.

/// fuel.json as plain JSON: nile reads it as JSON5, so a byte order mark, `//` and `/* */` comments and trailing commas
/// are dropped. Strings stay as they are.
fn strip_json5(text: &str) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => out.extend(chars.next()),
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => while chars.next_if(|&n| n != '\n').is_some() {},
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut star = false;
                for n in chars.by_ref() {
                    if star && n == '/' {
                        break;
                    }
                    star = n == '*';
                }
            }
            ']' | '}' => {
                let kept = out.trim_end().len();
                if out[..kept].ends_with(',') {
                    out.truncate(kept - 1);
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// The game's `fuel.json`, its launch file.
fn read_fuel(dir: &Path) -> Option<Value> {
    serde_json::from_str(&strip_json5(&String::from_utf8_lossy(&std::fs::read(dir.join("fuel.json")).ok()?))).ok()
}

/// What `fuel.json`'s `Main` starts: a program inside the install folder, its arguments as given, and the folder it
/// runs in (the install folder unless `WorkingSubdirOverride` names another).
#[derive(Debug, PartialEq)]
struct Fuel {
    command: String,
    args: Vec<String>,
    working_dir: Option<String>,
}

fn fuel_main(fuel: &Value) -> Option<Fuel> {
    let main = &fuel["Main"];
    let command = main["Command"].as_str().filter(|c| !c.trim().is_empty())?.to_owned();
    let args = main["Args"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    let working_dir = main["WorkingSubdirOverride"].as_str().filter(|d| !d.is_empty()).map(str::to_owned);
    Some(Fuel { command, args, working_dir })
}

/// What Play runs: `Main` with its arguments, then the owner's extra arguments. An alternative executable (relative to
/// `dir`, checked by the caller) runs in its own folder with only the extra ones.
fn launch_command(dir: &Path, fuel: &Fuel, options: &LaunchOptions) -> Command {
    let (exe, cwd, mut args) = match &options.exe {
        Some(exe) => {
            let exe = inside(dir, exe);
            let cwd = exe.parent().unwrap_or(dir).to_path_buf();
            (exe, cwd, Vec::new())
        }
        None => {
            let cwd = fuel.working_dir.as_deref().map_or_else(|| dir.to_path_buf(), |w| inside(dir, w));
            (inside(dir, &fuel.command), cwd, fuel.args.clone())
        }
    };
    args.extend(split_args(&options.args));
    let mut command = Command::new(exe);
    command.args(args).current_dir(cwd);
    command
}

/// The variables nile's `launch` sets for Amazon's FuelSDK: where the SDK is, the entitlement and SKU of the game, and
/// the account's first name.
fn fuel_env(cfg: &Path, entitlement: Option<&Value>, user: Option<&Value>) -> Vec<(&'static str, OsString)> {
    let sdk = cfg.join("SDK").join("Amazon Games Services");
    let text = |v: Option<&Value>| OsString::from(v.and_then(Value::as_str).unwrap_or_default());
    vec![
        ("FUEL_DIR", sdk.join("Legacy").into()),
        ("AMAZON_GAMES_SDK_PATH", sdk.join("AmazonGamesSDK").into()),
        ("AMAZON_GAMES_FUEL_ENTITLEMENT_ID", text(entitlement.map(|e| &e["id"]))),
        ("AMAZON_GAMES_FUEL_PRODUCT_SKU", text(entitlement.map(|e| &e["product"]["sku"]))),
        ("AMAZON_GAMES_FUEL_DISPLAY_NAME", text(user.map(|u| &u["name"]))),
    ]
}

/// nile fetches the SDK only when its folder is missing, so a broken one is deleted for the next nile run to fetch.
fn check_sdk(cfg: &Path) {
    let sdk = cfg.join("SDK");
    if sdk.join("Amazon Games Services").join("Legacy").join("FuelSDK_x64.dll").is_file() {
        return;
    }
    log!("nile: the Amazon Games SDK is incomplete; the next Nile run fetches it again");
    if let Some(_nile) = try_lock() {
        match std::fs::remove_dir_all(&sdk) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => log!("delete the Amazon Games SDK: {e}"),
            _ => {}
        }
    }
}

/// Starts an installed game without waiting for it. Returns its program's file name, to watch its process.
pub fn launch(id: &str, dir: &Path, options: &LaunchOptions) -> Result<String, Error> {
    let fuel = read_fuel(dir).as_ref().and_then(fuel_main).ok_or(msg!(noLaunchFile))?;
    let mut command = launch_command(dir, &fuel, options);
    let exe = PathBuf::from(command.get_program());
    if !exe.is_file() {
        return Err(msg!(gameProgramMissing, path = exe.display().to_string()).into());
    }
    let cfg = cfg();
    check_sdk(&cfg);
    let library = read_json(&cfg.join("library.json")).unwrap_or(Value::Null);
    command.envs(fuel_env(&cfg, entitlement(&library, id), current_user(&cfg).as_ref()));
    command.spawn()?;
    Ok(exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
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
        let games = games(&library, &installed, &covers, &HashSet::new());
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
        let game = &games(&library, &HashMap::new(), &HashMap::new(), &HashSet::new())[0];
        assert_eq!(game.title, "T");
        assert_eq!(game.art.header.url.as_deref(), Some("https://example.invalid/bg2.jpg"));
        assert_eq!(game.art.hero.url.as_deref(), Some("https://example.invalid/bg2.jpg"));
        let library = serde_json::json!([{"product": {"id": "p", "title": "T", "productDetail": {"iconUrl": "https://example.invalid/icon.png"}}}]);
        let game = &games(&library, &HashMap::new(), &HashMap::new(), &HashSet::new())[0];
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

    fn text(name: &str) -> String {
        String::from_utf8(fixture(name)).unwrap()
    }

    fn strings(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Vec<String> {
        args.into_iter().map(|a| a.as_ref().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn sizes_and_updates_parse() {
        assert_eq!(parse_download_size(&fixture("nile-install-info-stdout.txt")), Some(4_200_000_000));
        assert_eq!(parse_download_size(b""), None);
        assert_eq!(parse_download_size(b"{\"download_size\": \"x\"}"), None);
        assert_eq!(
            parse_updates(&fixture("nile-list-updates-stdout.txt")),
            Some(vec!["amzn1.adg.product.4c6f3fb8-e68a-4706-a8eb-490905dfa633".to_string()])
        );
        assert_eq!(parse_updates(b"[]\n"), Some(vec![]));
        // A traceback prints nothing on stdout: the last answer stays.
        assert_eq!(parse_updates(b""), None);
    }

    #[test]
    fn job_arguments_always_name_the_folder() {
        let base = Path::new(r"C:\Games");
        assert_eq!(strings(job_args("install", "--base-path", base, "amzn1.x")), ["install", "--base-path", r"C:\Games", "amzn1.x"]);
        assert_eq!(strings(job_args("update", "--path", Path::new(r"C:\Games\A"), "amzn1.x")), ["update", "--path", r"C:\Games\A", "amzn1.x"]);
        assert_eq!(strings(job_args("verify", "--path", Path::new(r"C:\Games\A"), "amzn1.x"))[..2], ["verify", "--path"]);
        // Never debug mode: it logs requests and arguments.
        let command = nile(job_args("install", "--base-path", base, "amzn1.x"));
        assert!(!command.get_args().any(|a| a == "-d" || a == "--debug"));
    }

    #[test]
    fn progress_counts_new_bytes_and_a_verify_shows_only_its_repair() {
        let line = |done: u64| format!("INFO [PROGRESS]:\t = Progress: 1.00 {done}/4200000000, Running for: 00:00:01, ETA: 100:00:00");
        let mut install = Tracker { action: ProgressAction::Install, most: 0 };
        let (first, grew) = install.line(&line(0));
        assert_eq!((first.unwrap().done_bytes, grew), (Some(0), false));
        let (p, grew) = install.line(&line(480_900_000));
        let p = p.unwrap();
        assert!(grew && p.action == ProgressAction::Install && p.total_bytes == Some(4_200_000_000));
        // ETA hours past 99.
        assert_eq!(p.secs_left, Some(360_000));
        // A line every second without new bytes keeps nothing alive.
        assert!(!install.line(&line(480_900_000)).1);
        assert_eq!(install.line("INFO [PROGRESS]:\t = Downloaded: 458.62 MiB, Written: 458.62 MiB"), (None, false));
        // Hashing reports nothing; downloading bad files is a repair.
        let mut verify = Tracker { action: ProgressAction::Verify, most: 0 };
        assert_eq!(verify.line(&line(0)), (None, false));
        let (p, grew) = verify.line(&line(1000));
        assert!(grew && p.unwrap().action == ProgressAction::Repair);
        // The fixture's lines, in order.
        let mut tracker = Tracker { action: ProgressAction::Install, most: 0 };
        let grew: Vec<bool> = text("nile-install-ok-stderr.txt").lines().map(|l| tracker.line(l).1).collect();
        assert_eq!(grew.iter().filter(|g| **g).count(), 1);
    }

    fn ran(success: bool, stdout: &str, log: &str) -> Run {
        Run { success, stdout: stdout.into(), log: log.into() }
    }

    fn told(result: Result<(), Error>) -> String {
        result.map_or_else(|e| e.to_string(), |()| "ok".into())
    }

    #[test]
    fn a_job_works_only_without_errors_leftovers_or_a_missing_record() {
        let install = |run: &Run, leftovers, recorded| told(outcome(run, ProgressAction::Install, leftovers, recorded));
        assert_eq!(install(&ran(true, "", &text("nile-install-ok-stderr.txt")), false, true), "ok");
        assert_eq!(install(&ran(true, "", &text("nile-install-up-to-date-stderr.txt")), false, true), "ok");
        // Exit 0 every time below: nile's failures are silent.
        let ok_log = text("nile-install-ok-stderr.txt");
        assert_eq!(install(&ran(true, &text("nile-install-checksum-stdout.txt"), &ok_log), false, true), "amazonIncomplete");
        assert_eq!(install(&ran(true, "", &ok_log), true, true), "amazonIncomplete");
        assert_eq!(install(&ran(true, "", &ok_log), false, false), "nileNotRecorded");
        let full = ran(true, "", &text("nile-install-no-space-stderr.txt"));
        assert_eq!(install(&full, false, false), "noDiskSpace");
        assert_eq!(told(outcome(&full, ProgressAction::Verify, false, true)), "noDiskSpaceVerify");
        let missing = ran(true, "", "INFO [CLI]:\t Searching\nERROR [CLI]:\t Couldn't find what you are looking for\n");
        assert_eq!(install(&missing, false, false), "Couldn't find what you are looking for");
        // A crash on the network, and another crash.
        let offline = ran(false, "", &text("nile-sync-offline-stderr.txt"));
        assert_eq!(install(&offline, false, true), "cantReachAmazon");
        let crash = ran(false, "", "Traceback (most recent call last):\n  File \"x\"\nKeyError: 'manifest'\n[PYI-1:ERROR] Failed to execute script 'cli' due to unhandled exception!\n");
        assert_eq!(install(&crash, false, true), "KeyError: 'manifest'");
        // Stopped by Cancel or Pause: exit 1 with nothing said.
        assert_eq!(install(&ran(false, "", &ok_log), false, true), "nileNoReason");
    }

    #[test]
    fn short_runs_need_their_success_line() {
        assert_eq!(told(said(&fixture("nile-uninstall-ok-stderr.txt"), "Game removed successfully")), "ok");
        assert_eq!(told(said(&fixture("nile-uninstall-not-installed-stderr.txt"), "Game removed successfully")), "Game isn't installed");
        assert_eq!(told(said(&fixture("nile-import-ok-stderr.txt"), "[IMPORT]:\t Imported ")), "ok");
        assert_eq!(
            told(said(&fixture("nile-import-failed-stderr.txt"), "[IMPORT]:\t Imported ")),
            "There are missing or corrupted files for Last Day of June. Failed import."
        );
        assert_eq!(told(said(b"", "Game removed successfully")), "nileNoReason");
    }

    #[test]
    fn verify_is_refused_while_an_update_is_pending() {
        let id = "amzn1.adg.product.4c6f3fb8-e68a-4706-a8eb-490905dfa633";
        assert_eq!(told(up_to_date(parse_updates(&fixture("nile-list-updates-stdout.txt")), id)), "updateFirst");
        assert_eq!(told(up_to_date(Some(vec!["amzn1.other".into()]), id)), "ok");
        assert_eq!(told(up_to_date(None, id)), "cantReachAmazon");
    }

    #[test]
    fn a_patch_file_written_during_the_job_is_a_failed_download() {
        let dir = temp_dir("patch");
        let before = SystemTime::now() - Duration::from_secs(60);
        std::fs::create_dir_all(dir.join("Data")).unwrap();
        std::fs::write(dir.join("Data").join("level1.pak"), "x").unwrap();
        assert!(!leftover_patch(&dir, before));
        std::fs::write(dir.join("Data").join("level1.pak.patch"), "").unwrap();
        assert!(leftover_patch(&dir, before));
        // One from before the job started isn't this job's.
        assert!(!leftover_patch(&dir, SystemTime::now() + Duration::from_secs(60)));
        assert!(!leftover_patch(&dir.join("missing"), before));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_move_rewrites_only_the_games_path() {
        let dir = temp_dir("move");
        let file = dir.join("installed.json");
        let other = r#"{"id": "b", "version": "2", "path": "C:\\B", "size": 2}"#;
        std::fs::write(&file, format!(r#"[{{"id": "a", "version": "1", "path": "C:\\A", "size": 1, "extra": true}}, {other}]"#)).unwrap();
        set_installed_path(&file, "a", Path::new(r"D:\Games\A")).unwrap();
        let entries: Vec<Value> = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(entries[0], serde_json::json!({"id": "a", "version": "1", "path": r"D:\Games\A", "size": 1, "extra": true}));
        assert_eq!(entries[1], serde_json::from_str::<Value>(other).unwrap());
        assert_eq!(parse_installed(&std::fs::read(&file).unwrap())["a"].path, PathBuf::from(r"D:\Games\A"));
        // An unknown game changes nothing, and no temp file is left.
        assert!(set_installed_path(&file, "c", Path::new(r"D:\C")).is_err());
        assert_eq!(serde_json::from_slice::<Vec<Value>>(&std::fs::read(&file).unwrap()).unwrap(), entries);
        assert!(!dir.join("installed.json.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_install_folder_is_the_title_without_forbidden_characters() {
        let library = serde_json::json!([
            {"product": {"id": "a", "title": "Dungeon: Endless™ <Definitive> \"Ed\" ®?"}},
            {"product": {"id": "b"}},
        ]);
        assert_eq!(folder_name(&library, "a"), "Dungeon Endless Definitive Ed ");
        assert_eq!(folder_name(&library, "b"), "b");
        assert_eq!(folder_name(&Value::Null, "c"), "c");
    }

    #[test]
    fn fuel_json_parses_as_json5() {
        let fuel = |name| serde_json::from_str::<Value>(&strip_json5(&text(name))).map(|v| fuel_main(&v));
        assert_eq!(
            fuel("sample-fuel.json").unwrap(),
            Some(Fuel { command: r"Binaries\Win64\Game.exe".into(), args: vec!["-windowed".into()], working_dir: Some(r"Binaries\Win64".into()) })
        );
        assert_eq!(
            fuel("sample-fuel-json5.json").unwrap(),
            Some(Fuel { command: "Game.exe".into(), args: vec!["-url".into(), "https://example.invalid/x".into()], working_dir: None })
        );
        // Slashes and commas inside strings stay.
        let tricky = strip_json5("{\"a\": \"x // y, ]\", /* \"b\": 1, */ \"c\": [1, 2,], \"d\": \"\\\"/*\" }");
        assert_eq!(serde_json::from_str::<Value>(&tricky).unwrap(), serde_json::json!({"a": "x // y, ]", "c": [1, 2], "d": "\"/*"}));
        assert_eq!(fuel_main(&serde_json::json!({"Main": {"Command": " "}})), None);
    }

    #[test]
    fn play_runs_main_with_the_fuel_variables() {
        let fuel = Fuel { command: r"Binaries\Win64\Game.exe".into(), args: vec!["-a b".into()], working_dir: Some("Binaries/Win64".into()) };
        let dir = Path::new(r"D:\Games\June");
        let options = LaunchOptions { args: "-windowed".into(), ..LaunchOptions::default() };
        let command = launch_command(dir, &fuel, &options);
        assert_eq!(command.get_program(), r"D:\Games\June\Binaries\Win64\Game.exe");
        assert_eq!(command.get_current_dir(), Some(Path::new(r"D:\Games\June\Binaries\Win64")));
        // Main's arguments as given, not split; then the owner's.
        assert_eq!(strings(command.get_args()), ["-a b", "-windowed"]);
        let plain = Fuel { working_dir: None, ..fuel };
        assert_eq!(launch_command(dir, &plain, &LaunchOptions::default()).get_current_dir(), Some(dir));
        let alt = LaunchOptions { exe: Some(r"tools\editor.exe".into()), ..options };
        let command = launch_command(dir, &plain, &alt);
        assert_eq!(command.get_current_dir(), Some(Path::new(r"D:\Games\June\tools")));
        assert_eq!(strings(command.get_args()), ["-windowed"]);

        let library: Value = serde_json::from_slice(&fixture("sample-library.json")).unwrap();
        let user: Value = serde_json::from_slice(&fixture("sample-current_user.json")).unwrap();
        let cfg = Path::new(r"C:\data\amazon\nile");
        let env: HashMap<_, _> =
            fuel_env(cfg, entitlement(&library, "amzn1.adg.product.4c6f3fb8-e68a-4706-a8eb-490905dfa633"), Some(&user)).into_iter().collect();
        assert_eq!(env["FUEL_DIR"], r"C:\data\amazon\nile\SDK\Amazon Games Services\Legacy");
        assert_eq!(env["AMAZON_GAMES_SDK_PATH"], r"C:\data\amazon\nile\SDK\Amazon Games Services\AmazonGamesSDK");
        assert_eq!(env["AMAZON_GAMES_FUEL_ENTITLEMENT_ID"], "00000000-0000-0000-0000-000000000001");
        assert_eq!(env["AMAZON_GAMES_FUEL_PRODUCT_SKU"], "TEST-SKU-1");
        assert_eq!(env["AMAZON_GAMES_FUEL_DISPLAY_NAME"], "Test");
        // Not in the library, no account: empty, as nile sets them.
        let env: HashMap<_, _> = fuel_env(cfg, None, None).into_iter().collect();
        assert_eq!((env["AMAZON_GAMES_FUEL_ENTITLEMENT_ID"].len(), env["AMAZON_GAMES_FUEL_DISPLAY_NAME"].len()), (0, 0));
    }

    #[test]
    fn play_needs_the_launch_file_and_its_program() {
        let dir = temp_dir("play");
        let refused = |dir: &Path| launch("x", dir, &LaunchOptions::default()).unwrap_err().to_string();
        assert_eq!(refused(&dir), "noLaunchFile");
        std::fs::write(dir.join("fuel.json"), fixture("sample-fuel.json")).unwrap();
        assert!(refused(&dir).starts_with("gameProgramMissing"), "{}", refused(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn updates_show_on_installed_games_only() {
        let library: Value = serde_json::from_slice(&fixture("sample-library.json")).unwrap();
        let dir = temp_dir("updates");
        let ldoj = "amzn1.adg.product.4c6f3fb8-e68a-4706-a8eb-490905dfa633";
        let dote = "amzn1.adg.product.e9b762cd-e130-4c5b-aaf5-070d785d1cd2";
        let installed = HashMap::from([(ldoj.to_string(), Installed { id: ldoj.into(), path: dir.clone(), size: None })]);
        let updates = HashSet::from([ldoj.to_string(), dote.to_string()]);
        let games = games(&library, &installed, &HashMap::new(), &updates);
        assert!(games[0].update_available);
        assert!(!games[1].update_available);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_last_update_answer_is_kept_until_forgotten() {
        let dir = temp_dir("updates-cache");
        let conn = db::open(&dir.join("library.sqlite")).unwrap();
        assert_eq!(cached(&conn, FOREVER, 1_000), None);
        db::put_info(&conn, UPDATES_KEY, UPDATES, r#"["a"]"#, 1_000).unwrap();
        assert_eq!(cached(&conn, UPDATES_MAX_AGE_SECS, 1_000), Some(vec!["a".to_string()]));
        // Older than 6 h: due for a check, still the answer shown.
        assert_eq!(cached(&conn, UPDATES_MAX_AGE_SECS, 1_001 + UPDATES_MAX_AGE_SECS), None);
        assert_eq!(cached(&conn, FOREVER, 1_001 + UPDATES_MAX_AGE_SECS), Some(vec!["a".to_string()]));
        forget_updates(Some(&conn));
        assert_eq!(cached(&conn, FOREVER, 1_000), None);
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_download_makes_other_nile_runs_refuse() {
        let job = Downloading::start();
        assert_eq!(lock_between_downloads().err().map(|e| e.to_string()).as_deref(), Some("amazonBusy"));
        drop(job);
        assert!(lock_between_downloads().is_ok());
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
