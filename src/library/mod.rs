//! Normalized game model shared by all stores.

pub mod epic;
pub mod steam;

use std::path::PathBuf;
use std::time::SystemTime;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Steam,
    Epic,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 2] = [ProviderKind::Steam, ProviderKind::Epic];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Epic => "epic",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == s)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::Epic => "Epic Games",
        }
    }

    pub fn provider(self) -> &'static dyn Provider {
        match self {
            Self::Steam => &steam::Steam,
            Self::Epic => &epic::Epic,
        }
    }

    /// Unique across providers; safe as a file name.
    pub fn key(self, external_id: &str) -> String {
        format!("{}-{external_id}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
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
    pub logo: Image,
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
    pub art: Art,
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
