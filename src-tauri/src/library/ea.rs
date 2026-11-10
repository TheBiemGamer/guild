//! EA games through the owner's Maxima fork (`maxima-cli guild …`, https://github.com/TheBiemGamer/Maxima). Maxima
//! makes every EA call and keeps its own files in `<data>/ea` (`MAXIMA_DATA_DIR`), the login included. Each command
//! prints one JSON object per line on stdout; its stderr is dropped unread, since a token could still be in it.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;
use tauri::AppHandle;

use super::gog::{Fed, feed};
use super::tool::{InstallEvent, LaunchOptions, StoreLogin};
use super::{Art, Error, Game, Image, Progress, ProgressAction, ProviderKind};
use crate::cache::{Dirs, db};
use crate::info;
use crate::msg::Msg;
use crate::tools;

const LIBRARY_TIMEOUT: Duration = Duration::from_secs(120);
const SHORT_TIMEOUT: Duration = Duration::from_secs(60);
/// `guild launch` checks the license and syncs cloud saves before the game starts.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(120);
/// `guild login` prints its URL before it waits for the browser, and the account once it has the code.
const LOGIN_STEP: Duration = Duration::from_secs(30);
/// Where `guild login` waits for the code.
const LOGIN_PORT: u16 = 31033;
const LOGIN_EXIT: Duration = Duration::from_secs(10);
const UNINSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Import waits on the UAC prompt of the game's setup.
const IMPORT_TIMEOUT: Duration = Duration::from_secs(60 * 60);
/// A download with no new bytes for this long is stopped. Maxima gives up after 2 minutes itself; this is for a hang.
const STALL: Duration = Duration::from_secs(10 * 60);
/// `info` kind of `guild info`'s `[download, disk]` bytes.
const INSTALL_INFO: &str = "ea-install-info";
const INSTALL_INFO_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;

/// The account name `guild account` last reported.
static ACCOUNT: Mutex<Option<String>> = Mutex::new(None);

/// `maxima-cli` with its data in `<data>/ea`; stderr dropped.
pub fn command(dirs: &Dirs) -> Command {
    let mut command = Command::new(tools::MAXIMA.exe());
    command
        .arg("guild")
        .env("MAXIMA_DATA_DIR", &dirs.ea)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    crate::platform::no_window(&mut command);
    command
}

/// `command` for a game. With its Steam twin (`steam`, from GOG GamesDB), Maxima hands umu that game's id on Linux, so
/// umu-protonfixes can apply its fixes.
fn game_command(dirs: &Dirs, steam: Option<u32>) -> Command {
    let mut command = command(dirs);
    if let Some(appid) = steam {
        command.env("MAXIMA_UMU_GAMEID", format!("umu-{appid}"));
    }
    command
}

/// One line of `maxima-cli guild` output. No `Debug` outside tests: `LoginUrl` holds single-use PKCE values.
#[cfg_attr(test, derive(Debug))]
#[derive(PartialEq)]
pub enum Event {
    LoginUrl(String),
    LoggedIn {
        name: String,
    },
    /// `guild account`: the name when logged in.
    Account(Option<String>),
    Game(EaGame),
    /// `guild install`: bytes downloaded of `total`; the setup step after it isn't counted.
    Progress {
        bytes: u64,
        total: u64,
    },
    /// `guild info`: what an install downloads and takes on disk.
    Info {
        download_bytes: u64,
        install_bytes: u64,
    },
    Done,
    Ok,
    /// `guild launch`: the game started; Maxima stays until it stops.
    Started,
    /// `guild launch`: the game stopped and its saves are up.
    Stopped,
    Error(String),
}

/// One `{"game": …}` line.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EaGame {
    offer_id: String,
    name: String,
    installed: bool,
    install_path: Option<PathBuf>,
    installed_version: Option<String>,
    latest_version: Option<String>,
}

