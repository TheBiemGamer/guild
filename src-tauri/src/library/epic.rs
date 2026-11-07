//! Epic games through Legendary (https://github.com/derrod/legendary), an open-source
//! Epic client, so the Epic Games Launcher doesn't need to be installed. We only run
//! its CLI; Legendary keeps the login tokens, downloads and launches.
//! JSON shapes are from Legendary 0.21's `cli.py` and `models/game.py`:
//! - `list --json`: `[{app_name, app_title, metadata: <Epic catalog item>, …}]`
//! - `list-installed --json`: `[{app_name, title, install_path, …}]`
//! - `status --json`: `{account: "<not logged in>" | name, …}`

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Component, Path, PathBuf, Prefix};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::msg::Msg;

use super::{Art, CREATE_NO_WINDOW, Dlc, Error, Game, Image, Progress, ProgressAction, Provider, ProviderKind};

/// What a running install reports back.
#[derive(Debug, Clone)]
pub enum InstallEvent {
    /// Legendary's process id, so the install can be cancelled.
    Started(u32),
    Progress(Progress),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Login {
    /// `legendary.exe` is in none of the places `tools::LEGENDARY.exe()` looks.
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
        launch(&game.external_id, &LaunchOptions::default(), false)
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

/// A copy's own launch options, kept in `game_prefs` as JSON. `exe` is relative to the install folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct LaunchOptions {
    /// Extra game arguments as typed; split by `split_args`.
    pub args: String,
    pub offline: bool,
    pub skip_version_check: bool,
    pub exe: Option<String>,
}

/// Splits typed arguments on whitespace; double quotes group. No shell, no variables.
pub fn split_args(text: &str) -> Vec<String> {
    let mut args = Vec::new();
    // Some once a token has started, so `""` gives an empty argument.
    let mut arg: Option<String> = None;
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                arg.get_or_insert_default();
            }
            c if c.is_whitespace() && !quoted => args.extend(arg.take()),
            c => arg.get_or_insert_default().push(c),
        }
    }
    args.extend(arg);
    args
}

/// `legendary launch` arguments. Flags from `legendary launch --help` (0.21.1); its "additional arguments
/// are passed to the game" go after them. `skip_version_check` also comes from Play without updating.
// shortcut: a game argument that spells a Legendary flag (or its prefix, like `--off`) is taken by Legendary.
pub fn launch_args(app_name: &str, options: &LaunchOptions, skip_version_check: bool) -> Vec<String> {
    let mut args = vec!["launch".to_string(), app_name.to_string()];
    if options.offline {
        args.push("--offline".into());
    }
    if skip_version_check || options.skip_version_check {
        args.push("--skip-version-check".into());
    }
    if let Some(exe) = &options.exe {
        args.extend(["--override-exe".into(), exe.clone()]);
    }
    args.extend(split_args(&options.args));
    args
}

/// `exe` (absolute, or relative to `install_dir`) as a path relative to `install_dir`, only when it is an
/// existing file inside it.
pub fn override_exe(install_dir: &Path, exe: &str) -> Result<String, Error> {
    let dir = install_dir.canonicalize().map_err(|_| msg!(installFolderGone))?;
    // Both canonical, so `..`, links and other drives can't slip past the prefix check.
    let file = install_dir.join(exe).canonicalize().map_err(|_| msg!(exeOutsideInstallFolder))?;
    match file.strip_prefix(&dir) {
        Ok(relative) if file.is_file() => Ok(relative.to_string_lossy().into_owned()),
        _ => Err(msg!(exeOutsideInstallFolder).into()),
    }
}

/// Starts a game and waits for Legendary. `skip_version_check` starts an out-of-date game anyway.
pub fn launch(app_name: &str, options: &LaunchOptions, skip_version_check: bool) -> Result<(), Error> {
    let mut command = Command::new(legendary_exe());
    command.args(launch_args(app_name, options, skip_version_check)).creation_flags(CREATE_NO_WINDOW);
    run(command)
}

/// Which way `sync-saves` goes. Legendary compares the newest local file with the cloud save's date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveSync {
    /// Before launch: take a newer cloud save, never upload.
    Download,
    /// After the game exits: send a newer local save, never download.
    Upload,
    /// Sync now: the newer side wins.
    Both,
    /// Keep cloud: download even over a newer local save.
    KeepCloud,
    /// Keep local: upload even over a newer cloud save.
    KeepLocal,
}

/// What a `sync-saves` run found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    /// Up to date, transferred, or nothing to sync.
    Synced,
    /// The local save is newer and uploading was off, so nothing changed.
    LocalNewer,
    /// The cloud save is newer and downloading was off, so nothing changed.
    CloudNewer,
    /// GOG: both sides changed since the last sync, so nothing changed. Asked like `CloudNewer`.
    Conflict,
    /// Why: often the tool's last message, as `system`.
    Failed(Msg),
}

/// Flags from `legendary sync-saves --help` (0.21.1).
fn sync_args(app_name: &str, sync: SaveSync) -> Vec<&str> {
    let mut args = vec!["sync-saves", app_name];
    args.extend_from_slice(match sync {
        SaveSync::Download => &["--skip-upload"][..],
        SaveSync::Upload => &["--skip-download"],
        SaveSync::Both => &[],
        SaveSync::KeepCloud => &["--skip-upload", "--force-download"],
        SaveSync::KeepLocal => &["--skip-download", "--force-upload"],
    });
    args
}

