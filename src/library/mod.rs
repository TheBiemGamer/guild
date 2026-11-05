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
    /// Unique across providers; safe as a file name.
    pub fn key(&self) -> String {
        format!("{}-{}", self.provider.as_str(), self.external_id)
    }
}

pub trait Provider {
    fn list_games(&self) -> Result<Vec<Game>, Error>;
}
