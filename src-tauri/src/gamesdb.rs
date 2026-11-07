//! GOG GamesDB (public, keyless): which Steam release is the same game as an Epic or GOG one, and a GOG
//! game's portrait cover. Matched by store ID, never by title. A twin is only taken when the answer is unambiguous.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::cache::db;
use crate::library::percent_encode;

const URL: &str = "https://gamesdb.gog.com/platforms";

/// `info` table kind of a cached answer.
pub const TWIN: &str = "twin3";
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
    vertical_cover: Option<ImageRef>,
}

#[derive(Deserialize)]
struct ImageRef {
    url_format: String,
}

#[derive(Deserialize)]
struct Release {
    platform_id: String,
    external_id: String,
}

/// Every distinct Steam app id of the game's Steam releases (`123` or `steam_123`), ascending.
/// GamesDB often lists more than the game: a test server, demo, soundtrack or edition.
pub fn parse_steam_apps(json: &str) -> Result<Vec<u32>, String> {
    let r: Response = serde_json::from_str(json).map_err(|e| format!("gamesdb: {e}"))?;
    let mut ids = BTreeSet::new();
    for rel in r.game.releases.iter().filter(|r| r.platform_id == "steam") {
        let id = rel.external_id.strip_prefix("steam_").unwrap_or(&rel.external_id);
        match id.parse::<u32>() {
            Ok(n) if id.bytes().all(|b| b.is_ascii_digit()) => ids.insert(n),
            _ => continue,
        };
    }
    Ok(ids.into_iter().collect())
}

/// The game's portrait cover at its original size; `None` when GamesDB has none.
pub fn parse_cover(json: &str) -> Option<String> {
    let r: Response = serde_json::from_str(json).ok()?;
    Some(r.game.vertical_cover?.url_format.replace("{formatter}", "").replace("{ext}", "jpg"))
}

/// The twin among the listed Steam apps: the only one in `owned` (the owner's Steam games), else the
/// only one listed. That last one merges only if owned, but still gives links and How long to beat.
pub fn resolve(apps: &[u32], owned: &HashSet<u32>) -> Option<u32> {
    let mut hits = apps.iter().filter(|id| owned.contains(id));
    match (hits.next(), hits.next(), apps) {
        (Some(&id), None, _) | (None, None, &[id]) => Some(id),
        _ => None,
    }
}

/// The app ids of the Steam games in `games` (installed or owned).
pub fn steam_ids(games: &[crate::library::Game]) -> HashSet<u32> {
    games
        .iter()
        .filter(|g| g.provider == crate::library::ProviderKind::Steam)
        .filter_map(|g| g.external_id.parse().ok())
        .collect()
}

/// `resolve` for every Epic or GOG game key with a list.
pub fn resolve_all(lists: &HashMap<String, Vec<u32>>, owned: &HashSet<u32>) -> HashMap<String, u32> {
    lists.iter().filter_map(|(key, apps)| Some((key.clone(), resolve(apps, owned)?))).collect()
}

/// Asks GamesDB for the Steam apps and the cover of an Epic or GOG copy key. A 404 means GamesDB doesn't know the game.
pub async fn look_up(http: &reqwest::Client, key: &str) -> Result<(Vec<u32>, Option<String>), String> {
    match fetch(http, key).await? {
        Some(body) => Ok((parse_steam_apps(&body)?, parse_cover(&body))),
        None => Ok((Vec::new(), None)),
    }
}

/// The GamesDB release URL of an Epic or GOG copy key: its store is GamesDB's platform, its id the release's.
fn release_url(key: &str) -> Option<String> {
    let (platform, id) = key.split_once('-').filter(|(p, id)| matches!(*p, "epic" | "gog") && !id.is_empty())?;
    Some(format!("{URL}/{platform}/external_releases/{}", percent_encode(id)))
}

