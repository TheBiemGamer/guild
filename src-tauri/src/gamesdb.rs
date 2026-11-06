//! GOG GamesDB (public, keyless): which Steam release is the same game as an Epic one.
//! Matched by store ID, never by title. A twin is only taken when the answer is unambiguous.

use std::collections::HashMap;
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::cache::db;
use crate::library::percent_encode;

const URL: &str = "https://gamesdb.gog.com/platforms/epic/external_releases";

/// `info` table kind of a cached answer.
pub const TWIN: &str = "twin2";
pub const TWIN_MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;
/// Minimum time between two GamesDB requests.
pub const TWIN_GAP: Duration = Duration::from_millis(500);
/// Prefix of the errors where GamesDB couldn't be reached at all (offline): a lookup run stops there.
pub const UNREACHABLE: &str = "gamesdb unreachable";

#[derive(Deserialize)]
struct Response {
    game: Game,
}

#[derive(Deserialize)]
struct Game {
    #[serde(default)]
    releases: Vec<Release>,
}

#[derive(Deserialize)]
struct Release {
    platform_id: String,
    external_id: String,
}

/// The Steam app id when the game's Steam releases (`123` or `steam_123`) are all one app.
pub fn parse_steam_twin(json: &str) -> Result<Option<u32>, String> {
    let r: Response = serde_json::from_str(json).map_err(|e| format!("gamesdb: {e}"))?;
    let mut ids = std::collections::BTreeSet::new();
    for rel in r.game.releases.iter().filter(|r| r.platform_id == "steam") {
        let id = rel.external_id.strip_prefix("steam_").unwrap_or(&rel.external_id);
        match id.parse::<u32>() {
            Ok(n) if id.bytes().all(|b| b.is_ascii_digit()) => ids.insert(n),
            _ => continue,
        };
    }
    Ok(if ids.len() == 1 { ids.pop_first() } else { None })
}

/// Asks GamesDB for the Steam twin of an Epic app name. A 404 means GamesDB doesn't know the game.
pub async fn look_up(http: &reqwest::Client, app_name: &str) -> Result<Option<u32>, String> {
    let url = format!("{URL}/{}", percent_encode(app_name));
    let resp = http.get(url).send().await.map_err(|e| format!("{UNREACHABLE}: {e}"))?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(format!("gamesdb: HTTP {status}"));
    }
    let body = resp.text().await.map_err(|e| format!("{UNREACHABLE}: {e}"))?;
    parse_steam_twin(&body)
}

#[derive(Serialize, Deserialize)]
struct Stored {
    steam: Option<u32>,
}

/// The cached answer: `Some(None)` is a remembered "no twin", `None` means ask GamesDB.
pub fn cached(conn: &Connection, epic_key: &str, now_secs: i64) -> Option<Option<u32>> {
    let json = db::cached_info(conn, epic_key, TWIN, TWIN_MAX_AGE_SECS, now_secs).ok().flatten()?;
    serde_json::from_str::<Stored>(&json).ok().map(|s| s.steam)
}

pub fn remember(conn: &Connection, epic_key: &str, twin: Option<u32>, now_secs: i64) {
    if let Ok(json) = serde_json::to_string(&Stored { steam: twin }) {
        _ = db::put_info(conn, epic_key, TWIN, &json, now_secs);
    }
}

/// Every fresh cached twin by Epic game key.
pub fn all_cached(conn: &Connection, now_secs: i64) -> HashMap<String, u32> {
    let Ok(mut stmt) = conn.prepare("SELECT key, json FROM info WHERE kind = ?1 AND fetched >= ?2") else {
        return HashMap::new();
    };
    let rows = stmt.query_map(rusqlite::params![TWIN, now_secs - TWIN_MAX_AGE_SECS], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    });
    let Ok(rows) = rows else { return HashMap::new() };
    rows.flatten()
        .filter_map(|(key, json)| Some((key, serde_json::from_str::<Stored>(&json).ok()?.steam?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gamesdb").join(name);
        std::fs::read_to_string(path).unwrap()
    }

    fn temp_db(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("gamesdb-{name}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        db::open(&dir.join("t.sqlite")).unwrap()
    }

    fn releases(rows: &[(&str, &str)]) -> String {
        let list: Vec<String> =
            rows.iter().map(|(p, id)| format!(r#"{{"platform_id":"{p}","external_id":"{id}"}}"#)).collect();
        format!(r#"{{"game":{{"releases":[{}]}}}}"#, list.join(","))
    }

    #[test]
    fn pumpkin_twin_is_last_day_of_june() {
        assert_eq!(parse_steam_twin(&fixture("pumpkin.json")).unwrap(), Some(635320));
    }

    #[test]
    fn no_steam_release_means_no_twin() {
        assert_eq!(parse_steam_twin(&releases(&[("gog", "1"), ("epic", "x")])).unwrap(), None);
        assert!(parse_steam_twin("nope").is_err());
    }

    #[test]
    fn two_steam_releases_mean_no_twin() {
        assert_eq!(parse_steam_twin(&releases(&[("steam", "1"), ("steam", "2")])).unwrap(), None);
    }

    #[test]
    fn duplicated_steam_row_is_one_twin() {
        assert_eq!(parse_steam_twin(&fixture("condor.json")).unwrap(), Some(202750));
    }

    #[test]
    fn two_different_steam_apps_are_no_twin() {
        assert_eq!(parse_steam_twin(&releases(&[("steam", "1"), ("steam", "steam_2")])).unwrap(), None);
        assert_eq!(parse_steam_twin(&releases(&[("steam", "1"), ("steam", "steam_1"), ("steam", "2")])).unwrap(), None);
    }

    #[test]
    fn non_numeric_steam_id_means_no_twin() {
        assert_eq!(parse_steam_twin(&releases(&[("steam", "abc")])).unwrap(), None);
    }

    #[test]
    fn twin_cache_keeps_misses_and_expires() {
        let conn = temp_db("cache");
        let day = 24 * 60 * 60;
        assert_eq!(cached(&conn, "epic-a", 1000), None);
        remember(&conn, "epic-a", None, 1000);
        remember(&conn, "epic-b", Some(635320), 1000);
        assert_eq!(cached(&conn, "epic-a", 1000 + day), Some(None));
        assert_eq!(cached(&conn, "epic-b", 1000 + day), Some(Some(635320)));
        assert_eq!(all_cached(&conn, 1000 + day), HashMap::from([("epic-b".to_string(), 635320)]));
        assert_eq!(cached(&conn, "epic-a", 1000 + 31 * day), None);
        assert!(all_cached(&conn, 1000 + 31 * day).is_empty());
    }
}
