//! One game: its page, art, details, play/install/uninstall, download size, how long to beat.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use super::library::rescan;
use super::{allow, blocking, open_db, unix_now, AppState, EpicWatch, Res};
use crate::cache;
use crate::igdb;
use crate::info::{self, GameInfo};
use crate::library::epic::{self, InstallEvent};
use crate::library::{Game, Image, ProviderKind};
use crate::settings;
use crate::view;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    key: String,
    title: String,
    provider: ProviderKind,
    installed: bool,
    /// Tile text, like `15 h 3 min`; `None` hides the tile (Epic doesn't report playtime).
    playtime_tile: Option<String>,
    /// Tile text, like `12 days ago`; `None` when never played.
    last_played_tile: Option<String>,
    /// Size on disk of an installed game, like `27.3 GB`.
    size: Option<String>,
    /// Only for an installed game (Steam knows a folder for games it hasn't installed yet).
    install_dir: Option<String>,
}

#[tauri::command]
pub fn game(state: State<'_, AppState>, key: String) -> Option<Detail> {
    let g = state.game(&key)?;
    Some(Detail {
        key,
        size: g.size_bytes.filter(|_| g.installed).map(view::size_label),
        install_dir: installed_dir(&g).map(|p| p.to_string_lossy().into_owned()),
        title: g.title,
        provider: g.provider,
        installed: g.installed,
        playtime_tile: view::playtime_tile(g.provider, g.playtime_minutes),
        last_played_tile: view::last_played_tile(g.last_played, SystemTime::now()),
    })
}

fn installed_dir(g: &Game) -> Option<&PathBuf> {
    g.install_dir.as_ref().filter(|_| g.installed)
}

/// The game's store page. The Legendary catalog has no product slug, so Epic gets a store search by title.
fn store_url(g: &Game) -> String {
    match g.provider {
        ProviderKind::Steam => format!("https://store.steampowered.com/app/{}", g.external_id),
        ProviderKind::Epic => {
            format!("https://store.epicgames.com/browse?q={}", crate::library::percent_encode(&g.title))
        }
    }
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

/// Opens the game's store page in the default browser.
#[tauri::command]
pub fn open_store_page(state: State<'_, AppState>, key: String) -> Res<()> {
    let game = state.game(&key).ok_or("This game is no longer in the library.")?;
    crate::library::open_uri(&store_url(&game)).map_err(|e| e.to_string())
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

async fn look_up_time_to_beat(
    state: &AppState,
    client_id: &str,
    secret: &str,
    game: &Game,
) -> Res<Option<igdb::TimeToBeat>> {
    let token = igdb::cached_token(&state.igdb, &state.http, client_id, secret).await?;
    let Some(id) = igdb::game_id(&state.http, &token, client_id, game.provider, &game.external_id).await? else {
        return Ok(None);
    };
    igdb::time_to_beat(&state.http, &token, client_id, id).await
}

/// How long to beat, from IGDB, matched by Steam app ID (Epic games have no usable IGDB ID, so
/// they get `None`). `None` too without saved credentials or when IGDB doesn't know the game.
/// Cached 30 days when found, 7 when not; a failed request is not cached.
// Attribution: IGDB asks for visible credit where its data is shown; the game page puts a small
// "Data from IGDB" link (`open_igdb`) under the tile.
#[tauri::command]
pub async fn time_to_beat(app: AppHandle, key: String) -> Option<TimeToBeatView> {
    let state = app.state::<AppState>();
    let game = state.game(&key).filter(|g| igdb::can_match(g.provider))?;
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
    let answer = match look_up_time_to_beat(&state, &client_id, &secret, &game).await {
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
    igdb::game_id(&state.http, &token, &client_id, ProviderKind::Steam, "447040").await?;
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

    #[test]
    fn store_url_links_steam_app_page() {
        assert_eq!(store_url(&game("447040", true, None, None)), "https://store.steampowered.com/app/447040");
    }

    #[test]
    fn store_url_searches_epic_store_by_encoded_title() {
        // The Legendary catalog carries no product slug, so Epic gets a store search.
        let mut epic_game = game("Assassin's Creed® Unity & more", false, None, None);
        epic_game.provider = ProviderKind::Epic;
        epic_game.external_id = "Flour".into();
        assert_eq!(
            store_url(&epic_game),
            "https://store.epicgames.com/browse?q=Assassin%27s%20Creed%C2%AE%20Unity%20%26%20more"
        );
    }
}
