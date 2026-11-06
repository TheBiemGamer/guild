//! One game: its page, art, details, play/install/uninstall, download size, how long to beat.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use super::library::rescan;
use super::{allow, blocking, open_db, unix_now, AppState, EpicWatch, Res};
use crate::cache;
use crate::entries::{self, Entry};
use crate::igdb;
use crate::info::{self, GameInfo};
use crate::library::epic::{self, InstallEvent};
use crate::library::{Game, Image, ProviderKind};
use crate::links::{self, LinkKind};
use crate::settings;
use crate::view;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    /// The entry key.
    key: String,
    title: String,
    stores: Vec<ProviderKind>,
    /// Any copy.
    installed: bool,
    copies: Vec<CopyView>,
    /// The copy key Play and Install use.
    chosen: String,
    /// Tile text, like `15 h 3 min`; `None` hides the tile (Epic doesn't report playtime).
    playtime_tile: Option<String>,
    /// `Steam + Epic` when both copies contributed to the playtime.
    playtime_note: Option<String>,
    /// Tile text, like `12 days ago`; `None` when never played.
    last_played_tile: Option<String>,
    /// `on Steam` / `on Epic Games`, only for an entry with several copies.
    last_played_store: Option<String>,
    /// Store pages, then info sites when the Steam app id is known.
    links: Vec<LinkKind>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyView {
    key: String,
    provider: ProviderKind,
    installed: bool,
    /// Size on disk of an installed copy, like `27.3 GB`.
    size: Option<String>,
    /// Only for an installed copy (Steam knows a folder for games it hasn't installed yet).
    install_dir: Option<String>,
}

#[tauri::command]
pub fn game(state: State<'_, AppState>, key: String) -> Option<Detail> {
    let s = state.lock();
    let entry = entries::find(&s.entries, &s.games, &key)?;
    Some(detail(entry, &s.games, &s.twins, SystemTime::now()))
}

fn detail(entry: &Entry, games: &[Game], twins: &HashMap<String, u32>, now: SystemTime) -> Detail {
    let (minutes, both) = entries::playtime(entry, games);
    // Steam's zero is a fact, so a Steam copy decides the tile; Epic alone has no playtime to show.
    let tile_provider = entry
        .copies
        .iter()
        .map(|&i| games[i].provider)
        .find(|&p| p == ProviderKind::Steam)
        .unwrap_or(games[entries::default_copy(entry, games)].provider);
    let last = entries::last_played(entry, games);
    let stores = entries::stores(entry, games);
    Detail {
        key: entry.key.clone(),
        title: games[entry.copies[0]].title.clone(),
        stores: stores.clone(),
        installed: entries::installed(entry, games),
        copies: entry
            .copies
            .iter()
            .map(|&i| {
                let g = &games[i];
                CopyView {
                    key: g.key(),
                    provider: g.provider,
                    installed: g.installed,
                    size: g.size_bytes.filter(|_| g.installed).map(view::size_label),
                    install_dir: installed_dir(g).map(|p| p.to_string_lossy().into_owned()),
                }
            })
            .collect(),
        chosen: games[entries::default_copy(entry, games)].key(),
        playtime_tile: view::playtime_tile(tile_provider, minutes),
        playtime_note: both.then(|| "Steam + Epic".to_string()),
        last_played_tile: view::last_played_tile(last.map(|(t, _)| t), now),
        last_played_store: last.filter(|_| entry.copies.len() > 1).map(|(_, p)| format!("on {}", p.name())),
        links: links::kinds(&stores, entries::steam_appid(entry, games, twins)),
    }
}

fn installed_dir(g: &Game) -> Option<&PathBuf> {
    g.install_dir.as_ref().filter(|_| g.installed)
}

