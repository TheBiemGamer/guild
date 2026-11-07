//! IGDB (owned by Twitch): "how long to beat". Games are matched by store ID, never by title.
//! The Client Secret is only ever sent to `id.twitch.tv`; no error text built here contains it.
//!
//! IGDB asks for visible attribution where its data is shown (see `commands::time_to_beat`).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::cache::db;
use crate::library::percent_encode;

const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const API_URL: &str = "https://api.igdb.com/v4";
/// A token this close to expiring is renewed instead of reused.
const RENEW_BEFORE: Duration = Duration::from_secs(60);
/// After a failed token request, don't ask Twitch again for this long.
const BACK_OFF: Duration = Duration::from_secs(10 * 60);
/// IGDB `external_game_source` id of Steam.
const STEAM_SOURCE: u32 = 1;
/// IGDB `external_game_source` id of the Epic Games Store.
const EPIC_SOURCE: u32 = 26;
/// `info` kind holding an Epic copy's store page URL, as IGDB gave it.
pub const EPIC_URL: &str = "epic-url";

/// How long a found answer stays good, and a "not found" one.
pub const FOUND_MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;
pub const MISSING_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub value: String,
    /// When Twitch stops accepting it.
    pub expires_at: Instant,
}

/// The app access token, plus the back-off after a failed request for one.
#[derive(Default)]
pub struct TokenCache {
    token: Option<Token>,
    failed_until: Option<Instant>,
}

impl TokenCache {
    /// The stored token, unless it expires within a minute.
    pub fn fresh(&self, now: Instant) -> Option<Token> {
        self.token.as_ref().filter(|t| t.expires_at > now + RENEW_BEFORE).cloned()
    }

    pub fn backing_off(&self, now: Instant) -> bool {
        self.failed_until.is_some_and(|until| now < until)
    }

    pub fn store(&mut self, token: Token) {
        self.token = Some(token);
        self.failed_until = None;
    }

    pub fn fail(&mut self, now: Instant) {
        self.failed_until = Some(now + BACK_OFF);
    }

    /// Credentials changed: forget the token and the back-off.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Seconds, as IGDB reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeToBeat {
    pub hastily: Option<u64>,
    pub normally: Option<u64>,
    pub completely: Option<u64>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

pub fn parse_token(body: &str, now: Instant) -> Result<Token, String> {
    let t: TokenResponse = serde_json::from_str(body).map_err(|_| "Twitch sent an answer this app can't read.")?;
    Ok(Token { value: t.access_token, expires_at: now + Duration::from_secs(t.expires_in) })
}

#[derive(Deserialize)]
struct ExternalGame {
    game: Option<u64>,
}

/// IGDB's game id from an `external_games` answer; `None` when IGDB doesn't know the store ID.
pub fn parse_game_id(body: &str) -> Result<Option<u64>, String> {
    let rows: Vec<ExternalGame> = serde_json::from_str(body).map_err(|_| "IGDB sent an answer we don't understand.")?;
    Ok(rows.into_iter().find_map(|r| r.game))
}

/// The first `game_time_to_beats` row with at least one value.
pub fn parse_time_to_beat(body: &str) -> Result<Option<TimeToBeat>, String> {
    let rows: Vec<TimeToBeat> = serde_json::from_str(body).map_err(|_| "IGDB sent an answer we don't understand.")?;
    Ok(rows.into_iter().find(|t| t.hastily.is_some() || t.normally.is_some() || t.completely.is_some()))
}

/// What Twitch said was wrong, e.g. "invalid client secret". Never contains `secret`.
fn token_failure(status: u16, body: &str, secret: &str) -> String {
    #[derive(Deserialize)]
    struct Failure {
        message: String,
    }
    let said = serde_json::from_str::<Failure>(body).map(|f| f.message).unwrap_or_default();
    if said.is_empty() || (!secret.is_empty() && said.contains(secret)) {
        format!("Twitch refused the credentials (HTTP {status}).")
    } else {
        format!("Twitch refused the credentials: {said} (HTTP {status}).")
    }
}

/// Asks Twitch for an app access token (client-credentials flow).
pub async fn token(http: &reqwest::Client, client_id: &str, secret: &str) -> Result<Token, String> {
    let body = format!(
        "client_id={}&client_secret={}&grant_type=client_credentials",
        percent_encode(client_id),
        percent_encode(secret)
    );
    let response = http
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|_| "Couldn't reach Twitch.".to_string())?;
    let status = response.status();
    let text = response.text().await.map_err(|_| "Couldn't read Twitch's answer.".to_string())?;
    if !status.is_success() {
        return Err(token_failure(status.as_u16(), &text, secret));
    }
    parse_token(&text, Instant::now())
}

