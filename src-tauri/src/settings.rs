//! User settings: plain values live in the SQLite `settings` table; the IGDB Client Secret lives
//! in Windows Credential Manager and never touches SQLite, a file, a log or the UI.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::cache::db;

pub const SHOW_BANNER: &str = "show_banner";
pub const AMBIENT: &str = "ambient";
pub const EPIC_INSTALL_DIR: &str = "epic_install_dir";
pub const IGDB_CLIENT_ID: &str = "igdb_client_id";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub show_banner: bool,
    pub ambient: bool,
    pub epic_install_dir: Option<PathBuf>,
    pub igdb_client_id: Option<String>,
}

/// Unset or unreadable values fall back to the defaults: banner and ambient color on.
pub fn load(conn: &Connection) -> Settings {
    let get = |name| db::setting(conn, name).ok().flatten();
    Settings {
        show_banner: get(SHOW_BANNER).is_none_or(|v| v != "false"),
        ambient: get(AMBIENT).is_none_or(|v| v != "false"),
        epic_install_dir: get(EPIC_INSTALL_DIR).map(PathBuf::from),
        igdb_client_id: get(IGDB_CLIENT_ID),
    }
}

/// Whether `name` may be set to `value` (`None` resets it). The secret has its own command.
pub fn validate(name: &str, value: Option<&str>) -> Result<(), String> {
    match name {
        SHOW_BANNER | AMBIENT => match value {
            None | Some("true" | "false") => Ok(()),
            Some(_) => Err(format!("{name} must be true or false.")),
        },
        EPIC_INSTALL_DIR => match value {
            Some(dir) if !Path::new(dir).is_dir() => Err(format!("{dir} isn't an existing folder.")),
            _ => Ok(()),
        },
        IGDB_CLIENT_ID => Ok(()),
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

const SERVICE: &str = "dev.noah.gamelauncher";
const SECRET_USER: &str = "igdb-client-secret";

/// The IGDB Client Secret, if one is saved.
pub fn secret() -> Option<String> {
    read(SECRET_USER)
}

/// Saves the secret, or deletes it for `None`. Errors never include the value.
pub fn set_secret(value: Option<&str>) -> Result<(), String> {
    write(SECRET_USER, value)
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
            eprintln!("credential manager read: {e}");
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
    fn defaults_have_banner_and_ambient_on() {
        let s = load(&memory());
        assert!(s.show_banner && s.ambient);
        assert_eq!((s.epic_install_dir, s.igdb_client_id), (None, None));
    }

    #[test]
    fn saved_values_load_back() {
        let conn = memory();
        let dir = std::env::temp_dir();
        update(&conn, SHOW_BANNER, Some("false")).unwrap();
        update(&conn, EPIC_INSTALL_DIR, Some(&format!("  {}  ", dir.display()))).unwrap();
        update(&conn, IGDB_CLIENT_ID, Some(" abc ")).unwrap();
        let s = load(&conn);
        assert!(!s.show_banner && s.ambient);
        assert_eq!(s.epic_install_dir, Some(dir));
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
        assert!(validate(AMBIENT, Some("maybe")).is_err());
        assert!(validate(EPIC_INSTALL_DIR, Some(r"Z:\no\such\folder")).is_err());
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
}
