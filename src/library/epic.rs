//! Installed Epic games, read from the per-user Epic Games Launcher (v2) data in
//! `%LOCALAPPDATA%\com.epicgames.eglv2`:
//! - `installations.jsonl`: append-only install log; the last line per `installationId` wins.
//! - `cache/prod/catalog-cache.json`: titles and art URLs by catalog item id.
//! - `last-played.json`: `{account: {AppName: RFC 3339 time}}`.
//!
//! The older launcher's `C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests\*.item` doesn't exist on v2 installs.
// shortcut: v2 only; add the ProgramData .item reader if this ever runs next to the old launcher.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use super::{Art, Error, Game, Image, Progress, Provider, ProviderKind};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Install {
    installation_id: String,
    complete: bool,
    dir: PathBuf,
    namespace: String,
    item_id: String,
    /// The launcher's `AppName`.
    artifact_id: String,
    /// Set on add-ons/content packs that belong to another installation.
    #[serde(default)]
    parent_installation_id: Option<String>,
}

pub struct Epic;

impl Provider for Epic {
    fn list_games(&self) -> Result<Vec<Game>, Error> {
        let data = data_dir();
        let log = match std::fs::read_to_string(data.join("installations.jsonl")) {
            Ok(log) => log,
            // Launcher not installed, or nothing ever installed.
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let catalog = read_json(&data.join("cache").join("prod").join("catalog-cache.json"))
            .and_then(|mut c| c.get_mut("items").map(Value::take))
            .unwrap_or_default();
        let last_played = last_played(&read_json(&data.join("last-played.json")).unwrap_or_default());
        // shortcut: uninstalls aren't confirmed to be logged, so a missing folder counts as uninstalled.
        Ok(installs(&log).iter().filter(|i| i.dir.exists()).map(|i| to_game(i, &catalog, &last_played)).collect())
    }

    fn launch(&self, game: &Game) -> Result<(), Error> {
        super::open_uri(&uri(&find_install(game)?, "launch"))
    }

    fn install(&self, game: &Game) -> Result<(), Error> {
        super::open_uri(&uri(&find_install(game)?, "install"))
    }

    // shortcut: the launcher has no known uninstall URI; the user does it in the Epic launcher.
    fn uninstall(&self, _game: &Game) -> Result<(), Error> {
        Err("uninstall Epic games from the Epic Games Launcher".into())
    }

    // shortcut: Epic has no RunningAppID equivalent; would need process matching against the install dir.
    fn running_game(&self) -> Option<String> {
        None
    }

    // shortcut: download progress isn't in any file found so far.
    fn download_progress(&self, _game: &Game) -> Option<Progress> {
        None
    }
}

fn data_dir() -> PathBuf {
    directories::BaseDirs::new().expect("Windows always has a home dir").data_local_dir().join("com.epicgames.eglv2")
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).map_err(|e| eprintln!("epic {}: {e}", path.display())).ok()?;
    serde_json::from_str(&text).map_err(|e| eprintln!("epic {}: {e}", path.display())).ok()
}

/// Latest record per installation, without the header line and add-ons.
fn installs(log: &str) -> Vec<Install> {
    let mut latest = HashMap::new();
    for line in log.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<Install>(line) {
            Ok(install) => _ = latest.insert(install.installation_id.clone(), install),
            // The first line is a `{"version":…,"entries":…}` header; anything else is worth a log line.
            Err(e) if line.contains("\"installationId\"") => eprintln!("epic installations.jsonl: {e}"),
            Err(_) => {}
        }
    }
    latest.into_values().filter(|i| i.parent_installation_id.is_none()).collect()
}

fn find_install(game: &Game) -> Result<Install, Error> {
    let log = std::fs::read_to_string(data_dir().join("installations.jsonl"))?;
    installs(&log)
        .into_iter()
        .find(|i| i.artifact_id == game.external_id)
        .ok_or_else(|| format!("{} is no longer in the Epic install list", game.title).into())
}

/// Same form the launcher writes into its own desktop shortcuts.
fn uri(install: &Install, action: &str) -> String {
    format!(
        "com.epicgames.launcher://apps/{}%3A{}%3A{}?action={action}&silent=true",
        install.namespace, install.item_id, install.artifact_id
    )
}

fn to_game(install: &Install, catalog: &Value, last_played: &HashMap<String, SystemTime>) -> Game {
    let item = &catalog[&install.item_id]["item"];
    let image = |kind: &str| Image {
        path: None,
        url: item["keyImages"]
            .as_array()
            .and_then(|images| images.iter().find(|k| k["type"] == kind))
            .and_then(|k| k["url"].as_str())
            .map(str::to_owned),
    };
    let folder = || install.dir.file_name().map_or_else(|| install.artifact_id.clone(), |f| f.to_string_lossy().into_owned());
    Game {
        provider: ProviderKind::Epic,
        external_id: install.artifact_id.clone(),
        title: item["title"].as_str().map_or_else(folder, str::to_owned),
        installed: install.complete,
        install_dir: Some(install.dir.clone()),
        // shortcut: playtime/journal.json is empty on this PC, so its format is unknown.
        playtime_minutes: None,
        last_played: last_played.get(&install.artifact_id).copied(),
        art: Art {
            cover: image("DieselGameBoxTall"),
            header: image("DieselGameBox"),
            hero: image("DieselGameBox"),
            logo: image("DieselGameBoxLogo"),
        },
    }
}