/// Opens an installed game's folder in File Explorer.
#[tauri::command]
pub fn open_folder(state: State<'_, AppState>, key: String) -> Res<()> {
    let game = state.game(&key).ok_or("This game is no longer in the library.")?;
    let dir = installed_dir(&game).ok_or("This game has no install folder.")?;
    if !dir.is_dir() {
        return Err(format!("The install folder is missing: {}", dir.display()));
    }
    std::process::Command::new("explorer.exe").arg(dir).spawn().map(drop).map_err(|e| e.to_string())
}

/// Opens one of the game page's links in the default browser. The URL is built here from the kind.
#[tauri::command]
pub async fn open_link(app: AppHandle, key: String, kind: LinkKind) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (appid, title, epic_key) = {
            let s = state.lock();
            let entry = entries::find(&s.entries, &s.games, &key).ok_or("This game is no longer in the library.")?;
            let epic = entry.copies.iter().map(|&i| &s.games[i]).find(|g| g.provider == ProviderKind::Epic);
            (entries::steam_appid(entry, &s.games, &s.twins), s.games[entry.copies[0]].title.clone(), epic.map(|g| g.key()))
        };
        let epic_url = epic_key.and_then(|k| igdb::epic_url(&open_db(&state)?, &k));
        let url = links::url(kind, appid, epic_url.as_deref(), &title).ok_or("This game has no page there.")?;
        crate::library::open_uri(&url).map_err(|e| e.to_string())
    })
    .await
}

#[derive(Serialize, Default)]
pub struct DetailArt {
    hero: Option<String>,
}

