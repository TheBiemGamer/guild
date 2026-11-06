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
        let steam = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam").ok()?;
        let id = running_id(steam.get_value("RunningAppID"))?;
        // After a Steam crash RunningAppID keeps its last value (seen on this PC), so only
        // trust it while the Steam process it was written by is still alive.
        let pid: u32 = steam.open_subkey("ActiveProcess").and_then(|k| k.get_value("pid")).ok()?;
        steam_alive(pid).then_some(id)
    }

    fn download_progress(&self, game: &Game) -> Option<Progress> {
        let (app, _) = SteamDir::locate().ok()?.find_app(game.external_id.parse().ok()?).ok()??;
        progress(&app)
    }
}

/// Active download progress from an appmanifest; `None` once complete or never started.
fn progress(app: &App) -> Option<Progress> {
    let (done, total) = (app.bytes_downloaded?, app.bytes_to_download?);
    (total > 0 && done < total).then(|| Progress { fraction: done as f32 / total as f32, text: download_label(done, total) })
}

fn download_label(done: u64, total: u64) -> String {
    let gb = |b: u64| b as f64 / (1u64 << 30) as f64;
    format!("Downloading {:.1} of {:.1} GB ({}%)", gb(done), gb(total), done * 100 / total.max(1))
}

/// The newest `userdata/<account>/config/localconfig.vdf`.
// shortcut: with several Steam accounts on this PC it follows whoever used Steam last; read loginusers.vdf if that's wrong.
fn newest_localconfig(steam_root: &Path) -> Option<PathBuf> {
    std::fs::read_dir(steam_root.join("userdata"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("config").join("localconfig.vdf"))
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .max()
        .map(|(_, path)| path)
}

fn read_localconfig(steam_root: &Path) -> Option<String> {
    let path = newest_localconfig(steam_root)?;
    std::fs::read_to_string(&path)
        .inspect_err(|e| eprintln!("steam {}: {e}", path.display()))
        .ok()
}

fn play_stats(steam_root: &Path) -> PlayStats {
    read_localconfig(steam_root).map(|text| parse_play_stats(&text)).unwrap_or_default()
}

/// The Steam account name shown in Settings, from the same file as `play_stats`.
pub fn persona_name(steam_root: &Path) -> Option<String> {
    parse_persona_name(&read_localconfig(steam_root)?)
}

fn parse_persona_name(text: &str) -> Option<String> {
    let vdf = Vdf::from(keyvalues_parser::parse(text).ok()?);
    let name = child(child(&vdf.value, "friends")?, "PersonaName")?.get_str()?;
    (!name.is_empty()).then(|| name.to_owned())
}

fn parse_play_stats(text: &str) -> PlayStats {
    let vdf = match keyvalues_parser::parse(text).map(Vdf::from) {
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

/// Whether `pid` is a live steam.exe (the name check guards against a reused pid).
fn steam_alive(pid: u32) -> bool {
    super::process_listed(&format!("PID eq {pid}"), "steam.exe")
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
    // Mid-update games keep FullyInstalled; first-time downloads don't have it yet.
    let installed = app.state_flags.is_some_and(|f| f.flags().any(|f| f == StateFlag::FullyInstalled));
    Game {
        provider: ProviderKind::Steam,
        external_id: app.app_id.to_string(),
        title: app.name.clone().unwrap_or_else(|| app.install_dir.clone()),
        installed,
        install_dir: Some(library.resolve_app_dir(app)),
        // Filled from localconfig.vdf by list_games.
        playtime_minutes: None,
        last_played: None,
        size_bytes: app.size_on_disk.filter(|_| installed),
        art: art(app.app_id, &librarycache.join(app.app_id.to_string())),
    }
}

/// Local files where Steam has them, with CDN URLs as the fallback for each slot.
/// The CDN still serves these legacy names for new apps too (404s are handled by the downloader).
fn art(app_id: u32, librarycache_dir: &Path) -> Art {
    let mut art = local_art(librarycache_dir);
    let cdn = |file| Some(format!("{CDN}/{app_id}/{file}"));
    art.cover.url = cdn("library_600x900.jpg");
    art.header.url = cdn("header.jpg");
    art.hero.url = cdn("library_hero.jpg");
    art.logo.url = cdn("logo.png");
    art
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
        slot.path.get_or_insert(path);
    }
    art
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
        assert_eq!(game.size_bytes, Some(16_521_526));
        assert_eq!(game.title, "Fixture Shooter");
        assert!(game.installed);
        assert_eq!(game.install_dir, Some(fixtures().join("steamapps/common/FixtureShooter")));
        assert_eq!(game.key(), "steam-10");
    }

    #[test]
    fn first_download_is_not_installed() {
        assert!(!games()[1].installed);
        assert_eq!(games()[1].size_bytes, None);
    }

    #[test]
    fn missing_name_falls_back_to_install_dir() {
        assert_eq!(games()[2].title, "NamelessDir");
    }

    #[test]
    fn finds_flat_art() {
        let art = &games()[0].art;
        let dir = fixtures().join("appcache/librarycache/10");
        assert_eq!(art.cover.path, Some(dir.join("library_600x900.jpg")));
        assert_eq!(art.header.path, Some(dir.join("header.jpg")));
        assert_eq!(art.logo.path, Some(dir.join("logo.png")));
        assert_eq!(art.hero.path, None);
    }

    #[test]
    fn finds_hashed_subfolder_art() {
        let art = &games()[1].art;
        let dir = fixtures().join("appcache/librarycache/20");
        assert_eq!(art.cover.path, Some(dir.join("aaa111/library_capsule.jpg")));
        assert_eq!(art.hero.path, Some(dir.join("bbb222/library_hero.jpg")));
        assert_eq!(art.header.path, None);
    }

    #[test]
    fn every_slot_has_a_cdn_fallback() {
        let art = &games()[2].art;
        assert_eq!(art.cover.path, None);
        assert_eq!(art.cover.url.as_deref(), Some(format!("{CDN}/30/library_600x900.jpg").as_str()));
        assert_eq!(art.logo.url.as_deref(), Some(format!("{CDN}/30/logo.png").as_str()));
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
    fn reads_persona_name_from_newest_localconfig() {
        assert_eq!(persona_name(&fixtures()).as_deref(), Some("fixture"));
        assert_eq!(persona_name(&fixtures().join("nowhere")), None);
        assert_eq!(parse_persona_name("\"UserLocalConfigStore\" { \"friends\" { } }"), None);
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
        let p = progress(&app(20)).unwrap();
        assert_eq!(p.fraction, 0.25);
        assert_eq!(p.text, "Downloading 0.0 of 0.0 GB (25%)");
        // SNUG-style finished manifest: downloaded == to_download.
        assert_eq!(progress(&app(10)), None);
        // No byte counters at all.
        assert_eq!(progress(&app(30)), None);
    }

    #[test]
    fn download_label_handles_zero_total() {
        assert_eq!(download_label(1 << 30, 4 << 30), "Downloading 1.0 of 4.0 GB (25%)");
        assert_eq!(download_label(0, 0), "Downloading 0.0 of 0.0 GB (0%)");
    }

    #[test]
    fn running_id_ignores_zero_and_missing_key() {
        assert_eq!(running_id(Ok(447040)), Some("447040".into()));
        assert_eq!(running_id(Ok(0)), None);
        assert_eq!(running_id(Err(std::io::ErrorKind::NotFound.into())), None);
    }

    #[test]
    fn no_cache_dir_means_no_local_art() {
        let art = &games()[2].art;
        assert!([&art.cover, &art.header, &art.hero, &art.logo].iter().all(|i| i.path.is_none()));
    }
}
