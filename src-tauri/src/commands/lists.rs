//! Favorites, hidden games and collections. Stored per copy key, so setting one on a game sets every copy.

use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use super::{AppState, Res, blocking, open_db};
use crate::cache;
use crate::entries;
use crate::msg::Msg;

#[derive(Serialize, specta::Type)]
pub struct Collection {
    id: i64,
    name: String,
    /// Games in it.
    count: usize,
}

/// Every copy key of the game with this key.
fn copy_keys(state: &AppState, key: &str) -> Res<Vec<String>> {
    let s = state.lock();
    let entry = entries::find(&s.entries, &s.games, key).ok_or(msg!(gameNotInLibrary))?;
    Ok(entry.copies.iter().map(|&i| s.games[i].key()).collect())
}

/// Runs a change on the database, then reads the lists again for the library.
fn change<T>(app: &AppHandle, f: impl FnOnce(&mut rusqlite::Connection) -> Res<T>) -> Res<T> {
    let state = app.state::<AppState>();
    let mut conn = open_db(&state).ok_or(msg!(noDb))?;
    let out = f(&mut conn)?;
    let lists = cache::db::lists(&conn)?;
    state.lock().lists = lists;
    Ok(out)
}

async fn set_flag(app: AppHandle, key: String, name: &'static str, on: bool) -> Res<()> {
    blocking(move || {
        let keys = copy_keys(&app.state::<AppState>(), &key)?;
        change(&app, |conn| {
            keys.iter()
                .try_for_each(|k| cache::db::set_pref(conn, k, name, on.then_some("1")))
                .map_err(Msg::from)
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_favorite(app: AppHandle, key: String, on: bool) -> Res<()> {
    set_flag(app, key, cache::db::FAVORITE, on).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_hidden(app: AppHandle, key: String, on: bool) -> Res<()> {
    set_flag(app, key, cache::db::HIDDEN, on).await
}

/// Every collection by name, with how many games are in it.
#[tauri::command]
#[specta::specta]
pub async fn collections(app: AppHandle) -> Res<Vec<Collection>> {
    blocking(move || {
        let state = app.state::<AppState>();
        let list = collections_in(&open_db(&state).ok_or(msg!(noDb))?)?;
        let s = state.lock();
        let mut counts: HashMap<i64, usize> = HashMap::new();
        for e in &s.entries {
            for id in s.lists.collections(e, &s.games) {
                *counts.entry(id).or_default() += 1;
            }
        }
        Ok(list
            .into_iter()
            .map(|(id, name)| Collection {
                id,
                count: counts.get(&id).copied().unwrap_or(0),
                name,
            })
            .collect())
    })
    .await
}

/// The name to store: trimmed, not empty, and not another collection's in any case.
fn check_name(name: &str, taken: &[(i64, String)], except: Option<i64>) -> Res<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(msg!(nameTheCollection));
    }
    let lower = name.to_lowercase();
    if taken
        .iter()
        .any(|(id, n)| Some(*id) != except && n.to_lowercase() == lower)
    {
        return Err(msg!(collectionExists, name = name));
    }
    Ok(name.to_string())
}

fn collections_in(conn: &rusqlite::Connection) -> Res<Vec<(i64, String)>> {
    cache::db::collections(conn).map_err(Msg::from)
}

/// Returns the new collection's id.
#[tauri::command]
#[specta::specta]
pub async fn create_collection(app: AppHandle, name: String) -> Res<i64> {
    blocking(move || {
        change(&app, |conn| {
            let name = check_name(&name, &collections_in(conn)?, None)?;
            cache::db::create_collection(conn, &name).map_err(Msg::from)
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn rename_collection(app: AppHandle, id: i64, name: String) -> Res<()> {
    blocking(move || {
        change(&app, |conn| {
            let name = check_name(&name, &collections_in(conn)?, Some(id))?;
            cache::db::rename_collection(conn, id, &name).map_err(Msg::from)
        })
    })
    .await
}

/// The games stay in the library.
#[tauri::command]
#[specta::specta]
pub async fn delete_collection(app: AppHandle, id: i64) -> Res<()> {
    blocking(move || {
        change(&app, |conn| {
            cache::db::delete_collection(conn, id).map_err(Msg::from)
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_in_collection(app: AppHandle, key: String, id: i64, on: bool) -> Res<()> {
    blocking(move || {
        let keys = copy_keys(&app.state::<AppState>(), &key)?;
        change(&app, |conn| {
            if !collections_in(conn)?.iter().any(|(c, _)| *c == id) {
                return Err(msg!(collectionGone));
            }
            cache::db::set_in_collection(conn, id, &keys, on).map_err(Msg::from)
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_unique_ignoring_case() {
        let taken = [(1, "Backlog".to_string()), (2, "Co-op".to_string())];
        assert_eq!(
            check_name("  Short games ", &taken, None).unwrap(),
            "Short games"
        );
        assert!(check_name("   ", &taken, None).is_err());
        assert!(check_name("backlog", &taken, None).is_err());
        // Renaming a collection to its own name in another case is fine; to another's isn't.
        assert_eq!(check_name("BACKLOG", &taken, Some(1)).unwrap(), "BACKLOG");
        assert!(check_name("co-op ", &taken, Some(1)).is_err());
    }
}
