//! Epic games through Legendary (https://github.com/derrod/legendary), an open-source
//! Epic client, so the Epic Games Launcher doesn't need to be installed. We only run
//! its CLI; Legendary keeps the login tokens, downloads and launches.
//! JSON shapes are from Legendary 0.21's `cli.py` and `models/game.py`:
//! - `list --json`: `[{app_name, app_title, metadata: <Epic catalog item>, …}]`
//! - `list-installed --json`: `[{app_name, title, install_path, …}]`
//! - `status --json`: `{account: "<not logged in>" | name, …}`

use std::collections::HashMap;
use std::io::ErrorKind;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

use super::{Art, Error, Game, Image, Progress, Provider, ProviderKind};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Login {
    /// `legendary.exe` isn't on PATH or in `%LOCALAPPDATA%\Programs\legendary`.
    NoLegendary,
    LoggedOut,
    LoggedIn,
    /// Legendary ran but couldn't confirm the login, e.g. offline: keep the last known games.
    Unreachable,
}

pub struct Epic;

impl Provider for Epic {
    fn list_games(&self) -> Result<Vec<Game>, Error> {
        Ok(games(&json(&["list", "--json"])?, &json(&["list-installed", "--json"])?))
    }

    /// Waits for Legendary, so a failure (e.g. an update is required) reaches the UI.
    fn launch(&self, game: &Game) -> Result<(), Error> {
        run(legendary(&["launch", &game.external_id]))
    }

    /// Waits until the download finishes, so the library rescans right after.
    // shortcut: progress shows in Legendary's own console window; parse its output for an in-app bar if needed.
    // shortcut: installs to Legendary's default folder; add `--base-path` when a folder setting exists.
    fn install(&self, game: &Game) -> Result<(), Error> {
        let mut command = legendary(&["-y", "install", &game.external_id, "--skip-sdl"]);
        // A visible console replaces the hidden one, so the user can watch and cancel the download.
        let status = command.creation_flags(CREATE_NEW_CONSOLE).status()?;
        if status.success() { Ok(()) } else { Err(format!("Legendary install exited with {status}").into()) }
    }

    fn uninstall(&self, game: &Game) -> Result<(), Error> {
        run(legendary(&["-y", "uninstall", &game.external_id]))
    }

    // shortcut: Legendary doesn't report running games; would need process matching against the install dir.
    fn running_game(&self) -> Option<String> {
        None
    }

    fn download_progress(&self, _game: &Game) -> Option<Progress> {
        None
    }
}

pub fn login_state() -> Login {
    match json(&["status", "--json"]) {
        Ok(status) if status["account"] == "<not logged in>" => Login::LoggedOut,
        Ok(_) => Login::LoggedIn,
        Err(e) if e.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == ErrorKind::NotFound) => {
            Login::NoLegendary
        }
        // `status` refreshes the session online and exits 1 when that fails.
        // shortcut: an expired session also lands here; it needs `legendary auth --delete` before "Log in" shows.
        Err(e) => {
            eprintln!("legendary status: {e}");
            Login::Unreachable
        }
    }
}

/// Hands a code from the login window to Legendary, which stores the session.
/// The code is single-use and short-lived; Legendary only accepts it as an argument.
pub fn log_in(authorization_code: &str) -> Result<(), Error> {
    run(legendary(&["auth", "--code", authorization_code]))
}

/// `%LOCALAPPDATA%\Programs\legendary\legendary.exe` if present (no PATH edit needed), else `legendary` on PATH.
fn legendary_exe() -> PathBuf {
    let local = directories::BaseDirs::new()
        .map(|d| d.data_local_dir().join("Programs").join("legendary").join("legendary.exe"));
    local.filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("legendary"))
}

fn legendary(args: &[&str]) -> Command {
    let mut command = Command::new(legendary_exe());
    command.args(args).creation_flags(CREATE_NO_WINDOW);
    command
}

fn run(mut command: Command) -> Result<(), Error> {
    let output = command.output()?;
    if output.status.success() { Ok(()) } else { Err(failure(&output).into()) }
}