/// The stored token if it is still good, else a new one. A failure pauses new requests for
/// 10 minutes. The lock is never held while waiting for Twitch.
pub async fn cached_token(
    cache: &Mutex<TokenCache>,
    http: &reqwest::Client,
    client_id: &str,
    secret: &str,
) -> Result<Token, String> {
    {
        let c = cache.lock().expect("token cache lock poisoned");
        let now = Instant::now();
        if let Some(token) = c.fresh(now) {
            return Ok(token);
        }
        if c.backing_off(now) {
            return Err("Twitch refused the credentials a moment ago; waiting a few minutes before retrying.".into());
        }
    }
    let result = token(http, client_id, secret).await;
    let mut c = cache.lock().expect("token cache lock poisoned");
    match &result {
        Ok(token) => c.store(token.clone()),
        Err(_) => c.fail(Instant::now()),
    }
    result
}

async fn post(
    http: &reqwest::Client,
    token: &Token,
    client_id: &str,
    endpoint: &str,
    query: String,
) -> Result<String, String> {
    let response = http
        .post(format!("{API_URL}/{endpoint}"))
        .header("Client-ID", client_id)
        .bearer_auth(&token.value)
        .body(query)
        .send()
        .await
        .map_err(|_| "Couldn't reach IGDB.".to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("IGDB answered HTTP {}.", status.as_u16()));
    }
    response.text().await.map_err(|_| "Couldn't read IGDB's answer.".to_string())
}

/// IGDB's id for a Steam game, matched on its app id. `Ok(None)` when IGDB doesn't know it.
pub async fn game_id_for_steam(
    http: &reqwest::Client,
    token: &Token,
    client_id: &str,
    appid: u32,
) -> Result<Option<u64>, String> {
    let query = format!("fields game; where external_game_source = {STEAM_SOURCE} & uid = \"{appid}\"; limit 1;");
    parse_game_id(&post(http, token, client_id, "external_games", query).await?)
}

/// The query for Epic rows named exactly `title`.
pub fn epic_title_query(title: &str) -> String {
    let title = title.replace('\\', r"\\").replace('"', "\\\"");
    format!("fields game,url; where external_game_source = {EPIC_SOURCE} & name = \"{title}\"; limit 10;")
}

#[derive(Debug, PartialEq, Eq)]
pub struct EpicMatch {
    pub game: u64,
    pub url: Option<String>,
}

#[derive(Deserialize)]
struct EpicRow {
    game: Option<u64>,
    url: Option<String>,
}

/// The IGDB game when there is at least one row and every row names the same game; a title
/// shared by different games is a guess, so it matches nothing.
pub fn parse_epic_title_match(body: &str) -> Result<Option<EpicMatch>, String> {
    let rows: Vec<EpicRow> = serde_json::from_str(body).map_err(|_| "IGDB sent an answer we don't understand.")?;
    let Some(game) = rows.first().and_then(|r| r.game) else { return Ok(None) };
    if rows.iter().any(|r| r.game != Some(game)) {
        return Ok(None);
    }
    Ok(Some(EpicMatch { game, url: store_url(rows) }))
}