/// Latest time per AppName across all accounts on this PC.
fn last_played(json: &Value) -> HashMap<String, SystemTime> {
    let mut latest = HashMap::new();
    for apps in json.as_object().into_iter().flat_map(|accounts| accounts.values()) {
        for (app, time) in apps.as_object().into_iter().flatten() {
            if let Some(t) = time.as_str().and_then(parse_rfc3339) {
                latest.entry(app.clone()).and_modify(|old: &mut SystemTime| *old = (*old).max(t)).or_insert(t);
            }
        }
    }
    latest
}

/// `2026-10-25T21:50:18.126117500+00:00` (or `Z`, or another offset) to whole seconds.
fn parse_rfc3339(s: &str) -> Option<SystemTime> {
    let num = |at: usize, len: usize| s.get(at..at + len)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (h, mi, sec) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    let offset = match s.rfind(['+', '-']).filter(|&i| i > 18) {
        Some(i) => (if &s[i..=i] == "-" { -1 } else { 1 }) * (num(i + 1, 2)? * 3600 + num(i + 4, 2)? * 60),
        None => 0, // `Z`
    };
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec - offset;
    Some(UNIX_EPOCH + Duration::from_secs(u64::try_from(secs).ok()?))
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/epic").join(name);
        std::fs::read_to_string(path).unwrap()
    }

    fn sorted_installs() -> Vec<Install> {
        let mut all = installs(&fixture("installations.jsonl"));
        all.sort_by(|a, b| a.artifact_id.cmp(&b.artifact_id));
        all
    }

    #[test]
    fn latest_record_wins_and_addons_header_and_junk_are_skipped() {
        let all = sorted_installs();
        let ids: Vec<_> = all.iter().map(|i| i.artifact_id.as_str()).collect();
        assert_eq!(ids, ["Fortnite", "bcd55b0d87c245dd867f5b1bd496f1df"]);
        // Fortnite's second record replaced the first.
        assert_eq!(all[0].dir, PathBuf::from(r"D:\Epic\Fortnite"));
        assert!(!all[1].complete);
    }

    #[test]
    fn maps_catalog_title_art_and_last_played() {
        let catalog: Value = serde_json::from_str::<Value>(&fixture("catalog-cache.json")).unwrap()["items"].clone();
        let last = last_played(&serde_json::from_str(&fixture("last-played.json")).unwrap());
        let all = sorted_installs();

        let fortnite = to_game(&all[0], &catalog, &last);
        assert_eq!(fortnite.title, "Fortnite");
        assert_eq!(fortnite.key(), "epic-Fortnite");
        assert!(fortnite.installed);
        assert_eq!(fortnite.art.cover.url.as_deref(), Some("https://cdn1.epicgames.com/tall.jpg"));
        assert_eq!(fortnite.art.hero.url.as_deref(), Some("https://cdn1.epicgames.com/wide.jpg"));
        assert_eq!(fortnite.art.logo.url, None);
        // Newest of the two accounts' entries.
        assert_eq!(fortnite.last_played, parse_rfc3339("2026-10-25T21:50:18Z"));

        // Not in the catalog: title falls back to the install folder name, still downloading.
        let unknown = to_game(&all[1], &catalog, &last);
        assert_eq!(unknown.title, "ArknightsEndfieldgowoU");
        assert!(!unknown.installed);
        assert_eq!(unknown.art, Art::default());
    }

    #[test]
    fn launch_uri_matches_the_launchers_own_shortcuts() {
        // From the Fortnite.url the launcher put on the desktop.
        assert_eq!(
            uri(&sorted_installs()[0], "launch"),
            "com.epicgames.launcher://apps/fn%3A4fe75bbc5a674f4f9b356b5c90567da5%3AFortnite?action=launch&silent=true"
        );
    }

    /// Real-world check against this PC's launcher data: `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a local Epic Games Launcher install"]
    fn real_epic_install_lists_games() {
        let games = Epic.list_games().unwrap();
        for g in &games {
            eprintln!("{} | {} | installed={} | cover={:?}", g.external_id, g.title, g.installed, g.art.cover.url);
        }
        assert!(!games.is_empty());
    }

    #[test]
    fn parses_rfc3339_with_offsets() {
        let at = |s| Some(UNIX_EPOCH + Duration::from_secs(s));
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), at(0));
        // Checked against Python's datetime.fromisoformat(...).timestamp().
        assert_eq!(parse_rfc3339("2026-10-25T21:50:18.126117500+00:00"), at(1_792_965_018));
        assert_eq!(parse_rfc3339("2026-10-25T23:50:18+02:00"), at(1_792_965_018));
        assert_eq!(parse_rfc3339("2024-02-29T12:00:00-01:30"), at(1_709_213_400));
        assert_eq!(parse_rfc3339("garbage"), None);
        assert_eq!(parse_rfc3339("1969-12-31T23:59:59Z"), None);
    }
}
