//! SQLite library cache so startup can render before the stores are scanned. The same file keeps
//! user data that is not a cache: settings and launch history.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::library::{Art, Error, Game, Image, ProviderKind};

// shortcut: no migrations. The file also holds the user's settings and launch history, so bumping
// its name in `Dirs` (library-v3.sqlite) wipes them; the next schema change must copy `settings`
// and `launches` over from the old file (or add real migrations) instead of just bumping.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS games (
    provider TEXT NOT NULL,
    external_id TEXT NOT NULL,
    title TEXT NOT NULL,
    installed INTEGER NOT NULL,
    install_dir TEXT,
    playtime_minutes INTEGER,
    last_played INTEGER,
    cover TEXT, cover_url TEXT,
    header TEXT, header_url TEXT,
    hero TEXT, hero_url TEXT,
    logo TEXT, logo_url TEXT,
    size_bytes INTEGER,
    PRIMARY KEY (provider, external_id)
);
CREATE TABLE IF NOT EXISTS launches (
    key TEXT PRIMARY KEY,
    at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    name TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS info (
    key TEXT NOT NULL,
    kind TEXT NOT NULL,
    json TEXT NOT NULL,
    fetched INTEGER NOT NULL,
    PRIMARY KEY (key, kind)
);";

pub fn open(path: &Path) -> Result<Connection, Error> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

pub fn load(conn: &Connection) -> Result<Vec<Game>, Error> {
    let mut stmt = conn.prepare(
        "SELECT provider, external_id, title, installed, install_dir, playtime_minutes, last_played,
                cover, cover_url, header, header_url, hero, hero_url, logo, logo_url, size_bytes FROM games",
    )?;
    let rows = stmt.query_map([], |r| {
        let Some(provider) = ProviderKind::parse(&r.get::<_, String>(0)?) else {
            return Ok(None);
        };
        let image = |r: &Row, i| -> rusqlite::Result<Image> {
            Ok(Image { path: r.get::<_, Option<String>>(i)?.map(PathBuf::from), url: r.get(i + 1)? })
        };
        Ok(Some(Game {
            provider,
            external_id: r.get(1)?,
            title: r.get(2)?,
            installed: r.get(3)?,
            install_dir: r.get::<_, Option<String>>(4)?.map(PathBuf::from),
            playtime_minutes: r.get::<_, Option<i64>>(5)?.map(|m| m as u64),
            last_played: r.get::<_, Option<i64>>(6)?.map(|s| UNIX_EPOCH + Duration::from_secs(s as u64)),
            size_bytes: r.get::<_, Option<i64>>(15)?.map(|b| b as u64),
            art: Art { cover: image(r, 7)?, header: image(r, 9)?, hero: image(r, 11)?, logo: image(r, 13)? },
        }))
    })?;
    // Rows from an unknown provider (e.g. written by a newer build) are skipped.
    Ok(rows.filter_map(Result::transpose).collect::<Result<_, _>>()?)
}

/// Records that `key` was launched at `at_secs`, keeping the latest time seen.
pub fn record_launch(conn: &Connection, key: &str, at_secs: i64) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO launches (key, at) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET at = max(at, excluded.at)",
        params![key, at_secs],
    )?;
    Ok(())
}

