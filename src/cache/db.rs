//! SQLite library cache so startup can render before the stores are scanned.

use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::{Connection, params};

use crate::library::{Art, Error, Game, ProviderKind};

// shortcut: no migrations; bump the file name (library-v2.sqlite) if the schema changes before there's data worth keeping.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS games (
    provider TEXT NOT NULL,
    external_id TEXT NOT NULL,
    title TEXT NOT NULL,
    installed INTEGER NOT NULL,
    install_dir TEXT,
    playtime_minutes INTEGER,
    last_played INTEGER,
    cover TEXT,
    header TEXT,
    hero TEXT,
    logo TEXT,
    PRIMARY KEY (provider, external_id)
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
        "SELECT provider, external_id, title, installed, install_dir, playtime_minutes,
                last_played, cover, header, hero, logo FROM games",
    )?;
    let path = |s: Option<String>| s.map(PathBuf::from);
    let rows = stmt.query_map([], |r| {
        let Some(provider) = ProviderKind::parse(&r.get::<_, String>(0)?) else {
            return Ok(None);
        };
        Ok(Some(Game {
            provider,
            external_id: r.get(1)?,
            title: r.get(2)?,
            installed: r.get(3)?,
            install_dir: path(r.get(4)?),
            playtime_minutes: r.get::<_, Option<i64>>(5)?.map(|m| m as u64),
            last_played: r
                .get::<_, Option<i64>>(6)?
                .map(|s| UNIX_EPOCH + Duration::from_secs(s as u64)),
            art: Art { cover: path(r.get(7)?), header: path(r.get(8)?), hero: path(r.get(9)?), logo: path(r.get(10)?) },
        }))
    })?;
    // Rows from an unknown provider (e.g. written by a newer build) are skipped.
    Ok(rows.filter_map(Result::transpose).collect::<Result<_, _>>()?)
}

/// Replaces every cached game of `provider` with `games` in one transaction.
pub fn replace(conn: &mut Connection, provider: ProviderKind, games: &[Game]) -> Result<(), Error> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM games WHERE provider = ?1", [provider.as_str()])?;
    {
        let mut insert = tx.prepare("INSERT INTO games VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)")?;
        let text = |p: &Option<PathBuf>| p.as_ref().map(|p| p.to_string_lossy().into_owned());
        for g in games {
            insert.execute(params![
                g.provider.as_str(),
                g.external_id,
                g.title,
                g.installed,
                text(&g.install_dir),
                g.playtime_minutes.map(|m| m as i64),
                g.last_played.and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64),
                text(&g.art.cover),
                text(&g.art.header),
                text(&g.art.hero),
                text(&g.art.logo),
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str) -> Game {
        Game {
            provider: ProviderKind::Steam,
            external_id: id.into(),
            title: format!("Game {id}"),
            installed: true,
            install_dir: Some(PathBuf::from(format!("C:\\Games\\{id}"))),
            playtime_minutes: Some(90),
            last_played: Some(UNIX_EPOCH + Duration::from_secs(1_785_680_112)),
            art: Art { cover: Some("c.jpg".into()), logo: Some("l.png".into()), ..Art::default() },
        }
    }

    #[test]
    fn round_trips_and_replaces_per_provider() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute("INSERT INTO games (provider, external_id, title, installed) VALUES ('future', 'x', 'X', 0)", [])
            .unwrap();

        replace(&mut conn, ProviderKind::Steam, &[game("1"), game("2")]).unwrap();
        replace(&mut conn, ProviderKind::Steam, &[game("2")]).unwrap();

        // Game 1 is gone, game 2 survives intact, the unknown provider row is ignored.
        assert_eq!(load(&conn).unwrap(), vec![game("2")]);
    }
}
