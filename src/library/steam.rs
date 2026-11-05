//! Installed Steam games from appmanifests, plus art from Steam's local cache and CDN.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use keyvalues_parser::{Value, Vdf};
use steamlocate::app::StateFlag;
use steamlocate::{App, Library, SteamDir};

use super::{Art, Error, Game, Progress, Provider, ProviderKind};

/// Playtime in minutes and last played time, per app id.
type PlayStats = HashMap<u32, (Option<u64>, Option<SystemTime>)>;

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
        let stats = play_stats(steam.path());
        for game in &mut games {
            if let Some(&(playtime, last_played)) = game.external_id.parse().ok().and_then(|id| stats.get(&id)) {
                game.playtime_minutes = playtime;
                game.last_played = last_played;
            }
        }
        Ok(games)
    }

    /// Starts Steam first if it isn't running.
    fn launch(&self, game: &Game) -> Result<(), Error> {
        super::open_uri(&format!("steam://rungameid/{}", game.external_id))
    }

    /// Steam shows its own install dialog.
    fn install(&self, game: &Game) -> Result<(), Error> {
        super::open_uri(&format!("steam://install/{}", game.external_id))
    }

    /// Steam shows its own confirmation dialog.
    fn uninstall(&self, game: &Game) -> Result<(), Error> {
        super::open_uri(&format!("steam://uninstall/{}", game.external_id))
    }

    fn running_game(&self) -> Option<String> {
        let key = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam");
        running_id(key.and_then(|k| k.get_value("RunningAppID")))
    }

    fn download_progress(&self, game: &Game) -> Option<Progress> {
        let (app, _) = SteamDir::locate().ok()?.find_app(game.external_id.parse().ok()?).ok()??;
        progress(&app)
    }
}

/// Active download progress from an appmanifest; `None` once complete or never started.
fn progress(app: &App) -> Option<Progress> {
    let (done, total) = (app.bytes_downloaded?, app.bytes_to_download?);
    (total > 0 && done < total).then_some(Progress { done, total })
}

/// Reads the newest `userdata/<account>/config/localconfig.vdf`.
// shortcut: with several Steam accounts on this PC it follows whoever used Steam last; read loginusers.vdf if that's wrong.
fn play_stats(steam_root: &Path) -> PlayStats {
    let newest = std::fs::read_dir(steam_root.join("userdata"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("config").join("localconfig.vdf"))
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .max();
    let Some((_, path)) = newest else { return PlayStats::new() };
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_play_stats(&text),
        Err(e) => {
            eprintln!("steam {}: {e}", path.display());
            PlayStats::new()
        }
    }
}

fn parse_play_stats(text: &str) -> PlayStats {
    let vdf = match Vdf::parse(text) {
        Ok(vdf) => vdf,
        Err(e) => {
            eprintln!("steam localconfig.vdf: {e}");
            return PlayStats::new();
        }
    };
    let apps = ["Software", "Valve", "Steam", "apps"].iter().try_fold(&vdf.value, |v, key| child(v, key));
    let Some(apps) = apps.and_then(Value::get_obj) else { return PlayStats::new() };

    let mut stats = PlayStats::new();
    for (id, values) in &apps.0 {
        let (Ok(id), Some(app)) = (id.parse::<u32>(), values.first()) else { continue };
        let num = |key| child(app, key)?.get_str()?.parse::<u64>().ok();
        let playtime = num("Playtime");
        // 0 means "never" in some entries.
        let last_played = num("LastPlayed").filter(|&s| s > 0).map(|s| UNIX_EPOCH + Duration::from_secs(s));
        if playtime.is_some() || last_played.is_some() {
            stats.insert(id, (playtime, last_played));
        }
    }
    stats
}

/// Child value by key, ignoring case: Steam writes both "apps" and "Apps".
fn child<'a>(value: &'a Value<'a>, key: &str) -> Option<&'a Value<'a>> {
    value.get_obj()?.0.iter().find(|(k, _)| k.eq_ignore_ascii_case(key))?.1.first()
}

/// Steam writes 0 when nothing runs; a missing key means Steam was never installed for this user.
fn running_id(raw: std::io::Result<u32>) -> Option<String> {
    raw.ok().filter(|&id| id != 0).map(|id| id.to_string())
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
        // Filled from localconfig.vdf by list_games.
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

/// Hero and logo for the detail view: local first, else CDN, both fetched at once.
pub async fn hero_and_logo(game: &Game, http: &reqwest::Client, art_dir: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    let local_or_cdn = |local: &Option<PathBuf>, file| {
        let local = local.clone();
        async move {
            match local {
                Some(path) => Some(path),
                None => fetch_art(http, &game.external_id, file, art_dir).await,
            }
        }
    };
    iced::futures::join!(local_or_cdn(&game.art.hero, "library_hero.jpg"), local_or_cdn(&game.art.logo, "logo.png"))
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
    fn reads_play_stats_from_newest_localconfig() {
        let stats = play_stats(&fixtures());
        let at = |s| Some(UNIX_EPOCH + Duration::from_secs(s));
        assert_eq!(stats.get(&10), Some(&(Some(903), at(1_785_680_112))));
        // Playtime only; LastPlayed "0" means never.
        assert_eq!(stats.get(&30), Some(&(Some(5), None)));
        // Cloud-only entry with no stats is left out.
        assert_eq!(stats.get(&7), None);
    }

    /// Real-world check that this parser copes with a full localconfig.vdf: `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs a local Steam install"]
    fn real_localconfig_parses() {
        let steam = SteamDir::locate().unwrap();
        let stats = play_stats(steam.path());
        eprintln!("{} apps with play stats", stats.len());
        assert!(!stats.is_empty());
    }

    #[test]
    fn broken_localconfig_gives_no_stats() {
        assert!(parse_play_stats("\"UserLocalConfigStore\" {").is_empty());
        assert!(parse_play_stats("\"UserLocalConfigStore\" { }").is_empty());
    }

    #[test]
    fn progress_only_while_downloading() {
        let library = Library::from_dir(&fixtures()).unwrap();
        let app = |id| library.app(id).unwrap().unwrap();
        assert_eq!(progress(&app(20)), Some(Progress { done: 1_000, total: 4_000 }));
        // SNUG-style finished manifest: downloaded == to_download.
        assert_eq!(progress(&app(10)), None);
        // No byte counters at all.
        assert_eq!(progress(&app(30)), None);
    }

    #[test]
    fn running_id_ignores_zero_and_missing_key() {
        assert_eq!(running_id(Ok(447040)), Some("447040".into()));
        assert_eq!(running_id(Ok(0)), None);
        assert_eq!(running_id(Err(std::io::ErrorKind::NotFound.into())), None);
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
