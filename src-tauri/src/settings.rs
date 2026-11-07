//! User settings: plain values live in the SQLite `settings` table; the IGDB Client Secret and the
//! SteamGridDB API key live in Windows Credential Manager and never touch SQLite, a file, a log or the UI.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::cache::db;
use crate::view::Sort;

pub const SHOW_BANNER: &str = "show_banner";
pub const AMBIENT: &str = "ambient";
pub const CLOUD_SAVES: &str = "cloud_saves";
pub const GOG_CLOUD_SAVES: &str = "gog_cloud_saves";
pub const CLOSE_TO_TRAY: &str = "close_to_tray";
pub const NOTIFY_DOWNLOADS: &str = "notify_downloads";
/// Not stored: the autostart entry in the registry is the truth (`commands::settings`).
pub const START_WITH_WINDOWS: &str = "start_with_windows";
pub const EPIC_INSTALL_DIR: &str = "epic_install_dir";
pub const GOG_INSTALL_DIR: &str = "gog_install_dir";
pub const AMAZON_INSTALL_DIR: &str = "amazon_install_dir";
pub const IGDB_CLIENT_ID: &str = "igdb_client_id";
/// The library's sort, as the toolbar last left it (serde names: `mostPlayed`, `longestToBeat`). It sets
/// the grouping too. An old `library_group` row is left alone and ignored.
pub const LIBRARY_SORT: &str = "library_sort";
/// Name of the signed-in Steam account. Not a secret; the refresh token is in Credential Manager.
pub const STEAM_ACCOUNT: &str = "steam_account";
/// The newest app version that has started here, for the "Updated to" notice. Internal: not in `validate`.
const SEEN_VERSION: &str = "seen_version";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub show_banner: bool,
    pub ambient: bool,
    /// Epic games sync their cloud saves around play unless the game says otherwise.
    pub cloud_saves: bool,
    /// The same for GOG games.
    pub gog_cloud_saves: bool,
    /// Closing the window hides it to the tray instead of quitting.
    pub close_to_tray: bool,
    /// A toast when a download finishes.
    pub notify_downloads: bool,
    pub epic_install_dir: Option<PathBuf>,
    pub gog_install_dir: Option<PathBuf>,
    pub amazon_install_dir: Option<PathBuf>,
    pub igdb_client_id: Option<String>,
    pub sort: Sort,
}

/// Unset or unreadable values fall back to the defaults: every switch on.
pub fn load(conn: &Connection) -> Settings {
    let get = |name| db::setting(conn, name).ok().flatten();
    Settings {
        show_banner: get(SHOW_BANNER).is_none_or(|v| v != "false"),
        ambient: get(AMBIENT).is_none_or(|v| v != "false"),
        cloud_saves: get(CLOUD_SAVES).is_none_or(|v| v != "false"),
        gog_cloud_saves: get(GOG_CLOUD_SAVES).is_none_or(|v| v != "false"),
        close_to_tray: get(CLOSE_TO_TRAY).is_none_or(|v| v != "false"),
        notify_downloads: get(NOTIFY_DOWNLOADS).is_none_or(|v| v != "false"),
        epic_install_dir: get(EPIC_INSTALL_DIR).map(PathBuf::from),
        gog_install_dir: get(GOG_INSTALL_DIR).map(PathBuf::from),
        amazon_install_dir: get(AMAZON_INSTALL_DIR).map(PathBuf::from),
        igdb_client_id: get(IGDB_CLIENT_ID),
        sort: get(LIBRARY_SORT).and_then(|v| Sort::parse(&v)).unwrap_or(Sort::Title),
    }
}

/// Whether `name` may be set to `value` (`None` resets it). The secret has its own command.
pub fn validate(name: &str, value: Option<&str>) -> Result<(), String> {
    match name {
        SHOW_BANNER | AMBIENT | CLOUD_SAVES | GOG_CLOUD_SAVES | CLOSE_TO_TRAY | NOTIFY_DOWNLOADS | START_WITH_WINDOWS => match value {
            None | Some("true" | "false") => Ok(()),
            Some(_) => Err(format!("{name} must be true or false.")),
        },
        EPIC_INSTALL_DIR | GOG_INSTALL_DIR | AMAZON_INSTALL_DIR => match value {
            Some(dir) if !Path::new(dir).is_dir() => Err(format!("{dir} isn't an existing folder.")),
            _ => Ok(()),
        },
        IGDB_CLIENT_ID => Ok(()),
        LIBRARY_SORT if value.is_some_and(|v| Sort::parse(v).is_none()) => Err(format!("Unknown sort: {value:?}")),
        LIBRARY_SORT => Ok(()),
        "igdb_secret" => Err("The IGDB secret is saved through its own command.".into()),
        _ => Err(format!("Unknown setting: {name}")),
    }
}