/// An event, or `None` for a line that isn't one (unknown lines are ignored).
pub fn parse_line(line: &str) -> Option<Event> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    let object = value.as_object()?;
    let text = |v: &Value| v.as_str().map(str::to_owned);
    if let Some(code) = object.get("error") {
        return Some(Event::Error(text(code)?));
    }
    if let Some(url) = object.get("login_url") {
        return Some(Event::LoginUrl(text(url)?));
    }
    match object.get("logged_in") {
        Some(Value::Object(user)) => {
            return Some(Event::LoggedIn {
                name: text(user.get("name")?)?,
            });
        }
        Some(Value::Bool(logged_in)) => {
            return Some(Event::Account(
                logged_in
                    .then(|| object.get("name").and_then(text))
                    .flatten(),
            ));
        }
        _ => {}
    }
    if let Some(game) = object.get("game") {
        return serde_json::from_value(game.clone()).ok().map(Event::Game);
    }
    if let Some(p) = object.get("progress") {
        return Some(Event::Progress {
            bytes: p.get("bytes")?.as_u64()?,
            total: p.get("total")?.as_u64()?,
        });
    }
    if let Some(i) = object.get("info") {
        return Some(Event::Info {
            download_bytes: i.get("download_bytes")?.as_u64()?,
            install_bytes: i.get("install_bytes")?.as_u64()?,
        });
    }
    let flag = |name| object.get(name) == Some(&Value::Bool(true));
    if flag("done") {
        return Some(Event::Done);
    }
    if flag("started") {
        return Some(Event::Started);
    }
    if flag("stopped") {
        return Some(Event::Stopped);
    }
    flag("ok").then_some(Event::Ok)
}

/// The owner's words for a `{"error": code}`.
pub fn error_msg(code: &str) -> Msg {
    match code {
        "login_required" => msg!(logInEaFirst),
        "login" => msg!(eaLoginRejected),
        "not_owned" => msg!(eaNotOwned),
        "no_build" => msg!(eaNoBuild),
        "path" => msg!(eaNotAFolder),
        "unsafe_path" => msg!(eaUnsafeFolder),
        "download" => msg!(eaDownloadFailed),
        "touchup" => msg!(eaSetupFailed),
        "elevation_declined" => msg!(eaElevationDeclined),
        "port_in_use" => msg!(eaPortInUse),
        "launch" => msg!(eaLaunchFailed),
        _ => msg!(eaNoReason),
    }
}

/// Runs a short `guild` command to its end and returns its events; `None` when it didn't answer in time.
fn run(dirs: &Dirs, args: &[&str], timeout: Duration) -> Option<Vec<Event>> {
    let mut command = command(dirs);
    command.args(args);
    match super::tool::run_with_timeout(command, b"", timeout) {
        Ok(Some(output)) => Some(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(parse_line)
                .collect(),
        ),
        Ok(None) => {
            tracing::warn!("maxima {}: no answer in {} s", args[0], timeout.as_secs());
            None
        }
        Err(e) => {
            tracing::warn!("maxima {}: {e}", args[0]);
            None
        }
    }
}

/// The first error code in `events`, as the owner's words.
fn first_error(events: &[Event]) -> Option<Msg> {
    events.iter().find_map(|e| match e {
        Event::Error(code) => Some(error_msg(code)),
        _ => None,
    })
}

/// The login state from `guild account`; also remembers the account name.
pub fn login(dirs: &Dirs) -> StoreLogin {
    // On Linux Maxima has no pin yet (`tools.rs`), so it is never found there.
    if tools::version(&tools::MAXIMA).is_none() {
        return StoreLogin::NoTool;
    }
    let account = run(dirs, &["account"], SHORT_TIMEOUT).and_then(|events| {
        events.into_iter().find_map(|e| match e {
            Event::Account(name) => Some(name),
            _ => None,
        })
    });
    let (login, name) = match account {
        Some(Some(name)) => (StoreLogin::LoggedIn, Some(name)),
        Some(None) => (StoreLogin::LoggedOut, None),
        None => return StoreLogin::Unreachable,
    };
    *ACCOUNT.lock().unwrap_or_else(|e| e.into_inner()) = name;
    login
}

