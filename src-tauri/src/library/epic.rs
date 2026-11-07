//! Epic games through Legendary (https://github.com/derrod/legendary), an open-source
//! Epic client, so the Epic Games Launcher doesn't need to be installed. We only run
//! its CLI; Legendary keeps the login tokens, downloads and launches.
//! JSON shapes are from Legendary 0.21's `cli.py` and `models/game.py`:
//! - `list --json`: `[{app_name, app_title, metadata: <Epic catalog item>, …}]`
//! - `list-installed --json`: `[{app_name, title, install_path, …}]`
//! - `status --json`: `{account: "<not logged in>" | name, …}`

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, ErrorKind};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use serde_json::Value;

use super::{Art, CREATE_NO_WINDOW, Dlc, Error, Game, Image, Progress, Provider, ProviderKind};

/// What a running install reports back.
#[derive(Debug, Clone)]
pub enum InstallEvent {
    /// Legendary's process id, so the install can be cancelled.
    Started(u32),
    Progress(Progress),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
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
        Ok(games(&json(&["list", "--json"])?, &installed_list()?))
    }

    /// Waits for Legendary, so a failure (e.g. an update is required) reaches the UI.
    fn launch(&self, game: &Game) -> Result<(), Error> {
        launch(&game.external_id, false)
    }

    fn install(&self, game: &Game) -> Result<(), Error> {
        install(&game.external_id, Some(true), None, |_| {})
    }

    fn uninstall(&self, game: &Game) -> Result<(), Error> {
        uninstall(&game.external_id)
    }

    /// Epic has no RunningAppID: the app watches the process of games it launched (`executable`).
    fn running_game(&self) -> Option<String> {
        None
    }

    /// Progress arrives through `install`'s events instead.
    fn download_progress(&self, _game: &Game) -> Option<Progress> {
        None
    }
}

/// Starts a game and waits for Legendary. `skip_version_check` starts an out-of-date game anyway.
pub fn launch(app_name: &str, skip_version_check: bool) -> Result<(), Error> {
    let mut command = legendary(&["launch", app_name]);
    if skip_version_check {
        command.arg("--skip-version-check");
    }
    run(command)
}

/// Installed games with their update flags. The update check is online, so when it fails
/// (e.g. offline) the plain list still answers, with no updates marked.
fn installed_list() -> Result<Value, Error> {
    with_fallback(json(&["list-installed", "--check-updates", "--json"]), || json(&["list-installed", "--json"]))
}

fn with_fallback(checked: Result<Value, Error>, plain: impl FnOnce() -> Result<Value, Error>) -> Result<Value, Error> {
    checked.or_else(|e| {
        eprintln!("legendary list-installed --check-updates: {e}");
        plain()
    })
}

/// Runs a hidden `legendary install` of a game or a DLC (Legendary's app name), reporting its pid and
/// progress as they come. Returns once the download ends; killing the pid (see `cancel`) ends it with an error.
/// Installs under `base_path` when given, else in Legendary's default folder; a DLC goes into its game's folder.
/// `with_dlc` (a game, not a DLC) installs its owned DLC too or skips them (`--with-dlcs` / `--skip-dlcs`).
pub fn install(
    app_name: &str,
    with_dlc: Option<bool>,
    base_path: Option<&Path>,
    on_event: impl FnMut(InstallEvent),
) -> Result<(), Error> {
    let mut command = legendary(&["-y", "install", app_name, "--skip-sdl"]);
    if let Some(with_dlc) = with_dlc {
        command.arg(if with_dlc { "--with-dlcs" } else { "--skip-dlcs" });
    }
    if let Some(dir) = base_path {
        command.arg("--base-path").arg(dir);
    }
    stream(command, on_event)
}

/// Updates an installed game (Legendary's app name) like `install`: same events, same cancel.
pub fn update(app_name: &str, on_event: impl FnMut(InstallEvent)) -> Result<(), Error> {
    stream(legendary(&["-y", "update", app_name, "--skip-sdl"]), on_event)
}

