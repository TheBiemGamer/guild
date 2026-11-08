//! Epic playtime: Epic's own figure from its playtime service (cached in the `info` table) plus the
//! sessions this app timed (the `playtime` table), which Legendary's launches never report to Epic. GOG copies
//! get GOG's own figure (`gog::PLAYTIME` rows, fetched by `commands::library`) plus their timed sessions, which are
//! never posted to GOG either. Games the owner added get their timed sessions the same way.
//! Legendary's access token is read from its session file only for the request; it never reaches a
//! log line, error string, Debug output or the frontend, and is never stored here.

use std::collections::HashMap;
use std::path::PathBuf;

use reqwest::StatusCode;
use rusqlite::Connection;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::cache::db;
use crate::library::{Game, ProviderKind, epic, gog};

/// The `info` row of Epic's last answer: minutes per artifact id.
const KEY: &str = "epic";
const KIND: &str = "epic-playtime";

/// The Epic Games Launcher's playtime service, `<URL>/<accountId>/all` (as Playnite's Epic library calls it).
const URL: &str =
    "https://library-service.live.use1a.on.epicgames.com/library/api/public/playtime/account";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    artifact_id: String,
    total_time: u64,
}

/// Epic's answer as whole minutes per artifact id, which is Legendary's app_name.
// shortcut: UNVERIFIED shape (a JSON array of `{accountId, artifactId, totalTime}`, totalTime in seconds), taken
// from Playnite's Epic library, not from a captured response. A shape that doesn't parse leaves own tracking.
pub fn parse(json: &str) -> Result<HashMap<String, u64>, String> {
    let items: Vec<Item> = serde_json::from_str(json).map_err(|e| format!("epic playtime: {e}"))?;
    Ok(items
        .into_iter()
        .map(|i| (i.artifact_id, i.total_time / 60))
        .collect())
}

/// The parts of Legendary's session the request needs. No `Debug`: it holds the access token.
#[derive(Deserialize)]
struct Session {
    access_token: String,
    account_id: String,
}

/// The Epic account's display name, for Settings.
#[derive(Deserialize)]
struct Account {
    #[serde(rename = "displayName")]
    display_name: String,
}

/// The logged-in Epic account's display name from Legendary's session file.
pub fn account_name() -> Option<String> {
    user_json::<Account>().map(|a| a.display_name)
}

fn session() -> Option<Session> {
    user_json()
}

/// Reads `user.json` in Legendary's config folder: `LEGENDARY_CONFIG_PATH`, else
/// `$XDG_CONFIG_HOME/legendary`, else `~/.config/legendary`, as Legendary's `lfs/lgndry.py` picks it.
fn user_json<T: DeserializeOwned>() -> Option<T> {
    let dir = match (
        std::env::var_os("LEGENDARY_CONFIG_PATH"),
        std::env::var_os("XDG_CONFIG_HOME"),
    ) {
        (Some(dir), _) => PathBuf::from(dir),
        (None, Some(config)) => PathBuf::from(config).join("legendary"),
        (None, None) => directories::BaseDirs::new()?
            .home_dir()
            .join(".config")
            .join("legendary"),
    };
    // Errors are dropped unread: serde's can quote the file.
    serde_json::from_slice(&std::fs::read(dir.join("user.json")).ok()?).ok()
}

/// Asks Epic's playtime service with Legendary's session. When Epic rejects the token (expired), `legendary
/// status` refreshes the session once and the request is repeated.
pub async fn fetch(http: &reqwest::Client) -> Result<HashMap<String, u64>, String> {
    let mut refreshed = false;
    loop {
        let session = session().ok_or("epic playtime: no Legendary session")?;
        // Errors are reduced to their kind: reqwest's carry the URL, which holds the account id.
        let response = http
            .get(format!("{URL}/{}/all", session.account_id))
            .bearer_auth(&session.access_token)
            .send()
            .await
            .map_err(|_| "epic playtime: unreachable")?;
        drop(session);
        match response.status() {
            StatusCode::UNAUTHORIZED if !refreshed => {
                refreshed = true;
                // Legendary refreshes an expired session when `status` logs in.
                tauri::async_runtime::spawn_blocking(epic::login_state)
                    .await
                    .map_err(|e| format!("epic playtime: {e}"))?;
            }
            status if status.is_success() => {
                return parse(
                    &response
                        .text()
                        .await
                        .map_err(|_| "epic playtime: unreachable")?,
                );
            }
            status => return Err(format!("epic playtime: HTTP {}", status.as_u16())),
        }
    }
}

