//! Text for the owner, as a message code plus parameters; the UI's language files hold the words (`msg.<code>` in
//! `src/lib/i18n/en.json`).
//! Never put an English sentence in an error or status the UI shows: make a code with `msg!`.
//! Raw errors from the OS, SQLite or a tool have no fixed text and go through as `system` with a `detail`.

use std::collections::BTreeMap;
use std::fmt;

/// `{ "code": "gameNotInLibrary", "params": { … } }`. A `Msg` param is rendered first and put in place.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Msg {
    pub code: String,
    #[serde(default)]
    pub params: BTreeMap<String, Param>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(untagged)]
pub enum Param {
    Msg(Msg),
    /// Shown as is: a title, a path, a tool's own words, or text stored before message codes.
    Text(String),
}

/// `msg!(gameNotInLibrary)`, `msg!(storeError, store = "GOG", reason = e)`.
macro_rules! msg {
    ($code:ident $(, $k:ident = $v:expr)* $(,)?) => {
        $crate::msg::Msg::new(stringify!($code))$(.with(stringify!($k), $v))*
    };
}

impl Msg {
    pub fn new(code: &str) -> Self {
        Self { code: code.into(), params: BTreeMap::new() }
    }

    pub fn with(mut self, key: &str, value: impl Into<Param>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }

    /// An error with no fixed text (OS, SQLite, IO, a tool's output), shown untranslated.
    pub fn system(detail: impl fmt::Display) -> Self {
        Self::new("system").with("detail", detail.to_string())
    }
}

