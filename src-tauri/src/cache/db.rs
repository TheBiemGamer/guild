//! SQLite library cache so startup can render before the stores are scanned. The same file keeps
//! user data that is not a cache: settings, launch history, per-game choices (`game_prefs`, favorites and
//! hidden games among them), playtime, collections and the games the owner added (`custom_games`). The `info`
//! table is cache only.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, TransactionBehavior, params};

use crate::library::custom::CustomGame;
use crate::library::{Art, Error, Game, Image, ProviderKind};
use crate::view::Lists;

/// Migration steps; `PRAGMA user_version` is how many have run. Schema changes are new steps at the end,
/// never edits to old ones, and the file name never changes.
const STEPS: &[&str] = &[
    // 1: the schema of the `library-v3.sqlite` file, so an existing one needs no work.
    "CREATE TABLE IF NOT EXISTS games (
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
    );",
    // 2: the Epic scan's DLC, update and cloud save flags are kept. Per-game choices and playtime get
    // their own tables, apart from the `info` cache.
    "ALTER TABLE games ADD COLUMN dlc TEXT;
    ALTER TABLE games ADD COLUMN update_available INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE games ADD COLUMN cloud_saves INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE games DROP COLUMN logo;
    ALTER TABLE games DROP COLUMN logo_url;
    CREATE TABLE game_prefs (
        key TEXT NOT NULL,
        name TEXT NOT NULL,
        value TEXT NOT NULL,
        PRIMARY KEY (key, name)
    );
    CREATE TABLE playtime (
        key TEXT NOT NULL,
        started INTEGER NOT NULL,
        seconds INTEGER NOT NULL
    );
    INSERT INTO game_prefs (key, name, value)
        SELECT key, 'cloud_saves', json FROM info WHERE kind = 'cloud-saves' AND json IN ('on', 'off');
    DELETE FROM info WHERE kind IN ('cloud-saves', 'twin', 'twin2');",
    // 3: the owner's collections, holding copy keys. Foreign keys are off, so `delete_collection` deletes
    // the members itself.
    "CREATE TABLE collections (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL
    );
    CREATE TABLE collection_games (
        collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
        key TEXT NOT NULL,
        PRIMARY KEY (collection_id, key)
    );",
    // 4: games the owner added (keys `custom-<id>`); their covers are files in the data folder.
    "CREATE TABLE custom_games (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        exe TEXT NOT NULL,
        args TEXT NOT NULL DEFAULT '',
        added INTEGER NOT NULL
    );",
    // 5: where each GOG game is installed (owner data; gogdl records nothing). Keys `gog-<product_id>`.
    "CREATE TABLE gog_installs (
        key TEXT PRIMARY KEY,
        path TEXT NOT NULL
    );",
];

/// Opens the database and brings its schema up to date. Every connection goes through here.
pub fn open(path: &Path) -> Result<Connection, Error> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut conn = Connection::open(path)?;
    // Background jobs write while commands do: wait for the lock instead of failing the write.
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get::<_, String>(0))?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn user_version(conn: &Connection) -> rusqlite::Result<usize> {
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .map(|v| v as usize)
}

/// Runs each step above `user_version`, each in its own transaction. Two connections opening at once
/// never both run a step: the version is read again under the write lock.
fn migrate(conn: &mut Connection) -> Result<(), Error> {
    if user_version(conn)? >= STEPS.len() {
        return Ok(());
    }
    for (done, step) in STEPS.iter().enumerate() {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if user_version(&tx)? > done {
            continue;
        }
        tx.execute_batch(step)?;
        tx.pragma_update(None, "user_version", done as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

const SIDECARS: [&str; 3] = ["-journal", "-wal", "-shm"];

/// `path` with `suffix` appended to its file name, as SQLite names its sidecar files.
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

/// The database file in `dir`: `library.sqlite`. Runs once at startup, before anything opens it.
/// The first time, an older `library-v3.sqlite` there is renamed to it, or copied when the rename fails
/// (another instance has it open). The old file is used only when neither works, and is never deleted.
pub fn file_in(dir: &Path) -> PathBuf {
    let new = dir.join("library.sqlite");
    let old = dir.join("library-v3.sqlite");
    if !old.exists() || (new.exists() && !is_leftover(&new)) {
        return new;
    }
    let keep_old = |why: &dyn std::fmt::Display| {
        tracing::warn!("database: {why}; keeps using {}", old.display());
        old.clone()
    };
    // SQLite would apply a stray journal of `library.sqlite` to the adopted file.
    if let Some(stray) = SIDECARS
        .iter()
        .map(|s| sidecar(&new, s))
        .find(|p| p.exists())
    {
        return keep_old(&format!("{} is in the way", stray.display()));
    }
    if new.exists() {
        // A `library.sqlite` from before settings existed only ever held a game cache. It is set aside, not deleted.
        let aside = dir.join("library-v1.sqlite");
        if aside.exists() {
            return keep_old(&format!(
                "{} is an old cache and {} exists",
                new.display(),
                aside.display()
            ));
        }
        if let Err(e) = std::fs::rename(&new, &aside) {
            return keep_old(&e);
        }
    }
    let adopted = rename_with_sidecars(&old, &new).or_else(|e| {
        tracing::warn!(
            "database: renaming {}: {e}; copying it instead",
            old.display()
        );
        copy(&old, &new)
    });
    match adopted {
        Ok(()) => new,
        // Another instance adopted it first.
        Err(_) if new.exists() => new,
        Err(e) => keep_old(&e),
    }
}

/// A file without a `settings` table: the game cache of the first builds, never anyone's data.
fn is_leftover(path: &Path) -> bool {
    let settings_tables = |conn: Connection| -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'settings'",
            [],
            |r| r.get(0),
        )
    };
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).and_then(settings_tables)
        == Ok(0)
}

