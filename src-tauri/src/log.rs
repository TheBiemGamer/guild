//! The log file, `logs/launcher.log` in the app's local data folder. Release builds have no console,
//! so diagnostics go here (and to stderr in debug builds). Never log tokens, secrets, passwords or codes.
//! At 1 MB it rolls to `launcher.1.log`; `launcher.4.log` is the oldest kept.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 1024 * 1024;
/// launcher.log plus launcher.1.log … launcher.4.log.
const KEEP: usize = 5;

/// Formats and writes one line: `log!("steam library: {e}")`.
macro_rules! log {
    ($($arg:tt)*) => { $crate::log::write(&format!($($arg)*)) };
}

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
        let file = OpenOptions::new().create(true).append(true).open(dir.join("launcher.log"))?;
        let len = file.metadata()?.len();
        Ok(Self { dir: dir.to_path_buf(), file, len })
    }

    fn append(&mut self, line: &str) -> io::Result<()> {
        self.file.write_all(line.as_bytes())?;
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
    dir.join(if n == 0 { "launcher.log".into() } else { format!("launcher.{n}.log") })
}

/// Shifts every file up one number and drops the oldest. Missing files are skipped.
fn rotate(dir: &Path) {
    let _ = fs::remove_file(name(dir, KEEP - 1));
    for n in (0..KEEP - 1).rev() {
        let _ = fs::rename(name(dir, n), name(dir, n + 1));
    }
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);

/// Starts writing to `dir`; until then (and if it can't open) lines only reach stderr in debug builds.
pub fn init(dir: &Path) {
    match Log::open(dir) {
        Ok(log) => *LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(log),
        Err(e) => eprintln!("log file {}: {e}", dir.display()),
    }
}

pub fn write(msg: &str) {
    #[cfg(debug_assertions)]
    eprintln!("{msg}");
    let mut log = LOG.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(log) = log.as_mut() {
        // A full disk or a lost file isn't worth failing over.
        let _ = log.append(&format!("{} {msg}\n", timestamp(SystemTime::now())));
    }
}

/// `YYYY-MM-DD HH:MM:SSZ` in UTC (local time needs the OS's time zone rules).
fn timestamp(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's civil-from-days.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("launcher-log-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read(dir: &Path, n: usize) -> Option<String> {
        fs::read_to_string(name(dir, n)).ok()
    }

    #[test]
    fn rotate_shifts_files_and_drops_the_oldest() {
        let dir = temp_dir("rotate");
        for n in 0..KEEP {
            fs::write(name(&dir, n), n.to_string()).unwrap();
        }
        rotate(&dir);
        assert_eq!(read(&dir, 0), None);
        for n in 1..KEEP {
            assert_eq!(read(&dir, n), Some((n - 1).to_string()));
        }
        assert!(!name(&dir, KEEP).exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rolls_an_oversized_file_at_open_and_on_write() {
        let dir = temp_dir("roll");
        File::create(name(&dir, 0)).unwrap().set_len(MAX_BYTES + 1).unwrap();
        let mut log = Log::open(&dir).unwrap();
        assert_eq!(log.len, 0);
        assert_eq!(fs::metadata(name(&dir, 1)).unwrap().len(), MAX_BYTES + 1);

        log.append("first\n").unwrap();
        log.len = MAX_BYTES;
        log.append("last\n").unwrap();
        assert_eq!(log.len, 0);
        assert_eq!(read(&dir, 1).unwrap(), "first\nlast\n");
        assert_eq!(fs::metadata(name(&dir, 2)).unwrap().len(), MAX_BYTES + 1);
        log.append("next\n").unwrap();
        assert_eq!(read(&dir, 0).unwrap(), "next\n");
        drop(log);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn timestamps_are_utc_calendar_time() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01 00:00:00Z");
        // 2024-02-29 13:05:09 UTC, a leap day.
        assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(1_709_211_909)), "2024-02-29 13:05:09Z");
    }
}