impl From<String> for Param {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for Param {
    fn from(s: &str) -> Self {
        Self::Text(s.into())
    }
}

impl From<Msg> for Param {
    fn from(m: Msg) -> Self {
        Self::Msg(m)
    }
}

/// A store error keeps its `Msg` if it is one; anything else is `system`.
impl From<crate::library::Error> for Msg {
    fn from(e: crate::library::Error) -> Self {
        match e {
            crate::library::Error::Msg(m) => m,
            e => Self::system(e),
        }
    }
}

/// For logs only: the code and its params, or a `system` detail as is.
impl fmt::Display for Msg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let (true, Some(detail)) = (self.code == "system", self.params.get("detail")) {
            return write!(f, "{detail}");
        }
        write!(f, "{}", self.code)?;
        for (k, v) in &self.params {
            write!(f, " {k}={v}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Param {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Msg(m) => write!(f, "({m})"),
            Self::Text(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for Msg {}

/// Text shown outside the webview: window titles, the tray, toasts. Same codes and words as `msg.<code>` in the
/// UI's language files (a test checks); a code missing from a language falls back to English.
const EN: &[(&str, &str)] = &[
    ("appName", "Game Launcher"),
    ("trayOpen", "Open"),
    ("trayDownloads", "Downloads"),
    ("trayQuit", "Quit"),
    ("logInEpicTitle", "Log in to Epic Games"),
    ("logInGogTitle", "Log in to GOG"),
    ("logInAmazonTitle", "Log in to Amazon"),
    ("readyToPlay", "Ready to play"),
    ("updated", "Updated"),
    ("repaired", "Repaired"),
    ("filesVerified", "Files verified"),
];

/// Dutch, as in `src/lib/i18n/nl.json`.
const NL: &[(&str, &str)] = &[
    ("appName", "Game Launcher"),
    ("trayOpen", "Openen"),
    ("trayDownloads", "Downloads"),
    ("trayQuit", "Afsluiten"),
    ("logInEpicTitle", "Inloggen bij Epic Games"),
    ("logInGogTitle", "Inloggen bij GOG"),
    ("logInAmazonTitle", "Inloggen bij Amazon"),
    ("readyToPlay", "Klaar om te spelen"),
    ("updated", "Bijgewerkt"),
    ("repaired", "Hersteld"),
    ("filesVerified", "Bestanden gecontroleerd"),
];

/// The languages with words, by code; the Language setting is one of these or "system".
const LANGUAGES: &[(&str, &[(&str, &str)])] = &[("en", EN), ("nl", NL)];

/// The language native text is in, set from the Language setting at start and on every change.
static LANGUAGE: std::sync::RwLock<&str> = std::sync::RwLock::new("en");

/// Whether the Language setting may be `value`.
pub fn is_language(value: &str) -> bool {
    value == "system" || LANGUAGES.iter().any(|(code, _)| *code == value)
}

/// The language for the Language setting: the one it names, or for "system" Windows' display language
/// (`system`, like "nl") when there are words for it, else English.
pub fn resolve(setting: &str, system: &str) -> &'static str {
    let wanted = if setting == "system" { system } else { setting };
    LANGUAGES.iter().find(|(code, _)| *code == wanted).map_or("en", |(code, _)| code)
}

/// Follows the Language setting; the tray needs relabelling after (`tray::relabel`).
pub fn set_language(setting: &str) {
    *LANGUAGE.write().unwrap_or_else(|e| e.into_inner()) = resolve(setting, crate::platform::ui_language());
}

/// The language the UI and native text are in ("en", "nl").
pub fn language() -> &'static str {
    *LANGUAGE.read().unwrap_or_else(|e| e.into_inner())
}

fn text(language: &str, code: &str) -> &'static str {
    let find = |table: &[(&str, &'static str)]| table.iter().find(|(c, _)| *c == code).map(|(_, text)| *text);
    let table = LANGUAGES.iter().find(|(l, _)| *l == language).map_or(EN, |(_, table)| table);
    find(table).or_else(|| find(EN)).unwrap_or("")
}

/// The native text for `code` in the app's language.
pub fn native(code: &str) -> &'static str {
    text(language(), code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_code_and_params() {
        let m = msg!(storeError, store = "GOG", reason = msg!(closeGameFirst));
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            r#"{"code":"storeError","params":{"reason":{"code":"closeGameFirst","params":{}},"store":"GOG"}}"#
        );
        assert_eq!(serde_json::from_str::<Msg>(&serde_json::to_string(&m).unwrap()).unwrap(), m);
        assert_eq!(serde_json::to_string(&msg!(noGamesFound)).unwrap(), r#"{"code":"noGamesFound","params":{}}"#);
    }

    #[test]
    fn store_errors_keep_their_code() {
        let boxed: crate::library::Error = msg!(updateFirst).into();
        assert_eq!(Msg::from(boxed), msg!(updateFirst));
        let raw: crate::library::Error = std::io::Error::other("disk on fire").into();
        assert_eq!(Msg::from(raw), Msg::system("disk on fire"));
    }

    #[test]
    fn logs_read_plainly() {
        assert_eq!(Msg::system("access denied").to_string(), "access denied");
        assert_eq!(msg!(storeError, store = "GOG", reason = msg!(gogStalled)).to_string(), "storeError reason=(gogStalled) store=GOG");
    }

    #[test]
    fn every_native_code_has_text() {
        assert_eq!(native("readyToPlay"), "Ready to play");
        assert!(EN.iter().all(|(_, text)| !text.is_empty()));
    }

    #[test]
    fn language_follows_the_setting_then_windows() {
        assert_eq!(resolve("system", "nl"), "nl");
        assert_eq!(resolve("system", "en"), "en");
        assert_eq!(resolve("nl", "en"), "nl");
        assert_eq!(resolve("en", "nl"), "en");
        assert!(is_language("system") && is_language("en") && is_language("nl") && !is_language("fr") && !is_language(""));
    }

    #[test]
    fn missing_native_text_falls_back_to_english() {
        assert_eq!(text("en", "trayQuit"), "Quit");
        assert!(EN.iter().all(|(code, _)| !text("nl", code).is_empty()));
        assert_eq!(text("en", "noSuchCode"), "");
    }
}