/// Epic's last answer, at any age. None yet, or unreadable, is empty.
fn cached(conn: &Connection) -> HashMap<String, u64> {
    conn.query_row(
        "SELECT json FROM info WHERE key = ?1 AND kind = ?2",
        [KEY, KIND],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|json| serde_json::from_str(&json).ok())
    .unwrap_or_default()
}

pub fn remember(conn: &Connection, minutes: &HashMap<String, u64>, now_secs: i64) {
    let json = serde_json::to_string(minutes).expect("a map of numbers serializes");
    if let Err(e) = db::put_info(conn, KEY, KIND, &json, now_secs) {
        tracing::warn!("epic playtime save: {e}");
    }
}

/// GOG's last answers in minutes, by product id. A game GOG answered without minutes (`null`) is left out.
fn gog_cached(conn: &Connection) -> HashMap<String, u64> {
    let Ok(mut rows) = conn.prepare("SELECT key, json FROM info WHERE kind = ?1") else {
        return HashMap::new();
    };
    let read = rows.query_map([gog::PLAYTIME], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    });
    read.map(|rows| {
        rows.flatten()
            .filter_map(|(key, json)| {
                Some((key.strip_prefix("gog-")?.to_owned(), json.parse().ok()?))
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Applies the stored playtime (Epic's and GOG's last answers and the sessions timed here) to `games`.
/// Returns whether anything changed.
pub fn merge(conn: &Connection, games: &mut [Game]) -> bool {
    let own = db::own_totals(conn).unwrap_or_else(|e| {
        tracing::warn!("own playtime: {e}");
        HashMap::new()
    });
    apply(games, &cached(conn), &gog_cached(conn), &own)
}

/// An Epic copy gets Epic's minutes (`service`, by app name) plus the minutes timed here, either alone when the other
/// is missing, unknown when both are; a GOG copy the same with GOG's minutes (`gog`, by product id). A game the owner
/// added has only the minutes timed here. Steam copies are left alone. Returns whether anything changed.
pub fn apply(
    games: &mut [Game],
    service: &HashMap<String, u64>,
    gog: &HashMap<String, u64>,
    own: &HashMap<String, i64>,
) -> bool {
    let mut changed = false;
    for g in games
        .iter_mut()
        .filter(|g| g.provider != ProviderKind::Steam)
    {
        let timed = own.get(&g.key()).map(|&s| s.max(0) as u64 / 60);
        let store = match g.provider {
            ProviderKind::Epic => service.get(&g.external_id).copied(),
            ProviderKind::Gog => gog.get(&g.external_id).copied(),
            _ => None,
        };
        let minutes = [store, timed].into_iter().flatten().reduce(|a, b| a + b);
        changed |= g.playtime_minutes != minutes;
        g.playtime_minutes = minutes;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::tests::game;

    fn epic(app_name: &str) -> Game {
        Game {
            provider: ProviderKind::Epic,
            external_id: app_name.into(),
            ..game(app_name, true, None, None)
        }
    }

    /// UNVERIFIED: shaped after the public description (Playnite's EpicLibrary `PlaytimeItem`:
    /// `accountId`, `artifactId`, `totalTime` in seconds), not a captured response.
    const RESPONSE: &str = r#"[
        {"accountId": "0123456789abcdef0123456789abcdef", "artifactId": "Fortnite", "totalTime": 5430},
        {"accountId": "0123456789abcdef0123456789abcdef", "artifactId": "Pumpkin", "totalTime": 59}
    ]"#;

    #[test]
    fn parses_epic_playtime_response() {
        let minutes = parse(RESPONSE).unwrap();
        assert_eq!(
            minutes,
            HashMap::from([("Fortnite".to_string(), 90), ("Pumpkin".to_string(), 0)])
        );
        assert_eq!(parse("[]").unwrap(), HashMap::new());
        // An error object or junk is not a playtime list.
        assert!(
            parse(r#"{"errorCode": "errors.com.epicgames.common.oauth.invalid_token"}"#).is_err()
        );
        assert!(parse("<html>").is_err());
    }

    #[test]
    fn epic_playtime_adds_service_and_own() {
        let mut games = vec![
            epic("Both"),
            epic("OwnOnly"),
            epic("ShortOwn"),
            epic("Neither"),
        ];
        let service = HashMap::from([("Both".to_string(), 120)]);
        let own = HashMap::from([
            ("epic-Both".to_string(), 3_600),
            ("epic-OwnOnly".to_string(), 600),
            // Under a minute: known, but no whole minute yet.
            ("epic-ShortOwn".to_string(), 59),
        ]);
        assert!(apply(&mut games, &service, &HashMap::new(), &own));
        let minutes: Vec<_> = games.iter().map(|g| g.playtime_minutes).collect();
        assert_eq!(minutes, [Some(180), Some(10), Some(0), None]);
        // Nothing new: nothing changed.
        assert!(!apply(&mut games, &service, &HashMap::new(), &own));
        // Epic's answer gone (none cached yet): the own total shows.
        assert!(apply(&mut games, &HashMap::new(), &HashMap::new(), &own));
        assert_eq!(games[0].playtime_minutes, Some(60));
    }

    /// Epic's `artifactId` is the release's app name (the catalog's `releaseInfo[].appId`), which is
    /// Legendary's `app_name`: in every cached Legendary metadata file the app_name is one of its
    /// releaseInfo appIds. Own sessions are keyed by copy key. Steam copies are never touched.
    #[test]
    fn epic_playtime_maps_artifact_id_to_app_name() {
        let steam = game("Half-Life", true, Some(7), None);
        let mut games = vec![epic("Pumpkin"), epic("Fortnite"), steam.clone()];
        let service = HashMap::from([
            ("Pumpkin".to_string(), 50),
            (steam.external_id.clone(), 999),
        ]);
        let own = HashMap::from([("Fortnite".to_string(), 6_000), (steam.key(), 6_000)]);
        apply(&mut games, &service, &service, &own);
        assert_eq!(games[0].playtime_minutes, Some(50));
        // Own totals are keyed `epic-<app_name>`, not by the bare app name.
        assert_eq!(games[1].playtime_minutes, None);
        assert_eq!(games[2], steam);
    }

    #[test]
    fn added_games_count_only_their_own_sessions() {
        let custom = |id: &str| Game {
            provider: ProviderKind::Custom,
            external_id: id.into(),
            ..game(id, true, None, None)
        };
        let mut games = vec![custom("18f2a"), custom("Fortnite")];
        // Epic's answer for a matching name never counts for an added game.
        let service = HashMap::from([("Fortnite".to_string(), 500)]);
        let own = HashMap::from([("custom-18f2a".to_string(), 7_200)]);
        assert!(apply(&mut games, &service, &service, &own));
        assert_eq!(
            games.iter().map(|g| g.playtime_minutes).collect::<Vec<_>>(),
            [Some(120), None]
        );
    }

    #[test]
    fn gog_playtime_adds_gog_and_own() {
        let gog = |id: &str| Game {
            provider: ProviderKind::Gog,
            external_id: id.into(),
            ..game(id, true, None, None)
        };
        let mut games = vec![gog("1"), gog("2"), gog("3"), epic("1")];
        // GOG's answer is by product id; Epic's map never counts for a GOG copy, nor GOG's for an Epic one.
        let gog_minutes = HashMap::from([("1".to_string(), 90), ("2".to_string(), 5)]);
        let epic_minutes = HashMap::from([("3".to_string(), 500)]);
        let own = HashMap::from([("gog-1".to_string(), 1_200), ("gog-3".to_string(), 120)]);
        assert!(apply(&mut games, &epic_minutes, &gog_minutes, &own));
        assert_eq!(
            games.iter().map(|g| g.playtime_minutes).collect::<Vec<_>>(),
            [Some(110), Some(5), Some(2), None]
        );
    }

    #[test]
    fn gog_answers_are_read_from_the_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.sqlite");
        let conn = db::open(&path).unwrap();
        db::put_info(&conn, "gog-1", gog::PLAYTIME, "42", 0).unwrap();
        // GOG answered without minutes.
        db::put_info(&conn, "gog-2", gog::PLAYTIME, "null", 0).unwrap();
        db::put_info(&conn, "gog-3", "gog-product", "7", 0).unwrap();
        assert_eq!(gog_cached(&conn), HashMap::from([("1".to_string(), 42)]));
    }
}
