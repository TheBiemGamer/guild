//! Store details for the game page: description, genres, developer, screenshots.
//! Steam's `appdetails` and Epic's catalog metadata are parsed into one plain-text shape.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cache::db;
use crate::library::{Error, ProviderKind};

/// Most screenshots kept per game.
pub const MAX_SCREENSHOTS: usize = 3;

/// What the game page shows. Missing facts are empty or `None` so the UI can hide them.
/// Cached and returned with screenshot URLs; the `screenshots` command turns those into local files.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    pub description: String,
    pub genres: Vec<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub released: Option<String>,
    pub screenshots: Vec<String>,
    /// Whether the Steam store page lists DLC; `None` when unknown (other sources, older caches).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_dlc: Option<bool>,
}

/// The `info` cache kind for a game's details.
pub const DETAILS: &str = "details";

/// Steam `appdetails` response for `appid`. `None` when the store has no page for it.
pub fn parse_steam_appdetails(json: &str, appid: &str) -> Option<GameInfo> {
    let root: Value = serde_json::from_str(json).ok()?;
    let entry = &root[appid];
    if entry["success"] != true {
        return None;
    }
    let data = &entry["data"];
    let short = data["short_description"]
        .as_str()
        .map(strip_html)
        .unwrap_or_default();
    let description = if short.is_empty() {
        data["about_the_game"]
            .as_str()
            .map(strip_html)
            .unwrap_or_default()
    } else {
        short
    };
    let first = |field: &str| {
        data[field]
            .as_array()
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    Some(GameInfo {
        description,
        genres: data["genres"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| g["description"].as_str())
            .map(str::to_owned)
            .collect(),
        developer: first("developers"),
        publisher: first("publishers"),
        released: data["release_date"]["date"]
            .as_str()
            .filter(|d| !d.is_empty())
            .map(str::to_owned),
        screenshots: data["screenshots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["path_thumbnail"].as_str())
            .take(MAX_SCREENSHOTS)
            .map(str::to_owned)
            .collect(),
        has_dlc: Some(data["dlc"].as_array().is_some_and(|d| !d.is_empty())),
    })
}

/// A GamesDB localized field (`{"en-US": .., "*": ..}`): English, else the default.
fn english(v: &Value) -> Option<&str> {
    v["en-US"]
        .as_str()
        .or_else(|| v["*"].as_str())
        .filter(|s| !s.is_empty())
}

/// GOG GamesDB game object (inside an `external_releases` answer). `None` when it has nothing to show.
pub fn parse_gamesdb_info(json: &str) -> Option<GameInfo> {
    let root: Value = serde_json::from_str(json).ok()?;
    let game = &root["game"];
    let first = |field: &str| {
        game[field]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|p| p["name"].as_str())
            .map(str::to_owned)
    };
    let parsed = GameInfo {
        description: english(&game["summary"])
            .map(strip_html)
            .unwrap_or_default(),
        genres: game["genres"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| english(&g["name"]))
            .map(str::to_owned)
            .collect(),
        developer: first("developers"),
        publisher: first("publishers"),
        released: game["first_release_date"]
            .as_str()
            .and_then(|d| d.split('T').next())
            .filter(|d| !d.is_empty())
            .map(str::to_owned),
        // An empty formatter is the original image; the art cache stores it at any size.
        screenshots: game["screenshots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["url_format"].as_str())
            .take(MAX_SCREENSHOTS)
            .map(|u| u.replace("{formatter}", "").replace("{ext}", "jpg"))
            .collect(),
        has_dlc: None,
    };
    (parsed != GameInfo::default()).then_some(parsed)
}

/// `base` with each of its empty fields taken from `extra`.
pub fn fill_missing(base: GameInfo, extra: GameInfo) -> GameInfo {
    GameInfo {
        description: if base.description.is_empty() {
            extra.description
        } else {
            base.description
        },
        genres: if base.genres.is_empty() {
            extra.genres
        } else {
            base.genres
        },
        developer: base.developer.or(extra.developer),
        publisher: base.publisher.or(extra.publisher),
        released: base.released.or(extra.released),
        screenshots: if base.screenshots.is_empty() {
            extra.screenshots
        } else {
            base.screenshots
        },
        has_dlc: base.has_dlc,
    }
}

/// The `info` cache kind for GamesDB details; `null` is a remembered miss.
pub const DETAILS_GAMESDB: &str = "details-gamesdb";
const GAMESDB_FOUND_SECS: i64 = 30 * 24 * 60 * 60;
const GAMESDB_MISS_SECS: i64 = 7 * 24 * 60 * 60;

/// Cached GamesDB details: a find lasts 30 days, a miss (`null`) 7; else a fetch whose result is cached.
pub fn gamesdb_cached_or_fetch(
    conn: Option<&Connection>,
    key: &str,
    now_secs: i64,
    fetch: impl FnOnce() -> Option<String>,
) -> Option<String> {
    let cached = |age| {
        conn.and_then(|c| {
            db::cached_info(c, key, DETAILS_GAMESDB, age, now_secs)
                .ok()
                .flatten()
        })
    };
    if let Some(json) = cached(GAMESDB_FOUND_SECS)
        && (json != "null" || cached(GAMESDB_MISS_SECS).is_some())
    {
        return Some(json);
    }
    let json = fetch()?;
    if let Some(c) = conn {
        _ = db::put_info(c, key, DETAILS_GAMESDB, &json, now_secs);
    }
    Some(json)
}

/// Epic catalog item (the `metadata` of a `legendary list --json` entry).
/// `None` when it carries nothing worth showing.
// shortcut: the catalog has no genres, publisher or screenshots (keyImages is box art only); `game_info` fills them from the Steam twin or GamesDB.
pub fn parse_epic_info(info: &Value) -> Option<GameInfo> {
    if !info.is_object() {
        return None;
    }
    let text = |field: &str| {
        info[field]
            .as_str()
            .map(strip_html)
            .filter(|s| !s.is_empty())
    };
    let title = text("title");
    // Most entries repeat the title as their description; that is no description.
    let description = text("description")
        .filter(|d| Some(d) != title.as_ref())
        .unwrap_or_default();
    // `creationDate` is when the catalog entry was made, not a release date, so it isn't used.
    let released = info["releaseInfo"]
        .as_array()
        .and_then(|r| r.iter().find_map(|i| i["dateAdded"].as_str()))
        .and_then(|d| d.split('T').next())
        .filter(|d| !d.is_empty())
        .map(str::to_owned);
    let parsed = GameInfo {
        description,
        genres: Vec::new(),
        developer: text("developer"),
        publisher: None,
        released,
        screenshots: Vec::new(),
        has_dlc: None,
    };
    let empty =
        parsed.description.is_empty() && parsed.developer.is_none() && parsed.released.is_none();
    (!empty).then_some(parsed)
}

/// Details JSON for every game in a `legendary list --json` answer, as `(game key, json)`.
/// A game with nothing to show gets `null`, so it is cached as "nothing" rather than asked again.
pub fn epic_details(list: &Value) -> Vec<(String, String)> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            let key = ProviderKind::Epic.key(g["app_name"].as_str()?);
            Some((
                key,
                serde_json::to_string(&parse_epic_info(&g["metadata"])).ok()?,
            ))
        })
        .collect()
}