/// The account name the last `login` read.
pub fn account_name() -> Option<String> {
    ACCOUNT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Forgets the login in Maxima.
pub fn log_out(dirs: &Dirs) -> Result<(), Error> {
    let events = run(dirs, &["logout"], SHORT_TIMEOUT).ok_or(msg!(eaNoReason))?;
    if !events.contains(&Event::Ok) {
        return Err(first_error(&events).unwrap_or(msg!(eaNoReason)).into());
    }
    *ACCOUNT.lock().unwrap_or_else(|e| e.into_inner()) = None;
    Ok(())
}

/// Logs in through `guild login` and EA's page in Guild's login window. `Ok(None)` when the owner closed the window.
/// Maxima is stopped on every way out but success, so its port is free for the next try.
pub fn log_in(app: &AppHandle, dirs: &Dirs) -> Result<Option<String>, Error> {
    let mut child = crate::platform::spawn_tool(command(dirs).arg("login").stdout(Stdio::piped()))?;
    let events = read_events(child.stdout.take().expect("stdout is piped"));
    let result = sign_in(app, &events);
    // After `logged_in` Maxima only exits; one that lingers is stopped too, so Log in never hangs on it.
    if (!matches!(result, Ok(Some(_))) || !exits_within(&mut child, LOGIN_EXIT))
        && let Err(e) = crate::platform::kill_tree(child.id())
    {
        tracing::warn!("stop maxima login: {e}");
        _ = child.kill();
    }
    _ = child.wait();
    if let Ok(Some(name)) = &result {
        *ACCOUNT.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.clone());
    }
    result
}

fn exits_within(child: &mut std::process::Child, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn sign_in(app: &AppHandle, events: &mpsc::Receiver<Event>) -> Result<Option<String>, Error> {
    let url = match events.recv_timeout(LOGIN_STEP) {
        Ok(Event::LoginUrl(url)) => url,
        Ok(Event::Error(code)) => return Err(error_msg(&code).into()),
        _ => return Err(msg!(cantStartEaLogin).into()),
    };
    let url = crate::ea_login::sign_in_page(&url).ok_or(msg!(cantStartEaLogin))?;
    let Some(code) = crate::login_window::run(
        app,
        "ea-login",
        "logInEaTitle",
        url,
        None,
        crate::ea_login::login_code,
    )?
    else {
        return Ok(None);
    };
    send_code(&code)?;
    match events.recv_timeout(LOGIN_STEP) {
        Ok(Event::LoggedIn { name }) => Ok(Some(name)),
        Ok(Event::Error(code)) => Err(error_msg(&code).into()),
        _ => Err(msg!(eaLoginRejected).into()),
    }
}

/// Reads `stdout`'s events on a thread until it closes, also after the receiver is gone, so the pipe never fills.
fn read_events(stdout: impl Read + Send + 'static) -> mpsc::Receiver<Event> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(event) = parse_line(&line) {
                _ = tx.send(event);
            }
        }
    });
    rx
}

/// Hands the code to `guild login`'s listener, as EA's app would. A plain socket: no proxy may see it.
fn send_code(code: &str) -> Result<(), Error> {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", LOGIN_PORT))?;
    stream.set_read_timeout(Some(LOGIN_STEP))?;
    write!(
        stream,
        "GET /auth?code={} HTTP/1.1\r\nHost: 127.0.0.1:{LOGIN_PORT}\r\nConnection: close\r\n\r\n",
        super::percent_encode(code)
    )?;
    // The answer is read only to wait for it; `LoggedIn` on stdout says whether it worked.
    _ = stream.read(&mut [0; 64]);
    Ok(())
}