/// GamesDB's answer body for an Epic or GOG copy key; `None` when it doesn't know the game (404).
pub async fn fetch(http: &reqwest::Client, key: &str) -> Result<Option<String>, String> {
    let url = release_url(key).ok_or_else(|| format!("gamesdb: no platform for {key}"))?;
    let resp = http.get(url).send().await.map_err(|e| format!("{UNREACHABLE}: {e}"))?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(format!("gamesdb: HTTP {status}"));
    }
    let body = resp.text().await.map_err(|e| format!("{UNREACHABLE}: {e}"))?;
    Ok(Some(body))
}

#[derive(Serialize, Deserialize)]
struct Stored {
    steam: Vec<u32>,
    /// GamesDB's portrait cover; GOG copies use it.
    #[serde(default)]
    cover: Option<String>,
}

/// The cached answer: `Some(vec![])` is a remembered "no Steam app", `None` means ask GamesDB.
pub fn cached(conn: &Connection, key: &str, now_secs: i64) -> Option<Vec<u32>> {
    let json = db::cached_info(conn, key, TWIN, TWIN_MAX_AGE_SECS, now_secs).ok().flatten()?;
    serde_json::from_str::<Stored>(&json).ok().map(|s| s.steam)
}

pub fn remember(conn: &Connection, key: &str, apps: &[u32], cover: Option<&str>, now_secs: i64) {
    if let Ok(json) = serde_json::to_string(&Stored { steam: apps.to_vec(), cover: cover.map(str::to_owned) }) {
        _ = db::put_info(conn, key, TWIN, &json, now_secs);
    }
}

/// Every fresh cached answer with its game key.
fn all_stored(conn: &Connection, now_secs: i64) -> Vec<(String, Stored)> {
    let Ok(mut stmt) = conn.prepare("SELECT key, json FROM info WHERE kind = ?1 AND fetched >= ?2") else {
        return Vec::new();
    };
    let rows = stmt.query_map(rusqlite::params![TWIN, now_secs - TWIN_MAX_AGE_SECS], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    });
    let Ok(rows) = rows else { return Vec::new() };
    rows.flatten().filter_map(|(key, json)| Some((key, serde_json::from_str::<Stored>(&json).ok()?))).collect()
}

/// Every fresh cached Steam app list by Epic or GOG game key.
pub fn all_cached(conn: &Connection, now_secs: i64) -> HashMap<String, Vec<u32>> {
    all_stored(conn, now_secs).into_iter().map(|(key, s)| (key, s.steam)).collect()
}