/// Answers for `sync-saves`' prompts. Legendary 0.21 has no flag that accepts the save folder it works out,
/// and `-y` skips a game whose folder isn't set yet. Each prompt ("Is this correct?" for the folder,
/// "Download cloud save?", "Upload local save?") takes yes on an empty line; a folder it can't work out
/// asks for a path instead, where an empty line skips the game.
const SYNC_ANSWERS: &[u8] = b"\n\n\n\n";

/// Longest a `sync-saves` run may take; a stalled network call must not hold the launch.
pub(super) const SYNC_TIMEOUT: Duration = Duration::from_secs(30);

/// Runs `legendary sync-saves` for one installed game. Never fails: a failure is an outcome.
pub fn sync_saves(app_name: &str, sync: SaveSync) -> SyncOutcome {
    match run_with_timeout(legendary(&sync_args(app_name, sync)), SYNC_ANSWERS, SYNC_TIMEOUT) {
        Ok(Some((success, _, log))) => parse_sync(success, &log),
        Ok(None) => SyncOutcome::Failed(msg!(savesTimedOut)),
        Err(e) => SyncOutcome::Failed(Msg::system(e)),
    }
}

/// Runs `command` with `input` on stdin (then closed, so a prompt beyond it ends the process instead of
/// hanging) and returns whether it succeeded, its stdout and its stderr. `None` when it outlives `timeout`: its
/// process tree is killed (legendary.exe and gogdl.exe unpack themselves into a child process).
pub(super) fn run_with_timeout(
    mut command: Command,
    input: &[u8],
    timeout: Duration,
) -> std::io::Result<Option<(bool, String, String)>> {
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    child.stdin.take().expect("stdin is piped").write_all(input)?;
    // Read aside, so a full pipe never stalls the process while this waits.
    let read = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            _ = pipe.read_to_end(&mut bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        })
    };
    let out = read(Box::new(child.stdout.take().expect("stdout is piped")));
    let err = read(Box::new(child.stderr.take().expect("stderr is piped")));
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some((status.success(), out.join().unwrap_or_default(), err.join().unwrap_or_default())));
        }
        if Instant::now() >= deadline {
            if let Err(e) = crate::platform::kill_tree(child.id()) {
                tracing::warn!("stop timed-out sync: {e}");
                _ = child.kill();
            }
            _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Reads Legendary 0.21's `sync-saves` log (stderr, `[cli] INFO: …` lines, from its `cli.py`). A side that
/// has no save at all (`date: N/A`) is no conflict: the other side simply goes over next time.
fn parse_sync(success: bool, log: &str) -> SyncOutcome {
    if !success {
        let last = log.lines().rev().map(str::trim).find(|l| !l.is_empty());
        // `[cli] ERROR: Login failed!` -> `Login failed!`
        let message = last.map(|l| l.split_once(": ").filter(|(head, _)| head.starts_with('[')).map_or(l, |(_, m)| m));
        return SyncOutcome::Failed(message.map_or(msg!(legendaryNoReason), Msg::system));
    }
    let has = |text: &str| log.contains(text);
    if has("Empty input, skipping...") || has("skipping due to --yes") {
        SyncOutcome::Failed(msg!(legendaryNoSaveFolder))
    } else if has("Save game uploading is disabled") && !has("- Cloud save date: N/A") {
        SyncOutcome::LocalNewer
    } else if has("Save game downloading is disabled") && !has("- Local save date: N/A") {
        SyncOutcome::CloudNewer
    } else {
        SyncOutcome::Synced
    }
}

/// Legendary's `supports_cloud_saves`: the catalog item (`metadata`) has a `customAttributes.CloudSaveFolder`.
fn has_cloud_saves(metadata: &Value) -> bool {
    !metadata["customAttributes"]["CloudSaveFolder"].is_null()
}

/// Installed games with their update flags. The update check is online, so when it fails
/// (e.g. offline) the plain list still answers, with no updates marked.
fn installed_list() -> Result<Value, Error> {
    with_fallback(json(&["list-installed", "--check-updates", "--json"]), || json(&["list-installed", "--json"]))
}

fn with_fallback(checked: Result<Value, Error>, plain: impl FnOnce() -> Result<Value, Error>) -> Result<Value, Error> {
    checked.or_else(|e| {
        tracing::warn!("legendary list-installed --check-updates: {e}");
        plain()
    })
}

/// Runs a hidden `legendary install` of a game or a DLC (Legendary's app name), reporting its pid and
/// progress as they come. Returns once the download ends; killing the pid (`platform::kill_tree`) ends it with an error.
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
    stream(command, ProgressAction::Install, on_event)
}

/// Updates an installed game (Legendary's app name) like `install`: same events, same cancel.
pub fn update(app_name: &str, on_event: impl FnMut(InstallEvent)) -> Result<(), Error> {
    stream(legendary(&["-y", "update", app_name, "--skip-sdl"]), ProgressAction::Update, on_event)
}