/// Validates, then saves. Blank text clears the setting; folders and IDs are trimmed.
pub fn update(conn: &Connection, name: &str, value: Option<&str>) -> Result<(), String> {
    let value = value.map(str::trim).filter(|v| !v.is_empty());
    validate(name, value)?;
    db::set_setting(conn, name, value).map_err(|e| e.to_string())
}

/// `current` when it is newer than the version that last started here, so the UI says "Updated to" once.
/// A first start and an older build (a dev build) say nothing; an older build also leaves the stored one.
pub fn take_updated(conn: &Connection, current: &str) -> Option<String> {
    let parse = |v: &str| -> Option<Vec<u64>> { v.split('.').map(|n| n.parse().ok()).collect() };
    let now = parse(current)?;
    let seen = db::setting(conn, SEEN_VERSION).ok().flatten().and_then(|v| parse(&v));
    if seen.as_ref().is_some_and(|seen| *seen >= now) {
        return None;
    }
    if let Err(e) = db::set_setting(conn, SEEN_VERSION, Some(current)) {
        log!("saving the seen version: {e}");
        return None;
    }
    seen.map(|_| current.to_string())
}

const SERVICE: &str = "dev.noah.gamelauncher";
const SECRET_USER: &str = "igdb-client-secret";
const GRID_KEY_USER: &str = "steamgriddb";
pub const STEAM_TOKEN_USER: &str = "steam-refresh-token";

/// The IGDB Client Secret, if one is saved.
pub fn secret() -> Option<String> {
    read(SECRET_USER)
}

/// Saves the secret, or deletes it for `None`. Errors never include the value.
pub fn set_secret(value: Option<&str>) -> Result<(), String> {
    write(SECRET_USER, value)
}

/// The SteamGridDB API key, if one is saved.
pub fn grid_key() -> Option<String> {
    read(GRID_KEY_USER)
}

/// Saves the SteamGridDB API key, or deletes it for `None`. Errors never include the value.
pub fn set_grid_key(value: Option<&str>) -> Result<(), String> {
    write(GRID_KEY_USER, value)
}

/// The Steam refresh token, if one is saved.
pub fn steam_token() -> Option<String> {
    read(STEAM_TOKEN_USER)
}

/// Saves the Steam refresh token, or deletes it for `None`. Errors never include the value.
pub fn set_steam_token(value: Option<&str>) -> Result<(), String> {
    write(STEAM_TOKEN_USER, value)
}

/// Windows Credential Manager, registered once as keyring-core's default store.
fn entry(user: &str) -> Result<keyring_core::Entry, String> {
    static STORE: std::sync::LazyLock<Result<(), String>> = std::sync::LazyLock::new(|| {
        let store = windows_native_keyring_store::Store::new().map_err(|e| e.to_string())?;
        keyring_core::set_default_store(store);
        Ok(())
    });
    STORE.clone().map_err(|e| format!("Credential Manager: {e}"))?;
    keyring_core::Entry::new(SERVICE, user).map_err(|e| format!("Credential Manager: {e}"))
}

fn read(user: &str) -> Option<String> {
    match entry(user).ok()?.get_password() {
        Ok(secret) => Some(secret),
        Err(keyring_core::Error::NoEntry) => None,
        Err(e) => {
            log!("credential manager read: {e}");
            None
        }
    }
}