/// The first row URL that is an Epic store page.
fn store_url(rows: Vec<EpicRow>) -> Option<String> {
    rows.into_iter().filter_map(|r| r.url).find(|u| u.starts_with("https://store.epicgames.com/"))
}

/// The Epic store page among an IGDB game's Epic rows.
pub fn parse_epic_store_url(body: &str) -> Result<Option<String>, String> {
    let rows: Vec<EpicRow> = serde_json::from_str(body).map_err(|_| "IGDB sent an answer we don't understand.")?;
    Ok(store_url(rows))
}

/// The Epic store page IGDB knows for a game found another way (by Steam app id).
pub async fn epic_url_for_game(
    http: &reqwest::Client,
    token: &Token,
    client_id: &str,
    game_id: u64,
) -> Result<Option<String>, String> {
    let query = format!("fields url; where game = {game_id} & external_game_source = {EPIC_SOURCE}; limit 10;");
    parse_epic_store_url(&post(http, token, client_id, "external_games", query).await?)
}

pub async fn epic_by_title(
    http: &reqwest::Client,
    token: &Token,
    client_id: &str,
    title: &str,
) -> Result<Option<EpicMatch>, String> {
    parse_epic_title_match(&post(http, token, client_id, "external_games", epic_title_query(title)).await?)
}

/// Remembers the store page URL of an Epic copy; it never expires and is replaced when re-learned.
pub fn remember_epic_url(conn: &Connection, epic_key: &str, url: &str) {
    if let Ok(json) = serde_json::to_string(url) {
        _ = db::put_info(conn, epic_key, EPIC_URL, &json, 0);
    }
}

pub fn epic_url(conn: &Connection, epic_key: &str) -> Option<String> {
    let json = db::cached_info(conn, epic_key, EPIC_URL, i64::MAX, 0).ok().flatten()?;
    serde_json::from_str(&json).ok()
}

pub async fn time_to_beat(
    http: &reqwest::Client,
    token: &Token,
    client_id: &str,
    game_id: u64,
) -> Result<Option<TimeToBeat>, String> {
    let query = format!("fields hastily,normally,completely; where game_id = {game_id}; limit 1;");
    parse_time_to_beat(&post(http, token, client_id, "game_time_to_beats", query).await?)
}

/// The cached answer for `key`: `Some(None)` is a remembered "not found", `None` means ask IGDB.
pub fn cached(conn: &Connection, key: &str, now_secs: i64) -> Option<Option<TimeToBeat>> {
    let json = db::cached_info(conn, key, "ttb", FOUND_MAX_AGE_SECS, now_secs).ok().flatten()?;
    let stored: Option<TimeToBeat> = serde_json::from_str(&json).ok()?;
    let fresh_missing = || db::cached_info(conn, key, "ttb", MISSING_MAX_AGE_SECS, now_secs).ok().flatten().is_some();
    (stored.is_some() || fresh_missing()).then_some(stored)
}

/// Every cached "normally" (seconds) by key, whatever its age: sorting and grouping use these and
/// never ask IGDB. One query, at start.
pub fn all_normally(conn: &Connection) -> HashMap<String, u64> {
    let Ok(mut stmt) = conn.prepare("SELECT key, json FROM info WHERE kind = 'ttb'") else { return HashMap::new() };
    let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) else {
        return HashMap::new();
    };
    rows.flatten()
        .filter_map(|(key, json)| {
            let t: TimeToBeat = serde_json::from_str::<Option<TimeToBeat>>(&json).ok()??;
            Some((key, t.normally.filter(|&s| s > 0)?))
        })
        .collect()
}

