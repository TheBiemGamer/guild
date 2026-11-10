//! EA games through the owner's Maxima fork (`maxima-cli guild …`, https://github.com/TheBiemGamer/Maxima). Maxima
//! makes every EA call and keeps its own files in `<data>/ea` (`MAXIMA_DATA_DIR`), the login included. Each command
//! prints one JSON object per line on stdout; its stderr is dropped unread, since a token could still be in it.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use tauri::AppHandle;

use super::tool::StoreLogin;
use super::{Art, Error, Game, Image, ProviderKind};
use crate::cache::{Dirs, db};
use crate::msg::Msg;
use crate::tools;

const LIBRARY_TIMEOUT: Duration = Duration::from_secs(120);
const SHORT_TIMEOUT: Duration = Duration::from_secs(60);
/// `guild login` prints its URL before it waits for the browser, and the account once it has the code.
const LOGIN_STEP: Duration = Duration::from_secs(30);
/// Where `guild login` waits for the code.
const LOGIN_PORT: u16 = 31033;
const LOGIN_EXIT: Duration = Duration::from_secs(10);

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
    Done,
    Ok,
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
    if object.get("done") == Some(&Value::Bool(true)) {
        return Some(Event::Done);
    }
    (object.get("ok") == Some(&Value::Bool(true))).then_some(Event::Ok)
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

/// Reads `stdout`'s events on a thread until it closes.
fn read_events(stdout: impl Read + Send + 'static) -> mpsc::Receiver<Event> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(event) = parse_line(&line)
                && tx.send(event).is_err()
            {
                break;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<Event> {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ea/library.jsonl"),
        )
        .unwrap();
        text.lines().filter_map(parse_line).collect()
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
        assert_eq!(
            parse_line(r#"{"error":"port_in_use"}"#),
            Some(Event::Error("port_in_use".into()))
        );
    }
}