// Verbs and flags below are from Legendary 0.21.1's `--help` for each command and its `cli.py`.

fn verify_args(app_name: &str) -> Vec<&str> {
    vec!["verify", app_name]
}

/// `repair` is an alias of `install` that turns on repair mode. It reuses the repair file `verify` just
/// wrote, redownloads only the bad and missing files, and doesn't update the game.
fn repair_args(app_name: &str) -> Vec<&str> {
    vec!["-y", "repair", app_name, "--skip-sdl"]
}

/// Moves the game folder into `dest`. Never `--skip-move`, which only rewrites Legendary's record.
fn move_args<'a>(app_name: &'a str, dest: &'a str) -> Vec<&'a str> {
    vec!["move", app_name, dest]
}

/// `--with-dlcs` imports the DLC found in the folder too, without asking.
fn import_args<'a>(app_name: &'a str, path: &'a str) -> Vec<&'a str> {
    vec!["import", "--with-dlcs", app_name, path]
}

/// Takes games installed by the Epic Games Launcher, once: nothing is exported to it and automatic sync stays off.
fn egl_sync_args() -> Vec<&'static str> {
    vec!["-y", "egl-sync", "--import-only", "--one-shot"]
}

/// What `legendary verify` found.
#[derive(Debug, PartialEq, Eq)]
enum Verify {
    Ok,
    /// Some files are bad or missing.
    Repair,
}

/// Verifies an installed game's files, then repairs the bad or missing ones, like `install`: same events,
/// same cancel. Verify reports progress as it checks files, repair as it downloads.
pub fn verify(app_name: &str, mut on_event: impl FnMut(InstallEvent)) -> Result<(), Error> {
    let (success, log) = run_logged(legendary(&verify_args(app_name)), &mut on_event)?;
    match parse_verify(success, &log)? {
        Verify::Ok => Ok(()),
        Verify::Repair => stream(legendary(&repair_args(app_name)), ProgressAction::Repair, on_event),
    }
}

/// Moves an installed game's folder (at `from`, when known) into `dest`, with the same events as `install`.
/// Legendary 0.21 renames the folder, so another drive is refused before it runs.
pub fn move_to(app_name: &str, from: Option<&Path>, dest: &str, on_event: impl FnMut(InstallEvent)) -> Result<(), Error> {
    if from.is_some_and(|from| !same_drive(from, Path::new(dest))) {
        return Err(msg!(otherDriveMove).into());
    }
    let (success, log) = run_logged(legendary(&move_args(app_name, dest)), on_event)?;
    Ok(log_outcome(success, &log, Some("Finished."))?)
}

/// Whether two paths are on the same drive (or network share).
// shortcut: compares path prefixes only; a folder mounted from another disk counts as the same drive.
pub(crate) fn same_drive(a: &Path, b: &Path) -> bool {
    let volume = |path: &Path| match path.components().next()? {
        Component::Prefix(p) => Some(match p.kind() {
            Prefix::Disk(d) | Prefix::VerbatimDisk(d) => d.to_ascii_uppercase().to_string(),
            _ => p.as_os_str().to_string_lossy().to_lowercase(),
        }),
        _ => None,
    };
    volume(a) == volume(b)
}

/// Makes Legendary take an existing install of an owned game at `path`.
pub fn import(app_name: &str, path: &str) -> Result<(), Error> {
    import_requires_existing_folder(Path::new(path))?;
    let (success, log) = run_logged(legendary(&import_args(app_name, path)), |_| {})?;
    Ok(log_outcome(success, &log, Some("has been imported."))?)
}

fn import_requires_existing_folder(path: &Path) -> Result<(), Msg> {
    if path.is_dir() { Ok(()) } else { Err(msg!(notAFolder, path = path.display().to_string())) }
}

/// Imports the games the Epic Games Launcher installed.
pub fn egl_sync() -> Result<(), Error> {
    let (success, log) = run_logged(legendary(&egl_sync_args()), |_| {})?;
    Ok(log_outcome(success, &log, None)?)
}

/// Runs a Legendary command, reporting its pid and `verify`'s progress, and returns whether it exited 0
/// and its log (stderr). stdin is closed, so a prompt ends the command instead of hanging it.
fn run_logged(mut command: Command, mut on_event: impl FnMut(InstallEvent)) -> Result<(bool, String), Error> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    on_event(InstallEvent::Started(child.id()));
    let mut stderr = child.stderr.take().expect("stderr is piped");
    // Read aside, so a full pipe never stalls the process.
    let reader = std::thread::spawn(move || {
        let mut log = Vec::new();
        _ = stderr.read_to_end(&mut log);
        String::from_utf8_lossy(&log).into_owned()
    });
    // Verify rewrites its progress line in place, ending each with `\r`.
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
    let mut chunk = Vec::new();
    while stdout.read_until(b'\r', &mut chunk)? > 0 {
        if let Some(progress) = parse_verify_progress(&String::from_utf8_lossy(&chunk)) {
            on_event(InstallEvent::Progress(progress));
        }
        chunk.clear();
    }
    let status = child.wait()?;
    Ok((status.success(), reader.join().unwrap_or_default()))
}

