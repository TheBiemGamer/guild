//! SQLite library cache so startup can render before the stores are scanned.

use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::{Connection, Row, params};

use crate::library::{Art, Error, Game, Image, ProviderKind};

// shortcut: no migrations; bump the file name in `Dirs` (library-v3.sqlite) if the schema changes before there's data worth keeping.
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
        "SELECT provider, external_id, title, installed, install_dir, playtime_minutes, last_played,
                cover, cover_url, header, header_url, hero, hero_url, logo, logo_url FROM games",
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
            art: Art { cover: image(r, 7)?, header: image(r, 9)?, hero: image(r, 11)?, logo: image(r, 13)? },
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
        let mut insert = tx.prepare("INSERT INTO games VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)")?;
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
}