/// The owned EA games from `guild library`.
pub fn list(dirs: &Dirs) -> Result<Vec<Game>, Error> {
    let events = run(dirs, &["library"], LIBRARY_TIMEOUT).ok_or("no answer")?;
    if let Some(e) = first_error(&events) {
        return Err(e.into());
    }
    if !events.contains(&Event::Done) {
        return Err("library cut short".into());
    }
    let conn = db::open(&dirs.db)?;
    let covers = crate::gamesdb::covers(&conn, super::unix_now());
    Ok(games(events, &covers))
}

/// The game lines as games; the portrait cover is GamesDB's (`covers`, by key).
fn games(events: Vec<Event>, covers: &std::collections::HashMap<String, String>) -> Vec<Game> {
    events
        .into_iter()
        .filter_map(|e| match e {
            Event::Game(g) => Some(g),
            _ => None,
        })
        .filter(|g| !g.offer_id.is_empty() && !g.name.trim().is_empty())
        .map(|g| {
            let key = ProviderKind::Ea.key(&g.offer_id);
            let update_available = g.installed
                && matches!((&g.installed_version, &g.latest_version), (Some(a), Some(b)) if a != b);
            Game {
                provider: ProviderKind::Ea,
                title: g.name.trim().to_owned(),
                installed: g.installed,
                install_dir: g.install_path.filter(|_| g.installed),
                playtime_minutes: None,
                last_played: None,
                size_bytes: None,
                art: Art {
                    cover: Image {
                        path: None,
                        url: covers.get(&key).cloned(),
                    },
                    ..Art::default()
                },
                dlc: Vec::new(),
                update_available,
                cloud_saves: false,
                external_id: g.offer_id,
            }
        })
        .collect()
}

/// An install's download and disk bytes from `guild info`, cached for a week. `(None, None)` when Maxima couldn't tell.
pub fn install_sizes(
    dirs: &Dirs,
    conn: Option<&Connection>,
    id: &str,
) -> (Option<u64>, Option<u64>) {
    let json = info::cached_or_run_tool(
        conn,
        &ProviderKind::Ea.key(id),
        INSTALL_INFO,
        INSTALL_INFO_MAX_AGE_SECS,
        super::unix_now(),
        || {
            let events = run(dirs, &["info", id], SHORT_TIMEOUT)?;
            let sizes = info_sizes(&events);
            if sizes.is_none() {
                let reason = first_error(&events).unwrap_or(msg!(eaNoReason));
                tracing::warn!("maxima info {id}: {reason}");
            }
            serde_json::to_string(&sizes?).ok()
        },
    );
    json.and_then(|j| serde_json::from_str::<(u64, u64)>(&j).ok())
        .map_or((None, None), |(download, disk)| {
            (Some(download), Some(disk))
        })
}

fn info_sizes(events: &[Event]) -> Option<(u64, u64)> {
    events.iter().find_map(|e| match e {
        Event::Info {
            download_bytes,
            install_bytes,
        } => Some((*download_bytes, *install_bytes)),
        _ => None,
    })
}

/// The folder an install gets inside its base folder: the title without `: / * ? < > \ | ™ " ®` and trailing dots or
/// spaces (Windows drops those), or the offer id the same way when that leaves nothing.
pub fn install_folder(title: &str, id: &str) -> String {
    let clean = |text: &str| {
        let name: String = text
            .chars()
            .filter(|c| !":/*?<>\\|™\"®".contains(*c))
            .collect();
        name.trim().trim_end_matches('.').trim_end().to_owned()
    };
    let name = clean(title);
    if name.is_empty() { clean(id) } else { name }
}

/// What a `guild install`, `uninstall` or `import` said so far.
#[derive(Default)]
struct Job {
    done: bool,
    error: Option<String>,
}

