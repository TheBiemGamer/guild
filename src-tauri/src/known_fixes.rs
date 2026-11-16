//! Heroic's community known fixes (github.com/Heroic-Games-Launcher/known-fixes): per-game environment variables for
//! Epic, GOG and Amazon Windows games, matched by the store's own id. Only the variables are applied; the winetricks
//! verbs and installers it also lists would mean long downloads inside a Play, and umu's protonfixes already cover
//! many of those games.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;

use crate::cache::{Dirs, db};

const URL: &str =
    "https://raw.githubusercontent.com/Heroic-Games-Launcher/known-fixes/main/known_fixes.json";
/// `info` key and kind of the fixes, as fetched: `{"<store>": {"<id>": {"envVariables": {…}, …}}}`.
const KIND: &str = "known-fixes";
const MAX_AGE_SECS: i64 = 24 * 60 * 60;

/// The variables the fixes list for the copy `key`, never one Guild was started with: the player's own environment
/// wins. A Play without the list fetches it first (5 s at most); a stale list is used and refreshed behind it.
// shortcut: offline, a Play without the list tries again each time (5 s at most), like umu's database in `wine`.
pub fn env(key: &str) -> Vec<(String, String)> {
    let Ok(conn) = db::open(&Dirs::new().db) else {
        return Vec::new();
    };
    let now = crate::library::unix_now();
    let fresh = db::cached_info(&conn, KIND, KIND, MAX_AGE_SECS, now)
        .ok()
        .flatten();
    let json = match fresh {
        Some(json) => Some(json),
        None => match db::cached_info(&conn, KIND, KIND, i64::MAX, now)
            .ok()
            .flatten()
        {
            Some(stale) => {
                refresh_behind();
                Some(stale)
            }
            None => fetch().inspect(|json| _ = db::put_info(&conn, KIND, KIND, json, now)),
        },
    };
    let fixes = json.and_then(|j| serde_json::from_str::<Value>(&j).ok());
    fixes
        .map(|f| variables(&f, key))
        .unwrap_or_default()
        .into_iter()
        .filter(|(name, _)| std::env::var_os(name).is_none())
        .collect()
}

/// The `envVariables` of the copy `key` (`epic-<app name>`, `gog-<id>`, `amazon-<product id>`), as text.
fn variables(fixes: &Value, key: &str) -> Vec<(String, String)> {
    let Some((store, id)) = key.split_once('-') else {
        return Vec::new();
    };
    let Some(Value::Object(vars)) = fixes
        .get(store)
        .and_then(|s| s.get(id)?.get("envVariables"))
    else {
        return Vec::new();
    };
    vars.iter()
        .filter_map(|(name, value)| {
            let text = match value {
                Value::String(s) => s.clone(),
                // As Heroic, whose launches these were tested with, passes them.
                Value::Bool(b) => b.to_string(),
                Value::Number(n) => n.to_string(),
                _ => return None,
            };
            Some((name.clone(), text))
        })
        .collect()
}

/// Refreshes the cached list on its own thread, one refresh at a time.
fn refresh_behind() {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("known-fixes".into())
        .spawn(|| {
            if let (Some(json), Ok(conn)) = (fetch(), db::open(&Dirs::new().db)) {
                _ = db::put_info(&conn, KIND, KIND, &json, crate::library::unix_now());
            }
            RUNNING.store(false, Ordering::SeqCst);
        });
    if spawned.is_err() {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

fn fetch() -> Option<String> {
    let text = tauri::async_runtime::block_on(async {
        crate::wine::http()
            .get(URL)
            .timeout(Duration::from_secs(5))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await
    })
    .inspect_err(|e| tracing::warn!("known fixes: {e}"))
    .ok()?;
    // Only a list that parses is cached.
    serde_json::from_str::<Value>(&text).ok().map(|_| text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_by_store_and_id_with_values_as_text() {
        let fixes = serde_json::json!({
            "epic": {
                "Heather": { "title": "x", "envVariables": { "USE_FAKE_EPIC_EXE": true } },
                "Fortnite": { "envVariables": { "USE_FAKE_EPIC_EXE": "0", "N": 2 } },
                "Sugar": { "winetricks": ["vcrun2022"] }
            },
            "gog": { "1207658924": { "envVariables": { "A": "b" } } }
        });
        assert_eq!(
            variables(&fixes, "epic-Heather"),
            [("USE_FAKE_EPIC_EXE".into(), "true".into())]
        );
        assert_eq!(
            variables(&fixes, "epic-Fortnite"),
            [
                ("N".into(), "2".into()),
                ("USE_FAKE_EPIC_EXE".into(), "0".into())
            ]
        );
        assert!(variables(&fixes, "epic-Sugar").is_empty());
        assert!(variables(&fixes, "gog-Heather").is_empty());
        assert_eq!(
            variables(&fixes, "gog-1207658924"),
            [("A".into(), "b".into())]
        );
        assert!(variables(&fixes, "custom-1").is_empty());
    }
}