fn write(user: &str, value: Option<&str>) -> Result<(), String> {
    let entry = entry(user)?;
    let result = match value {
        Some(secret) => entry.set_password(secret),
        None => entry.delete_credential(),
    };
    match result {
        Err(keyring_core::Error::NoEntry) if value.is_none() => Ok(()),
        other => other.map_err(|e| format!("Credential Manager: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Connection {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("launcher-settings-test-{}-{n}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        db::open(&path).unwrap()
    }

    #[test]
    fn defaults_have_every_switch_on() {
        let s = load(&memory());
        assert!(s.show_banner && s.ambient && s.cloud_saves && s.gog_cloud_saves && s.close_to_tray && s.notify_downloads);
        assert_eq!((s.epic_install_dir, s.gog_install_dir, s.amazon_install_dir, s.igdb_client_id), (None, None, None, None));
        assert_eq!(s.sort, Sort::Title);
    }

    #[test]
    fn sort_loads_back_and_rejects_unknown_names() {
        let conn = memory();
        update(&conn, LIBRARY_SORT, Some("longestToBeat")).unwrap();
        assert!(update(&conn, LIBRARY_SORT, Some("size")).is_err());
        assert_eq!(load(&conn).sort, Sort::LongestToBeat);
    }

    #[test]
    fn old_library_group_is_ignored() {
        let conn = memory();
        // Saved by an older build; the sort sets the grouping now.
        db::set_setting(&conn, "library_group", Some("playtime")).unwrap();
        assert!(update(&conn, "library_group", Some("store")).is_err());
        assert_eq!(load(&conn), load(&memory()));
    }

    #[test]
    fn saved_values_load_back() {
        let conn = memory();
        let dir = std::env::temp_dir();
        update(&conn, SHOW_BANNER, Some("false")).unwrap();
        update(&conn, CLOUD_SAVES, Some("false")).unwrap();
        update(&conn, GOG_CLOUD_SAVES, Some("false")).unwrap();
        update(&conn, CLOSE_TO_TRAY, Some("false")).unwrap();
        update(&conn, NOTIFY_DOWNLOADS, Some("false")).unwrap();
        update(&conn, EPIC_INSTALL_DIR, Some(&format!("  {}  ", dir.display()))).unwrap();
        update(&conn, GOG_INSTALL_DIR, Some(&dir.display().to_string())).unwrap();
        update(&conn, AMAZON_INSTALL_DIR, Some(&dir.display().to_string())).unwrap();
        update(&conn, IGDB_CLIENT_ID, Some(" abc ")).unwrap();
        let s = load(&conn);
        assert!(!s.show_banner && s.ambient && !s.cloud_saves && !s.gog_cloud_saves && !s.close_to_tray && !s.notify_downloads);
        assert_eq!(s.epic_install_dir, Some(dir.clone()));
        assert_eq!(s.gog_install_dir, Some(dir.clone()));
        assert_eq!(s.amazon_install_dir, Some(dir));
        assert_eq!(s.igdb_client_id.as_deref(), Some("abc"));
        // Blank resets.
        update(&conn, SHOW_BANNER, None).unwrap();
        update(&conn, IGDB_CLIENT_ID, Some("  ")).unwrap();
        let s = load(&conn);
        assert!(s.show_banner);
        assert_eq!(s.igdb_client_id, None);
    }

    #[test]
    fn validate_rejects_the_secret_unknown_names_and_bad_values() {
        assert!(validate("igdb_secret", Some("x")).is_err());
        assert!(validate("igdb_secret", None).is_err());
        assert!(validate("nope", Some("x")).is_err());
        // Written only by connect and disconnect.
        assert!(validate(STEAM_ACCOUNT, Some("x")).is_err());
        assert!(validate(AMBIENT, Some("maybe")).is_err());
        assert!(validate(GOG_CLOUD_SAVES, Some("on")).is_err());
        assert!(validate(GOG_CLOUD_SAVES, Some("false")).is_ok());
        assert!(validate(START_WITH_WINDOWS, Some("yes")).is_err());
        assert!(validate(START_WITH_WINDOWS, Some("true")).is_ok());
        assert!(validate(EPIC_INSTALL_DIR, Some(r"Z:\no\such\folder")).is_err());
        assert!(validate(GOG_INSTALL_DIR, Some(r"Z:\no\such\folder")).is_err());
        assert!(validate(AMAZON_INSTALL_DIR, Some(r"Z:\no\such\folder")).is_err());
        assert!(validate(AMBIENT, Some("false")).is_ok());
        assert!(validate(EPIC_INSTALL_DIR, None).is_ok());
    }

    #[test]
    fn update_never_stores_a_rejected_value() {
        let conn = memory();
        assert!(update(&conn, "igdb_secret", Some("x")).is_err());
        assert_eq!(db::setting(&conn, "igdb_secret").unwrap(), None);
    }

    #[test]
    fn updated_is_said_once_and_only_after_an_older_version() {
        let conn = memory();
        // First start: nothing to announce, but remembered.
        assert_eq!(take_updated(&conn, "0.2.0"), None);
        assert_eq!(take_updated(&conn, "0.2.0"), None);
        // Numeric, not text: 0.10.0 is newer than 0.9.0.
        db::set_setting(&conn, SEEN_VERSION, Some("0.9.0")).unwrap();
        assert_eq!(take_updated(&conn, "0.10.0").as_deref(), Some("0.10.0"));
        assert_eq!(take_updated(&conn, "0.10.0"), None);
        // An older dev build says nothing and keeps the installed version.
        assert_eq!(take_updated(&conn, "0.3.0"), None);
        assert_eq!(db::setting(&conn, SEEN_VERSION).unwrap().as_deref(), Some("0.10.0"));
        assert!(validate(SEEN_VERSION, Some("9.9.9")).is_err());
    }

    #[test]
    #[ignore = "touches Windows Credential Manager"]
    fn secret_round_trips_through_credential_manager() {
        let user = "igdb-client-secret-test";
        write(user, Some("test")).unwrap();
        assert_eq!(read(user).as_deref(), Some("test"));
        write(user, None).unwrap();
        assert_eq!(read(user), None);
        // Deleting what isn't there is fine.
        write(user, None).unwrap();
    }

    #[test]
    #[ignore = "touches Windows Credential Manager"]
    fn steam_token_round_trips() {
        let user = "steam-refresh-token-test";
        write(user, Some("test")).unwrap();
        assert_eq!(read(user).as_deref(), Some("test"));
        write(user, None).unwrap();
        assert_eq!(read(user), None);
    }
}
