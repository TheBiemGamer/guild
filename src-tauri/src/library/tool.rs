//! What the store tools (Legendary, gogdl, Nile) share: their job events, launch options and save syncs, and how
//! their processes are run and their logs read.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::Progress;
use crate::msg::Msg;

/// An Epic, GOG or Amazon login as its tool (Legendary, gogdl, Nile) reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum StoreLogin {
    /// The tool's exe is in none of the places `tools::Tool::exe` looks.
    NoTool,
    LoggedOut,
    LoggedIn,
    /// The tool couldn't confirm the login, e.g. offline: keep the last known games.
    Unreachable,
}

/// What a running install reports back.
#[derive(Debug, Clone)]
pub enum InstallEvent {
    /// Legendary's process id, so the install can be cancelled.
    Started(u32),
    Progress(Progress),
}

/// A copy's own launch options, kept in `game_prefs` as JSON. `exe` is relative to the install folder.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
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

/// Runs `command` with `input` on stdin (then closed, so a prompt beyond it ends the process instead of
/// hanging) and returns its output. `None` when it outlives `timeout`: its process tree is killed (legendary.exe,
/// gogdl.exe and nile.exe unpack themselves into a child process).
pub fn run_with_timeout(
    mut command: Command,
    input: &[u8],
    timeout: Duration,
) -> std::io::Result<Option<Output>> {
    let mut child = crate::platform::spawn_tool(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )?;
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(input)?;
    // Read aside, so a full pipe never stalls the process while this waits.
    let read = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    };
    let out = read(Box::new(child.stdout.take().expect("stdout is piped")));
    let err = read(Box::new(child.stderr.take().expect("stderr is piped")));
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(Output {
                status,
                stdout: out.join().unwrap_or_default(),
                stderr: err.join().unwrap_or_default(),
            }));
        }
        if Instant::now() >= deadline {
            if let Err(e) = crate::platform::kill_tree(child.id()) {
                tracing::warn!("stop timed-out tool: {e}");
                _ = child.kill();
            }
            _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Whether two paths are on the same drive (or network share).
// shortcut: compares path prefixes only; a folder mounted from another disk counts as the same drive.
#[cfg(windows)]
pub fn same_drive(a: &Path, b: &Path) -> bool {
    use std::path::{Component, Prefix};
    let volume = |path: &Path| match path.components().next()? {
        Component::Prefix(p) => Some(match p.kind() {
            Prefix::Disk(d) | Prefix::VerbatimDisk(d) => d.to_ascii_uppercase().to_string(),
            _ => p.as_os_str().to_string_lossy().to_lowercase(),
        }),
        _ => None,
    };
    volume(a) == volume(b)
}

/// Whether two paths are on the same filesystem, by the device of each one's nearest existing folder.
#[cfg(unix)]
pub fn same_drive(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let device = |path: &Path| {
        path.ancestors()
            .find_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.dev())
    };
    device(a) == device(b)
}

/// `[cli] ERROR: …` or `[cli] CRITICAL: …` (`logger.fatal`).
pub fn is_error_line(line: &str) -> bool {
    line.contains("] ERROR: ") || line.contains("] CRITICAL: ")
}

/// The last error in a Legendary or gogdl log, else its last line, without its `[NAME] LEVEL: ` head; `empty` when
/// the log is empty.
pub fn last_error(log: &str, empty: Msg) -> Msg {
    let lines = || log.lines().rev().map(str::trim).filter(|l| !l.is_empty());
    let Some(line) = lines()
        .find(|l| is_error_line(l))
        .or_else(|| lines().next())
    else {
        return empty;
    };
    Msg::system(
        line.split_once(": ")
            .filter(|(head, _)| head.starts_with('['))
            .map_or(line, |(_, m)| m),
    )
}

/// A JSON file, with or without a byte order mark.
pub fn read_json(path: &Path) -> Option<Value> {
    let bytes = fs_err::read(path).ok()?;
    serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_args_groups_quotes_and_skips_spaces() {
        assert!(split_args("").is_empty());
        assert!(split_args("   ").is_empty());
        assert_eq!(
            split_args("  -windowed   -w 1280 "),
            ["-windowed", "-w", "1280"]
        );
        assert_eq!(
            split_args(r#"-name "Big Boss" x"#),
            ["-name", "Big Boss", "x"]
        );
        assert_eq!(
            split_args(r#"--path="C:\My Games\x" %APPDATA% & del"#),
            [r"--path=C:\My Games\x", "%APPDATA%", "&", "del"]
        );
        assert_eq!(split_args(r#"a "" b"#), ["a", "", "b"]);
        assert_eq!(split_args(r#""unclosed quote"#), ["unclosed quote"]);
    }

    #[test]
    fn sync_run_gives_up_after_its_timeout() {
        let cmd = |script: &str| {
            #[cfg(windows)]
            let (shell, flag) = ("cmd", "/C");
            #[cfg(unix)]
            let (shell, flag) = ("sh", "-c");
            let mut command = Command::new(shell);
            crate::platform::no_window(command.args([flag, script]));
            command
        };
        let started = Instant::now();
        let stalled = run_with_timeout(
            cmd(if cfg!(windows) {
                "ping -n 30 127.0.0.1 >nul"
            } else {
                "sleep 30"
            }),
            b"\n",
            Duration::from_millis(300),
        )
        .unwrap();
        assert_eq!(stalled, None);
        assert!(started.elapsed() < Duration::from_secs(10));
        let done = run_with_timeout(
            cmd(if cfg!(windows) {
                "echo out& echo hi 1>&2"
            } else {
                "echo out; echo hi 1>&2"
            }),
            b"\n",
            Duration::from_secs(10),
        )
        .unwrap()
        .unwrap();
        assert!(done.status.success());
        let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
        assert_eq!(
            (text(&done.stdout), text(&done.stderr)),
            ("out".into(), "hi".into())
        );
    }

    #[cfg(unix)]
    #[test]
    fn same_drive_compares_filesystems() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(same_drive(tmp.path(), &tmp.path().join("not/made/yet")));
        assert!(!same_drive(tmp.path(), Path::new("/proc")));
    }

    #[cfg(windows)]
    #[test]
    fn same_drive_compares_volumes() {
        assert!(same_drive(
            Path::new(r"C:\Games\Goo"),
            Path::new(r"c:\Other")
        ));
        assert!(same_drive(
            Path::new(r"C:\Games\Goo"),
            Path::new(r"\\?\C:\Other")
        ));
        assert!(!same_drive(
            Path::new(r"C:\Games\Goo"),
            Path::new(r"D:\Games")
        ));
        assert!(same_drive(
            Path::new(r"\\nas\games\Goo"),
            Path::new(r"\\NAS\Games\Other")
        ));
        assert!(!same_drive(
            Path::new(r"\\nas\games\Goo"),
            Path::new(r"C:\Games")
        ));
    }
}