/// Renames `old` and its sidecars to `new`, the main file last so it never arrives without them.
/// Puts back what moved when a rename fails.
fn rename_with_sidecars(old: &Path, new: &Path) -> Result<(), Error> {
    let mut moved = Vec::new();
    let mut rename = || -> std::io::Result<()> {
        for suffix in SIDECARS {
            let (from, to) = (sidecar(old, suffix), sidecar(new, suffix));
            if from.exists() {
                std::fs::rename(&from, &to)?;
                moved.push((from, to));
            }
        }
        std::fs::rename(old, new)
    };
    let result = rename();
    if result.is_err() {
        for (from, to) in moved {
            _ = std::fs::rename(to, from);
        }
    }
    Ok(result?)
}

/// Copies `old` to `new` through SQLite, so a file another instance is writing is copied whole.
fn copy(old: &Path, new: &Path) -> Result<(), Error> {
    let tmp = new.with_extension(format!("sqlite.{}.tmp", std::process::id()));
    _ = std::fs::remove_file(&tmp);
    let conn = Connection::open_with_flags(old, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.execute("VACUUM INTO ?1", [tmp.to_string_lossy()])?;
    drop(conn);
    // A hard link fails when `new` exists, so the copy never replaces a file another instance just made.
    let linked = std::fs::hard_link(&tmp, new);
    _ = std::fs::remove_file(&tmp);
    Ok(linked?)
}

pub fn load(conn: &Connection) -> Result<Vec<Game>, Error> {
    let mut stmt = conn.prepare(
        "SELECT provider, external_id, title, installed, install_dir, playtime_minutes, last_played,
                cover, cover_url, header, header_url, hero, hero_url, size_bytes, dlc, update_available, cloud_saves
         FROM games",
    )?;
    let rows = stmt.query_map([], |r| {
        let Some(provider) = ProviderKind::parse(&r.get::<_, String>(0)?) else {
            return Ok(None);
        };
        let image = |r: &Row, i| -> rusqlite::Result<Image> {
            Ok(Image {
                path: r.get::<_, Option<String>>(i)?.map(PathBuf::from),
                url: r.get(i + 1)?,
            })
        };
        Ok(Some(Game {
            provider,
            external_id: r.get(1)?,
            title: r.get(2)?,
            installed: r.get(3)?,
            install_dir: r.get::<_, Option<String>>(4)?.map(PathBuf::from),
            playtime_minutes: r.get::<_, Option<i64>>(5)?.map(|m| m as u64),
            last_played: r
                .get::<_, Option<i64>>(6)?
                .map(|s| UNIX_EPOCH + Duration::from_secs(s as u64)),
            size_bytes: r.get::<_, Option<i64>>(13)?.map(|b| b as u64),
            art: Art {
                cover: image(r, 7)?,
                header: image(r, 9)?,
                hero: image(r, 11)?,
            },
            // A list that doesn't parse is a cache miss: the next Epic scan fills it again.
            dlc: r
                .get::<_, Option<String>>(14)?
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default(),
            update_available: r.get(15)?,
            cloud_saves: r.get(16)?,
        }))
    })?;
    // Rows from an unknown provider (e.g. written by a newer build) are skipped.
    Ok(rows
        .filter_map(Result::transpose)
        .collect::<Result<_, _>>()?)
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

/// Records a play session of the copy `key` timed here: started at `started_secs`, `seconds` long.
pub fn add_session(
    conn: &Connection,
    key: &str,
    started_secs: i64,
    seconds: i64,
) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO playtime (key, started, seconds) VALUES (?1, ?2, ?3)",
        params![key, started_secs, seconds.max(0)],
    )?;
    Ok(())
}

