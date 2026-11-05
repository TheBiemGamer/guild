//! Normalized game model shared by all stores.

pub mod steam;

use std::path::PathBuf;
use std::time::SystemTime;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Steam,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Steam => "steam",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "steam" => Some(Self::Steam),
            _ => None,
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

/// Local art files. Any of them may be missing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Art {
    pub cover: Option<PathBuf>,
    pub header: Option<PathBuf>,
    pub hero: Option<PathBuf>,
    pub logo: Option<PathBuf>,
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
pub trait Provider {
    fn list_games(&self) -> Result<Vec<Game>, Error>;
    fn launch(&self, game: &Game) -> Result<(), Error>;
    fn install(&self, game: &Game) -> Result<(), Error>;
    fn uninstall(&self, game: &Game) -> Result<(), Error>;
    /// `external_id` of the game this store reports as running, if any.
    fn running_game(&self) -> Option<String>;
    /// `None` when no download is in progress for `game`.
    fn download_progress(&self, game: &Game) -> Option<Progress>;
}

/// Opens a store URI (`steam://…`) with its registered handler.
/// explorer.exe hands it to the shell without cmd.exe parsing, so `&` in Epic URIs is safe.
// shortcut: explorer never reports a missing handler (Windows shows its own dialog); use ShellExecuteW if we need the error in-app.
pub fn open_uri(uri: &str) -> Result<(), Error> {
    std::process::Command::new("explorer.exe").arg(uri).spawn()?;
    Ok(())
}