/// Remembers the answer; "not found" is stored as `null` so it isn't asked again on every visit.
pub fn remember(conn: &Connection, key: &str, answer: Option<TimeToBeat>, now_secs: i64) {
    if let Ok(json) = serde_json::to_string(&answer) {
        _ = db::put_info(conn, key, "ttb", &json, now_secs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // unverified: shaped from docs, not a live capture (fixtures in tests/fixtures/igdb)
    const TOKEN: &str = include_str!("../tests/fixtures/igdb/token.json");
    const EXTERNAL_GAMES: &str = include_str!("../tests/fixtures/igdb/external_games.json");
    const TIME_TO_BEAT: &str = include_str!("../tests/fixtures/igdb/time_to_beat.json");

    fn token_expiring_in(secs: u64, now: Instant) -> Token {
        Token { value: "t".into(), expires_at: now + Duration::from_secs(secs) }
    }

    #[test]
    fn parse_token_reads_value_and_lifetime() {
        // unverified: shaped from docs, not a live capture
        let now = Instant::now();
        let t = parse_token(TOKEN, now).unwrap();
        assert_eq!(t.value, "jostpf5q0uzmxmkba9iyug38kjtgh");
        assert_eq!(t.expires_at, now + Duration::from_secs(5_011_271));
        assert!(parse_token("{}", now).is_err());
    }

    #[test]
    fn parse_game_id_reads_the_igdb_game() {
        // unverified: shaped from docs, not a live capture
        assert_eq!(parse_game_id(EXTERNAL_GAMES).unwrap(), Some(1942));
        assert_eq!(parse_game_id("[]").unwrap(), None);
        assert!(parse_game_id("nope").is_err());
    }

    #[test]
    fn parse_time_to_beat_reads_seconds() {
        // unverified: shaped from docs, not a live capture
        let t = parse_time_to_beat(TIME_TO_BEAT).unwrap().unwrap();
        assert_eq!((t.hastily, t.normally, t.completely), (Some(86_400), Some(115_200), Some(216_000)));
        assert_eq!(parse_time_to_beat("[]").unwrap(), None);
        assert_eq!(parse_time_to_beat(r#"[{"id":1,"game_id":2}]"#).unwrap(), None);
        let partial = parse_time_to_beat(r#"[{"game_id":2,"normally":3600}]"#).unwrap().unwrap();
        assert_eq!((partial.hastily, partial.normally, partial.completely), (None, Some(3600), None));
    }

    #[test]
    fn token_is_reused_until_expiry() {
        let now = Instant::now();
        let mut cache = TokenCache::default();
        assert!(cache.fresh(now).is_none());
        cache.store(token_expiring_in(120, now));
        assert_eq!(cache.fresh(now).map(|t| t.value), Some("t".to_string()));
        assert!(cache.fresh(now + Duration::from_secs(59)).is_some());
        assert!(cache.fresh(now + Duration::from_secs(61)).is_none(), "within 60 s of expiry it is renewed");
    }

    #[test]
    fn failed_token_backs_off() {
        let now = Instant::now();
        let mut cache = TokenCache::default();
        assert!(!cache.backing_off(now));
        cache.fail(now);
        assert!(cache.backing_off(now + Duration::from_secs(599)));
        assert!(!cache.backing_off(now + Duration::from_secs(601)));
        cache.reset();
        assert!(!cache.backing_off(now), "new credentials clear the back-off");
        cache.fail(now);
        cache.store(token_expiring_in(3600, now));
        assert!(!cache.backing_off(now), "a good token clears it too");
    }

    const EPIC_TITLE: &str = include_str!("../tests/fixtures/igdb/epic_title.json");

    #[test]
    fn epic_title_match_accepts_rows_of_one_game() {
        let m = parse_epic_title_match(EPIC_TITLE).unwrap().unwrap();
        assert_eq!(m.game, 36543);
        assert_eq!(m.url.as_deref(), Some("https://store.epicgames.com/en-US/p/last-day-of-june"));
    }

    #[test]
    fn epic_title_match_rejects_mixed_games_and_empty() {
        assert_eq!(parse_epic_title_match(r#"[{"game":1},{"game":2}]"#).unwrap(), None);
        assert_eq!(parse_epic_title_match("[]").unwrap(), None);
        assert_eq!(parse_epic_title_match(r#"[{"game":1},{}]"#).unwrap(), None);
        assert!(parse_epic_title_match("nope").is_err());
    }

    #[test]
    fn epic_title_query_escapes_quotes_and_backslashes() {
        assert_eq!(
            epic_title_query(r#"Say "Hi" \o/"#),
            r#"fields game,url; where external_game_source = 26 & name = "Say \"Hi\" \\o/"; limit 10;"#
        );
    }

    #[test]
    fn epic_match_ignores_non_store_urls() {
        let m = parse_epic_title_match(r#"[{"game":7,"url":"https://example.com/x"}]"#).unwrap().unwrap();
        assert_eq!((m.game, m.url), (7, None));
    }

    #[test]
    fn epic_store_url_from_a_games_epic_rows() {
        assert_eq!(
            parse_epic_store_url(EPIC_TITLE).unwrap().as_deref(),
            Some("https://store.epicgames.com/en-US/p/last-day-of-june")
        );
        assert_eq!(parse_epic_store_url(r#"[{"url":"https://example.com/x"}]"#).unwrap(), None);
        assert_eq!(parse_epic_store_url("[]").unwrap(), None);
        assert!(parse_epic_store_url("nope").is_err());
    }

    #[test]
    fn epic_url_is_remembered_and_overwritten() {
        let conn = temp_db("epic-url");
        assert_eq!(epic_url(&conn, "epic-A"), None);
        remember_epic_url(&conn, "epic-A", "https://store.epicgames.com/a");
        remember_epic_url(&conn, "epic-A", "https://store.epicgames.com/b");
        assert_eq!(epic_url(&conn, "epic-A").as_deref(), Some("https://store.epicgames.com/b"));
    }

    #[test]
    fn token_failure_never_repeats_the_secret() {
        let said = token_failure(400, r#"{"status":400,"message":"invalid client secret"}"#, "hunter2");
        assert_eq!(said, "Twitch refused the credentials: invalid client secret (HTTP 400).");
        let echoed = token_failure(400, r#"{"message":"bad hunter2"}"#, "hunter2");
        assert!(!echoed.contains("hunter2"));
        assert_eq!(token_failure(500, "<html>", "hunter2"), "Twitch refused the credentials (HTTP 500).");
    }

    fn temp_db(name: &str) -> Connection {
        let path = std::env::temp_dir().join(format!("launcher-igdb-{name}-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        db::open(&path).unwrap()
    }

    #[test]
    fn found_answers_last_30_days_and_missing_ones_7() {
        let conn = temp_db("ttb");
        let day = 24 * 60 * 60;
        let ttb = TimeToBeat { hastily: None, normally: Some(3600), completely: None };
        assert_eq!(cached(&conn, "steam-1", 0), None);
        remember(&conn, "steam-1", Some(ttb), 0);
        remember(&conn, "steam-2", None, 0);
        assert_eq!(cached(&conn, "steam-1", 29 * day), Some(Some(ttb)));
        assert_eq!(cached(&conn, "steam-1", 31 * day), None);
        assert_eq!(cached(&conn, "steam-2", 6 * day), Some(None), "not found is remembered");
        assert_eq!(cached(&conn, "steam-2", 8 * day), None, "and retried after a week");
    }

    #[test]
    fn all_normally_keeps_known_times_of_any_age() {
        let conn = temp_db("all-normally");
        let ttb = |normally| Some(TimeToBeat { hastily: Some(60), normally, completely: None });
        remember(&conn, "steam-1", ttb(Some(3600)), 0);
        remember(&conn, "steam-2", ttb(None), 0);
        remember(&conn, "steam-3", None, 0);
        remember(&conn, "steam-4", ttb(Some(0)), 0);
        assert_eq!(all_normally(&conn), HashMap::from([("steam-1".to_string(), 3600)]));
    }
}