/// Seconds played per copy key, summed over the sessions timed here.
pub fn own_totals(conn: &Connection) -> Result<HashMap<String, i64>, Error> {
    let mut stmt = conn.prepare("SELECT key, sum(seconds) FROM playtime GROUP BY key")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn setting(conn: &Connection, name: &str) -> Result<Option<String>, Error> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE name = ?1", [name], |r| {
            r.get(0)
        })
        .optional()?)
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

/// The owner's choice `name` for the game copy `key`, such as `cloud_saves`.
pub fn pref(conn: &Connection, key: &str, name: &str) -> Result<Option<String>, Error> {
    Ok(conn
        .query_row(
            "SELECT value FROM game_prefs WHERE key = ?1 AND name = ?2",
            [key, name],
            |r| r.get(0),
        )
        .optional()?)
}

/// Stores a per-game choice, or removes it when `value` is `None`.
pub fn set_pref(
    conn: &Connection,
    key: &str,
    name: &str,
    value: Option<&str>,
) -> Result<(), Error> {
    match value {
        Some(value) => conn.execute(
            "INSERT INTO game_prefs (key, name, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(key, name) DO UPDATE SET value = excluded.value",
            params![key, name, value],
        )?,
        None => conn.execute(
            "DELETE FROM game_prefs WHERE key = ?1 AND name = ?2",
            [key, name],
        )?,
    };
    Ok(())
}

/// `game_prefs` names that mark a copy as a favorite or hidden; the value is "1", no row is off.
pub const FAVORITE: &str = "favorite";
pub const HIDDEN: &str = "hidden";

/// Favorites, hidden games and collection members, by copy key.
pub fn lists(conn: &Connection) -> Result<Lists, Error> {
    let mut lists = Lists::default();
    let mut stmt = conn.prepare("SELECT key, name FROM game_prefs WHERE name IN (?1, ?2)")?;
    for row in stmt.query_map([FAVORITE, HIDDEN], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (key, name) = row?;
        if name == FAVORITE {
            &mut lists.favorite
        } else {
            &mut lists.hidden
        }
        .insert(key);
    }
    let mut stmt =
        conn.prepare("SELECT key, collection_id FROM collection_games ORDER BY collection_id")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (key, id) = row?;
        lists.collections.entry(key).or_default().push(id);
    }
    Ok(lists)
}