/// `Verification progress: 12/345 (3.5%) [10.0 MiB/s]` (stdout).
fn parse_verify_progress(line: &str) -> Option<Progress> {
    let (_, rest) = line.split_once("Verification progress: ")?;
    let (files, rest) = rest.split_once(" (")?;
    let (num, total) = files.split_once('/')?;
    let percent: f32 = rest.split('%').next()?.parse().ok()?;
    Some(Progress {
        fraction: (percent / 100.0).clamp(0.0, 1.0),
        action: ProgressAction::Verify,
        done_bytes: None,
        total_bytes: None,
        secs_left: None,
        done_files: Some(num.parse().ok()?),
        total_files: Some(total.parse().ok()?),
    })
}

/// Reads `verify`'s log (`verify_game` in 0.21's `cli.py`), which exits 0 whatever it found.
fn parse_verify(success: bool, log: &str) -> Result<Verify, Msg> {
    if success && log.contains("Verification finished successfully.") {
        Ok(Verify::Ok)
    } else if success && log.contains("Verification failed, ") {
        Ok(Verify::Repair)
    } else {
        Err(log_error(log))
    }
}

/// Legendary 0.21 exits 0 after most failures, so the log decides: it worked when it exited 0 and logged
/// the `done` line, or, without one, logged no error.
fn log_outcome(success: bool, log: &str, done: Option<&str>) -> Result<(), Msg> {
    let finished = match done {
        Some(done) => log.lines().any(|l| l.trim_end().ends_with(done)),
        None => !log.lines().any(is_error_line),
    };
    if success && finished { Ok(()) } else { Err(log_error(log)) }
}

/// `[cli] ERROR: …` or `[cli] CRITICAL: …` (`logger.fatal`).
fn is_error_line(line: &str) -> bool {
    line.contains("] ERROR: ") || line.contains("] CRITICAL: ")
}

/// The last error in a log, else its last line, without the `[cli] ERROR: ` head.
fn log_error(log: &str) -> Msg {
    let lines = || log.lines().rev().map(str::trim).filter(|l| !l.is_empty());
    let Some(line) = lines().find(|l| is_error_line(l)).or_else(|| lines().next()) else {
        return msg!(legendaryNoReason);
    };
    Msg::system(line.split_once(": ").filter(|(head, _)| head.starts_with('[')).map_or(line, |(_, m)| m))
}

fn stream(mut command: Command, action: ProgressAction, mut on_event: impl FnMut(InstallEvent)) -> Result<(), Error> {
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    on_event(InstallEvent::Started(child.id()));
    let mut downloaded_mib = 0.0;
    let mut last_line = String::new();
    for line in BufReader::new(child.stderr.take().expect("stderr is piped")).lines().map_while(Result::ok) {
        if let Some(progress) = parse_progress(&line, action, &mut downloaded_mib) {
            on_event(InstallEvent::Progress(progress));
        }
        if !line.trim().is_empty() {
            last_line = line;
        }
    }
    let status = child.wait()?;
    if status.success() { Ok(()) } else { Err(msg!(legendaryInstallFailed, status = status.to_string(), detail = last_line).into()) }
}

/// Uninstalls a game or a DLC (Legendary's app name).
pub fn uninstall(app_name: &str) -> Result<(), Error> {
    run(legendary(&["-y", "uninstall", app_name]))
}

/// Legendary's DLManager logs, about once a second:
/// `= Progress: 12.34% (123/456), Running for 00:00:10, ETA: 00:01:00`
/// ` - Downloaded: 123.45 MiB, Written: 234.56 MiB`
/// The Downloaded line follows the Progress line, so the progress carries the previous second's total.
fn parse_progress(line: &str, action: ProgressAction, downloaded_mib: &mut f64) -> Option<Progress> {
    if let Some((_, rest)) = line.split_once(" - Downloaded: ") {
        if let Some(mib) = rest.split_whitespace().next().and_then(|n| n.parse().ok()) {
            *downloaded_mib = mib;
        }
        return None;
    }
    let (_, rest) = line.split_once("= Progress: ")?;
    let percent: f32 = rest.split('%').next()?.trim().parse().ok()?;
    let secs_left = rest.split_once("ETA: ").and_then(|(_, eta)| {
        eta.trim().split(':').try_fold(0u64, |secs, part| Some(secs * 60 + part.parse::<u64>().ok()?))
    });
    Some(Progress {
        fraction: (percent / 100.0).clamp(0.0, 1.0),
        action,
        done_bytes: Some((*downloaded_mib * 1048576.0) as u64),
        total_bytes: None,
        secs_left,
        done_files: None,
        total_files: None,
    })
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
        Err(Error::Io(e)) if e.kind() == ErrorKind::NotFound => {
            Login::NoLegendary
        }
        // `status` refreshes the session online and exits 1 when that fails.
        // shortcut: an expired session also lands here; it needs `legendary auth --delete` before "Log in" shows.
        Err(e) => {
            tracing::warn!("legendary status: {e}");
            Login::Unreachable
        }
    }
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

