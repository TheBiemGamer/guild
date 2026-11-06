//! Store details for the game page: description, genres, developer, screenshots.
//! Steam's `appdetails` and Epic's catalog metadata are parsed into one plain-text shape.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cache::db;

/// Most screenshots kept per game.
const MAX_SCREENSHOTS: usize = 3;

/// What the game page shows. Missing facts are empty or `None` so the UI can hide them.
/// Cached with screenshot URLs; the command swaps those for local files before returning.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    pub description: String,
    pub genres: Vec<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub released: Option<String>,
    pub screenshots: Vec<String>,
}

/// The same shape as sent to the UI (screenshots are local paths there).
pub type GameInfoView = GameInfo;

/// Steam `appdetails` response for `appid`. `None` when the store has no page for it.
pub fn parse_steam_appdetails(json: &str, appid: &str) -> Option<GameInfo> {
    let root: Value = serde_json::from_str(json).ok()?;
    let entry = &root[appid];
    if entry["success"] != true {
        return None;
    }
    let data = &entry["data"];
    let short = data["short_description"].as_str().map(strip_html).unwrap_or_default();
    let description = if short.is_empty() {
        data["about_the_game"].as_str().map(strip_html).unwrap_or_default()
    } else {
        short
    };
    let first = |field: &str| data[field].as_array().and_then(|a| a.first()).and_then(Value::as_str).map(str::to_owned);
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
        released: data["release_date"]["date"].as_str().filter(|d| !d.is_empty()).map(str::to_owned),
        screenshots: data["screenshots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["path_thumbnail"].as_str())
            .take(MAX_SCREENSHOTS)
            .map(str::to_owned)
            .collect(),
    })
}

/// Epic catalog item (the `metadata` of a `legendary list --json` entry).
/// `None` when it carries nothing worth showing.
// shortcut: the catalog has no genres, publisher or screenshots (keyImages is box art only), so Epic games show none.
pub fn parse_epic_info(info: &Value) -> Option<GameInfo> {
    if !info.is_object() {
        return None;
    }
    let text = |field: &str| info[field].as_str().map(strip_html).filter(|s| !s.is_empty());
    let title = text("title");
    // Most entries repeat the title as their description; that is no description.
    let description = text("description").filter(|d| Some(d) != title.as_ref()).unwrap_or_default();
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
    };
    let empty = parsed.description.is_empty() && parsed.developer.is_none() && parsed.released.is_none();
    (!empty).then_some(parsed)
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
        let starts_tag = after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
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
        if name == "br" || (closing && matches!(name.as_str(), "p" | "li" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6")) {
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
    decoded.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

/// The cached answer if there is one, else whatever `fetch` returns.
pub fn pick(cached: Option<String>, fetch: impl FnOnce() -> Option<String>) -> Option<String> {
    cached.or_else(fetch)
}

/// Cached JSON for `(key, "details")` when fresh, else a fetch whose result is cached.
/// A failed fetch writes nothing, so the next visit tries again.
pub fn cached_or_fetch(
    conn: Option<&Connection>,
    key: &str,
    max_age_secs: i64,
    now_secs: i64,
    fetch: impl FnOnce() -> Option<String>,
) -> Option<String> {
    let cached = conn.and_then(|c| db::cached_info(c, key, "details", max_age_secs, now_secs).ok().flatten());
    let mut fetched = false;
    let json = pick(cached, || {
        fetched = true;
        fetch()
    })?;
    if let Some(c) = conn.filter(|_| fetched) {
        _ = db::put_info(c, key, "details", &json, now_secs);
    }
    Some(json)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn steam_fixture() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/steam/appdetails-447040.json");
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
    }

    #[test]
    fn steam_failure_is_none() {
        assert_eq!(parse_steam_appdetails(r#"{"447040":{"success":false}}"#, "447040"), None);
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
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legendary/list-entry.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn parses_epic_catalog_item() {
        let info = parse_epic_info(&epic_fixture()).unwrap();
        assert!(info.description.starts_with("A deep, interactive adventure about love and loss"));
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
    fn strips_html_to_plain_text() {
        assert_eq!(strip_html("<p>A &amp; B</p><br>C"), "A & B\nC");
        assert_eq!(strip_html("1 &lt; 2 &quot;x&quot; it&#39;s&nbsp;ok"), "1 < 2 \"x\" it's ok");
        assert_eq!(strip_html("a<br/>\n\n<br />b"), "a\nb");
        assert_eq!(strip_html("&amp;lt;"), "&lt;");
        assert_eq!(strip_html("2 < 3 and <b>x</b>"), "2 < 3 and x");
        assert_eq!(strip_html(""), "");
    }

    #[test]
    fn failed_fetch_is_not_cached() {
        let (conn, dir) = temp_db("info-failed");
        assert_eq!(cached_or_fetch(Some(&conn), "steam-1", 100, 1_000, || None), None);
        assert_eq!(db::cached_info(&conn, "steam-1", "details", 100, 1_000).unwrap(), None);
        // A successful one is.
        let got = cached_or_fetch(Some(&conn), "steam-1", 100, 1_000, || Some("{}".into()));
        assert_eq!(got.as_deref(), Some("{}"));
        assert_eq!(db::cached_info(&conn, "steam-1", "details", 100, 1_000).unwrap().as_deref(), Some("{}"));
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cached_info_is_used_when_fetch_fails() {
        assert_eq!(pick(Some("cached".into()), || panic!("must not fetch")).as_deref(), Some("cached"));
        assert_eq!(pick(None, || Some("fresh".into())).as_deref(), Some("fresh"));
        assert_eq!(pick(None, || None), None);

        let (conn, dir) = temp_db("info-cached");
        db::put_info(&conn, "steam-1", "details", "old", 1_000).unwrap();
        let got = cached_or_fetch(Some(&conn), "steam-1", 100, 1_050, || panic!("must not fetch"));
        assert_eq!(got.as_deref(), Some("old"));
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