/// Caches every `(key, details json)` in one transaction.
pub fn remember_all(
    conn: &Connection,
    all: &[(String, String)],
    now_secs: i64,
) -> Result<(), Error> {
    let tx = conn.unchecked_transaction()?;
    for (key, json) in all {
        db::put_info(&tx, key, DETAILS, json, now_secs)?;
    }
    tx.commit()?;
    Ok(())
}

/// Store markup as plain text: tags dropped, `<br>` and block ends become line breaks,
/// common entities decoded, blank lines removed.
pub fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        // Only `<name`, `</name` and `<!` start a tag; "2 < 3" is text.
        let starts_tag =
            after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        let Some(close) = after.find('>').filter(|_| starts_tag) else {
            out.push('<');
            rest = after;
            continue;
        };
        let closing = after.starts_with('/');
        let name: String = after[..close]
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if name == "br"
            || (closing
                && matches!(
                    name.as_str(),
                    "p" | "li" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                ))
        {
            out.push('\n');
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    // `&amp;` last, so "&amp;lt;" ends up as the text "&lt;".
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    decoded
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Cached JSON for `(key, kind)` when fresh, else a fetch whose result is cached.
/// A failed fetch writes nothing, so the next visit tries again.
pub fn cached_or_fetch(
    conn: Option<&Connection>,
    key: &str,
    kind: &str,
    max_age_secs: i64,
    now_secs: i64,
    fetch: impl FnOnce() -> Option<String>,
) -> Option<String> {
    if let Some(cached) = conn.and_then(|c| {
        db::cached_info(c, key, kind, max_age_secs, now_secs)
            .ok()
            .flatten()
    }) {
        return Some(cached);
    }
    let json = fetch()?;
    if let Some(c) = conn {
        _ = db::put_info(c, key, kind, &json, now_secs);
    }
    Some(json)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn steam_fixture() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/steam/appdetails-447040.json");
        std::fs::read_to_string(path).unwrap()
    }

    fn temp_db(name: &str) -> (Connection, PathBuf) {
        let dir = std::env::temp_dir().join(format!("launcher-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (db::open(&dir.join("t.sqlite")).unwrap(), dir)
    }

    #[test]
    fn parses_steam_appdetails_fixture() {
        let info = parse_steam_appdetails(&steam_fixture(), "447040").unwrap();
        assert!(info.description.starts_with("Welcome to San Francisco."));
        assert_eq!(info.genres, vec!["Action", "Adventure"]);
        assert_eq!(info.developer.as_deref(), Some("Ubisoft"));
        assert_eq!(info.publisher.as_deref(), Some("Ubisoft"));
        assert_eq!(info.released.as_deref(), Some("28 Nov, 2016"));
        // The fixture has 5 screenshots; only 3 are kept.
        assert_eq!(info.screenshots.len(), 3);
        assert!(info.screenshots[0].starts_with("https://"));
        // Its `dlc` array lists 30 app ids.
        assert_eq!(info.has_dlc, Some(true));
        let no_dlc = r#"{"1":{"success":true,"data":{"short_description":"x"}}}"#;
        assert_eq!(
            parse_steam_appdetails(no_dlc, "1").unwrap().has_dlc,
            Some(false)
        );
    }

    #[test]
    fn steam_failure_is_none() {
        assert_eq!(
            parse_steam_appdetails(r#"{"447040":{"success":false}}"#, "447040"),
            None
        );
        assert_eq!(parse_steam_appdetails("not json", "447040"), None);
        assert_eq!(parse_steam_appdetails(&steam_fixture(), "1"), None);
    }

    #[test]
    fn steam_falls_back_to_about_the_game() {
        let json = r#"{"1":{"success":true,"data":{"short_description":"","about_the_game":"<p>Hi &amp; bye</p>"}}}"#;
        let info = parse_steam_appdetails(json, "1").unwrap();
        assert_eq!(info.description, "Hi & bye");
        assert!(info.genres.is_empty() && info.developer.is_none() && info.screenshots.is_empty());
    }

    fn epic_fixture() -> Value {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legendary/list-entry.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn parses_epic_catalog_item() {
        let info = parse_epic_info(&epic_fixture()).unwrap();
        assert!(
            info.description
                .starts_with("A deep, interactive adventure about love and loss")
        );
        assert_eq!(info.developer.as_deref(), Some("505 Games"));
        assert_eq!(info.released.as_deref(), Some("2019-06-26"));
        // The catalog's keyImages are box art only.
        assert!(info.screenshots.is_empty() && info.genres.is_empty() && info.publisher.is_none());
    }

    #[test]
    fn epic_description_equal_to_title_is_empty() {
        let mut item = epic_fixture();
        item["description"] = item["title"].clone();
        let info = parse_epic_info(&item).unwrap();
        assert_eq!(info.description, "");
        assert_eq!(info.developer.as_deref(), Some("505 Games"));
    }

    #[test]
    fn epic_item_without_details_is_none() {
        let item: Value = serde_json::from_str(
            r#"{"id":"4fe","title":"Fortnite","description":"Fortnite","creationDate":"2020-01-01T00:00:00.000Z","keyImages":[]}"#,
        )
        .unwrap();
        assert_eq!(parse_epic_info(&item), None);
        assert_eq!(parse_epic_info(&Value::Null), None);
    }

    #[test]
    fn one_epic_list_caches_every_game() {
        let (conn, dir) = temp_db("info-epic-all");
        let list = serde_json::json!([
            {"app_name": "Pumpkin", "metadata": epic_fixture()},
            {"app_name": "Fortnite", "metadata": {"title": "Fortnite", "description": "Fortnite"}},
            {"app_title": "no app_name, skipped"},
        ]);
        let all = epic_details(&list);
        assert_eq!(all.len(), 2);
        remember_all(&conn, &all, 1_000).unwrap();

        let shown = db::cached_info(&conn, "epic-Pumpkin", "details", 100, 1_050)
            .unwrap()
            .unwrap();
        let info: Option<GameInfo> = serde_json::from_str(&shown).unwrap();
        assert_eq!(info.unwrap().developer.as_deref(), Some("505 Games"));
        // Nothing to show is remembered as null, so the next visit doesn't run `legendary list` again.
        assert_eq!(
            db::cached_info(&conn, "epic-Fortnite", "details", 100, 1_050)
                .unwrap()
                .as_deref(),
            Some("null")
        );
        let again = cached_or_fetch(Some(&conn), "epic-Fortnite", DETAILS, 100, 1_050, || {
            panic!("must not fetch")
        });
        assert_eq!(again.as_deref(), Some("null"));
        // The 7-day lifetime still applies.
        assert_eq!(
            db::cached_info(&conn, "epic-Fortnite", "details", 100, 1_200).unwrap(),
            None
        );
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn gamesdb_info_reads_summary_genres_and_screenshots() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gamesdb/pumpkin.json");
        let info = parse_gamesdb_info(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(
            info.description
                .starts_with("A deep, interactive adventure about love and loss")
        );
        assert_eq!(info.genres, vec!["Role-playing (RPG)", "Adventure"]);
        assert_eq!(info.developer.as_deref(), Some("Ovosonico"));
        assert_eq!(info.publisher.as_deref(), Some("505 Games"));
        assert_eq!(info.released.as_deref(), Some("2017-08-31"));
        // The fixture has 4 screenshots; only 3 are kept, with the template filled in.
        assert_eq!(info.screenshots.len(), 3);
        assert!(info.screenshots[0].starts_with("https://images.gog.com/f3e96fe8"));
        assert!(info.screenshots[0].ends_with(".jpg?namespace=gamesdb"));
        assert!(!info.screenshots[0].contains('{'));
        assert_eq!(parse_gamesdb_info(r#"{"game":{"id":"1"}}"#), None);
        assert_eq!(parse_gamesdb_info("nope"), None);
    }

    #[test]
    fn fill_missing_keeps_base_and_fills_gaps() {
        let base = GameInfo {
            description: "mine".into(),
            developer: Some("Me".into()),
            ..GameInfo::default()
        };
        let extra = GameInfo {
            description: "theirs".into(),
            genres: vec!["RPG".into()],
            developer: Some("Them".into()),
            publisher: Some("Pub".into()),
            released: Some("2020-01-01".into()),
            screenshots: vec!["u".into()],
            has_dlc: Some(true),
        };
        let got = fill_missing(base, extra);
        assert_eq!(got.description, "mine");
        assert_eq!(got.developer.as_deref(), Some("Me"));
        assert_eq!(got.genres, vec!["RPG"]);
        assert_eq!(got.publisher.as_deref(), Some("Pub"));
        assert_eq!(got.released.as_deref(), Some("2020-01-01"));
        assert_eq!(got.screenshots, vec!["u"]);
    }

    #[test]
    fn gamesdb_details_cache_keeps_finds_longer_than_misses() {
        let (conn, dir) = temp_db("info-gamesdb");
        let day = 24 * 60 * 60;
        gamesdb_cached_or_fetch(Some(&conn), "epic-A", 0, || Some("{}".into()));
        gamesdb_cached_or_fetch(Some(&conn), "epic-B", 0, || Some("null".into()));
        let again =
            |key, at| gamesdb_cached_or_fetch(Some(&conn), key, at, || Some("new".into())).unwrap();
        assert_eq!(again("epic-A", 20 * day), "{}");
        assert_eq!(again("epic-B", 6 * day), "null");
        assert_eq!(again("epic-B", 8 * day), "new");
        assert_eq!(again("epic-A", 31 * day), "new");
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn strips_html_to_plain_text() {
        assert_eq!(strip_html("<p>A &amp; B</p><br>C"), "A & B\nC");
        assert_eq!(
            strip_html("1 &lt; 2 &quot;x&quot; it&#39;s&nbsp;ok"),
            "1 < 2 \"x\" it's ok"
        );
        assert_eq!(strip_html("a<br/>\n\n<br />b"), "a\nb");
        assert_eq!(strip_html("&amp;lt;"), "&lt;");
        assert_eq!(strip_html("2 < 3 and <b>x</b>"), "2 < 3 and x");
        assert_eq!(strip_html(""), "");
    }

    #[test]
    fn failed_fetch_is_not_cached() {
        let (conn, dir) = temp_db("info-failed");
        assert_eq!(
            cached_or_fetch(Some(&conn), "steam-1", DETAILS, 100, 1_000, || None),
            None
        );
        assert_eq!(
            db::cached_info(&conn, "steam-1", DETAILS, 100, 1_000).unwrap(),
            None
        );
        // A successful one is.
        let got = cached_or_fetch(Some(&conn), "steam-1", DETAILS, 100, 1_000, || {
            Some("{}".into())
        });
        assert_eq!(got.as_deref(), Some("{}"));
        assert_eq!(
            db::cached_info(&conn, "steam-1", DETAILS, 100, 1_000)
                .unwrap()
                .as_deref(),
            Some("{}")
        );
        // Without a database it still answers, it just can't remember.
        assert_eq!(
            cached_or_fetch(None, "steam-1", DETAILS, 100, 1_000, || Some("x".into())).as_deref(),
            Some("x")
        );
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fresh_cache_is_used_without_fetching_and_kinds_stay_apart() {
        let (conn, dir) = temp_db("info-cached");
        db::put_info(&conn, "epic-A", DETAILS, "old", 1_000).unwrap();
        let got = cached_or_fetch(Some(&conn), "epic-A", DETAILS, 100, 1_050, || {
            panic!("must not fetch")
        });
        assert_eq!(got.as_deref(), Some("old"));
        // Another kind for the same game is its own entry (the download size).
        let size = cached_or_fetch(Some(&conn), "epic-A", "size", 100, 1_050, || {
            Some("42".into())
        });
        assert_eq!(size.as_deref(), Some("42"));
        assert_eq!(
            db::cached_info(&conn, "epic-A", "size", 100, 1_050)
                .unwrap()
                .as_deref(),
            Some("42")
        );
        assert_eq!(
            db::cached_info(&conn, "epic-A", DETAILS, 100, 1_050)
                .unwrap()
                .as_deref(),
            Some("old")
        );
        // Too old: fetched again.
        let fresh = cached_or_fetch(Some(&conn), "epic-A", DETAILS, 100, 1_200, || {
            Some("new".into())
        });
        assert_eq!(fresh.as_deref(), Some("new"));
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
