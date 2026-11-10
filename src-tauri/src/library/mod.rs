//! Normalized game model shared by all stores.

pub mod amazon;
pub mod custom;
pub mod epic;
pub mod gog;
pub mod steam;
pub mod tool;

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// What store, tool, database and file work can fail with. `Msg::from` is the one place it becomes text for the
/// owner: a `Msg` stays as it is, everything else is `system` with its own words as the detail.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Msg(#[from] crate::msg::Msg),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    SteamFiles(#[from] steamlocate::Error),
    /// A tool's or service's own words.
    #[error("{0}")]
    Text(String),
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Self::Text(s.into())
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, specta::Type,
)]
#[serde(rename = "Store")] // its TypeScript name
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Steam,
    Epic,
    Gog,
    Amazon,
    Ea,
    /// A program the owner added (`custom_games`); never merged with a store copy.
    Custom,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 6] = [
        ProviderKind::Steam,
        ProviderKind::Epic,
        ProviderKind::Gog,
        ProviderKind::Amazon,
        ProviderKind::Ea,
        ProviderKind::Custom,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Epic => "epic",
            Self::Gog => "gog",
            Self::Amazon => "amazon",
            Self::Ea => "ea",
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
            Self::Amazon => "Amazon Games",
            Self::Ea => "EA",
            Self::Custom => "added games",
        }
    }

    /// Unique across providers; safe as a file name.
    pub fn key(self, external_id: &str) -> String {
        format!("{}-{external_id}", self.as_str())
    }
}

/// What a job in progress is doing. Steam's are all `Download`; Steam doesn't say which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
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
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// 0.0 to 1.0.
    // A plain `number` in TypeScript: clamped, never NaN, so never `null` in JSON.
    #[specta(type = u32)]
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

/// Seconds since the Unix epoch, as the database stores times; 0 if the clock is before 1970.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Percent-encodes every byte except RFC 3986 unreserved characters: for a URL query value or an
/// `x-www-form-urlencoded` body.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encodes_all_but_unreserved() {
        assert_eq!(percent_encode("ab-1_.~"), "ab-1_.~");
        assert_eq!(percent_encode("a&b=c d"), "a%26b%3Dc%20d");
        assert_eq!(percent_encode("®"), "%C2%AE");
    }
}
