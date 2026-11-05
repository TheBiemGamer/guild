//! Installed Steam games from appmanifests, plus art from Steam's local cache and CDN.

use std::path::{Path, PathBuf};

use steamlocate::app::StateFlag;
use steamlocate::{App, Library, SteamDir};

use super::{Art, Error, Game, Provider, ProviderKind};

/// Shared redistributables Steam installs next to games.
// shortcut: hard-coded; filtering tools/SDKs properly needs the app "type" from appinfo.vdf, do that when they show up.
const NOT_GAMES: &[u32] = &[228980];

const CDN: &str = "https://shared.steamstatic.com/store_item_assets/steam/apps";

pub struct Steam;

impl Provider for Steam {
    fn list_games(&self) -> Result<Vec<Game>, Error> {
        let steam = SteamDir::locate()?;
        let librarycache = steam.path().join("appcache").join("librarycache");
        let mut games = Vec::new();
        for library in steam.libraries()? {
            match library {
                Ok(library) => games.extend(library_games(&library, &librarycache)),
                // e.g. a library on an unplugged drive; the others still load.
                Err(e) => eprintln!("steam library: {e}"),
            }
        }
        Ok(games)
    }
}

fn library_games(library: &Library, librarycache: &Path) -> Vec<Game> {
    library
        .apps()
        .filter_map(|app| match app {
            Ok(app) if NOT_GAMES.contains(&app.app_id) => None,
            Ok(app) => Some(to_game(library, &app, librarycache)),
            Err(e) => {
                eprintln!("steam manifest: {e}");
                None
            }
        })
        .collect()
}

fn to_game(library: &Library, app: &App, librarycache: &Path) -> Game {
    Game {
        provider: ProviderKind::Steam,
        external_id: app.app_id.to_string(),
        title: app.name.clone().unwrap_or_else(|| app.install_dir.clone()),
        // Mid-update games keep FullyInstalled; first-time downloads don't have it yet.
        installed: app.state_flags.is_some_and(|f| f.flags().any(|f| f == StateFlag::FullyInstalled)),
        install_dir: Some(library.resolve_app_dir(app)),
        // shortcut: playtime and last played live in userdata/<user>/config/localconfig.vdf; M3 reads them for sorting.
        playtime_minutes: None,
        last_played: None,
        art: local_art(&librarycache.join(app.app_id.to_string())),
    }
}

/// Art in `librarycache/<appid>`: flat files for older apps, one level of
/// hash-named subfolders (and `library_capsule`/`library_header` names) for newer ones.
fn local_art(dir: &Path) -> Art {
    let mut art = Art::default();
    let files = std::fs::read_dir(dir).into_iter().flatten().flatten().flat_map(|entry| {
        let path = entry.path();
        if path.is_dir() {
            std::fs::read_dir(&path).into_iter().flatten().flatten().map(|e| e.path()).collect()
        } else {
            vec![path]
        }
    });
    for path in files {
        let slot = match path.file_name().and_then(|n| n.to_str()) {
            Some("library_600x900.jpg" | "library_capsule.jpg") => &mut art.cover,
            Some("header.jpg" | "library_header.jpg") => &mut art.header,
            Some("library_hero.jpg") => &mut art.hero,
            Some("logo.png") => &mut art.logo,
            _ => continue,
        };
        slot.get_or_insert(path);
    }
    art
}

/// Best image for a grid card: local cover, CDN cover, local header, CDN header.
/// `None` means show a title card.
pub async fn cover_source(game: &Game, http: &reqwest::Client, art_dir: &Path) -> Option<PathBuf> {
    if let Some(cover) = &game.art.cover {
        return Some(cover.clone());
    }
    if let Some(cover) = fetch_art(http, &game.external_id, "library_600x900.jpg", art_dir).await {
        return Some(cover);
    }
    if let Some(header) = &game.art.header {
        return Some(header.clone());
    }
    fetch_art(http, &game.external_id, "header.jpg", art_dir).await
}