/// Ours first, then the owner's own install (`tools::LEGENDARY`).
fn legendary_exe() -> PathBuf {
    crate::tools::LEGENDARY.exe()
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
fn failure(output: &std::process::Output) -> Msg {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let status = output.status.to_string();
    match stderr.lines().rev().find(|l| !l.trim().is_empty()) {
        Some(last) => msg!(legendaryFailed, status = status, detail = last),
        None => msg!(legendaryFailedSilent, status = status),
    }
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
/// installed, so nothing installed disappears. A DLC (any item whose `mainGameItem` names another
/// item's catalog id) goes under that base game when the base is listed; otherwise the rules above apply to it.
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
    // Some DLC (ARK's maps) are listed as games; naming another listed item as its base is what makes a DLC.
    fn base_of<'a>(g: &'a Value, bases: &HashSet<&str>) -> Option<&'a str> {
        g["metadata"]["mainGameItem"]["id"]
            .as_str()
            .filter(|id| bases.contains(id) && g["metadata"]["id"].as_str() != Some(id))
    }
    // A DLC of a DLC (an add-on for an ARK map) goes under the root game, so the chain never hides itself.
    let parents: HashMap<&str, &str> =
        items.iter().filter_map(|g| Some((g["metadata"]["id"].as_str()?, base_of(g, &bases)?))).collect();
    let mut dlc: HashMap<&str, Vec<Dlc>> = HashMap::new();
    let items: Vec<&Value> = items
        .into_iter()
        .filter(|g| {
            let (Some(name), Some(mut base)) = (g["app_name"].as_str(), base_of(g, &bases)) else {
                return true;
            };
            // Bounded, so a cycle in the catalog can't hang the scan.
            for _ in 0..parents.len() {
                match parents.get(base) {
                    Some(parent) => base = *parent,
                    None => break,
                }
            }
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
            game.cloud_saves = has_cloud_saves(&g["metadata"]);
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
        // Legendary tracks neither: playtime comes from `epic_playtime`, last played from launches here.
        playtime_minutes: None,
        last_played: None,
        size_bytes: installed.and_then(|l| l["install_size"].as_u64()),
        art,
        dlc: Vec::new(),
        update_available: installed.is_some_and(has_update),
        cloud_saves: false,
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
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn opts(args: &str) -> LaunchOptions {
        LaunchOptions { args: args.into(), ..LaunchOptions::default() }
    }

    #[test]
    fn split_args_groups_quotes_and_skips_spaces() {
        assert!(split_args("").is_empty());
        assert!(split_args("   ").is_empty());
        assert_eq!(split_args("  -windowed   -w 1280 "), ["-windowed", "-w", "1280"]);
        assert_eq!(split_args(r#"-name "Big Boss" x"#), ["-name", "Big Boss", "x"]);
        assert_eq!(split_args(r#"--path="C:\My Games\x" %APPDATA% & del"#), [r"--path=C:\My Games\x", "%APPDATA%", "&", "del"]);
        assert_eq!(split_args(r#"a "" b"#), ["a", "", "b"]);
        assert_eq!(split_args(r#""unclosed quote"#), ["unclosed quote"]);
    }

    #[test]
    fn launch_args_from_options() {
        assert_eq!(launch_args("Fn", &LaunchOptions::default(), false), ["launch", "Fn"]);
        assert_eq!(launch_args("Fn", &LaunchOptions::default(), true), ["launch", "Fn", "--skip-version-check"]);
        let all = LaunchOptions {
            args: r#"-windowed "-name=Big Boss""#.into(),
            offline: true,
            skip_version_check: true,
            exe: Some(r"bin\game.exe".into()),
        };
        assert_eq!(
            launch_args("Fn", &all, false),
            ["launch", "Fn", "--offline", "--skip-version-check", "--override-exe", r"bin\game.exe", "-windowed", "-name=Big Boss"]
        );
        assert_eq!(launch_args("Fn", &all, true).iter().filter(|a| *a == "--skip-version-check").count(), 1);
        assert_eq!(launch_args("Fn", &opts("-dx11"), false), ["launch", "Fn", "-dx11"]);
    }

    #[test]
    fn override_exe_must_be_inside_install_dir() {
        let root = std::env::temp_dir().join(format!("launcher-exe-{}", std::process::id()));
        let game = root.join("Game");
        std::fs::create_dir_all(game.join("bin")).unwrap();
        std::fs::write(game.join("bin").join("game.exe"), b"").unwrap();
        std::fs::write(root.join("outside.exe"), b"").unwrap();
        assert_eq!(override_exe(&game, r"bin\game.exe").unwrap(), r"bin\game.exe");
        assert_eq!(override_exe(&game, "bin/game.exe").unwrap(), r"bin\game.exe");
        let absolute = game.join("bin").join("game.exe");
        assert_eq!(override_exe(&game, &absolute.to_string_lossy()).unwrap(), r"bin\game.exe");
        assert!(override_exe(&game, r"..\outside.exe").is_err());
        assert!(override_exe(&game, r"bin\..\..\outside.exe").is_err());
        assert!(override_exe(&game, &root.join("outside.exe").to_string_lossy()).is_err());
        assert!(override_exe(&game, r"bin\missing.exe").is_err());
        assert!(override_exe(&game, "bin").is_err());
        assert!(override_exe(&game, "").is_err());
        assert!(override_exe(&game, r"Z:\elsewhere\game.exe").is_err());
        assert!(override_exe(&root.join("Gone"), "game.exe").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

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
    fn item_naming_its_base_game_is_dlc_whatever_its_category() {
        // Shaped like ARK's maps: listed as games, but `mainGameItem` names ARK's catalog id.
        let list = serde_json::json!([
            { "app_name": "aafc587fbf654758802c8e41e4fb3255", "app_title": "ARK: Survival Evolved",
              "metadata": { "id": "743e47ee84ac49a1a49f4781da70e0d0", "categories": [{ "path": "games" }] } },
            { "app_name": "ArkRagnarok", "app_title": "ARK Ragnarok",
              "metadata": { "id": "ragnarok", "categories": [{ "path": "public" }, { "path": "games" }, { "path": "applications" }],
                            "mainGameItem": { "id": "743e47ee84ac49a1a49f4781da70e0d0" } } }
        ]);
        let all = games(&list, &Value::Null);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].dlc, [Dlc { id: "ArkRagnarok".into(), title: "ARK Ragnarok".into(), installed: false }]);
    }

    #[test]
    fn dlc_of_a_dlc_goes_under_the_root_game() {
        // ARK, a map naming ARK, and an installed add-on naming the map.
        let list = serde_json::json!([
            { "app_name": "Ark", "app_title": "ARK: Survival Evolved",
              "metadata": { "id": "ark", "categories": [{ "path": "games" }] } },
            { "app_name": "ArkRagnarok", "app_title": "ARK Ragnarok",
              "metadata": { "id": "ragnarok", "categories": [{ "path": "games" }], "mainGameItem": { "id": "ark" } } },
            { "app_name": "RagnarokExtra", "app_title": "Ragnarok Extra",
              "metadata": { "id": "extra", "categories": [{ "path": "addons" }], "mainGameItem": { "id": "ragnarok" } } }
        ]);
        let installed = serde_json::json!([{ "app_name": "RagnarokExtra", "title": "Ragnarok Extra", "is_dlc": true }]);
        let all = games(&list, &installed);
        assert_eq!(all.len(), 1);
        assert_eq!(
            all[0].dlc,
            [
                Dlc { id: "ArkRagnarok".into(), title: "ARK Ragnarok".into(), installed: false },
                Dlc { id: "RagnarokExtra".into(), title: "Ragnarok Extra".into(), installed: true },
            ]
        );
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
    fn cloud_save_folder_detected() {
        // Real metadata without a CloudSaveFolder (Last Day of June); the Fortnite fixture has one.
        assert!(!has_cloud_saves(&fixture("list-entry.json")));
        assert!(has_cloud_saves(&fixture("list.json")[0]["metadata"]));
        assert!(!has_cloud_saves(&Value::Null));
        let all = sorted_games();
        assert!(all[0].cloud_saves);
        // Goo has no customAttributes; Imported isn't in the owned list, so there's no metadata at all.
        assert!(!all[1].cloud_saves && !all[2].cloud_saves);
    }

    #[test]
    fn sync_args_before_and_after_launch() {
        assert_eq!(sync_args("Goo", SaveSync::Download), ["sync-saves", "Goo", "--skip-upload"]);
        assert_eq!(sync_args("Goo", SaveSync::Upload), ["sync-saves", "Goo", "--skip-download"]);
        assert_eq!(sync_args("Goo", SaveSync::KeepCloud), ["sync-saves", "Goo", "--skip-upload", "--force-download"]);
        assert_eq!(sync_args("Goo", SaveSync::KeepLocal), ["sync-saves", "Goo", "--skip-download", "--force-upload"]);
        assert_eq!(sync_args("Goo", SaveSync::Both), ["sync-saves", "Goo"]);
        // No `-y`: Legendary 0.21 skips a game whose save folder isn't set yet under it. Empty lines on stdin
        // take each prompt's default (yes) instead, and skip a folder Legendary can't work out.
        assert!(SYNC_ANSWERS.iter().all(|&b| b == b'\n'));
    }

    #[test]
    fn sync_run_gives_up_after_its_timeout() {
        let cmd = |script: &str| {
            let mut command = Command::new("cmd");
            command.args(["/C", script]).creation_flags(CREATE_NO_WINDOW);
            command
        };
        let started = Instant::now();
        let stalled = run_with_timeout(cmd("ping -n 30 127.0.0.1 >nul"), b"\n", Duration::from_millis(300)).unwrap();
        assert_eq!(stalled, None);
        assert!(started.elapsed() < Duration::from_secs(10));
        let done = run_with_timeout(cmd("echo out& echo hi 1>&2"), b"\n", Duration::from_secs(10)).unwrap().unwrap();
        assert!(done.0);
        assert_eq!((done.1.trim(), done.2.trim()), ("out", "hi"));
    }

    #[test]
    fn sync_output_decides_the_outcome() {
        use SyncOutcome::*;
        let log = |lines: &[&str]| lines.iter().map(|l| format!("[cli] INFO: {l}")).collect::<Vec<_>>().join("\n");
        let same = log(&["Got 1 remote save game(s)", r#"Checking "Goo" (Goo)"#, r#"Save game for "Goo" is up to date, skipping..."#]);
        assert_eq!(parse_sync(true, &same), Synced);
        let none = log(&["No cloud or local savegame found."]);
        assert_eq!(parse_sync(true, &none), Synced);
        let downloaded = log(&[
            r#"Cloud save for "Goo" is newer:"#,
            "- Cloud save date: 2026-11-02 10:00:00",
            "- Local save date: 2026-11-01 10:00:00",
            "Downloading remote savegame...",
        ]);
        assert_eq!(parse_sync(true, &downloaded), Synced);
        let uploaded = log(&[
            r#"Local save for "Goo" is newer"#,
            "- Cloud save date: 2026-11-01 10:00:00",
            "- Local save date: 2026-11-02 10:00:00",
            "Uploading local savegame...",
        ]);
        assert_eq!(parse_sync(true, &uploaded), Synced);

        // Before launch (`--skip-upload`): a newer local save is a conflict, unless the cloud has none yet.
        let local_newer = log(&[
            r#"Local save for "Goo" is newer"#,
            "- Cloud save date: 2026-11-01 10:00:00",
            "- Local save date: 2026-11-02 10:00:00",
            "Save game uploading is disabled, skipping...",
        ]);
        assert_eq!(parse_sync(true, &local_newer), LocalNewer);
        let cloud_empty = log(&[
            r#"Local save for "Goo" is newer"#,
            "- Cloud save date: N/A",
            "- Local save date: 2026-11-02 10:00:00",
            "Save game uploading is disabled, skipping...",
        ]);
        assert_eq!(parse_sync(true, &cloud_empty), Synced);

        // After the game (`--skip-download`): a newer cloud save is a conflict, unless there's no local save.
        let cloud_newer = log(&[
            r#"Cloud save for "Goo" is newer:"#,
            "- Cloud save date: 2026-11-02 10:00:00",
            "- Local save date: 2026-11-01 10:00:00",
            "Save game downloading is disabled, skipping...",
        ]);
        assert_eq!(parse_sync(true, &cloud_newer), CloudNewer);
        let local_empty = log(&[
            r#"Cloud save for "Goo" is newer:"#,
            "- Cloud save date: 2026-11-02 10:00:00",
            "- Local save date: N/A",
            "Save game downloading is disabled, skipping...",
        ]);
        assert_eq!(parse_sync(true, &local_empty), Synced);

        // Offline, or another Legendary error: its last message.
        let offline = "[cli] INFO: Logging in...\n[cli] ERROR: Login failed! Cannot continue with download process.\n";
        assert_eq!(parse_sync(false, offline), Failed(Msg::system("Login failed! Cannot continue with download process.")));
        assert_eq!(parse_sync(false, ""), Failed(msg!(legendaryNoReason)));
        // A save folder with variables Legendary can't fill in: the empty answer skips the game.
        let no_folder = log(&[
            r#"Computed save path: "{AppData}/Goo""#,
            "Empty input, skipping...",
        ]);
        assert_eq!(parse_sync(true, &no_folder), Failed(msg!(legendaryNoSaveFolder)));
    }

    #[test]
    fn parses_legendary_progress_lines() {
        let mut mib = 0.0;
        let progress = "[DLManager] INFO: = Progress: 12.34% (123/456), Running for 00:00:10, ETA: 00:01:05";
        let install = ProgressAction::Install;
        let first = parse_progress(progress, install, &mut mib).unwrap();
        assert!((first.fraction - 0.1234).abs() < 1e-6);
        assert_eq!((first.action, first.done_bytes, first.total_bytes, first.secs_left), (install, Some(0), None, Some(65)));

        assert_eq!(parse_progress("[DLManager] INFO:  - Downloaded: 2048.00 MiB, Written: 3000.00 MiB", install, &mut mib), None);
        assert_eq!(mib, 2048.0);
        assert_eq!(parse_progress(progress, install, &mut mib).unwrap().done_bytes, Some(2 << 30));
        assert_eq!(parse_progress(progress, ProgressAction::Repair, &mut mib).unwrap().action, ProgressAction::Repair);

        // Other log lines and broken numbers are ignored.
        assert_eq!(parse_progress("[DLManager] INFO:  + Download	- 10.00 MiB/s (raw)", install, &mut mib), None);
        assert_eq!(parse_progress("= Progress: abc%", install, &mut mib), None);
    }

    #[test]
    fn manage_install_args() {
        assert_eq!(verify_args("Goo"), ["verify", "Goo"]);
        // `repair` is `install` in repair mode: it reuses verify's repair file and doesn't update the game.
        assert_eq!(repair_args("Goo"), ["-y", "repair", "Goo", "--skip-sdl"]);
        // Never `--skip-move`: that only rewrites Legendary's record and leaves the files behind.
        assert_eq!(move_args("Goo", r"D:\Games"), ["move", "Goo", r"D:\Games"]);
        assert_eq!(import_args("Goo", r"D:\Games\Goo"), ["import", "--with-dlcs", "Goo", r"D:\Games\Goo"]);
        // Import only: nothing is exported to the Epic Games Launcher, and automatic sync isn't switched on.
        assert_eq!(egl_sync_args(), ["-y", "egl-sync", "--import-only", "--one-shot"]);
    }

    #[test]
    fn moves_stay_on_the_same_drive() {
        assert!(same_drive(Path::new(r"C:\Games\Goo"), Path::new(r"c:\Other")));
        assert!(same_drive(Path::new(r"C:\Games\Goo"), Path::new(r"\\?\C:\Other")));
        assert!(!same_drive(Path::new(r"C:\Games\Goo"), Path::new(r"D:\Games")));
        assert!(same_drive(Path::new(r"\\nas\games\Goo"), Path::new(r"\\NAS\Games\Other")));
        assert!(!same_drive(Path::new(r"\\nas\games\Goo"), Path::new(r"C:\Games")));
        // Refused before Legendary runs, so no process starts.
        let mut started = false;
        let refused = move_to("Goo", Some(Path::new(r"C:\Games\Goo")), r"D:\Games", |_| started = true);
        assert_eq!(Msg::from(refused.unwrap_err()), msg!(otherDriveMove));
        assert!(!started);
    }

    #[test]
    fn import_requires_existing_folder() {
        let dir = std::env::temp_dir();
        assert!(super::import_requires_existing_folder(&dir).is_ok());
        assert!(super::import_requires_existing_folder(&dir.join("launcher-no-such-folder")).is_err());
        let file = dir.join(format!("launcher-import-{}.txt", std::process::id()));
        std::fs::write(&file, "x").unwrap();
        assert!(super::import_requires_existing_folder(&file).is_err());
        _ = std::fs::remove_file(file);
    }

    #[test]
    fn verify_output_decides_whether_to_repair() {
        // Legendary 0.21's `verify_game` (cli.py) logs these and exits 0 either way.
        let ok = "[cli] INFO: Loading installed manifest for \"Goo\"\n[cli] INFO: Verifying \"Goo\" version \"1\"\n\
                  [cli] INFO: Verification finished successfully.\n";
        assert_eq!(parse_verify(true, ok), Ok(Verify::Ok));
        let broken = "[cli] ERROR: File is missing: \"Goo.exe\"\n\
                      [cli] ERROR: Verification failed, 0 file(s) corrupted, 1 file(s) are missing.\n\
                      [cli] INFO: Run \"legendary repair Goo\" to repair your game installation.\n";
        assert_eq!(parse_verify(true, broken), Ok(Verify::Repair));
        let gone = "[cli] INFO: Loading installed manifest for \"Goo\"\n\
                    [cli] ERROR: Install path \"C:\\Goo\" does not exist, make sure all necessary mounts are available.\n";
        assert_eq!(parse_verify(true, gone), Err(Msg::system("Install path \"C:\\Goo\" does not exist, make sure all necessary mounts are available.")));
        assert_eq!(parse_verify(false, ""), Err(msg!(legendaryNoReason)));
    }

    #[test]
    fn verify_progress_lines() {
        let p = parse_verify_progress("\nVerification progress: 12/345 (3.5%) [10.0 MiB/s]\t\r").unwrap();
        assert!((p.fraction - 0.035).abs() < 1e-6);
        assert_eq!((p.action, p.done_files, p.total_files, p.done_bytes), (ProgressAction::Verify, Some(12), Some(345), None));
        assert_eq!(parse_verify_progress("Checking for importable games..."), None);
    }

    #[test]
    fn log_decides_moves_imports_and_syncs() {
        // `move`, `import` and `egl-sync` exit 0 after their errors in 0.21, so the log decides.
        let moved = "[cli] INFO: Moving \"Goo\" from \"C:/Games\" to \"D:\\Games\"\n[cli] INFO: Finished.\n";
        assert_eq!(log_outcome(true, moved, Some("Finished.")), Ok(()));
        let other_drive = "[cli] INFO: Moving \"Goo\" from \"C:/Games\" to \"D:\\Games\"\n\
                           [cli] ERROR: Moving to a different drive is not supported.\n";
        assert_eq!(log_outcome(true, other_drive, Some("Finished.")), Err(Msg::system("Moving to a different drive is not supported.")));
        // A DLC that fails to import doesn't undo the game's import.
        let imported = "[cli] ERROR: No files belonging to DLC \"Hat\" (Hat) found in the specified location.\n\
                        [cli] INFO: Game \"Goo\" has been imported.\n";
        assert_eq!(log_outcome(true, imported, Some("has been imported.")), Ok(()));
        let wrong = "[cli] CRITICAL: Did not find game \"Goo\" on account.\n";
        assert_eq!(log_outcome(true, wrong, Some("has been imported.")), Err(Msg::system("Did not find game \"Goo\" on account.")));
        // egl-sync has no closing line: any error fails it.
        assert_eq!(log_outcome(true, "[cli] INFO: Reading EGL game manifests...\n", None), Ok(()));
        let no_meta = "[cli] ERROR: Legendary is missing game metadata, please login.\n";
        assert_eq!(log_outcome(true, no_meta, None), Err(Msg::system("Legendary is missing game metadata, please login.")));
        // A prompt that read the closed stdin crashes with a traceback.
        assert_eq!(log_outcome(false, "Traceback...\nEOFError: EOF when reading a line\n", None), Err(Msg::system("EOFError: EOF when reading a line")));
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
        assert_eq!(failure(&output), msg!(legendaryFailed, status = "exit code: 1", detail = "[cli] ERROR: Login failed, cannot continue!"));
    }
}