fn stream(mut command: Command, mut on_event: impl FnMut(InstallEvent)) -> Result<(), Error> {
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    on_event(InstallEvent::Started(child.id()));
    let mut downloaded_mib = 0.0;
    let mut last_line = String::new();
    for line in BufReader::new(child.stderr.take().expect("stderr is piped")).lines().map_while(Result::ok) {
        if let Some(progress) = parse_progress(&line, &mut downloaded_mib) {
            on_event(InstallEvent::Progress(progress));
        }
        if !line.trim().is_empty() {
            last_line = line;
        }
    }
    let status = child.wait()?;
    if status.success() { Ok(()) } else { Err(format!("Legendary install failed ({status}): {last_line}").into()) }
}

/// Uninstalls a game or a DLC (Legendary's app name).
pub fn uninstall(app_name: &str) -> Result<(), Error> {
    run(legendary(&["-y", "uninstall", app_name]))
}

/// Stops an install started by `install`, including Legendary's download worker processes.
/// Legendary resumes the download next time.
pub fn cancel(pid: u32) -> Result<(), Error> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
    if status.success() { Ok(()) } else { Err(format!("taskkill exited with {status}").into()) }
}

/// Legendary's DLManager logs, about once a second:
/// `= Progress: 12.34% (123/456), Running for 00:00:10, ETA: 00:01:00`
/// ` - Downloaded: 123.45 MiB, Written: 234.56 MiB`
/// The Downloaded line follows the Progress line, so the text shows the previous second's total.
fn parse_progress(line: &str, downloaded_mib: &mut f64) -> Option<Progress> {
    if let Some((_, rest)) = line.split_once(" - Downloaded: ") {
        if let Some(mib) = rest.split_whitespace().next().and_then(|n| n.parse().ok()) {
            *downloaded_mib = mib;
        }
        return None;
    }
    let (_, rest) = line.split_once("= Progress: ")?;
    let percent: f32 = rest.split('%').next()?.trim().parse().ok()?;
    let eta = rest.split_once("ETA: ").map_or("--:--:--", |(_, eta)| eta.trim());
    let text = format!("Installing {percent:.0}% · {:.1} GB downloaded · {eta} left", *downloaded_mib / 1024.0);
    Some(Progress { fraction: (percent / 100.0).clamp(0.0, 1.0), text })
}

/// `legendary info <app> --json`: the game and its manifest. Slow (an online call), so cache the result.
pub fn info_json(game: &Game) -> Result<Value, Error> {
    json(&["info", &game.external_id, "--json"])
}

/// Bytes to download, from `manifest.download_size` of `info_json` (verified on a real capture).
pub fn download_size_from_info(info: &Value) -> Option<u64> {
    info["manifest"]["download_size"].as_u64()
}

/// `legendary list --json`: every owned game with its Epic catalog item (`metadata`).
/// The scan only keeps art, so the game page asks again; callers cache every entry from one call.
pub fn catalog() -> Result<Value, Error> {
    json(&["list", "--json"])
}

/// File name of an installed game's main executable (from `list-installed`), to spot it in the process list.
pub fn executable(game: &Game) -> Result<Option<String>, Error> {
    let installed = json(&["list-installed", "--json"])?;
    let exe = installed
        .as_array()
        .into_iter()
        .flatten()
        .find(|g| g["app_name"] == game.external_id.as_str())
        .and_then(|g| g["executable"].as_str())
        .and_then(|exe| Path::new(exe).file_name())
        .map(|name| name.to_string_lossy().into_owned());
    Ok(exe)
}