/// Downloads `file` for `appid` into `art_dir/<appid>/` unless it is already there.
/// `None` on 404 (many apps lack some images) or any other failure.
// shortcut: 404s are retried every launch; persist misses if that ever costs noticeable startup time.
async fn fetch_art(http: &reqwest::Client, appid: &str, file: &str, art_dir: &Path) -> Option<PathBuf> {
    let path = art_dir.join(appid).join(file);
    if path.exists() {
        return Some(path);
    }
    let url = format!("{CDN}/{appid}/{file}");
    let bytes = match http.get(&url).send().await.and_then(|r| r.error_for_status()) {
        Ok(response) => response.bytes().await,
        Err(e) => Err(e),
    };
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(e) if e.status() == Some(reqwest::StatusCode::NOT_FOUND) => return None,
        Err(e) => {
            eprintln!("steam art {url}: {e}");
            return None;
        }
    };
    // Write then rename so a crash never leaves a truncated image behind.
    let tmp = path.with_extension("part");
    let written = std::fs::create_dir_all(path.parent()?)
        .and_then(|()| std::fs::write(&tmp, &bytes))
        .and_then(|()| std::fs::rename(&tmp, &path));
    match written {
        Ok(()) => Some(path),
        Err(e) => {
            eprintln!("steam art {}: {e}", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/steam")
    }

    fn games() -> Vec<Game> {
        let library = Library::from_dir(&fixtures()).unwrap();
        let mut games = library_games(&library, &fixtures().join("appcache/librarycache"));
        games.sort_by(|a, b| a.external_id.cmp(&b.external_id));
        games
    }

    #[test]
    fn maps_manifests_and_skips_redistributables_and_broken_files() {
        let ids: Vec<_> = games().iter().map(|g| g.external_id.clone()).collect();
        // 228980 is the redistributables package, 40 is an unparseable manifest.
        assert_eq!(ids, ["10", "20", "30"]);
    }

    #[test]
    fn maps_installed_game() {
        let game = &games()[0];
        assert_eq!(game.title, "Fixture Shooter");
        assert!(game.installed);
        assert_eq!(game.install_dir, Some(fixtures().join("steamapps/common/FixtureShooter")));
        assert_eq!(game.key(), "steam-10");
    }

    #[test]
    fn first_download_is_not_installed() {
        assert!(!games()[1].installed);
    }

    #[test]
    fn missing_name_falls_back_to_install_dir() {
        assert_eq!(games()[2].title, "NamelessDir");
    }

    #[test]
    fn finds_flat_art() {
        let art = &games()[0].art;
        let dir = fixtures().join("appcache/librarycache/10");
        assert_eq!(art.cover, Some(dir.join("library_600x900.jpg")));
        assert_eq!(art.header, Some(dir.join("header.jpg")));
        assert_eq!(art.logo, Some(dir.join("logo.png")));
        assert_eq!(art.hero, None);
    }

    #[test]
    fn finds_hashed_subfolder_art() {
        let art = &games()[1].art;
        let dir = fixtures().join("appcache/librarycache/20");
        assert_eq!(art.cover, Some(dir.join("aaa111/library_capsule.jpg")));
        assert_eq!(art.hero, Some(dir.join("bbb222/library_hero.jpg")));
        assert_eq!(art.header, None);
    }

    #[test]
    fn no_cache_dir_means_no_art() {
        assert_eq!(games()[2].art, Art::default());
    }

    /// Hits the real CDN: `cargo test -- --ignored`.
    #[test]
    #[ignore = "network"]
    fn cdn_downloads_cover_and_treats_404_as_missing() {
        let dir = std::env::temp_dir().join(format!("launcher-cdn-test-{}", std::process::id()));
        let http = reqwest::Client::new();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let cover = fetch_art(&http, "2988410", "library_600x900.jpg", &dir).await.unwrap();
            assert!(image::open(&cover).is_ok());
            // App 1 doesn't exist, so the CDN answers 404.
            assert_eq!(fetch_art(&http, "1", "library_600x900.jpg", &dir).await, None);
        });
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