impl Job {
    /// Takes one stdout line; returns its progress, if it is a progress line.
    fn line(&mut self, line: &str, action: ProgressAction) -> Option<Progress> {
        match parse_line(line)? {
            Event::Progress { bytes, total } => Some(Progress {
                fraction: if total == 0 {
                    0.0
                } else {
                    (bytes as f64 / total as f64).clamp(0.0, 1.0) as f32
                },
                action,
                done_bytes: Some(bytes),
                total_bytes: Some(total),
                secs_left: None,
                done_files: None,
                total_files: None,
            }),
            Event::Done => {
                self.done = true;
                None
            }
            Event::Error(code) => {
                self.error.get_or_insert(code);
                None
            }
            _ => None,
        }
    }

    /// The outcome once Maxima exited: only `done` with exit 0 is success. A killed run (Pause, Cancel) says neither.
    fn result(self, success: bool) -> Result<(), Msg> {
        match self.error {
            Some(code) => Err(error_msg(&code)),
            None if self.done && success => Ok(()),
            None => Err(msg!(eaNoReason)),
        }
    }
}

/// Runs `guild install`, reporting its pid first (so Pause and Cancel can stop it), then its progress. Stopped after
/// `STALL` without new bytes, counted from the first.
fn download(
    dirs: &Dirs,
    steam: Option<u32>,
    args: &[&OsStr],
    action: ProgressAction,
    mut on_event: impl FnMut(InstallEvent),
) -> Result<(), Error> {
    let mut child =
        crate::platform::spawn_tool(game_command(dirs, steam).args(args).stdout(Stdio::piped()))?;
    let pid = child.id();
    on_event(InstallEvent::Started(pid));
    let stdout = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut job = Job::default();
    let mut most = 0;
    let fed = feed(&rx, STALL, false, |line| {
        let Some(progress) = job.line(line, action) else {
            return false;
        };
        let bytes = progress.done_bytes.unwrap_or(0);
        on_event(InstallEvent::Progress(progress));
        let grew = bytes > most;
        most = most.max(bytes);
        grew
    });
    if fed == Fed::Stalled {
        if let Err(e) = crate::platform::kill_tree(pid) {
            tracing::warn!("stop stalled maxima: {e}");
            _ = child.kill();
        }
        _ = child.wait();
        return Err(msg!(eaStalled).into());
    }
    let status = child.wait()?;
    Ok(job.result(status.success())?)
}

/// Downloads a game into `<base>/<install_folder(title)>` and runs its setup (one UAC prompt on Windows). Returns the
/// folder. Run again into the same folder, Maxima skips the files it already has: that is how a paused install resumes.
pub fn install(
    dirs: &Dirs,
    id: &str,
    steam: Option<u32>,
    title: &str,
    base: &Path,
    on_event: impl FnMut(InstallEvent),
) -> Result<PathBuf, Error> {
    let folder = base.join(install_folder(title, id));
    fs_err::create_dir_all(&folder)?;
    download(
        dirs,
        steam,
        &[OsStr::new("install"), OsStr::new(id), folder.as_os_str()],
        ProgressAction::Install,
        on_event,
    )?;
    Ok(folder)
}

/// Brings an installed game to EA's current build: an install into its own folder, which downloads only the files
/// whose size changed.
pub fn update(
    dirs: &Dirs,
    id: &str,
    steam: Option<u32>,
    dir: &Path,
    on_event: impl FnMut(InstallEvent),
) -> Result<(), Error> {
    download(
        dirs,
        steam,
        &[OsStr::new("install"), OsStr::new(id), dir.as_os_str()],
        ProgressAction::Update,
        on_event,
    )
}

/// Runs a job without progress to its end.
fn short_job(
    dirs: &Dirs,
    steam: Option<u32>,
    args: &[&OsStr],
    timeout: Duration,
) -> Result<(), Error> {
    let mut command = game_command(dirs, steam);
    command.args(args);
    let output = super::tool::run_with_timeout(command, b"", timeout)?.ok_or(msg!(eaNoReason))?;
    let mut job = Job::default();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        job.line(line, ProgressAction::Install);
    }
    Ok(job.result(output.status.success())?)
}