/// Every fresh cached cover of a GOG game, by its key.
pub fn covers(conn: &Connection, now_secs: i64) -> HashMap<String, String> {
    all_stored(conn, now_secs)
        .into_iter()
        .filter(|(key, _)| key.starts_with("gog-"))
        .filter_map(|(key, s)| Some((key, s.cover?)))
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

    fn owned(ids: &[u32]) -> HashSet<u32> {
        ids.iter().copied().collect()
    }

    #[test]
    fn pumpkin_lists_last_day_of_june() {
        assert_eq!(parse_steam_apps(&fixture("pumpkin.json")).unwrap(), [635320]);
    }

    #[test]
    fn no_steam_release_lists_nothing() {
        assert!(parse_steam_apps(&releases(&[("gog", "1"), ("epic", "x")])).unwrap().is_empty());
        assert!(parse_steam_apps("nope").is_err());
    }

    #[test]
    fn duplicated_steam_row_is_one_app() {
        assert_eq!(parse_steam_apps(&fixture("condor.json")).unwrap(), [202750]);
    }

    #[test]
    fn different_steam_apps_are_all_listed() {
        let rows = releases(&[("steam", "2"), ("steam", "steam_1"), ("steam", "1")]);
        assert_eq!(parse_steam_apps(&rows).unwrap(), [1, 2]);
        assert_eq!(parse_steam_apps(&fixture("brill.json")).unwrap(), [381210, 471230]);
        assert_eq!(parse_steam_apps(&fixture("heather.json")).unwrap(), [100003, 1174180]);
    }

    #[test]
    fn non_numeric_steam_id_is_skipped() {
        assert!(parse_steam_apps(&releases(&[("steam", "abc")])).unwrap().is_empty());
    }

    #[test]
    fn several_steam_apps_resolve_to_the_owned_one() {
        let brill = parse_steam_apps(&fixture("brill.json")).unwrap();
        assert_eq!(resolve(&brill, &owned(&[70, 381210])), Some(381210));
        let heather = parse_steam_apps(&fixture("heather.json")).unwrap();
        assert_eq!(resolve(&heather, &owned(&[1174180])), Some(1174180));
        // Owning none of several gives nothing to link to either.
        assert_eq!(resolve(&brill, &owned(&[70])), None);
    }

    #[test]
    fn several_owned_steam_apps_give_no_twin() {
        assert_eq!(resolve(&[381210, 471230], &owned(&[381210, 471230])), None);
    }

    #[test]
    fn single_steam_app_still_works() {
        assert_eq!(resolve(&[635320], &owned(&[635320])), Some(635320));
        // Not owned: still the twin, for links and How long to beat; `entries::group` won't merge it.
        assert_eq!(resolve(&[635320], &owned(&[])), Some(635320));
        assert_eq!(resolve(&[], &owned(&[635320])), None);
        let lists = HashMap::from([("epic-A".to_string(), vec![635320]), ("epic-B".to_string(), vec![])]);
        assert_eq!(resolve_all(&lists, &owned(&[])), HashMap::from([("epic-A".to_string(), 635320)]));
    }

    #[test]
    fn twin_cache_keeps_misses_and_expires() {
        let conn = temp_db("cache");
        let day = 24 * 60 * 60;
        assert_eq!(cached(&conn, "epic-a", 1000), None);
        remember(&conn, "epic-a", &[], None, 1000);
        remember(&conn, "epic-b", &[381210, 471230], None, 1000);
        assert_eq!(cached(&conn, "epic-a", 1000 + day), Some(vec![]));
        assert_eq!(cached(&conn, "epic-b", 1000 + day), Some(vec![381210, 471230]));
        let all = all_cached(&conn, 1000 + day);
        assert_eq!(all.len(), 2);
        assert_eq!(all["epic-b"], [381210, 471230]);
        assert_eq!(cached(&conn, "epic-a", 1000 + 31 * day), None);
        assert!(all_cached(&conn, 1000 + 31 * day).is_empty());
    }

    /// Lists same-title Steam/Epic pairs that still aren't one card, with why. Reads the database
    /// (`LAUNCHER_DB`, a copy, or else the app's `library.sqlite`) read-only, and never merges anything.
    /// A pair whose Epic game has no fresh cached `twin3` list is asked from GamesDB, at most 15 times.
    /// `LAUNCHER_DB=<copy> cargo test leftover_same_title_pairs -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn leftover_same_title_pairs() {
        use crate::library::ProviderKind;
        let path = std::env::var_os("LAUNCHER_DB").map_or_else(|| crate::cache::Dirs::new().db, Into::into);
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let games = db::load(&conn).unwrap();
        let mut lists = all_cached(&conn, now);
        let steam = steam_ids(&games);
        let pairs: Vec<(usize, usize)> = games
            .iter()
            .enumerate()
            .filter(|(_, g)| g.provider == ProviderKind::Epic)
            .flat_map(|(e, eg)| {
                games.iter().enumerate().filter(move |(_, sg)| {
                    sg.provider == ProviderKind::Steam && sg.title.trim().eq_ignore_ascii_case(eg.title.trim())
                }).map(move |(s, _)| (e, s))
            })
            .collect();
        let apart = |lists: &HashMap<String, Vec<u32>>| -> Vec<(usize, usize)> {
            let entries = crate::entries::group(&games, &resolve_all(lists, &steam));
            let together = |e, s| entries.iter().any(|en| en.copies.contains(&e) && en.copies.contains(&s));
            pairs.iter().copied().filter(|&(e, s)| !together(e, s)).collect()
        };
        let http = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().unwrap();
        let mut asked = 0;
        for (e, _) in apart(&lists) {
            let key = games[e].key();
            if lists.contains_key(&key) || asked == 15 {
                continue;
            }
            if asked > 0 {
                std::thread::sleep(TWIN_GAP);
            }
            asked += 1;
            match tauri::async_runtime::block_on(look_up(&http, &key)) {
                Ok((apps, _)) => _ = lists.insert(key, apps),
                Err(err) => println!("{key}: {err}"),
            }
        }
        let twins = resolve_all(&lists, &steam);
        let left = apart(&lists);
        println!("{} same-title pairs, {} apart, {asked} asked from GamesDB", pairs.len(), left.len());
        for (e, s) in left {
            let (eg, sg) = (&games[e], &games[s]);
            let id: u32 = sg.external_id.parse().unwrap();
            let why = match (lists.get(&eg.key()), twins.get(&eg.key())) {
                (None, _) => "no GamesDB answer".to_string(),
                (Some(apps), _) if apps.is_empty() => "GamesDB lists no Steam app (or doesn't know the game)".to_string(),
                (Some(apps), _) if !apps.contains(&id) => format!("GamesDB lists {apps:?}, not {id}"),
                (Some(apps), None) => format!("several listed apps owned: {apps:?}"),
                (Some(_), Some(&t)) if t == id => "another Epic game claims the same app".to_string(),
                (Some(_), Some(t)) => format!("resolved to {t}"),
            };
            println!("{} | {} | {} | {why}", eg.title, eg.key(), sg.key());
        }
    }

    #[test]
    fn gog_release_lists_its_steam_twin_and_cover() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gog/gdb-1425609977.json");
        let body = std::fs::read_to_string(path).unwrap();
        assert_eq!(parse_steam_apps(&body).unwrap(), [635320]);
        assert_eq!(
            parse_cover(&body).as_deref(),
            Some("https://images.gog.com/279e02c9da2d01c299b54fcdab89ccad3a88d46c05e3dcdf904f13611895ed67.jpg?namespace=gamesdb")
        );
        assert_eq!(parse_cover(&fixture("pumpkin.json")), None);
        assert_eq!(parse_cover("nope"), None);
    }

    #[test]
    fn release_url_takes_the_platform_from_the_key() {
        let url = |key| release_url(key);
        assert_eq!(url("epic-Pumpkin").as_deref(), Some("https://gamesdb.gog.com/platforms/epic/external_releases/Pumpkin"));
        assert_eq!(url("gog-1425609977").as_deref(), Some("https://gamesdb.gog.com/platforms/gog/external_releases/1425609977"));
        assert_eq!(url("epic-a b").as_deref(), Some("https://gamesdb.gog.com/platforms/epic/external_releases/a%20b"));
        assert_eq!(url("steam-70"), None);
        assert_eq!(url("custom-x"), None);
        assert_eq!(url("gog-"), None);
    }

    #[test]
    fn gog_covers_come_from_the_twin_cache() {
        let conn = temp_db("covers");
        remember(&conn, "gog-1", &[635320], Some("https://images.gog.com/c.jpg"), 1000);
        remember(&conn, "gog-2", &[], None, 1000);
        remember(&conn, "epic-a", &[], Some("https://images.gog.com/e.jpg"), 1000);
        assert_eq!(covers(&conn, 1000), HashMap::from([("gog-1".to_string(), "https://images.gog.com/c.jpg".to_string())]));
        assert_eq!(all_cached(&conn, 1000)["gog-1"], [635320]);
        // An answer stored before covers were kept still reads.
        db::put_info(&conn, "gog-3", TWIN, r#"{"steam":[70]}"#, 1000).unwrap();
        assert_eq!(cached(&conn, "gog-3", 1000), Some(vec![70]));
    }

    #[test]
    fn old_single_answers_are_asked_again() {
        let conn = temp_db("old");
        db::put_info(&conn, "epic-Brill", "twin2", r#"{"steam":null}"#, 1000).unwrap();
        assert_eq!(cached(&conn, "epic-Brill", 1000), None);
        assert!(all_cached(&conn, 1000).is_empty());
    }
}
