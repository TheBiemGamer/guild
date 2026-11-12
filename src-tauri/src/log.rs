//! The log file, `logs/launcher.log` in the app's local data folder. Release builds have no console, so
//! `tracing` events from this crate go here (and to stderr in debug builds), one per line:
//! `2026-11-07 14:03:12  WARN steam library: …` in local time. Never log tokens, secrets, passwords or codes.
//! At 1 MB it rolls to `launcher.1.log`; `launcher.4.log` is the oldest kept.

use fs_err::{self as fs, File, OpenOptions};
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use jiff::{Timestamp, tz::TimeZone};
use tracing::Level;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::{format::Writer, time::FormatTime};
use tracing_subscriber::prelude::*;

const MAX_BYTES: u64 = 1024 * 1024;
/// launcher.log plus launcher.1.log … launcher.4.log.
const KEEP: usize = 5;

struct Log {
    dir: PathBuf,
    file: File,
    len: u64,
}

impl Log {
    /// Opens launcher.log in `dir` for appending, rolling it first when it is already over the limit.
    fn open(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let mut log = Self::open_file(dir)?;
        if log.len > MAX_BYTES {
            log.roll()?;
        }
        Ok(log)
    }

    fn open_file(dir: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("launcher.log"))?;
        let len = file.metadata()?.len();
        Ok(Self {
            dir: dir.to_path_buf(),
            file,
            len,
        })
    }

    fn append(&mut self, line: &[u8]) -> io::Result<()> {
        self.file.write_all(line)?;
        self.len += line.len() as u64;
        if self.len > MAX_BYTES {
            self.roll()?;
        }
        Ok(())
    }

    fn roll(&mut self) -> io::Result<()> {
        rotate(&self.dir);
        *self = Self::open_file(&self.dir)?;
        Ok(())
    }
}

/// launcher.N.log, or launcher.log for 0.
fn name(dir: &Path, n: usize) -> PathBuf {
    dir.join(if n == 0 {
        "launcher.log".into()
    } else {
        format!("launcher.{n}.log")
    })
}

/// Shifts every file up one number and drops the oldest. Missing files are skipped.
fn rotate(dir: &Path) {
    let _ = fs::remove_file(name(dir, KEEP - 1));
    for n in (0..KEEP - 1).rev() {
        let _ = fs::rename(name(dir, n), name(dir, n + 1));
    }
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);

/// Writes each formatted event, whole, to the open file; nothing before `init` opens it or when it couldn't.
struct LogFile;

impl Write for LogFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(log) = LOG.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            // A full disk or a lost file isn't worth failing over.
            let _ = log.append(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct LocalTime;

impl FormatTime for LocalTime {
    fn format_time(&self, w: &mut Writer<'_>) -> fmt::Result {
        write!(w, "{}", stamp(Timestamp::now(), TimeZone::system()))
    }
}

/// `YYYY-MM-DD HH:MM:SS` on the clock in `tz`.
fn stamp(at: Timestamp, tz: TimeZone) -> String {
    at.to_zoned(tz).strftime("%Y-%m-%d %H:%M:%S").to_string()
}

/// Where a Windows game's output (umu-run, pressure-vessel, Proton) goes: `logs/game-<copy key>.log`, rewritten at
/// each launch, so a launch that fails can be explained in launcher.log.
pub fn game_output(key: &str) -> PathBuf {
    crate::cache::Dirs::new()
        .logs
        .join(format!("game-{key}.log"))
}

/// The last `lines` lines of a copy's game output, each indented; empty when it has none.
pub fn game_output_tail(key: &str, lines: usize) -> String {
    tail(&game_output(key), lines)
}

fn tail(path: &Path, lines: usize) -> String {
    let text = std::fs::read(path).unwrap_or_default();
    let text = String::from_utf8_lossy(&text);
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|l| format!("\n    {l}"))
        .collect()
}

/// Sends this crate's info and above to the log file in `dir` (and stderr in debug builds). The file is
/// written unbuffered, so nothing waits to be flushed when the app exits.
pub fn init(dir: &Path) {
    #[cfg(debug_assertions)]
    let writer = (|| LogFile).and(io::stderr);
    #[cfg(not(debug_assertions))]
    let writer = || LogFile;
    let layer = tracing_subscriber::fmt::layer()
        .with_timer(LocalTime)
        .with_target(false)
        .with_writer(writer);
    let _ = tracing_subscriber::registry()
        .with(layer.with_filter(Targets::new().with_target("launcher", Level::INFO)))
        .try_init();
    match Log::open(dir) {
        Ok(log) => *LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(log),
        Err(e) => tracing::error!("log file {}: {e}", dir.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(dir: &Path, n: usize) -> Option<String> {
        fs::read_to_string(name(dir, n)).ok()
    }

    #[test]
    fn rotate_shifts_files_and_drops_the_oldest() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        for n in 0..KEEP {
            fs::write(name(dir, n), n.to_string()).unwrap();
        }
        rotate(dir);
        assert_eq!(read(dir, 0), None);
        for n in 1..KEEP {
            assert_eq!(read(dir, n), Some((n - 1).to_string()));
        }
        assert!(!name(dir, KEEP).exists());
    }

    #[test]
    fn rolls_an_oversized_file_at_open_and_on_write() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        File::create(name(dir, 0))
            .unwrap()
            .set_len(MAX_BYTES + 1)
            .unwrap();
        let mut log = Log::open(dir).unwrap();
        assert_eq!(log.len, 0);
        assert_eq!(fs::metadata(name(dir, 1)).unwrap().len(), MAX_BYTES + 1);

        log.append(b"first\n").unwrap();
        log.len = MAX_BYTES;
        log.append(b"last\n").unwrap();
        assert_eq!(log.len, 0);
        assert_eq!(read(dir, 1).unwrap(), "first\nlast\n");
        assert_eq!(fs::metadata(name(dir, 2)).unwrap().len(), MAX_BYTES + 1);
        log.append(b"next\n").unwrap();
        assert_eq!(read(dir, 0).unwrap(), "next\n");
        drop(log);
    }

    #[test]
    fn timestamps_are_local_clock_time() {
        let tz = TimeZone::get("Europe/Amsterdam").unwrap();
        // 2024-02-29 23:05:09 UTC, the last hour of a leap day: already March 1st in winter time (+1).
        assert_eq!(
            stamp(Timestamp::from_second(1_709_247_909).unwrap(), tz.clone()),
            "2024-03-01 00:05:09"
        );
        // 2024-07-01 12:00:00 UTC in summer time (+2).
        assert_eq!(
            stamp(Timestamp::from_second(1_719_835_200).unwrap(), tz),
            "2024-07-01 14:00:00"
        );
    }

    #[test]
    fn a_game_output_tail_is_its_last_lines_indented() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.log");
        assert_eq!(tail(&path, 2), "");
        fs::write(&path, "a\nb\nc\n").unwrap();
        assert_eq!(tail(&path, 2), "\n    b\n    c");
    }
}