/// Whether a process named `exe` runs.
// shortcut: games whose executable is a bootstrapper that hands off to another exe stop counting as running early.
pub fn is_running(exe: &str) -> bool {
    super::process_listed(&format!("IMAGENAME eq {exe}"), exe)
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

/// Legendary's own version, for About. Asked once per app run; Settings reads it on every open.
pub fn legendary_version() -> Option<String> {
    static VERSION: OnceLock<Option<String>> = OnceLock::new();
    VERSION
        .get_or_init(|| {
            let output = legendary(&["--version"]).output().ok()?;
            parse_legendary_version(&String::from_utf8_lossy(&output.stdout))
        })
        .clone()
}

/// `legendary version "0.21.1", codename "Lowlife"` -> `0.21.1`
fn parse_legendary_version(text: &str) -> Option<String> {
    let version = text.split('"').nth(1)?;
    (!version.is_empty()).then(|| version.to_owned())
}

/// Hands a code from the login window to Legendary, which stores the session.
/// The code is single-use and short-lived; Legendary only accepts it as an argument.
pub fn log_in(authorization_code: &str) -> Result<(), Error> {
    run(legendary(&["auth", "--code", authorization_code]))
}

/// Makes Legendary forget the login.
pub fn log_out() -> Result<(), Error> {
    run(legendary(&["auth", "--delete"]))
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

/// Whether an Epic catalog item (a `list --json` entry) has `path` among its `metadata.categories`.
fn has_category(item: &Value, path: &str) -> bool {
    item["metadata"]["categories"].as_array().into_iter().flatten().any(|c| c["path"] == path)
}

/// Games, as opposed to Unreal Engine assets, engines, DLC, apps, servers and hidden content.
fn is_game(item: &Value) -> bool {
    has_category(item, "games")
}

/// Owned games, marked installed where Legendary has them, plus installed items missing
/// from the owned list (e.g. imported ones). Owned items that aren't games are left out unless
/// installed, so nothing installed disappears. A DLC (`addons`) goes under its base game, found by
/// the catalog id in its `mainGameItem`; one whose base isn't listed is treated like other non-games.
fn games(owned: &Value, installed: &Value) -> Vec<Game> {
    let mut installed: HashMap<&str, &Value> =
        installed.as_array().into_iter().flatten().filter_map(|g| Some((g["app_name"].as_str()?, g))).collect();
    // `list --json` nests DLC in its base game's `dlcs`; flattened, the catalog id finds the base either way.
    let items: Vec<&Value> = owned
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|g| std::iter::once(g).chain(g["dlcs"].as_array().into_iter().flatten()))
        .collect();
    // Bases that will be listed: games and installed items.
    let bases: HashSet<&str> = items
        .iter()
        .filter(|g| is_game(g) || g["app_name"].as_str().is_some_and(|n| installed.contains_key(n)))
        .filter_map(|g| g["metadata"]["id"].as_str())
        .collect();
    let mut dlc: HashMap<&str, Vec<Dlc>> = HashMap::new();
    let items: Vec<&Value> = items
        .into_iter()
        .filter(|g| {
            let base = g["metadata"]["mainGameItem"]["id"].as_str().filter(|id| bases.contains(id));
            let (Some(name), Some(base), true) = (g["app_name"].as_str(), base, has_category(g, "addons")) else {
                return true;
            };
            let title = g["app_title"].as_str().unwrap_or(name).to_owned();
            dlc.entry(base).or_default().push(Dlc { id: name.to_owned(), title, installed: installed.remove(name).is_some() });
            false
        })
        .collect();
    let mut games: Vec<Game> = items
        .into_iter()
        .filter_map(|g| {
            let name = g["app_name"].as_str()?;
            let local = installed.remove(name);
            if local.is_none() && !is_game(g) {
                return None;
            }
            let title = g["app_title"].as_str().or(local.and_then(|l| l["title"].as_str())).unwrap_or(name);
            let mut game = game(name, title, local, art(&g["metadata"]));
            game.dlc = g["metadata"]["id"].as_str().and_then(|id| dlc.remove(id)).unwrap_or_default();
            game.dlc.sort_by(|a, b| a.title.cmp(&b.title));
            Some(game)
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
        size_bytes: installed.and_then(|l| l["install_size"].as_u64()),
        art,
        dlc: Vec::new(),
        update_available: installed.is_some_and(has_update),
    }
}

/// `list-installed --check-updates --json` marks an update with `update_available`, or by a `remote_version`
/// that differs from `version`. // shortcut: both spellings are read because no real capture with an update exists yet.
fn has_update(local: &Value) -> bool {
    local["update_available"].as_bool().unwrap_or_else(|| {
        local["remote_version"].as_str().is_some_and(|remote| local["version"].as_str().is_some_and(|v| v != remote))
    })
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
        assert_eq!(fortnite.size_bytes, Some(1234));
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

    /// Trimmed from real Legendary metadata: a game, its DLC, an Unreal Engine plugin, an engine,
    /// an app, and a DLC of a game that isn't owned.
    fn categories() -> Value {
        fixture("list-categories.json")
    }

    const NEW_VEGAS: &str = "5daeb974a22a435988892319b3a4f476";
    const SPOTIFY: &str = "2b04bd96d2c34822b37c91ece7fa5f1c";

    #[test]
    fn parses_update_available_from_list_installed() {
        let all = sorted_games();
        // Fortnite: the store's version differs. Imported: no remote version, so no update.
        assert!(all[0].update_available);
        assert!(!all[2].update_available);
        let flagged = serde_json::json!([{ "app_name": "A", "version": "1", "update_available": true }]);
        assert!(games(&Value::Null, &flagged)[0].update_available);
        let same = serde_json::json!([{ "app_name": "A", "version": "1", "remote_version": "1" }]);
        assert!(!games(&Value::Null, &same)[0].update_available);
        // Not installed: nothing to update.
        assert!(!all[1].update_available);
    }

    #[test]
    fn failed_update_check_falls_back_to_the_plain_list() {
        let plain = || Ok(serde_json::json!([{ "app_name": "A" }]));
        assert_eq!(with_fallback(Err("offline".into()), plain).unwrap(), serde_json::json!([{ "app_name": "A" }]));
        assert_eq!(with_fallback(Ok(Value::Null), || Err("unused".into())).unwrap(), Value::Null);
        assert!(with_fallback(Err("a".into()), || Err("b".into())).is_err());
    }

    #[test]
    fn list_keeps_only_games() {
        let ids: Vec<_> = games(&categories(), &Value::Null).into_iter().map(|g| g.external_id).collect();
        assert_eq!(ids, [NEW_VEGAS]);
    }

    #[test]
    fn installed_non_game_is_kept() {
        let installed = serde_json::json!([{ "app_name": SPOTIFY, "title": "Spotify", "install_path": r"C:\Spotify" }]);
        let all = games(&categories(), &installed);
        let spotify = all.iter().find(|g| g.external_id == SPOTIFY).expect("installed app stays");
        assert!(spotify.installed);
        assert_eq!(spotify.title, "Spotify Music and Podcasts");
        assert_eq!(all.len(), 2);
    }

    const LONESOME_ROAD: &str = "4fa3d8d9b2cb4714a19a38d1a598be8f";

    fn lonesome_road(installed: bool) -> Dlc {
        Dlc { id: LONESOME_ROAD.into(), title: "Fallout New Vegas®: Lonesome Road™".into(), installed }
    }

    #[test]
    fn addon_attaches_to_its_base_game() {
        assert_eq!(games(&categories(), &Value::Null)[0].dlc, [lonesome_road(false)]);

        // Installed, it shows as installed under its game, not as a game of its own.
        let installed = serde_json::json!([{ "app_name": LONESOME_ROAD, "title": "Lonesome Road", "is_dlc": true }]);
        let all = games(&categories(), &installed);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].dlc, [lonesome_road(true)]);

        // `list --json` nests DLC under its base game's `dlcs`.
        let mut list = categories();
        let dlc = list.as_array_mut().unwrap().remove(1);
        list[0]["dlcs"] = serde_json::json!([dlc]);
        assert_eq!(games(&list, &Value::Null)[0].dlc, [lonesome_road(false)]);
    }

    #[test]
    fn dlc_without_base_in_library_is_hidden() {
        const SAND_PATCH: &str = "0e07b42c0ffe4cfdb2095568f461f38f";
        let all = games(&categories(), &Value::Null);
        assert!(all.iter().all(|g| g.external_id != SAND_PATCH && g.dlc.iter().all(|d| d.id != SAND_PATCH)));

        // Installed, it stays as its own entry: nothing installed disappears.
        let installed = serde_json::json!([{ "app_name": SAND_PATCH, "title": "Sand Patch Grade", "is_dlc": true }]);
        assert!(games(&categories(), &installed).iter().any(|g| g.external_id == SAND_PATCH));
    }

    #[test]
    fn dlc_of_a_base_left_out_is_not_lost_when_installed() {
        // A DLC of Spotify, an app that isn't listed unless installed.
        let mut list = categories();
        list.as_array_mut().unwrap().push(serde_json::json!({
            "app_name": "SpotifyDlc", "app_title": "Spotify Extra",
            "metadata": { "id": "x", "categories": [{ "path": "addons" }],
                          "mainGameItem": { "id": "b55fccb269ad47928804c681b16b8e93" } }
        }));
        let all = games(&list, &Value::Null);
        assert!(all.iter().all(|g| g.external_id != "SpotifyDlc" && g.dlc.iter().all(|d| d.id != "SpotifyDlc")));

        let installed = serde_json::json!([{ "app_name": "SpotifyDlc", "title": "Spotify Extra", "is_dlc": true }]);
        let all = games(&list, &installed);
        let dlc = all.iter().find(|g| g.external_id == "SpotifyDlc").expect("installed DLC stays");
        assert!(dlc.installed);
    }

    #[test]
    fn parses_legendary_progress_lines() {
        let mut mib = 0.0;
        let progress = "[DLManager] INFO: = Progress: 12.34% (123/456), Running for 00:00:10, ETA: 00:01:05";
        let first = parse_progress(progress, &mut mib).unwrap();
        assert!((first.fraction - 0.1234).abs() < 1e-6);
        assert_eq!(first.text, "Installing 12% · 0.0 GB downloaded · 00:01:05 left");

        assert_eq!(parse_progress("[DLManager] INFO:  - Downloaded: 2048.00 MiB, Written: 3000.00 MiB", &mut mib), None);
        assert_eq!(mib, 2048.0);
        assert_eq!(parse_progress(progress, &mut mib).unwrap().text, "Installing 12% · 2.0 GB downloaded · 00:01:05 left");

        // Other log lines and broken numbers are ignored.
        assert_eq!(parse_progress("[DLManager] INFO:  + Download	- 10.00 MiB/s (raw)", &mut mib), None);
        assert_eq!(parse_progress("= Progress: abc%", &mut mib), None);
    }

    #[test]
    fn download_size_from_info_reads_the_fixture() {
        // Captured from `legendary info <AppName> --json` (A Short Hike); the size is `manifest.download_size`.
        assert_eq!(download_size_from_info(&fixture("info.json")), Some(224_976_506));
    }

    #[test]
    fn download_size_from_info_handles_missing_fields() {
        assert_eq!(download_size_from_info(&serde_json::json!({})), None);
        assert_eq!(download_size_from_info(&serde_json::json!({ "manifest": { "download_size": "big" } })), None);
    }

    #[test]
    fn parses_the_legendary_version() {
        assert_eq!(parse_legendary_version("legendary version \"0.21.1\", codename \"Lowlife\"
").as_deref(), Some("0.21.1"));
        assert_eq!(parse_legendary_version("something else"), None);
        assert_eq!(parse_legendary_version("legendary version \"\""), None);
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