/// Latest launch time in seconds for every launched game key.
pub fn launches(conn: &Connection) -> Result<HashMap<String, i64>, Error> {
    let mut stmt = conn.prepare("SELECT key, at FROM launches")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn setting(conn: &Connection, name: &str) -> Result<Option<String>, Error> {
    Ok(conn.query_row("SELECT value FROM settings WHERE name = ?1", [name], |r| r.get(0)).optional()?)
}

/// Stores `value` under `name`, or removes the setting when `value` is `None`.
pub fn set_setting(conn: &Connection, name: &str, value: Option<&str>) -> Result<(), Error> {
    match value {
        Some(value) => conn.execute(
            "INSERT INTO settings (name, value) VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET value = excluded.value",
            params![name, value],
        )?,
        None => conn.execute("DELETE FROM settings WHERE name = ?1", [name])?,
    };
    Ok(())
}

/// Cached JSON for `(key, kind)` if it was fetched within `max_age_secs` of `now_secs`.
pub fn cached_info(
    conn: &Connection,
    key: &str,
    kind: &str,
    max_age_secs: i64,
    now_secs: i64,
) -> Result<Option<String>, Error> {
    Ok(conn
        .query_row(
            "SELECT json FROM info WHERE key = ?1 AND kind = ?2 AND fetched >= ?3",
            params![key, kind, now_secs - max_age_secs],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn put_info(conn: &Connection, key: &str, kind: &str, json: &str, now_secs: i64) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO info (key, kind, json, fetched) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(key, kind) DO UPDATE SET json = excluded.json, fetched = excluded.fetched",
        params![key, kind, json, now_secs],
    )?;
    Ok(())
}

/// Replaces every cached game of `provider` with `games` in one transaction.
pub fn replace(conn: &mut Connection, provider: ProviderKind, games: &[Game]) -> Result<(), Error> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM games WHERE provider = ?1", [provider.as_str()])?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO games (provider, external_id, title, installed, install_dir, playtime_minutes, last_played,
                cover, cover_url, header, header_url, hero, hero_url, logo, logo_url, size_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        )?;
        let text = |p: &Option<PathBuf>| p.as_ref().map(|p| p.to_string_lossy().into_owned());
        for g in games.iter().filter(|g| g.provider == provider) {
            let a = &g.art;
            insert.execute(params![
                g.provider.as_str(),
                g.external_id,
                g.title,
                g.installed,
                text(&g.install_dir),
                g.playtime_minutes.map(|m| m as i64),
                g.last_played.and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64),
                text(&a.cover.path),
                a.cover.url,
                text(&a.header.path),
                a.header.url,
                text(&a.hero.path),
                a.hero.url,
                text(&a.logo.path),
                a.logo.url,
                g.size_bytes.map(|b| b as i64),
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(provider: ProviderKind, id: &str) -> Game {
        Game {
            provider,
            external_id: id.into(),
            title: format!("Game {id}"),
            installed: true,
            install_dir: Some(PathBuf::from(format!("C:\\Games\\{id}"))),
            playtime_minutes: Some(90),
            last_played: Some(UNIX_EPOCH + Duration::from_secs(1_785_680_112)),
            size_bytes: Some(27_300_000_000),
            art: Art {
                cover: Image { path: Some("c.jpg".into()), url: Some("https://x/c.jpg".into()) },
                logo: Image { path: None, url: Some("https://x/l.png".into()) },
                ..Art::default()
            },
        }
    }

    #[test]
    fn round_trips_and_replaces_per_provider() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute("INSERT INTO games (provider, external_id, title, installed) VALUES ('future', 'x', 'X', 0)", [])
            .unwrap();
        let (steam, epic) = (ProviderKind::Steam, ProviderKind::Epic);

        replace(&mut conn, steam, &[game(steam, "1"), game(steam, "2")]).unwrap();
        replace(&mut conn, epic, &[game(epic, "Fortnite")]).unwrap();
        replace(&mut conn, steam, &[game(steam, "2")]).unwrap();

        // Steam game 1 is gone, the Epic game is untouched, the unknown provider row is ignored.
        let mut loaded = load(&conn).unwrap();
        loaded.sort_by(|a, b| a.external_id.cmp(&b.external_id));
        assert_eq!(loaded, vec![game(steam, "2"), game(epic, "Fortnite")]);
    }

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn
    }

    #[test]
    fn launches_keep_the_latest_time() {
        let conn = memory();
        record_launch(&conn, "epic-Fortnite", 1_000).unwrap();
        record_launch(&conn, "epic-Fortnite", 2_000).unwrap();
        record_launch(&conn, "epic-Fortnite", 1_500).unwrap();
        assert_eq!(launches(&conn).unwrap().get("epic-Fortnite"), Some(&2_000));
    }

    #[test]
    fn settings_set_read_and_delete() {
        let conn = memory();
        set_setting(&conn, "igdb_client_id", Some("abc")).unwrap();
        assert_eq!(setting(&conn, "igdb_client_id").unwrap().as_deref(), Some("abc"));
        set_setting(&conn, "igdb_client_id", None).unwrap();
        assert_eq!(setting(&conn, "igdb_client_id").unwrap(), None);
    }

    #[test]
    fn cached_info_respects_max_age() {
        let conn = memory();
        put_info(&conn, "steam-1", "details", r#"{"a":1}"#, 100).unwrap();
        assert_eq!(cached_info(&conn, "steam-1", "details", 50, 120).unwrap().as_deref(), Some(r#"{"a":1}"#));
        assert_eq!(cached_info(&conn, "steam-1", "details", 50, 200).unwrap(), None);
        assert_eq!(cached_info(&conn, "steam-1", "ttb", 50, 120).unwrap(), None);
    }
}