#[tauri::command]
pub async fn detail_art(app: AppHandle, key: String) -> DetailArt {
    let state = app.state::<AppState>();
    let Some(game) = state.game(&key) else { return DetailArt::default() };
    let dir = state.dirs.art.join(&key);
    // The game page shows a text title, so the logo art isn't fetched.
    let hero = cache::resolve(&game.art.hero, &state.http, dir.join("hero")).await;
    DetailArt { hero: hero.and_then(|p| allow(&app, p)) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Play,
    Install,
    Uninstall,
}

/// Steam requests return at once (Steam shows its own dialogs). Epic requests return when
/// Legendary is done; installs report progress through `poll` meanwhile.
#[tauri::command]
pub async fn act(app: AppHandle, action: Action, key: String) -> Res<()> {
    let game = app.state::<AppState>().game(&key).ok_or("This game is no longer in the library.")?;
    blocking(move || act_blocking(&app, action, &game)).await
}

fn act_blocking(app: &AppHandle, action: Action, game: &Game) -> Res<()> {
    let state = app.state::<AppState>();
    let key = game.key();
    let store = game.provider.provider();
    if game.provider == ProviderKind::Steam {
        if action == Action::Install {
            state.lock().tracked.insert(key.clone());
        }
        let result = match action {
            Action::Play => store.launch(game).inspect(|()| record_play(&state, &key)),
            Action::Install => store.install(game),
            Action::Uninstall => store.uninstall(game),
        };
        return result.map_err(|e| format!("Steam: {e}"));
    }
    let result = match action {
        Action::Play => store.launch(game).map(|()| {
            record_play(&state, &key);
            // The game started either way; without its exe name it just isn't shown as running.
            let exe = epic::executable(game).unwrap_or_else(|e| {
                eprintln!("legendary list-installed {key}: {e}");
                None
            });
            state.lock().epic_watch = exe.map(|exe| EpicWatch { key: key.clone(), exe, since: Instant::now() });
        }),
        Action::Install => {
            let base = open_db(&state).map(|c| settings::load(&c).epic_install_dir).unwrap_or_default();
            let result = epic::install(game, base.as_deref(), |event| {
                let mut s = state.lock();
                match event {
                    InstallEvent::Started(pid) => _ = s.install_pids.insert(key.clone(), pid),
                    InstallEvent::Progress(p) => _ = s.progress.insert(key.clone(), p),
                }
            });
            let mut s = state.lock();
            s.install_pids.remove(&key);
            s.progress.remove(&key);
            // A cancelled install fails by design; don't report it.
            if s.cancelled.remove(&key) { Ok(()) } else { result }
        }
        Action::Uninstall => store.uninstall(game),
    };
    if action != Action::Play {
        rescan(app, true);
    }
    result.map_err(|e| format!("Epic Games: {e}"))
}

/// How long an Epic download size stays good; sizes change only with game updates.
const SIZE_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;
const INFO_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;

/// Download size of an Epic game that isn't installed, like `27.3 GB`, for the install popup and game page.
/// Asks Legendary online (slow, seconds), so the bytes are cached for a week. None if unknown.
#[tauri::command]
pub async fn download_size(app: AppHandle, key: String) -> Option<String> {
    let game = app.state::<AppState>().game(&key).filter(|g| g.provider == ProviderKind::Epic)?;
    let bytes = blocking(move || {
        let conn = open_db(&app.state::<AppState>());
        info::cached_or_fetch(conn.as_ref(), &key, "size", SIZE_MAX_AGE_SECS, unix_now(), || {
            let info = epic::info_json(&game).inspect_err(|e| eprintln!("legendary info {key}: {e}")).ok()?;
            epic::download_size_from_info(&info).map(|size| size.to_string())
        })
    })
    .await?;
    bytes.parse().ok().map(view::size_label)
}

/// Store details for the game page (description, genres, developer, screenshots).
/// Fetched once a week per game; a failed fetch is not cached, so the next visit retries.
/// An Epic game the catalog says nothing about is cached as `null` like the rest of its list.
/// Screenshots come back as local files, downloaded on first use.
#[tauri::command]
pub async fn game_info(app: AppHandle, key: String) -> Option<GameInfo> {
    let game = app.state::<AppState>().game(&key)?;
    let json = {
        let (app, key, game) = (app.clone(), key.clone(), game.clone());
        blocking(move || {
            let state = app.state::<AppState>();
            let conn = open_db(&state);
            let now = unix_now();
            let http = state.http.clone();
            info::cached_or_fetch(conn.as_ref(), &key, info::DETAILS, INFO_MAX_AGE_SECS, now, || match game.provider {
                ProviderKind::Steam => {
                    let url = format!(
                        "https://store.steampowered.com/api/appdetails?appids={}&l=english",
                        game.external_id
                    );
                    let body = tauri::async_runtime::block_on(async {
                        http.get(url).send().await?.error_for_status()?.text().await
                    })
                    .inspect_err(|e| eprintln!("appdetails {key}: {e}"))
                    .ok()?;
                    serde_json::to_string(&info::parse_steam_appdetails(&body, &game.external_id)?).ok()
                }
                // One list answers for every Epic game, so all of them are cached from it.
                ProviderKind::Epic => {
                    let list = epic::catalog().inspect_err(|e| eprintln!("legendary list {key}: {e}")).ok()?;
                    let all = info::epic_details(&list);
                    if let Some(c) = &conn
                        && let Err(e) = info::remember_all(c, &all, now)
                    {
                        eprintln!("epic details cache: {e}");
                    }
                    Some(all.into_iter().find(|(k, _)| *k == key).map_or_else(|| "null".into(), |(_, json)| json))
                }
            })
        })
        .await?
    };
    // `null` is a cached "nothing to show".
    let mut details: GameInfo = serde_json::from_str::<Option<GameInfo>>(&json).ok()??;
    let state = app.state::<AppState>();
    let dir = state.dirs.art.join(&key);
    // Downloaded side by side; awaited in order, so the screenshots keep theirs.
    let downloads: Vec<_> = details
        .screenshots
        .iter()
        .enumerate()
        .map(|(n, shot)| {
            let (http, dest) = (state.http.clone(), dir.join(format!("shot{n}")));
            let image = Image { path: None, url: Some(shot.clone()) };
            tauri::async_runtime::spawn(async move { cache::resolve(&image, &http, dest).await })
        })
        .collect();
    let mut shots = Vec::new();
    for download in downloads {
        if let Ok(Some(path)) = download.await {
            shots.extend(allow(&app, path));
        }
    }
    details.screenshots = shots;
    Some(details)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeToBeatView {
    normally: Option<String>,
    hastily: Option<String>,
    completely: Option<String>,
}

fn time_to_beat_view(t: igdb::TimeToBeat) -> Option<TimeToBeatView> {
    let label = |secs: Option<u64>| secs.filter(|&s| s > 0).map(view::hours_label);
    let shown = TimeToBeatView { normally: label(t.normally), hastily: label(t.hastily), completely: label(t.completely) };
    (shown.normally.is_some() || shown.hastily.is_some() || shown.completely.is_some()).then_some(shown)
}

/// A Steam app id matches by id; otherwise an Epic copy matches by exact title. The Epic page URL
/// IGDB reports is remembered for the copy.
async fn look_up_time_to_beat(
    app: &AppHandle,
    state: &AppState,
    client_id: &str,
    secret: &str,
    appid: Option<u32>,
    title: &str,
    epic_key: Option<&str>,
) -> Res<Option<igdb::TimeToBeat>> {
    let token = igdb::cached_token(&state.igdb, &state.http, client_id, secret).await?;
    let mut id = None;
    if let Some(appid) = appid {
        id = igdb::game_id_for_steam(&state.http, &token, client_id, appid).await?;
    }
    if let (None, Some(epic_key)) = (id, epic_key)
        && let Some(m) = igdb::epic_by_title(&state.http, &token, client_id, title).await?
    {
        id = Some(m.game);
        if let Some(url) = m.url {
            let epic_key = epic_key.to_string();
            let app = app.clone();
            blocking(move || {
                if let Some(conn) = open_db(&app.state::<AppState>()) {
                    igdb::remember_epic_url(&conn, &epic_key, &url);
                }
            })
            .await;
        }
    }
    let Some(id) = id else { return Ok(None) };
    igdb::time_to_beat(&state.http, &token, client_id, id).await
}

/// How long to beat, from IGDB, matched by Steam app ID (an Epic-only game by exact title, and
/// only when IGDB has one game for it). `None` too without saved credentials or when IGDB doesn't know the game.
/// Cached 30 days when found, 7 when not; a failed request is not cached.
// Attribution: IGDB asks for visible credit where its data is shown; the game page puts a small
// "Data from IGDB" link (`open_igdb`) under the tile.
#[tauri::command]
pub async fn time_to_beat(app: AppHandle, key: String) -> Option<TimeToBeatView> {
    let state = app.state::<AppState>();
    let (appid, title, epic_key) = {
        let s = state.lock();
        let entry = entries::find(&s.entries, &s.games, &key)?;
        let epic = entry.copies.iter().map(|&i| &s.games[i]).find(|g| g.provider == ProviderKind::Epic);
        (
            entries::steam_appid(entry, &s.games, &s.twins),
            epic.map_or_else(String::new, |g| g.title.clone()),
            epic.map(|g| g.key()),
        )
    };
    let now = unix_now();
    let (cached, credentials) = {
        let (app, key) = (app.clone(), key.clone());
        blocking(move || {
            let conn = open_db(&app.state::<AppState>());
            let cached = conn.as_ref().and_then(|c| igdb::cached(c, &key, now));
            if cached.is_some() {
                return (cached, None);
            }
            let client_id = conn.and_then(|c| settings::load(&c).igdb_client_id);
            (None, client_id.zip(settings::secret()))
        })
        .await
    };
    if let Some(answer) = cached {
        return answer.and_then(time_to_beat_view);
    }
    let (client_id, secret) = credentials?;
    let answer = match look_up_time_to_beat(&app, &state, &client_id, &secret, appid, &title, epic_key.as_deref()).await {
        Ok(answer) => answer,
        Err(e) => {
            eprintln!("how long to beat {key}: {e}");
            return None;
        }
    };
    {
        let (app, key) = (app.clone(), key.clone());
        blocking(move || {
            if let Some(conn) = open_db(&app.state::<AppState>()) {
                igdb::remember(&conn, &key, answer, now);
            }
        })
        .await;
    }
    answer.and_then(time_to_beat_view)
}

/// Settings "Test": gets a token with the saved credentials and looks up Steam app 447040.
/// Errors say what Twitch or IGDB answered and never contain the secret.
#[tauri::command]
pub async fn test_igdb(app: AppHandle) -> Res<()> {
    let state = app.state::<AppState>();
    let (client_id, secret) = {
        let app = app.clone();
        blocking(move || {
            let conn = cache::db::open(&app.state::<AppState>().dirs.db).map_err(|e| e.to_string())?;
            let client_id = settings::load(&conn).igdb_client_id.ok_or("Enter the Client ID first.")?;
            let secret = settings::secret().ok_or("Save the Client Secret first.")?;
            Ok::<_, String>((client_id, secret))
        })
        .await?
    };
    let token = igdb::token(&state.http, &client_id, &secret).await?;
    state.igdb.lock().expect("token cache lock poisoned").store(token.clone());
    igdb::game_id_for_steam(&state.http, &token, &client_id, 447040).await?;
    Ok(())
}

/// Remembers a launch from here, for every store, so last played survives rescans.
fn record_play(state: &AppState, key: &str) {
    let secs = unix_now();
    if let Some(g) = state.lock().games.iter_mut().find(|g| g.key() == key) {
        g.last_played = Some(UNIX_EPOCH + Duration::from_secs(secs as u64));
    }
    let saved = cache::db::open(&state.dirs.db).and_then(|c| cache::db::record_launch(&c, key, secs));
    if let Err(e) = saved {
        eprintln!("record launch: {e}");
    }
}

/// Stops a running Epic install; Legendary resumes it next time.
#[tauri::command]
pub async fn cancel_install(app: AppHandle, key: String) -> Res<()> {
    let pid = {
        let state = app.state::<AppState>();
        let mut s = state.lock();
        let pid = *s.install_pids.get(&key).ok_or("No install is running for this game.")?;
        s.cancelled.insert(key);
        pid
    };
    blocking(move || epic::cancel(pid).map_err(|e| format!("Couldn't cancel the install: {e}"))).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::tests::game;

    fn pumpkin(steam_last: Option<u64>, epic_last: Option<u64>) -> Vec<Game> {
        let steam = Game { external_id: "635320".into(), ..game("Pumpkin Jack", true, Some(120), steam_last) };
        let epic = Game {
            provider: ProviderKind::Epic,
            external_id: "Pumpkin".into(),
            ..game("Pumpkin Jack", false, None, epic_last)
        };
        vec![epic, steam]
    }

    fn pumpkin_detail(games: &[Game], twins: &HashMap<String, u32>) -> Detail {
        let entries = entries::group(games, twins);
        detail(&entries[0], games, twins, UNIX_EPOCH + Duration::from_secs(1_000 * 86_400))
    }

    #[test]
    fn detail_for_a_merged_entry() {
        let games = pumpkin(None, None);
        let d = pumpkin_detail(&games, &HashMap::from([("epic-Pumpkin".to_string(), 635320)]));
        assert_eq!(d.key, "steam-635320");
        assert_eq!(d.chosen, "steam-635320");
        assert_eq!(d.copies.len(), 2);
        assert_eq!(d.playtime_tile.as_deref(), Some("2 h 0 min"));
        assert_eq!(d.playtime_note, None);
        assert_eq!(d.links.len(), 5);
    }

    #[test]
    fn last_played_store_only_for_merged_entries() {
        let games = pumpkin(Some(86_400), Some(2 * 86_400));
        let merged = pumpkin_detail(&games, &HashMap::from([("epic-Pumpkin".to_string(), 635320)]));
        assert_eq!(merged.last_played_store.as_deref(), Some("on Epic Games"));
        let single = pumpkin_detail(&games, &HashMap::new());
        assert_eq!(single.last_played_store, None);
    }
}