/// Deletes the game's own files and its `__Installer` folder; files the game wrote itself (saves) stay.
pub fn uninstall(dirs: &Dirs, id: &str) -> Result<(), Error> {
    short_job(
        dirs,
        None,
        &[OsStr::new("uninstall"), OsStr::new(id)],
        UNINSTALL_TIMEOUT,
    )
}

/// Registers an existing install at `path` (the folder holding `__Installer`) by running its setup (one UAC prompt on
/// Windows).
pub fn import(dirs: &Dirs, id: &str, steam: Option<u32>, path: &Path) -> Result<(), Error> {
    short_job(
        dirs,
        steam,
        &[OsStr::new("import"), OsStr::new(id), path.as_os_str()],
        IMPORT_TIMEOUT,
    )
}

/// Starts the game through `guild launch` and returns Maxima once it reports `started`. Maxima stays alive while the
/// game runs (it answers the game's license requests), so it is watched as the game. Not `spawn_tool`: its job object
/// would stop the game with Guild. On Linux Maxima brings its own Proton and prefix, so nothing wraps it here.
pub fn launch(
    dirs: &Dirs,
    id: &str,
    steam: Option<u32>,
    options: &LaunchOptions,
) -> Result<Child, Error> {
    let mut command = game_command(dirs, steam);
    command
        .args(["launch", id, "--"])
        .args(super::tool::split_args(&options.args));
    start(command, LAUNCH_TIMEOUT)
}

