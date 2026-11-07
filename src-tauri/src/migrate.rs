//! Moves what 0.1.0, when the app was called Game Launcher, left under the old names: the local folder, the
//! Credential Manager entries and the Start with Windows entry. Each step runs at every start and does nothing once
//! done. The old WebView2 folder (`%LOCALAPPDATA%\dev.noah.gamelauncher`) is only webview cache and stays.

use std::path::{Path, PathBuf};

/// The local folder's name, under `%LOCALAPPDATA%`. The installer puts the exe in the same folder.
const FOLDER: &str = "Guild";
const OLD_FOLDER: &str = "launcher";
const OLD_SERVICE: &str = "dev.noah.gamelauncher";
/// The Run entry's name: tauri-plugin-autostart names it after `productName`.
const OLD_AUTOSTART: &str = "Game Launcher";

/// The folder that holds `data` and `cache`: `%LOCALAPPDATA%\Guild`, or the old folder while its data couldn't be
/// moved (another program had a file open), so nothing starts over empty.
pub fn root(local: &Path) -> PathBuf {
    let (new, old) = (local.join(FOLDER), local.join(OLD_FOLDER));
    if !new.join("data").exists() && old.join("data").exists() { old } else { new }
}

/// Moves the old folder's contents into the new one unless the new one already has data. Runs before anything
/// opens a file there; returns what happened, for the log (which lives in that folder).
pub fn folder(local: &Path) -> Option<String> {
    let (new, old) = (local.join(FOLDER), local.join(OLD_FOLDER));
    if !old.join("data").exists() {
        return None;
    }
    if new.join("data").exists() {
        return Some(format!("{} is in use, so {} stays as it is", new.display(), old.display()));
    }
    // `data` first: while it can't move (another program has the database open), nothing moves.
    if let Err(e) = std::fs::create_dir_all(&new).and_then(|()| std::fs::rename(old.join("data"), new.join("data"))) {
        return Some(format!("moving {} to {}: {e}; trying again next start", old.display(), new.display()));
    }
    for entry in std::fs::read_dir(&old).into_iter().flatten().flatten() {
        let to = new.join(entry.file_name());
        if !to.exists() {
            let _ = std::fs::rename(entry.path(), to);
        }
    }
    Some(match std::fs::remove_dir(&old) {
        Ok(()) => format!("moved {} to {}", old.display(), new.display()),
        Err(_) => format!("moved {} to {}; some files stayed behind", old.display(), new.display()),
    })
}

/// Copies each of `users`' secrets from the old service to `service` when only the old one has it, reads it back,
/// then deletes the old entry. Logs names, never values.
pub fn secrets(
    entry: impl Fn(&str, &str) -> keyring_core::Result<keyring_core::Entry>,
    service: &str,
    users: &[&str],
) {
    for user in users {
        let moved = (|| -> keyring_core::Result<bool> {
            let old = entry(OLD_SERVICE, user)?;
            let secret = match old.get_password() {
                Err(keyring_core::Error::NoEntry) => return Ok(false),
                other => other?,
            };
            let new = entry(service, user)?;
            match new.get_password() {
                Err(keyring_core::Error::NoEntry) => {}
                other => {
                    other?;
                    tracing::info!("credential {user}: already under {service}, the old entry stays");
                    return Ok(false);
                }
            }
            new.set_password(&secret)?;
            if new.get_password()? != secret {
                return Err(keyring_core::Error::Invalid(user.to_string(), "read back differently".into()));
            }
            old.delete_credential()?;
            Ok(true)
        })();
        match moved {
            Ok(true) => tracing::info!("credential {user}: moved from {OLD_SERVICE} to {service}"),
            Ok(false) => {}
            Err(e) => tracing::warn!("credential {user}: moving from {OLD_SERVICE}: {e}"),
        }
    }
}