fn json(args: &[&str]) -> Result<Value, Error> {
    let output = legendary(args).output()?;
    if !output.status.success() {
        return Err(failure(&output).into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// Legendary logs to stderr; its last line says what went wrong.
fn failure(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no output");
    format!("Legendary failed ({}): {last}", output.status)
}

/// Owned games, marked installed where Legendary has them, plus installed games missing
/// from the owned list (e.g. imported ones), so nothing installed disappears.
fn games(owned: &Value, installed: &Value) -> Vec<Game> {
    let mut installed: HashMap<&str, &Value> =
        installed.as_array().into_iter().flatten().filter_map(|g| Some((g["app_name"].as_str()?, g))).collect();
    let mut games: Vec<Game> = owned
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            let name = g["app_name"].as_str()?;
            let local = installed.remove(name);
            let title = g["app_title"].as_str().or(local.and_then(|l| l["title"].as_str())).unwrap_or(name);
            Some(game(name, title, local, art(&g["metadata"])))
        })
        .collect();
    games.extend(installed.into_iter().map(|(name, local)| {
        game(name, local["title"].as_str().unwrap_or(name), Some(local), Art::default())
    }));
    games
}

fn game(name: &str, title: &str, installed: Option<&Value>, art: Art) -> Game {
    Game {
        provider: ProviderKind::Epic,
        external_id: name.to_owned(),
        title: title.to_owned(),
        installed: installed.is_some(),
        install_dir: installed.and_then(|l| l["install_path"].as_str()).map(PathBuf::from),
        // shortcut: Legendary doesn't track playtime or last played.
        playtime_minutes: None,
        last_played: None,
        art,
    }
}

/// Epic catalog `keyImages`: tall box art is the cover, the wide one is header and hero.
fn art(metadata: &Value) -> Art {
    let image = |kind: &str| Image {
        path: None,
        url: metadata["keyImages"]
            .as_array()
            .and_then(|images| images.iter().find(|k| k["type"] == kind))
            .and_then(|k| k["url"].as_str())
            .map(str::to_owned),
    };
    Art {
        cover: image("DieselGameBoxTall"),
        header: image("DieselGameBox"),
        hero: image("DieselGameBox"),
        logo: image("DieselGameBoxLogo"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn fixture(name: &str) -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legendary").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn sorted_games() -> Vec<Game> {
        let mut all = games(&fixture("list.json"), &fixture("list-installed.json"));
        all.sort_by(|a, b| a.external_id.cmp(&b.external_id));
        all
    }

    #[test]
    fn merges_owned_and_installed() {
        let all = sorted_games();
        let ids: Vec<_> = all.iter().map(|g| g.external_id.as_str()).collect();
        // Goo is owned only, Fortnite owned and installed, Imported installed only.
        assert_eq!(ids, ["Fortnite", "Goo", "Imported"]);

        let fortnite = &all[0];
        assert!(fortnite.installed);
        assert_eq!(fortnite.install_dir, Some(PathBuf::from(r"C:\Games\Fortnite")));
        assert_eq!(fortnite.key(), "epic-Fortnite");
        assert_eq!(fortnite.art.cover.url.as_deref(), Some("https://cdn1.epicgames.com/tall.jpg"));
        assert_eq!(fortnite.art.hero.url.as_deref(), Some("https://cdn1.epicgames.com/wide.jpg"));

        let goo = &all[1];
        assert_eq!(goo.title, "World of Goo");
        assert!(!goo.installed);
        assert_eq!(goo.install_dir, None);
        // No keyImages at all.
        assert_eq!(goo.art, Art::default());

        let imported = &all[2];
        assert_eq!(imported.title, "Imported Game");
        assert!(imported.installed);
    }

    #[test]
    fn junk_json_gives_no_games() {
        assert!(games(&Value::Null, &Value::Null).is_empty());
        let no_names = serde_json::json!([{ "app_title": "x" }]);
        assert!(games(&no_names, &no_names).is_empty());
    }

    #[test]
    fn failure_reports_the_last_stderr_line() {
        use std::os::windows::process::ExitStatusExt;
        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(1),
            stdout: Vec::new(),
            stderr: b"[cli] INFO: Logging in...\n[cli] ERROR: Login failed, cannot continue!\n\n".to_vec(),
        };
        assert_eq!(failure(&output), "Legendary failed (exit code: 1): [cli] ERROR: Login failed, cannot continue!");
    }
}