/// Spawns `command` and waits up to `timeout` for `started`; on anything else it is stopped and the reason returned.
fn start(mut command: Command, timeout: Duration) -> Result<Child, Error> {
    let mut child = command.stdout(Stdio::piped()).spawn()?;
    let events = read_events(child.stdout.take().expect("stdout is piped"));
    let deadline = Instant::now() + timeout;
    let reason = loop {
        match events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Event::Started) => return Ok(child),
            Ok(Event::Error(code)) => break error_msg(&code),
            Ok(_) => {}
            // Timed out, or stdout closed without `started`.
            Err(_) => break msg!(eaNoReason),
        }
    };
    // shortcut: on Linux a plain spawn leads no process group, so `kill_tree` fails and only maxima-cli stops; a game it
    // already started runs on. Give it its own group if that shows up.
    // After an error code Maxima exits by itself.
    if matches!(child.try_wait(), Ok(None))
        && let Err(e) = crate::platform::kill_tree(child.id())
    {
        tracing::warn!("stop maxima launch: {e}");
        _ = child.kill();
    }
    _ = child.wait();
    Err(reason.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(name: &str) -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/ea/{name}.jsonl")),
        )
        .unwrap()
    }

    fn fixture() -> Vec<Event> {
        lines("library").lines().filter_map(parse_line).collect()
    }

    /// The progress a job's lines report, and its outcome after exiting with `success`.
    fn job(name: &str, success: bool) -> (Vec<Progress>, Result<(), Msg>) {
        let mut job = Job::default();
        let progress = lines(name)
            .lines()
            .filter_map(|l| job.line(l, ProgressAction::Install))
            .collect();
        (progress, job.result(success))
    }

    /// A shell that prints `lines` (one JSON object each) and then runs `then`, standing in for `guild launch`.
    fn fake_launch(lines: &[&str], then: &str) -> Command {
        #[cfg(windows)]
        let (shell, flag, sep) = ("cmd", "/C", " & ");
        #[cfg(unix)]
        let (shell, flag, sep) = ("sh", "-c", "; ");
        // sh would strip the quotes of an unquoted line.
        let quote = if cfg!(windows) { "" } else { "'" };
        let mut script: Vec<String> = lines
            .iter()
            .map(|l| format!("echo {quote}{l}{quote}"))
            .collect();
        script.push(then.into());
        let mut command = Command::new(shell);
        // cmd takes the line as typed: escaped quotes would reach `echo`.
        #[cfg(windows)]
        std::os::windows::process::CommandExt::raw_arg(
            &mut command,
            format!("{flag} {}", script.join(sep)),
        );
        #[cfg(unix)]
        command.args([flag, &script.join(sep)]);
        command.stdin(Stdio::null());
        crate::platform::no_window(&mut command);
        command
    }

    const STAY: &str = if cfg!(windows) {
        "ping -n 30 127.0.0.1 >nul"
    } else {
        "sleep 30"
    };

    #[test]
    fn umu_gameid_comes_from_the_steam_twin() {
        let gameid = |steam| {
            game_command(&Dirs::new(), steam)
                .get_envs()
                .find(|(k, _)| *k == "MAXIMA_UMU_GAMEID")
                .and_then(|(_, v)| v.map(ToOwned::to_owned))
        };
        assert_eq!(gameid(Some(2316580)), Some("umu-2316580".into()));
        assert_eq!(gameid(None), None);
    }

    #[test]
    fn launch_waits_for_started() {
        let started = Instant::now();
        let mut child = start(fake_launch(&[r#"{"started":true}"#], STAY), LAUNCH_TIMEOUT).unwrap();
        assert!(started.elapsed() < Duration::from_secs(10));
        // Maxima stays while the game runs.
        assert!(matches!(child.try_wait(), Ok(None)));
        _ = crate::platform::kill_tree(child.id());
        _ = child.kill();
        _ = child.wait();
    }

    #[test]
    fn launch_error_code_is_shown() {
        let result = start(
            fake_launch(&[r#"{"error":"port_in_use"}"#], STAY),
            LAUNCH_TIMEOUT,
        );
        assert_eq!(result.err().map(Msg::from), Some(msg!(eaPortInUse)));
    }

    #[test]
    fn launch_without_started_has_no_reason() {
        // Exits without `started`.
        let result = start(fake_launch(&[r#"{"ok":true}"#], "exit 0"), LAUNCH_TIMEOUT);
        assert_eq!(result.err().map(Msg::from), Some(msg!(eaNoReason)));
        // Never says anything.
        let result = start(fake_launch(&[], STAY), Duration::from_millis(300));
        assert_eq!(result.err().map(Msg::from), Some(msg!(eaNoReason)));
    }

    #[test]
    fn progress_lines_become_progress() {
        let (progress, result) = job("install", true);
        assert_eq!(result, Ok(()));
        let fractions: Vec<f32> = progress.iter().map(|p| p.fraction).collect();
        assert_eq!(fractions, [0.0, 0.25, 1.0]);
        assert_eq!(progress[1].done_bytes, Some(3_915_829_247));
        assert!(
            progress
                .iter()
                .all(|p| p.total_bytes == Some(15_663_316_990)
                    && p.action == ProgressAction::Install
                    && p.secs_left.is_none())
        );
        // A run killed by Pause or Cancel ends without `done`.
        let mut killed = Job::default();
        killed.line(
            r#"{"progress":{"bytes":1,"total":0}}"#,
            ProgressAction::Update,
        );
        assert_eq!(killed.result(false), Err(msg!(eaNoReason)));
    }

    #[test]
    fn a_failed_job_maps_its_code() {
        assert_eq!(
            job("install-declined", false).1,
            Err(msg!(eaElevationDeclined))
        );
        // `done` without exit 0 is no success either.
        assert_eq!(job("install", false).1, Err(msg!(eaNoReason)));
    }

    #[test]
    fn info_gives_the_sizes() {
        let events: Vec<Event> = lines("info").lines().filter_map(parse_line).collect();
        assert_eq!(info_sizes(&events), Some((15_663_316_990, 16_953_192_983)));
    }

    #[test]
    fn install_folder_strips_forbidden_characters() {
        assert_eq!(
            install_folder("Tales of Kenzera™: ZAU", "Origin.OFR.50.0005198"),
            "Tales of Kenzera ZAU"
        );
        assert_eq!(
            install_folder("Mass Effect™ Legendary Edition", "x"),
            "Mass Effect Legendary Edition"
        );
        assert_eq!(install_folder("Who? ...", "x"), "Who");
        assert_eq!(install_folder(r#":/*?<>\|"#, "DR:1"), "DR1");
    }

    #[test]
    fn library_lines_become_games() {
        let events = fixture();
        assert_eq!(events.last(), Some(&Event::Done));
        let games = games(events, &Default::default());
        assert_eq!(games.len(), 21);
        let zau = games
            .iter()
            .find(|g| g.external_id == "Origin.OFR.50.0005198")
            .unwrap();
        assert_eq!(zau.key(), "ea-Origin.OFR.50.0005198");
        assert_eq!(zau.title, "Tales of Kenzera™: ZAU");
        assert!(zau.installed && !zau.update_available);
        assert_eq!(zau.install_dir, Some(PathBuf::from(r"C:\Games\EA")));
        assert!(
            games
                .iter()
                .filter(|g| !g.installed)
                .all(|g| g.install_dir.is_none() && !g.update_available)
        );
        let summary: Vec<String> = games
            .iter()
            .map(|g| format!("{} | {} | {}", g.key(), g.title, g.installed))
            .collect();
        insta::assert_yaml_snapshot!(summary);
    }

    #[test]
    fn differing_versions_are_an_update() {
        let line = r#"{"game":{"offer_id":"A","content_id":"1","slug":"a","name":" A ","installed":true,"install_path":"C:\\A","installed_version":"1","latest_version":"2","has_cloud_save":false}}"#;
        let games = games(vec![parse_line(line).unwrap()], &Default::default());
        assert!(games[0].update_available);
        assert_eq!(games[0].title, "A");
    }

    #[test]
    fn every_error_code_has_a_message() {
        for code in [
            "login_required",
            "login",
            "not_owned",
            "no_build",
            "path",
            "unsafe_path",
            "download",
            "touchup",
            "elevation_declined",
            "port_in_use",
            "launch",
        ] {
            assert_ne!(error_msg(code), msg!(eaNoReason), "{code}");
        }
        assert_eq!(error_msg("internal"), msg!(eaNoReason));
    }

    #[test]
    fn unknown_lines_are_ignored() {
        for line in [
            "",
            "not json",
            "[1]",
            r#"{"progress_note":1}"#,
            r#"{"progress":{"bytes":1}}"#,
            r#"{"done":false}"#,
            r#"{"game":{"x":1}}"#,
        ] {
            assert_eq!(parse_line(line), None, "{line}");
        }
        assert_eq!(
            parse_line(r#"{"login_url":"https://accounts.ea.com/connect/auth?x"}"#),
            Some(Event::LoginUrl(
                "https://accounts.ea.com/connect/auth?x".into()
            ))
        );
        assert_eq!(
            parse_line(r#"{"logged_in":{"name":"Noah","user_id":"1"}}"#),
            Some(Event::LoggedIn {
                name: "Noah".into()
            })
        );
        assert_eq!(
            parse_line(r#"{"logged_in":true,"name":"Noah","user_id":"1"}"#),
            Some(Event::Account(Some("Noah".into())))
        );
        assert_eq!(
            parse_line(r#"{"logged_in":false}"#),
            Some(Event::Account(None))
        );
        assert_eq!(parse_line(r#"{"ok":true}"#), Some(Event::Ok));
        assert_eq!(parse_line(r#"{"started":true}"#), Some(Event::Started));
        assert_eq!(parse_line(r#"{"stopped":true}"#), Some(Event::Stopped));
        assert_eq!(
            parse_line(r#"{"error":"port_in_use"}"#),
            Some(Event::Error("port_in_use".into()))
        );
    }
}