/// Moves Start with Windows from the old Run entry to this exe, keeping it off when Task Manager turned it off.
#[cfg(windows)]
pub fn autostart(app: &tauri::AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    let Some(on) = crate::platform::autostart(OLD_AUTOSTART) else { return };
    if on && let Err(e) = app.autolaunch().enable() {
        tracing::warn!("start with windows: turning it on for this exe: {e}");
        return;
    }
    match crate::platform::remove_autostart(OLD_AUTOSTART) {
        Ok(()) => tracing::info!("start with windows: moved from {OLD_AUTOSTART} ({})", if on { "on" } else { "off" }),
        Err(e) => tracing::warn!("start with windows: removing the {OLD_AUTOSTART} entry: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyring_core::api::CredentialStoreApi;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("launcher-migrate-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn old_install(local: &Path) {
        fs::create_dir_all(local.join("launcher/data/logs")).unwrap();
        fs::write(local.join("launcher/data/library.sqlite"), "db").unwrap();
        fs::create_dir_all(local.join("launcher/cache/art")).unwrap();
    }

    #[test]
    fn fresh_pc_uses_the_new_folder() {
        let local = temp_dir("fresh");
        assert_eq!(folder(&local), None);
        assert_eq!(root(&local), local.join("Guild"));
        assert!(!local.join("Guild").exists());
    }

    #[test]
    fn moves_the_old_folder_once() {
        let local = temp_dir("move");
        old_install(&local);
        assert_eq!(root(&local), local.join("launcher"));
        assert!(folder(&local).unwrap().starts_with("moved"));
        assert_eq!(fs::read_to_string(local.join("Guild/data/library.sqlite")).unwrap(), "db");
        assert!(local.join("Guild/cache/art").is_dir());
        assert!(!local.join("launcher").exists());
        assert_eq!(root(&local), local.join("Guild"));
        assert_eq!(folder(&local), None);
    }

    #[test]
    fn moves_into_the_install_folder_and_leaves_the_exe() {
        let local = temp_dir("installed");
        old_install(&local);
        fs::create_dir_all(local.join("Guild")).unwrap();
        fs::write(local.join("Guild/Guild.exe"), "exe").unwrap();
        assert!(folder(&local).unwrap().starts_with("moved"));
        assert!(local.join("Guild/Guild.exe").exists());
        assert!(local.join("Guild/data/library.sqlite").exists());
        assert!(!local.join("launcher").exists());
    }

    #[test]
    fn a_failed_move_keeps_using_the_old_folder() {
        let local = temp_dir("failed");
        old_install(&local);
        // A file where the folder should go, so creating it fails.
        fs::write(local.join("Guild"), "").unwrap();
        assert!(folder(&local).unwrap().contains("trying again"));
        assert!(local.join("launcher/data/library.sqlite").exists());
        assert!(local.join("launcher/cache/art").is_dir());
        assert_eq!(root(&local), local.join("launcher"));
    }

    #[test]
    fn leaves_the_old_folder_when_both_have_data() {
        let local = temp_dir("both");
        old_install(&local);
        fs::create_dir_all(local.join("Guild/data")).unwrap();
        assert!(folder(&local).unwrap().contains("stays"));
        assert!(local.join("launcher/data/library.sqlite").exists());
        assert_eq!(root(&local), local.join("Guild"));
    }

    #[test]
    fn moves_only_what_is_missing() {
        let local = temp_dir("partial");
        old_install(&local);
        fs::create_dir_all(local.join("Guild/cache")).unwrap();
        folder(&local);
        assert!(local.join("Guild/data/library.sqlite").exists());
        // The new cache was kept, so the old one is left behind.
        assert!(local.join("launcher/cache/art").is_dir());
        assert_eq!(root(&local), local.join("Guild"));
    }

    #[test]
    fn secrets_move_once_and_never_overwrite() {
        let store = keyring_core::mock::Store::new().unwrap();
        let entry = |s: &str, u: &str| store.build(s, u, None);
        entry(OLD_SERVICE, "a").unwrap().set_password("one").unwrap();
        entry(OLD_SERVICE, "b").unwrap().set_password("old").unwrap();
        entry("new", "b").unwrap().set_password("new").unwrap();
        secrets(entry, "new", &["a", "b", "c"]);
        assert_eq!(entry("new", "a").unwrap().get_password().unwrap(), "one");
        assert!(matches!(entry(OLD_SERVICE, "a").unwrap().get_password(), Err(keyring_core::Error::NoEntry)));
        assert_eq!(entry("new", "b").unwrap().get_password().unwrap(), "new");
        assert_eq!(entry(OLD_SERVICE, "b").unwrap().get_password().unwrap(), "old");
        assert!(matches!(entry("new", "c").unwrap().get_password(), Err(keyring_core::Error::NoEntry)));
        secrets(entry, "new", &["a"]);
        assert_eq!(entry("new", "a").unwrap().get_password().unwrap(), "one");
    }

    #[test]
    fn a_failed_copy_keeps_the_old_secret() {
        let store = keyring_core::mock::Store::new().unwrap();
        let entry = |s: &str, u: &str| store.build(s, u, None);
        entry(OLD_SERVICE, "a").unwrap().set_password("one").unwrap();
        let new = entry("new", "a").unwrap();
        let cred: &keyring_core::mock::Cred = new.as_any().downcast_ref().unwrap();
        // The first call (the presence check) fails, so nothing is copied.
        cred.set_error(keyring_core::Error::NoStorageAccess("locked".into()));
        secrets(entry, "new", &["a"]);
        assert_eq!(entry(OLD_SERVICE, "a").unwrap().get_password().unwrap(), "one");
    }
}
