//! Normalized game model shared by all stores.

pub mod custom;
pub mod epic;
pub mod gog;
pub mod steam;

use std::path::PathBuf;
use std::time::SystemTime;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Steam,
    Epic,
    Gog,
    /// A program the owner added (`custom_games`); never merged with a store copy.
    Custom,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 4] = [ProviderKind::Steam, ProviderKind::Epic, ProviderKind::Gog, ProviderKind::Custom];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Epic => "epic",
            Self::Gog => "gog",
            Self::Custom => "custom",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == s)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::Epic => "Epic Games",
            Self::Gog => "GOG",
            Self::Custom => "added games",
        }
    }

    pub fn provider(self) -> &'static dyn Provider {
        match self {
            Self::Steam => &steam::Steam,
            Self::Epic => &epic::Epic,
            Self::Gog => &gog::Gog,
            Self::Custom => &custom::Custom,
        }
    }

    /// Unique across providers; safe as a file name.
    pub fn key(self, external_id: &str) -> String {
        format!("{}-{external_id}", self.as_str())
    }
}

/// What a job in progress is doing. Steam's are all `Download`; Steam doesn't say which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProgressAction {
    Download,
    Install,
    Update,
    Repair,
    Verify,
}

/// Download progress as each store reports it: Steam in bytes, Legendary as a percentage with an ETA.
/// The UI formats it (`src/lib/format.ts`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// 0.0 to 1.0.
    pub fraction: f32,
    pub action: ProgressAction,
    /// Downloaded so far; `None` for Verify.
    pub done_bytes: Option<u64>,
    /// The whole download; only Steam knows it.
    pub total_bytes: Option<u64>,
    /// `None` until it's known.
    pub secs_left: Option<u64>,
    /// Verify: files checked, and how many there are.
    pub done_files: Option<u64>,
    pub total_files: Option<u64>,
}

/// One art slot: a local file, a URL to download into our art cache, or both (local wins).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Image {
    pub path: Option<PathBuf>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Art {
    /// Portrait 2:3.
    pub cover: Image,
    /// Landscape; the grid's fallback when there is no cover.
    pub header: Image,
    pub hero: Image,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Game {
    pub provider: ProviderKind,
    pub external_id: String,
    pub title: String,
    pub installed: bool,
    pub install_dir: Option<PathBuf>,
    pub playtime_minutes: Option<u64>,
    pub last_played: Option<SystemTime>,
    /// Bytes on disk for an installed game; `None` until measured.
    pub size_bytes: Option<u64>,
    pub art: Art,
    /// Owned DLC whose base game this is. Only Epic fills it; Steam's comes from the `dlc` command.
    pub dlc: Vec<Dlc>,
    /// An installed Epic game has a newer version. Steam updates itself, so it is always false there.
    pub update_available: bool,
    /// An Epic game keeps its saves in Epic's cloud (its catalog item names a `CloudSaveFolder`).
    pub cloud_saves: bool,
}

/// A DLC under its base game. `id` is the store's id for it (Legendary's app name).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Dlc {
    pub id: String,
    pub title: String,
    pub installed: bool,
}

impl Game {
    pub fn key(&self) -> String {
        self.provider.key(&self.external_id)
    }
}

/// Every action hands off to the store client and returns once the request is made.
pub trait Provider: Sync {
    fn list_games(&self) -> Result<Vec<Game>, Error>;
    fn launch(&self, game: &Game) -> Result<(), Error>;
    fn install(&self, game: &Game) -> Result<(), Error>;
    fn uninstall(&self, game: &Game) -> Result<(), Error>;
    /// `external_id` of the game this store reports as running, if any.
    fn running_game(&self) -> Option<String>;
    /// `None` when no download is in progress for `game`.
    fn download_progress(&self, game: &Game) -> Option<Progress>;
}

/// Opens a store URI (`steam://…`, `com.epicgames.launcher://…`) with its registered handler.
/// explorer.exe hands it to the shell without cmd.exe parsing, so `&` in the URI is safe.
// shortcut: explorer never reports a missing handler (Windows shows its own dialog); use ShellExecuteW if we need the error in-app.
pub fn open_uri(uri: &str) -> Result<(), Error> {
    std::process::Command::new("explorer.exe").arg(uri).spawn()?;
    Ok(())
}

/// Opens an `https://` URL in the default browser. explorer.exe would open the Documents folder
/// for anything but a registered protocol; rundll32 hands the URL straight to the shell, no cmd.exe,
/// so `&` and `?` are safe.
pub fn open_url(url: &str) -> Result<(), Error> {
    use std::os::windows::process::CommandExt;
    if !url.starts_with("https://") {
        return Err(format!("not an https URL: {url}").into());
    }
    std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", url])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(())
}

/// Percent-encodes every byte except RFC 3986 unreserved characters: for a URL query value or an
/// `x-www-form-urlencoded` body.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Whether `tasklist /FI <filter>` (e.g. `PID eq 42`) lists a process named `image`.
// shortcut: starts a tasklist process; callers only ask while a game is believed to be running.
pub fn process_listed(filter: &str, image: &str) -> bool {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("tasklist")
        .args(["/FI", filter, "/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    output.is_ok_and(|o| lists_image(&String::from_utf8_lossy(&o.stdout), image))
}

/// tasklist CSV rows look like `"steam.exe","16176","Console","1","131.968 K"`.
fn lists_image(tasklist_csv: &str, image: &str) -> bool {
    let wanted = format!("\"{}\"", image.to_ascii_lowercase());
    tasklist_csv.lines().any(|row| row.to_ascii_lowercase().starts_with(&wanted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_url_refuses_non_https() {
        for url in ["http://example.com", "steam://open/main", "file:///C:/Windows", "notepad.exe", ""] {
            assert!(open_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn lists_image_matches_whole_name_ignoring_case() {
        let row = "\"Steam.exe\",\"16176\",\"Console\",\"1\",\"131.968 K\"\r\n";
        assert!(lists_image(row, "steam.exe"));
        // No match: tasklist prints an INFO line instead of a row.
        assert!(!lists_image("INFO: No tasks are running which match the specified criteria.\r\n", "steam.exe"));
        // Pid reused by another program, or a name that only starts the same.
        assert!(!lists_image("\"notepad.exe\",\"16176\",\"Console\",\"1\",\"9 K\"", "steam.exe"));
        assert!(!lists_image("\"steamwebhelper.exe\",\"16176\",\"Console\",\"1\",\"9 K\"", "steam.exe"));
    }

    #[test]
    fn percent_encodes_all_but_unreserved() {
        assert_eq!(percent_encode("ab-1_.~"), "ab-1_.~");
        assert_eq!(percent_encode("a&b=c d"), "a%26b%3Dc%20d");
        assert_eq!(percent_encode("®"), "%C2%AE");
    }
}