/// Every collection as `(id, name)`, by name.
pub fn collections(conn: &Connection) -> Result<Vec<(i64, String)>, Error> {
    let mut stmt =
        conn.prepare("SELECT id, name FROM collections ORDER BY name COLLATE NOCASE, id")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Returns the new collection's id.
pub fn create_collection(conn: &Connection, name: &str) -> Result<i64, Error> {
    conn.execute("INSERT INTO collections (name) VALUES (?1)", [name])?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_collection(conn: &Connection, id: i64, name: &str) -> Result<(), Error> {
    conn.execute(
        "UPDATE collections SET name = ?2 WHERE id = ?1",
        params![id, name],
    )?;
    Ok(())
}

/// Deletes the collection and its members; the games themselves stay.
pub fn delete_collection(conn: &mut Connection, id: i64) -> Result<(), Error> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM collection_games WHERE collection_id = ?1",
        [id],
    )?;
    tx.execute("DELETE FROM collections WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(())
}

/// Adds the copy keys to the collection, or takes them out.
pub fn set_in_collection(
    conn: &mut Connection,
    id: i64,
    keys: &[String],
    on: bool,
) -> Result<(), Error> {
    let tx = conn.transaction()?;
    for key in keys {
        if on {
            tx.execute(
                "INSERT OR IGNORE INTO collection_games (collection_id, key) VALUES (?1, ?2)",
                params![id, key],
            )?;
        } else {
            tx.execute(
                "DELETE FROM collection_games WHERE collection_id = ?1 AND key = ?2",
                params![id, key],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn custom_row(r: &Row) -> rusqlite::Result<CustomGame> {
    Ok(CustomGame {
        id: r.get(0)?,
        name: r.get(1)?,
        exe: r.get(2)?,
        args: r.get(3)?,
        added: r.get(4)?,
    })
}

/// Every game the owner added, oldest first.
pub fn custom_games(conn: &Connection) -> Result<Vec<CustomGame>, Error> {
    let mut stmt =
        conn.prepare("SELECT id, name, exe, args, added FROM custom_games ORDER BY added, id")?;
    let rows = stmt.query_map([], custom_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn custom_game(conn: &Connection, id: &str) -> Result<Option<CustomGame>, Error> {
    Ok(conn
        .query_row(
            "SELECT id, name, exe, args, added FROM custom_games WHERE id = ?1",
            [id],
            custom_row,
        )
        .optional()?)
}

/// Adds the game, or updates it when its id is there; an update keeps its `added` time.
pub fn save_custom_game(conn: &Connection, g: &CustomGame) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO custom_games (id, name, exe, args, added) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, exe = excluded.exe, args = excluded.args",
        params![g.id, g.name, g.exe, g.args, g.added],
    )?;
    Ok(())
}

pub fn delete_custom_game(conn: &Connection, id: &str) -> Result<(), Error> {
    conn.execute("DELETE FROM custom_games WHERE id = ?1", [id])?;
    Ok(())
}

/// Every GOG install folder by copy key (`gog-<product_id>`). Written by the GOG install.
pub fn gog_installs(conn: &Connection) -> Result<HashMap<String, PathBuf>, Error> {
    let mut stmt = conn.prepare("SELECT key, path FROM gog_installs")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            PathBuf::from(r.get::<_, String>(1)?),
        ))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Records where a GOG game is installed, or forgets it for `None`.
pub fn set_gog_install(conn: &Connection, key: &str, path: Option<&Path>) -> Result<(), Error> {
    match path {
        Some(path) => conn.execute(
            "INSERT INTO gog_installs (key, path) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET path = excluded.path",
            params![key, path.to_string_lossy()],
        )?,
        None => conn.execute("DELETE FROM gog_installs WHERE key = ?1", [key])?,
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

pub fn put_info(
    conn: &Connection,
    key: &str,
    kind: &str,
    json: &str,
    now_secs: i64,
) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO info (key, kind, json, fetched) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(key, kind) DO UPDATE SET json = excluded.json, fetched = excluded.fetched",
        params![key, kind, json, now_secs],
    )?;
    Ok(())
}

pub fn delete_info(conn: &Connection, key: &str, kind: &str) -> Result<(), Error> {
    conn.execute("DELETE FROM info WHERE key = ?1 AND kind = ?2", [key, kind])?;
    Ok(())
}

/// Replaces every cached game of `provider` with `games` in one transaction.
pub fn replace(conn: &mut Connection, provider: ProviderKind, games: &[Game]) -> Result<(), Error> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM games WHERE provider = ?1", [provider.as_str()])?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO games (provider, external_id, title, installed, install_dir, playtime_minutes, last_played,
                cover, cover_url, header, header_url, hero, hero_url, size_bytes, dlc, update_available, cloud_saves)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
                g.last_played
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64),
                text(&a.cover.path),
                a.cover.url,
                text(&a.header.path),
                a.header.url,
                text(&a.hero.path),
                a.hero.url,
                g.size_bytes.map(|b| b as i64),
                (!g.dlc.is_empty()).then(|| serde_json::to_string(&g.dlc).unwrap_or_default()),
                g.update_available,
                g.cloud_saves,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Dlc;

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
                cover: Image {
                    path: Some("c.jpg".into()),
                    url: Some("https://x/c.jpg".into()),
                },
                hero: Image {
                    path: None,
                    url: Some("https://x/h.png".into()),
                },
                ..Art::default()
            },
            dlc: Vec::new(),
            update_available: false,
            cloud_saves: false,
        }
    }

    fn memory() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        conn
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("launcher-db-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A file as the v3 build left it: step 1 only, `user_version` 0, rollback journal.
    fn v3_file(path: &Path, settings: &[(&str, &str)]) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(STEPS[0]).unwrap();
        for (name, value) in settings {
            set_setting(&conn, name, Some(value)).unwrap();
        }
    }

    fn all_settings(conn: &Connection) -> Vec<(String, String)> {
        let mut stmt = conn
            .prepare("SELECT name, value FROM settings ORDER BY name")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn round_trips_and_replaces_per_provider() {
        let mut conn = memory();
        conn.execute("INSERT INTO games (provider, external_id, title, installed) VALUES ('future', 'x', 'X', 0)", []).unwrap();
        let (steam, epic) = (ProviderKind::Steam, ProviderKind::Epic);

        replace(&mut conn, steam, &[game(steam, "1"), game(steam, "2")]).unwrap();
        replace(&mut conn, epic, &[game(epic, "Fortnite")]).unwrap();
        replace(&mut conn, steam, &[game(steam, "2")]).unwrap();

        // Steam game 1 is gone, the Epic game is untouched, the unknown provider row is ignored.
        let mut loaded = load(&conn).unwrap();
        loaded.sort_by(|a, b| a.external_id.cmp(&b.external_id));
        assert_eq!(loaded, vec![game(steam, "2"), game(epic, "Fortnite")]);
    }

    #[test]
    fn games_round_trip_dlc_update_and_cloud_saves() {
        let mut conn = memory();
        let epic = ProviderKind::Epic;
        let mut g = game(epic, "Fallout");
        g.dlc = vec![
            Dlc {
                id: "LonesomeRoad".into(),
                title: "Lonesome Road".into(),
                installed: true,
            },
            Dlc {
                id: "OldWorldBlues".into(),
                title: "Old World Blues".into(),
                installed: false,
            },
        ];
        g.update_available = true;
        g.cloud_saves = true;
        replace(&mut conn, epic, &[g.clone(), game(epic, "Plain")]).unwrap();
        let mut loaded = load(&conn).unwrap();
        loaded.sort_by(|a, b| a.external_id.cmp(&b.external_id));
        assert_eq!(loaded, vec![g, game(epic, "Plain")]);
    }

    #[test]
    fn migrates_v3_file_keeping_settings_and_launches() {
        let dir = temp_dir("migrate");
        let path = dir.join("library.sqlite");
        v3_file(
            &path,
            &[("igdb_client_id", "abc"), ("steam_account", "noah")],
        );
        {
            let conn = Connection::open(&path).unwrap();
            record_launch(&conn, "steam-30", 1_000).unwrap();
            conn.execute(
                "INSERT INTO games (provider, external_id, title, installed, cover_url, logo, logo_url, size_bytes)
                 VALUES ('epic', 'Fallout', 'Fallout', 1, 'https://x/c.jpg', 'l.png', 'https://x/l.png', 42)",
                [],
            )
            .unwrap();
            put_info(&conn, "epic-Fallout", "cloud-saves", "off", 10).unwrap();
            put_info(&conn, "epic-Other", "cloud-saves", "default", 10).unwrap();
            put_info(&conn, "epic-Fallout", "cloud-saves-sync", r#"{"at":5}"#, 10).unwrap();
            put_info(&conn, "epic-Fallout", "twin", "{}", 10).unwrap();
            put_info(&conn, "epic-Fallout", "twin2", "{}", 10).unwrap();
            put_info(&conn, "epic-Fallout", "twin3", "[30]", 10).unwrap();
        }

        let conn = open(&path).unwrap();
        assert_eq!(user_version(&conn).unwrap(), STEPS.len());
        let settings = [
            ("igdb_client_id".into(), "abc".into()),
            ("steam_account".into(), "noah".into()),
        ];
        assert_eq!(all_settings(&conn), settings);
        assert_eq!(
            launches(&conn).unwrap(),
            HashMap::from([("steam-30".to_string(), 1_000)])
        );
        // The choice moved; "default" means no choice, so nothing moved for it.
        assert_eq!(
            pref(&conn, "epic-Fallout", "cloud_saves")
                .unwrap()
                .as_deref(),
            Some("off")
        );
        assert_eq!(pref(&conn, "epic-Other", "cloud_saves").unwrap(), None);
        let kinds: Vec<String> = conn
            .prepare("SELECT kind FROM info ORDER BY kind")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(kinds, ["cloud-saves-sync", "twin3"]);
        let games = load(&conn).unwrap();
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!((g.title.as_str(), g.size_bytes), ("Fallout", Some(42)));
        assert_eq!(g.art.cover.url.as_deref(), Some("https://x/c.jpg"));
        assert!(g.dlc.is_empty() && !g.update_available && !g.cloud_saves);
        assert!(
            !columns(&conn, "games")
                .iter()
                .any(|c| c.starts_with("logo"))
        );
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fresh_file_gets_latest_schema() {
        let dir = temp_dir("fresh");
        let conn = open(&dir.join("sub").join("library.sqlite")).unwrap();
        assert_eq!(user_version(&conn).unwrap(), STEPS.len());
        let journal: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(journal, "wal");
        let games = columns(&conn, "games");
        assert!(
            ["dlc", "update_available", "cloud_saves", "size_bytes"]
                .iter()
                .all(|c| games.iter().any(|g| g == c))
        );
        assert!(!games.iter().any(|c| c.starts_with("logo")));
        assert_eq!(columns(&conn, "game_prefs"), ["key", "name", "value"]);
        assert_eq!(columns(&conn, "playtime"), ["key", "started", "seconds"]);
        assert_eq!(columns(&conn, "collections"), ["id", "name"]);
        assert_eq!(columns(&conn, "collection_games"), ["collection_id", "key"]);
        assert_eq!(
            columns(&conn, "custom_games"),
            ["id", "name", "exe", "args", "added"]
        );
        assert_eq!(columns(&conn, "gog_installs"), ["key", "path"]);
        set_pref(&conn, "epic-A", "cloud_saves", Some("on")).unwrap();
        set_pref(&conn, "epic-A", "cloud_saves", Some("off")).unwrap();
        assert_eq!(
            pref(&conn, "epic-A", "cloud_saves").unwrap().as_deref(),
            Some("off")
        );
        set_pref(&conn, "epic-A", "cloud_saves", None).unwrap();
        assert_eq!(pref(&conn, "epic-A", "cloud_saves").unwrap(), None);
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_twice_is_a_no_op() {
        let dir = temp_dir("twice");
        let path = dir.join("library.sqlite");
        v3_file(&path, &[("a", "1")]);
        let first = open(&path).unwrap();
        set_pref(&first, "epic-A", "cloud_saves", Some("on")).unwrap();
        // A second connection while the first is open, then a fresh one after it closed.
        let second = open(&path).unwrap();
        assert_eq!(user_version(&second).unwrap(), STEPS.len());
        drop((first, second));
        let third = open(&path).unwrap();
        assert_eq!(all_settings(&third), [("a".into(), "1".into())]);
        assert_eq!(
            pref(&third, "epic-A", "cloud_saves").unwrap().as_deref(),
            Some("on")
        );
        drop(third);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn old_v3_file_is_renamed_and_kept() {
        let settings_of = |path: &Path| all_settings(&open(path).unwrap());

        // No file at all: a fresh library.sqlite.
        let dir = temp_dir("rename-none");
        assert_eq!(file_in(&dir), dir.join("library.sqlite"));
        std::fs::remove_dir_all(&dir).unwrap();

        // Only the v3 file: renamed, with its (empty, so not hot) journal.
        let dir = temp_dir("rename-only");
        v3_file(&dir.join("library-v3.sqlite"), &[("steam_account", "noah")]);
        std::fs::write(dir.join("library-v3.sqlite-journal"), b"").unwrap();
        let path = file_in(&dir);
        assert_eq!(path, dir.join("library.sqlite"));
        assert!(
            !dir.join("library-v3.sqlite").exists()
                && !dir.join("library-v3.sqlite-journal").exists()
        );
        assert!(dir.join("library.sqlite-journal").exists());
        assert_eq!(
            settings_of(&path),
            [("steam_account".into(), "noah".into())]
        );
        // From then on, nothing moves.
        assert_eq!(file_in(&dir), path);
        std::fs::remove_dir_all(&dir).unwrap();

        // Both files: library.sqlite wins and the v3 file is left alone.
        let dir = temp_dir("rename-both");
        v3_file(&dir.join("library-v3.sqlite"), &[("who", "old")]);
        drop(open(&dir.join("library.sqlite")).unwrap());
        set_setting(
            &open(&dir.join("library.sqlite")).unwrap(),
            "who",
            Some("new"),
        )
        .unwrap();
        let path = file_in(&dir);
        assert_eq!(path, dir.join("library.sqlite"));
        assert_eq!(settings_of(&path), [("who".into(), "new".into())]);
        let old = Connection::open(dir.join("library-v3.sqlite")).unwrap();
        assert_eq!(
            (all_settings(&old), user_version(&old).unwrap()),
            (vec![("who".into(), "old".into())], 0)
        );
        drop(old);
        std::fs::remove_dir_all(&dir).unwrap();

        // A library.sqlite from the first builds (a games table, no settings) is set aside, and v3 adopted.
        let dir = temp_dir("rename-leftover");
        v3_file(&dir.join("library-v3.sqlite"), &[("who", "v3")]);
        Connection::open(dir.join("library.sqlite"))
            .unwrap()
            .execute_batch(
                "CREATE TABLE games (provider TEXT, external_id TEXT, title TEXT, logo TEXT)",
            )
            .unwrap();
        let path = file_in(&dir);
        assert_eq!(path, dir.join("library.sqlite"));
        assert!(dir.join("library-v1.sqlite").exists() && !dir.join("library-v3.sqlite").exists());
        assert_eq!(settings_of(&path), [("who".into(), "v3".into())]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Another instance holding the v3 file open blocks the rename on Windows: it is copied instead.
    #[cfg(windows)]
    #[test]
    fn old_v3_file_held_open_is_copied() {
        let dir = temp_dir("rename-held");
        let old = dir.join("library-v3.sqlite");
        v3_file(&old, &[("who", "v3")]);
        let held = Connection::open(&old).unwrap();
        assert_eq!(all_settings(&held).len(), 1);
        let path = file_in(&dir);
        assert_eq!(path, dir.join("library.sqlite"));
        assert!(old.exists());
        assert_eq!(
            all_settings(&open(&path).unwrap()),
            [("who".into(), "v3".into())]
        );
        let tmp = |e: std::io::Result<std::fs::DirEntry>| {
            e.unwrap().path().extension() == Some("tmp".as_ref())
        };
        assert!(!std::fs::read_dir(&dir).unwrap().any(tmp));
        drop(held);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn step_two_file_gets_collections_and_keeps_prefs() {
        let dir = temp_dir("step3");
        let path = dir.join("library.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(STEPS[0]).unwrap();
            conn.execute_batch(STEPS[1]).unwrap();
            conn.pragma_update(None, "user_version", 2).unwrap();
            set_pref(&conn, "epic-A", FAVORITE, Some("1")).unwrap();
        }
        let conn = open(&path).unwrap();
        assert_eq!(user_version(&conn).unwrap(), STEPS.len());
        assert_eq!(
            pref(&conn, "epic-A", FAVORITE).unwrap().as_deref(),
            Some("1")
        );
        assert!(collections(&conn).unwrap().is_empty());
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn step_three_file_gets_custom_games_and_keeps_collections() {
        let dir = temp_dir("step4");
        let path = dir.join("library.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            for step in &STEPS[..3] {
                conn.execute_batch(step).unwrap();
            }
            conn.pragma_update(None, "user_version", 3).unwrap();
            create_collection(&conn, "Backlog").unwrap();
        }
        let conn = open(&path).unwrap();
        assert_eq!(user_version(&conn).unwrap(), STEPS.len());
        assert_eq!(collections(&conn).unwrap().len(), 1);
        assert!(custom_games(&conn).unwrap().is_empty());
        // `args` may be left out.
        conn.execute(
            "INSERT INTO custom_games (id, name, exe, added) VALUES ('a', 'A', 'a.exe', 5)",
            [],
        )
        .unwrap();
        assert_eq!(custom_game(&conn, "a").unwrap().unwrap().args, "");
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn step_four_file_gets_gog_installs_and_keeps_custom_games() {
        let dir = temp_dir("step5");
        let path = dir.join("library.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            for step in &STEPS[..4] {
                conn.execute_batch(step).unwrap();
            }
            conn.pragma_update(None, "user_version", 4).unwrap();
            conn.execute(
                "INSERT INTO custom_games (id, name, exe, added) VALUES ('a', 'A', 'a.exe', 5)",
                [],
            )
            .unwrap();
        }
        let conn = open(&path).unwrap();
        assert_eq!(user_version(&conn).unwrap(), STEPS.len());
        assert_eq!(custom_games(&conn).unwrap().len(), 1);
        assert!(gog_installs(&conn).unwrap().is_empty());
        conn.execute(
            r"INSERT INTO gog_installs (key, path) VALUES ('gog-1', 'C:\Games\A')",
            [],
        )
        .unwrap();
        assert_eq!(
            gog_installs(&conn).unwrap(),
            HashMap::from([("gog-1".to_string(), PathBuf::from(r"C:\Games\A"))])
        );
        set_gog_install(&conn, "gog-1", Some(Path::new(r"D:\Games\A"))).unwrap();
        set_gog_install(&conn, "gog-2", Some(Path::new(r"C:\Games\B"))).unwrap();
        assert_eq!(
            gog_installs(&conn).unwrap()["gog-1"],
            PathBuf::from(r"D:\Games\A")
        );
        set_gog_install(&conn, "gog-1", None).unwrap();
        assert_eq!(
            gog_installs(&conn).unwrap().into_keys().collect::<Vec<_>>(),
            ["gog-2"]
        );
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn custom_games_save_update_and_delete() {
        let conn = memory();
        let mut g = CustomGame {
            id: "b".into(),
            name: "Hollow Knight".into(),
            exe: r"D:\hk.exe".into(),
            args: String::new(),
            added: 20,
        };
        let first = CustomGame {
            id: "a".into(),
            name: "Zork".into(),
            exe: r"C:\zork.exe".into(),
            args: "-x".into(),
            added: 10,
        };
        save_custom_game(&conn, &g).unwrap();
        save_custom_game(&conn, &first).unwrap();
        assert_eq!(custom_games(&conn).unwrap(), [first.clone(), g.clone()]);
        // An edit keeps the time it was added.
        g.name = "Hollow Knight: Voidheart".into();
        g.args = "-windowed".into();
        save_custom_game(
            &conn,
            &CustomGame {
                added: 99,
                ..g.clone()
            },
        )
        .unwrap();
        assert_eq!(custom_game(&conn, "b").unwrap(), Some(g));
        delete_custom_game(&conn, "b").unwrap();
        assert_eq!(custom_game(&conn, "b").unwrap(), None);
        assert_eq!(custom_games(&conn).unwrap(), [first]);
    }

    #[test]
    fn lists_read_favorites_hidden_and_collections() {
        let mut conn = memory();
        set_pref(&conn, "steam-1", FAVORITE, Some("1")).unwrap();
        set_pref(&conn, "epic-A", HIDDEN, Some("1")).unwrap();
        set_pref(&conn, "epic-A", "cloud_saves", Some("on")).unwrap();
        let backlog = create_collection(&conn, "Backlog").unwrap();
        let coop = create_collection(&conn, "co-op").unwrap();
        let keys = ["steam-1".to_string(), "epic-B".to_string()];
        set_in_collection(&mut conn, backlog, &keys, true).unwrap();
        set_in_collection(&mut conn, backlog, &keys, true).unwrap();
        set_in_collection(&mut conn, coop, &keys[..1], true).unwrap();
        let found = lists(&conn).unwrap();
        assert_eq!(found.favorite, ["steam-1".to_string()].into());
        assert_eq!(found.hidden, ["epic-A".to_string()].into());
        assert_eq!(found.collections["steam-1"], [backlog, coop]);
        assert_eq!(found.collections["epic-B"], [backlog]);
        assert_eq!(
            collections(&conn).unwrap(),
            [(backlog, "Backlog".into()), (coop, "co-op".into())]
        );

        rename_collection(&conn, coop, "Alone").unwrap();
        assert_eq!(
            collections(&conn).unwrap(),
            [(coop, "Alone".into()), (backlog, "Backlog".into())]
        );
        set_in_collection(&mut conn, backlog, &keys[1..], false).unwrap();
        delete_collection(&mut conn, coop).unwrap();
        assert_eq!(collections(&conn).unwrap(), [(backlog, "Backlog".into())]);
        assert_eq!(
            lists(&conn).unwrap().collections,
            HashMap::from([("steam-1".to_string(), vec![backlog])])
        );
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
    fn own_playtime_adds_session_length() {
        let conn = memory();
        assert_eq!(own_totals(&conn).unwrap(), HashMap::new());
        add_session(&conn, "epic-Fortnite", 1_000, 1_800).unwrap();
        add_session(&conn, "epic-Fortnite", 5_000, 45).unwrap();
        add_session(&conn, "epic-Pumpkin", 9_000, 600).unwrap();
        // A clock that went backwards never takes time away.
        add_session(&conn, "epic-Pumpkin", 9_500, -30).unwrap();
        let totals = own_totals(&conn).unwrap();
        assert_eq!(
            totals,
            HashMap::from([
                ("epic-Fortnite".to_string(), 1_845),
                ("epic-Pumpkin".to_string(), 600)
            ])
        );
    }

    #[test]
    fn settings_set_read_and_delete() {
        let conn = memory();
        set_setting(&conn, "igdb_client_id", Some("abc")).unwrap();
        assert_eq!(
            setting(&conn, "igdb_client_id").unwrap().as_deref(),
            Some("abc")
        );
        set_setting(&conn, "igdb_client_id", None).unwrap();
        assert_eq!(setting(&conn, "igdb_client_id").unwrap(), None);
    }

    #[test]
    fn cached_info_respects_max_age() {
        let conn = memory();
        put_info(&conn, "steam-1", "details", r#"{"a":1}"#, 100).unwrap();
        assert_eq!(
            cached_info(&conn, "steam-1", "details", 50, 120)
                .unwrap()
                .as_deref(),
            Some(r#"{"a":1}"#)
        );
        assert_eq!(
            cached_info(&conn, "steam-1", "details", 50, 200).unwrap(),
            None
        );
        assert_eq!(cached_info(&conn, "steam-1", "ttb", 50, 120).unwrap(), None);
    }
}
