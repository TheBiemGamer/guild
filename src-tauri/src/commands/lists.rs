//! Favorites, hidden games and collections. Stored per copy key, so setting one on a game sets every copy.

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
    /// Up to three entry keys of its games, most recently played first.
    covers: Vec<String>,
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
    // Held to the publish below, so two changes can't publish their reads out of order.
    let _turn = state.lists_write.lock().expect("lists lock poisoned");
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
        Ok(list
            .into_iter()
            .map(|(id, name)| {
                let (count, covers) = collection_summary(id, &s.entries, &s.games, &s.lists);
                Collection {
                    id,
                    name,
                    count,
                    covers,
                }
            })
            .collect())
    })
    .await
}

/// How many games a collection holds, hidden ones left out, and up to three of their keys, most recently
/// played first. `entries` are in title order, so the stable sort puts never-played ones after, by title.
fn collection_summary(
    id: i64,
    entries: &[entries::Entry],
    games: &[crate::library::Game],
    lists: &crate::view::Lists,
) -> (usize, Vec<String>) {
    let mut members: Vec<&entries::Entry> = entries
        .iter()
        .filter(|e| !lists.hidden(e, games) && lists.collections(e, games).contains(&id))
        .collect();
    members.sort_by_key(|e| std::cmp::Reverse(entries::last_played(e, games).map(|p| p.0)));
    let covers = members.iter().take(3).map(|e| e.key.clone()).collect();
    (members.len(), covers)
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

    #[test]
    fn collection_summary_skips_hidden_and_takes_three_latest_covers() {
        use crate::view::tests::game;
        use std::collections::{HashMap, HashSet};
        // Titles sort a..e; played 1..4 for a..d, e never, and "b" hidden.
        let games: Vec<_> = [("a", 1), ("b", 2), ("c", 3), ("d", 4), ("e", 0)]
            .into_iter()
            .map(|(t, n)| game(t, true, None, (n > 0).then_some(n)))
            .collect();
        let entries = entries::group(&games, &HashMap::new());
        let keys: Vec<String> = games.iter().map(|g| g.key()).collect();
        let lists = crate::view::Lists {
            hidden: HashSet::from([keys[1].clone()]),
            collections: keys.iter().map(|k| (k.clone(), vec![7])).collect(),
            ..Default::default()
        };
        let (count, covers) = collection_summary(7, &entries, &games, &lists);
        assert_eq!(count, 4);
        assert_eq!(covers, [keys[3].clone(), keys[2].clone(), keys[0].clone()]);
        assert_eq!(collection_summary(8, &entries, &games, &lists), (0, vec![]));
    }
}
