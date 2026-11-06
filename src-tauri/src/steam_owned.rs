//! The full Steam library (owned, installed or not) from Steam's servers, cached in the `info` table.
//! The refresh token is only read here to log on; it never reaches a log line, error string or Debug output.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use steam_vent::auth::RefreshToken;
use steam_vent::{Connection as Session, ConnectionError, ConnectionTrait, EResult, NetworkError, ServerList};
use steam_vent_proto_steam::steammessages_player_steamclient::CPlayer_GetOwnedGames_Request;

use crate::cache::db::{cached_info, put_info};

pub const OWNED: &str = "owned";
pub const OWNED_KEY: &str = "steam";
pub const OWNED_MAX_AGE_SECS: i64 = 6 * 60 * 60;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct OwnedGame {
    pub appid: u32,
    pub name: String,
    pub playtime_minutes: u64,
}

#[derive(Debug)]
pub enum OwnedError {
    /// The token expired or Steam refused it: the owner has to connect again.
    TokenRejected,
    /// Offline, a timeout or a server hiccup: try again later.
    Network(String),
}

/// What a finished fetch means for the stored list and the Settings dot.
#[derive(Debug)]
pub enum OwnedOutcome {
    Store(Vec<OwnedGame>),
    KeepAndFlag,
    KeepQuiet,
}

pub fn outcome(result: Result<Vec<OwnedGame>, OwnedError>) -> OwnedOutcome {
    match result {
        Ok(games) => OwnedOutcome::Store(games),
        Err(OwnedError::TokenRejected) => OwnedOutcome::KeepAndFlag,
        Err(OwnedError::Network(_)) => OwnedOutcome::KeepQuiet,
    }
}

fn rejected(result: EResult) -> bool {
    matches!(result, EResult::AccessDenied | EResult::InvalidPassword | EResult::Expired)
}

fn from_connection(e: ConnectionError) -> OwnedError {
    match e {
        ConnectionError::LoginError(_) => OwnedError::TokenRejected,
        e => OwnedError::Network(e.to_string()),
    }
}

fn from_network(e: NetworkError) -> OwnedError {
    match e {
        NetworkError::ApiError(r) if rejected(r) => OwnedError::TokenRejected,
        e => OwnedError::Network(e.to_string()),
    }
}

/// Logs on with the saved token and lists everything the account owns. The connection is dropped on return.
pub async fn fetch(token: &str) -> Result<Vec<OwnedGame>, OwnedError> {
    // Errors of `RefreshToken::new` only describe the token's shape, never its content.
    let token = RefreshToken::new(token.to_owned()).map_err(|_| OwnedError::TokenRejected)?;
    if token.remaining_lifetime().is_none() {
        return Err(OwnedError::TokenRejected);
    }
    let servers = ServerList::discover().await.map_err(|e| OwnedError::Network(e.to_string()))?;
    let session = Session::login_with_refresh_token(&servers, &token).await.map_err(from_connection)?;
    let owned = session
        .service_method(CPlayer_GetOwnedGames_Request {
            steamid: Some(session.steam_id().into()),
            include_appinfo: Some(true),
            include_played_free_games: Some(true),
            ..Default::default()
        })
        .await
        .map_err(from_network)?;
    Ok(owned
        .games
        .iter()
        .map(|g| OwnedGame { appid: g.appid() as u32, name: g.name().to_owned(), playtime_minutes: g.playtime_forever() as u64 })
        .collect())
}

/// The stored list and when it was fetched, at any age.
pub fn cached(conn: &Connection) -> Option<(Vec<OwnedGame>, i64)> {
    let (json, fetched) = conn
        .query_row("SELECT json, fetched FROM info WHERE key = ?1 AND kind = ?2", [OWNED_KEY, OWNED], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })
        .ok()?;
    Some((serde_json::from_str(&json).ok()?, fetched))
}

pub fn remember(conn: &Connection, games: &[OwnedGame], now_secs: i64) {
    let saved = serde_json::to_string(games)
        .map_err(|e| e.to_string())
        .and_then(|json| put_info(conn, OWNED_KEY, OWNED, &json, now_secs).map_err(|e| e.to_string()));
    if let Err(e) = saved {
        eprintln!("steam owned save: {e}");
    }
}

// Task 4 calls this when the owner disconnects Steam, and removes the allow.
#[allow(dead_code)]
pub fn forget(conn: &Connection) {
    if let Err(e) = conn.execute("DELETE FROM info WHERE key = ?1 AND kind = ?2", [OWNED_KEY, OWNED]) {
        eprintln!("steam owned forget: {e}");
    }
}

/// Whether the stored list is still within `OWNED_MAX_AGE_SECS`.
pub fn fresh(conn: &Connection, now_secs: i64) -> bool {
    matches!(cached_info(conn, OWNED_KEY, OWNED, OWNED_MAX_AGE_SECS, now_secs), Ok(Some(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(name: &str) -> Connection {
        let path = std::env::temp_dir().join(format!("launcher-owned-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        crate::cache::db::open(&path).unwrap()
    }

    #[test]
    fn owned_round_trips_through_the_info_cache() {
        let conn = db("roundtrip");
        assert_eq!(cached(&conn), None);
        let games = vec![OwnedGame { appid: 70, name: "Half-Life".into(), playtime_minutes: 0 }];
        remember(&conn, &games, 1_000);
        assert_eq!(cached(&conn), Some((games.clone(), 1_000)));
        assert!(fresh(&conn, 1_000 + OWNED_MAX_AGE_SECS));
        assert!(!fresh(&conn, 1_001 + OWNED_MAX_AGE_SECS));
        forget(&conn);
        assert_eq!(cached(&conn), None);
    }

    #[test]
    fn rejected_token_keeps_owned_and_flags_attention() {
        assert!(matches!(outcome(Err(OwnedError::TokenRejected)), OwnedOutcome::KeepAndFlag));
        assert!(matches!(outcome(Err(OwnedError::Network("x".into()))), OwnedOutcome::KeepQuiet));
        assert!(matches!(outcome(Ok(vec![])), OwnedOutcome::Store(_)));
    }

    #[test]
    fn only_auth_eresults_reject_the_token() {
        for r in [EResult::AccessDenied, EResult::InvalidPassword, EResult::Expired] {
            assert!(matches!(from_network(NetworkError::ApiError(r)), OwnedError::TokenRejected));
        }
        assert!(matches!(from_network(NetworkError::ApiError(EResult::Busy)), OwnedError::Network(_)));
        assert!(matches!(from_network(NetworkError::Timeout), OwnedError::Network(_)));
        assert!(matches!(from_connection(ConnectionError::Aborted), OwnedError::Network(_)));
    }
}
