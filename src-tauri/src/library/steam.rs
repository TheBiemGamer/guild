//! Installed Steam games from appmanifests, plus art from Steam's local cache and CDN.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use keyvalues_parser::{Value, Vdf};
use steamlocate::app::StateFlag;
use steamlocate::{App, Library, SteamDir};

use super::{Art, Error, Game, Progress, ProgressAction, ProviderKind};
use crate::platform::steam_pid;
use crate::steam_owned::OwnedGame;

/// Playtime in minutes and last played time, per app id.
type PlayStats = HashMap<u32, (Option<u64>, Option<SystemTime>)>;

/// Shared redistributables Steam installs next to games: hidden when appinfo.vdf can't say an app's type.
const NOT_GAMES: &[u32] = &[228980];

/// App types (`common.type` in appinfo.vdf, lowercased) that aren't games: Proton, the Steam Linux Runtime, SteamVR
/// and the redistributables are `tool`, launch configs `config`.
const NOT_GAME_TYPES: &[&str] = &["tool", "config"];

const CDN: &str = "https://shared.steamstatic.com/store_item_assets/steam/apps";

pub fn list_games() -> Result<Vec<Game>, Error> {
    let steam = SteamDir::locate()?;
    let librarycache = steam.path().join("appcache").join("librarycache");
    let types = app_types(&steam.path().join("appcache").join("appinfo.vdf"));
    let mut games = Vec::new();
    for library in steam.libraries()? {
        match library {
            Ok(library) => games.extend(library_games(&library, &librarycache, types.as_ref())),
            // e.g. a library on an unplugged drive; the others still load.
            Err(e) => tracing::warn!("steam library: {e}"),
        }
    }
    let stats = play_stats(steam.path());
    for game in &mut games {
        if let Some(&(playtime, last_played)) =
            game.external_id.parse().ok().and_then(|id| stats.get(&id))
        {
            game.playtime_minutes = playtime;
            game.last_played = last_played;
        }
    }
    Ok(games)
}

/// Starts Steam first if it isn't running.
pub fn launch(game: &Game) -> Result<(), Error> {
    crate::platform::shell_open(format!("steam://rungameid/{}", game.external_id))
}

/// Steam shows its own install dialog.
pub fn install(game: &Game) -> Result<(), Error> {
    crate::platform::shell_open(format!("steam://install/{}", game.external_id))
}

/// Steam shows its own confirmation dialog.
pub fn uninstall(game: &Game) -> Result<(), Error> {
    crate::platform::shell_open(format!("steam://uninstall/{}", game.external_id))
}

pub fn running_game() -> Option<String> {
    let id = running_id(crate::platform::steam_running_app())?;
    // After a Steam crash RunningAppID keeps its last value (seen on this PC), so only
    // trust it while the Steam process it was written by is still alive.
    // shortcut: a Flatpak can't see host processes, so there a crash's stale id counts as running until Steam starts
    // again; find another liveness signal if that shows up.
    (crate::platform::flatpak() || steam_alive(steam_pid()?)).then_some(id)
}

pub fn download_progress(game: &Game) -> Option<Progress> {
    let (app, _) = SteamDir::locate()
        .ok()?
        .find_app(game.external_id.parse().ok()?)
        .ok()??;
    let pid = steam_pid();
    let writes = pid.and_then(crate::platform::process_bytes_written);
    let mut anchors = ANCHORS.lock().unwrap_or_else(|e| e.into_inner());
    progress(&app, &mut anchors, pid.zip(writes), Instant::now())
}

/// Steam rewrites an appmanifest only when a download pauses or ends, so its byte counters stand still
/// while it runs. Between rewrites the bytes steam.exe writes to disk follow the staged bytes closely
/// (measured on this PC), so progress is the manifest's `BytesStaged` plus steam.exe's writes since then.
// shortcut: other writes Steam makes meanwhile (cloud saves, workshop) count too, and with several
// half-done downloads queued in Steam each one moves with the writes; the manifest's flags are as stale as its counters.
struct Anchor {
    /// Steam's pid and the manifest's `BytesStaged` when this anchor was set; either changing resets it.
    pid: u32,
    staged0: u64,
    writes0: u64,
    /// The previous sample, for the speed.
    at: Instant,
    staged: u64,
    /// Bytes staged per second, smoothed.
    rate: f64,
}

static ANCHORS: Mutex<Option<HashMap<u32, Anchor>>> = Mutex::new(None);

/// Every owned game as a not-installed `Game` with its art. Built once per owned list: the art lookup
/// reads the disk for every game.
pub fn owned_games(owned: &[OwnedGame], librarycache: &Path) -> Vec<Game> {
    owned
        .iter()
        .map(|o| {
            let id = o.appid.to_string();
            Game {
                provider: ProviderKind::Steam,
                title: o.name.clone(),
                installed: false,
                install_dir: None,
                playtime_minutes: Some(o.playtime_minutes),
                last_played: o.last_played.map(|s| UNIX_EPOCH + Duration::from_secs(s)),
                size_bytes: None,
                art: art(o.appid, &librarycache.join(&id)),
                dlc: Vec::new(),
                update_available: false,
                cloud_saves: false,
                external_id: id,
            }
        })
        .collect()
}

/// The installed games plus every other owned game (from `owned_games`). Owned playtime wins for every
/// match; Steam's last played only fills in where the local one is missing.
pub fn with_owned(mut installed: Vec<Game>, owned: &[Game]) -> Vec<Game> {
    let mut known: HashMap<String, usize> = installed
        .iter()
        .enumerate()
        .map(|(i, g)| (g.external_id.clone(), i))
        .collect();
    for o in owned {
        if let Some(&i) = known.get(&o.external_id) {
            installed[i].playtime_minutes = o.playtime_minutes;
            installed[i].last_played = installed[i].last_played.or(o.last_played);
            continue;
        }
        known.insert(o.external_id.clone(), installed.len());
        installed.push(o.clone());
    }
    installed
}

/// Active download progress from an appmanifest, moved on by steam.exe's writes (`steam` is its pid and
/// bytes written so far); `None` once complete or never started.
fn progress(
    app: &App,
    anchors: &mut Option<HashMap<u32, Anchor>>,
    steam: Option<(u32, u64)>,
    now: Instant,
) -> Option<Progress> {
    let (done, total) = (app.bytes_downloaded?, app.bytes_to_download?);
    if total == 0 || done >= total {
        if let Some(a) = anchors.as_mut() {
            a.remove(&app.app_id);
        }
        return None;
    }
    let (staged0, to_stage) = (
        app.bytes_staged.unwrap_or(0),
        app.bytes_to_stage.unwrap_or(0),
    );
    let Some((pid, writes)) = steam.filter(|_| to_stage > staged0) else {
        return Some(download(done as f32 / total as f32, done, total, None));
    };
    let fresh = || Anchor {
        pid,
        staged0,
        writes0: writes,
        at: now,
        staged: staged0,
        rate: 0.0,
    };
    let a = anchors
        .get_or_insert_with(HashMap::new)
        .entry(app.app_id)
        .or_insert_with(fresh);
    if a.pid != pid || a.staged0 != staged0 || writes < a.writes0 {
        *a = fresh();
    }
    // Capped at 99%: only the manifest says when it is done.
    let staged = (staged0 + (writes - a.writes0))
        .min(to_stage / 100 * 99)
        .max(staged0);
    let dt = now.duration_since(a.at).as_secs_f64();
    if dt >= 1.0 {
        let r = (staged - a.staged) as f64 / dt;
        a.rate = if a.rate == 0.0 {
            r
        } else {
            0.3 * r + 0.7 * a.rate
        };
        (a.at, a.staged) = (now, staged);
    }
    let secs_left = (a.rate > 0.0).then(|| ((to_stage - staged) as f64 / a.rate) as u64);
    let fraction = staged as f32 / to_stage as f32;
    Some(download(
        fraction,
        (fraction as f64 * total as f64) as u64,
        total,
        secs_left,
    ))
}

fn download(fraction: f32, done: u64, total: u64, secs_left: Option<u64>) -> Progress {
    Progress {
        fraction,
        action: ProgressAction::Download,
        done_bytes: Some(done),
        total_bytes: Some(total),
        secs_left,
        done_files: None,
        total_files: None,
    }
}

/// The newest `userdata/<account>/config/localconfig.vdf`.
// shortcut: with several Steam accounts on this PC it follows whoever used Steam last; read loginusers.vdf if that's wrong.
fn newest_localconfig(steam_root: &Path) -> Option<PathBuf> {
    fs_err::read_dir(steam_root.join("userdata"))
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
    fs_err::read_to_string(&path)
        .inspect_err(|e| tracing::warn!("steam {}: {e}", path.display()))
        .ok()
}

fn play_stats(steam_root: &Path) -> PlayStats {
    read_localconfig(steam_root)
        .map(|text| parse_play_stats(&text))
        .unwrap_or_default()
}

fn parse_play_stats(text: &str) -> PlayStats {
    let vdf = match keyvalues_parser::parse(text).map(Vdf::from) {
        Ok(vdf) => vdf,
        Err(e) => {
            tracing::warn!("steam localconfig.vdf: {e}");
            return PlayStats::new();
        }
    };
    let apps = ["Software", "Valve", "Steam", "apps"]
        .iter()
        .try_fold(&vdf.value, |v, key| child(v, key));
    let Some(apps) = apps.and_then(Value::get_obj) else {
        return PlayStats::new();
    };

    let mut stats = PlayStats::new();
    for (id, values) in &apps.0 {
        let (Ok(id), Some(app)) = (id.parse::<u32>(), values.first()) else {
            continue;
        };
        let num = |key| child(app, key)?.get_str()?.parse::<u64>().ok();
        let playtime = num("Playtime");
        // 0 means "never" in some entries.
        let last_played = num("LastPlayed")
            .filter(|&s| s > 0)
            .map(|s| UNIX_EPOCH + Duration::from_secs(s));
        if playtime.is_some() || last_played.is_some() {
            stats.insert(id, (playtime, last_played));
        }
    }
    stats
}

/// DLC app ids from PICS app info text VDF (`extended.listofdlc`, comma-separated).
pub fn dlc_ids(appinfo_vdf: &str) -> Vec<u32> {
    let Ok(vdf) = keyvalues_parser::parse(appinfo_vdf) else {
        return Vec::new();
    };
    let vdf = Vdf::from(vdf);
    let list = child(&vdf.value, "extended")
        .and_then(|e| child(e, "listofdlc"))
        .and_then(Value::get_str);
    list.into_iter()
        .flat_map(|l| l.split(','))
        .filter_map(|id| id.trim().parse().ok())
        .collect()
}

/// The app's name from PICS app info text VDF (`common.name`).
pub fn app_name(appinfo_vdf: &str) -> Option<String> {
    let vdf = Vdf::from(keyvalues_parser::parse(appinfo_vdf).ok()?);
    Some(
        child(child(&vdf.value, "common")?, "name")?
            .get_str()?
            .to_owned(),
    )
}

/// Child value by key, ignoring case: Steam writes both "apps" and "Apps".
pub(crate) fn child<'a>(value: &'a Value<'a>, key: &str) -> Option<&'a Value<'a>> {
    value
        .get_obj()?
        .0
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))?
        .1
        .first()
}

/// Every app's type from Steam's binary app info cache, or `None` when it can't be read.
fn app_types(appinfo: &Path) -> Option<HashMap<u32, String>> {
    let types = parse_app_types(&std::fs::read(appinfo).ok()?);
    if types.is_none() {
        tracing::warn!("steam appinfo.vdf: unknown format");
    }
    types
}

/// `appinfo.common.type` per app from binary appinfo.vdf version 29 (Steam's since 2024), which keeps keys in a
/// string table at the end of the file. Each app is its id, its size, a 60-byte header and a binary key-value tree.
fn parse_app_types(data: &[u8]) -> Option<HashMap<u32, String>> {
    let mut r = Reader { data, pos: 0 };
    if r.u32()? != 0x0756_4429 {
        return None;
    }
    r.take(4)?; // universe
    let mut table = Reader {
        data,
        pos: usize::try_from(r.u64()?).ok()?,
    };
    let count = table.u32()?;
    let strings = (0..count)
        .map(|_| table.cstr())
        .collect::<Option<Vec<_>>>()?;
    let mut types = HashMap::new();
    loop {
        let app = r.u32()?;
        if app == 0 {
            return Some(types);
        }
        let size = r.u32()? as usize;
        let end = r.pos.checked_add(size)?;
        let mut entry = Reader {
            data: data.get(..end)?,
            pos: r.pos + 60,
        };
        let path = ["appinfo", "common", "type"];
        if let Some(kind) = entry.find(&strings, &path) {
            types.insert(app, String::from_utf8_lossy(kind).to_ascii_lowercase());
        }
        r.pos = end;
    }
}

/// A bounds-checked cursor over binary appinfo.vdf.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let bytes = self.data.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    /// A NUL-terminated string, without the NUL.
    fn cstr(&mut self) -> Option<&'a [u8]> {
        let len = self.data.get(self.pos..)?.iter().position(|&b| b == 0)?;
        let s = self.take(len)?;
        self.pos += 1;
        Some(s)
    }

    /// A key: an index into the string table.
    fn key(&mut self, strings: &[&'a [u8]]) -> Option<&'a [u8]> {
        strings.get(self.u32()? as usize).copied()
    }

    /// The string at `path` (keys ignoring case) in the object the cursor is in; `None` when it isn't there.
    fn find(&mut self, strings: &[&'a [u8]], path: &[&str]) -> Option<&'a [u8]> {
        loop {
            let kind = *self.take(1)?.first()?;
            if kind == END {
                return None;
            }
            let hit = self.key(strings)?.eq_ignore_ascii_case(path[0].as_bytes());
            match kind {
                OBJECT if hit && path.len() > 1 => return self.find(strings, &path[1..]),
                STRING if hit && path.len() == 1 => return self.cstr(),
                _ => self.skip(kind, strings)?,
            }
        }
    }

    /// Steps over one value of type `kind`.
    fn skip(&mut self, kind: u8, strings: &[&'a [u8]]) -> Option<()> {
        match kind {
            OBJECT => loop {
                let kind = *self.take(1)?.first()?;
                if kind == END {
                    return Some(());
                }
                self.key(strings)?;
                self.skip(kind, strings)?;
            },
            STRING => self.cstr().map(drop),
            // int32, float32, pointer, color
            2 | 3 | 4 | 6 => self.take(4).map(drop),
            // uint64, int64
            7 | 10 => self.take(8).map(drop),
            _ => None,
        }
    }
}

const OBJECT: u8 = 0;
const STRING: u8 = 1;
const END: u8 = 8;

/// Steam writes 0 when nothing runs; a missing key means Steam was never installed for this user.
fn running_id(raw: std::io::Result<u32>) -> Option<String> {
    raw.ok().filter(|&id| id != 0).map(|id| id.to_string())
}

/// Whether `pid` is a live Steam client (the name check guards against a reused pid).
fn steam_alive(pid: u32) -> bool {
    crate::platform::pid_is(pid, if cfg!(windows) { "steam.exe" } else { "steam" })
}

/// Whether an installed app is a game, by its appinfo type, else by `NOT_GAMES`.
fn is_game(app_id: u32, types: Option<&HashMap<u32, String>>) -> bool {
    match types.and_then(|t| t.get(&app_id)) {
        Some(kind) => !NOT_GAME_TYPES.contains(&kind.as_str()),
        None => !NOT_GAMES.contains(&app_id),
    }
}

fn library_games(
    library: &Library,
    librarycache: &Path,
    types: Option<&HashMap<u32, String>>,
) -> Vec<Game> {
    library
        .apps()
        .filter_map(|app| match app {
            Ok(app) if !is_game(app.app_id, types) => None,
            Ok(app) => Some(to_game(library, &app, librarycache)),
            Err(e) => {
                tracing::warn!("steam manifest: {e}");
                None
            }
        })
        .collect()
}

fn to_game(library: &Library, app: &App, librarycache: &Path) -> Game {
    // Mid-update games keep FullyInstalled; first-time downloads don't have it yet.
    let installed = app
        .state_flags
        .is_some_and(|f| f.flags().any(|f| f == StateFlag::FullyInstalled));
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
        dlc: Vec::new(),
        update_available: false,
        cloud_saves: false,
    }
}

/// Local files where Steam has them, with CDN URLs as the fallback for each slot.
/// The CDN still serves these legacy names for new apps too (404s are handled by the downloader).
pub(crate) fn art(app_id: u32, librarycache_dir: &Path) -> Art {
    let mut art = local_art(librarycache_dir);
    let cdn = |file| Some(format!("{CDN}/{app_id}/{file}"));
    art.cover.url = cdn("library_600x900.jpg");
    art.header.url = cdn("header.jpg");
    art.hero.url = cdn("library_hero.jpg");
    art
}

/// Art in `librarycache/<appid>`: flat files for older apps, one level of
/// hash-named subfolders (and `library_capsule`/`library_header` names) for newer ones.
fn local_art(dir: &Path) -> Art {
    let mut art = Art::default();
    let files = fs_err::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                fs_err::read_dir(&path)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .collect()
            } else {
                vec![path]
            }
        });
    for path in files {
        let slot = match path.file_name().and_then(|n| n.to_str()) {
            Some("library_600x900.jpg" | "library_capsule.jpg") => &mut art.cover,
            Some("header.jpg" | "library_header.jpg") => &mut art.header,
            Some("library_hero.jpg") => &mut art.hero,
            _ => continue,
        };
        slot.path.get_or_insert(path);
    }
    art
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_dlc_list_and_name_from_app_info() {
        let vdf = r#""appinfo" { "common" { "name" "Base Game" } "extended" { "listofdlc" "10, 20,x" } }"#;
        assert_eq!(dlc_ids(vdf), [10, 20]);
        assert_eq!(app_name(vdf).as_deref(), Some("Base Game"));
        assert!(dlc_ids(r#""appinfo" { "common" { "name" "No DLC" } }"#).is_empty());
        assert!(dlc_ids("not vdf {").is_empty());
        assert_eq!(app_name("not vdf {"), None);
    }

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/steam")
    }

    fn games() -> Vec<Game> {
        let library = Library::from_dir(&fixtures()).unwrap();
        let mut games = library_games(&library, &fixtures().join("appcache/librarycache"), None);
        games.sort_by(|a, b| a.external_id.cmp(&b.external_id));
        games
    }

    #[test]
    fn with_owned_adds_uninstalled_games_and_keeps_installed() {
        let mut portal = games().remove(0);
        portal.external_id = "400".into();
        portal.title = "Portal".into();
        let owned = [
            OwnedGame {
                appid: 400,
                name: "Portal".into(),
                playtime_minutes: 51,
                last_played: None,
            },
            OwnedGame {
                appid: 70,
                name: "Half-Life".into(),
                playtime_minutes: 0,
                last_played: None,
            },
        ];
        let all = with_owned(
            vec![portal],
            &owned_games(&owned, &fixtures().join("appcache/librarycache")),
        );
        assert_eq!(all.len(), 2);
        let (p, hl) = (&all[0], &all[1]);
        assert!(p.installed);
        assert_eq!(p.playtime_minutes, Some(51));
        assert_eq!(hl.title, "Half-Life");
        assert!(!hl.installed);
        assert_eq!(hl.install_dir, None);
        assert_eq!(hl.size_bytes, None);
        assert!(
            hl.art
                .cover
                .url
                .as_deref()
                .unwrap()
                .ends_with("/70/library_600x900.jpg")
        );
    }

    #[test]
    fn owned_last_played_fills_only_missing_times() {
        let at = |s| Some(UNIX_EPOCH + Duration::from_secs(s));
        let mut local = games().remove(0);
        local.last_played = at(500);
        let mut unplayed = games().remove(2);
        unplayed.last_played = None;
        let owned = [
            OwnedGame {
                appid: 10,
                name: "Fixture Shooter".into(),
                playtime_minutes: 1,
                last_played: Some(900),
            },
            OwnedGame {
                appid: 30,
                name: "NamelessDir".into(),
                playtime_minutes: 1,
                last_played: Some(700),
            },
            OwnedGame {
                appid: 70,
                name: "Half-Life".into(),
                playtime_minutes: 1,
                last_played: Some(300),
            },
            OwnedGame {
                appid: 80,
                name: "Never".into(),
                playtime_minutes: 0,
                last_played: None,
            },
        ];
        let all = with_owned(
            vec![local, unplayed],
            &owned_games(&owned, &fixtures().join("appcache/librarycache")),
        );
        let played: Vec<_> = all.iter().map(|g| g.last_played).collect();
        assert_eq!(played, [at(500), at(700), at(300), None]);
    }

    #[test]
    fn maps_manifests_and_skips_redistributables_and_broken_files() {
        let ids: Vec<_> = games().iter().map(|g| g.external_id.clone()).collect();
        // 228980 is the redistributables package, 40 is an unparseable manifest.
        assert_eq!(ids, ["10", "20", "30"]);
    }

    /// A v29 appinfo.vdf: app 10 a game, 20 a tool, 30 with no type; `int32` and `uint64` values to step over.
    fn appinfo_v29() -> Vec<u8> {
        let strings: [&[u8]; 5] = [b"appinfo", b"appid", b"common", b"type", b"size"];
        let key = |i: u32| i.to_le_bytes().to_vec();
        let app = |id: u32, kind: Option<&str>| {
            let mut kv = vec![OBJECT];
            kv.extend(key(0));
            kv.push(2);
            kv.extend(key(1));
            kv.extend(id.to_le_bytes());
            kv.push(OBJECT);
            kv.extend(key(2));
            kv.push(7);
            kv.extend(key(4));
            kv.extend(9u64.to_le_bytes());
            if let Some(kind) = kind {
                kv.push(STRING);
                kv.extend(key(3));
                kv.extend(kind.as_bytes());
                kv.push(0);
            }
            kv.extend([END, END, END]);
            let mut entry = id.to_le_bytes().to_vec();
            entry.extend(((60 + kv.len()) as u32).to_le_bytes());
            entry.extend([0; 60]);
            entry.extend(kv);
            entry
        };
        let mut body: Vec<u8> =
            [app(10, Some("Game")), app(20, Some("Tool")), app(30, None)].concat();
        body.extend(0u32.to_le_bytes());
        let mut file = 0x0756_4429u32.to_le_bytes().to_vec();
        file.extend(1u32.to_le_bytes());
        file.extend(((16 + body.len()) as u64).to_le_bytes());
        file.extend(body);
        file.extend((strings.len() as u32).to_le_bytes());
        for s in strings {
            file.extend(s);
            file.push(0);
        }
        file
    }

    #[test]
    fn reads_app_types_from_appinfo() {
        let types = parse_app_types(&appinfo_v29()).unwrap();
        assert_eq!(types.get(&10).map(String::as_str), Some("game"));
        assert_eq!(types.get(&20).map(String::as_str), Some("tool"));
        assert_eq!(types.get(&30), None);
        assert!(is_game(10, Some(&types)));
        assert!(!is_game(20, Some(&types)));
        // No type: the hard-coded list decides.
        assert!(is_game(30, Some(&types)));
        assert!(!is_game(228_980, Some(&types)));
        assert!(!is_game(228_980, None));
    }

    #[test]
    fn broken_appinfo_gives_no_types() {
        let file = appinfo_v29();
        assert_eq!(parse_app_types(&file[..40]), None);
        assert_eq!(parse_app_types(b"not appinfo"), None);
        assert_eq!(parse_app_types(&[]), None);
    }

    /// Real-world check against the local appinfo.vdf: `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs a local Steam install"]
    fn real_appinfo_parses() {
        let steam = SteamDir::locate().unwrap();
        let types = app_types(&steam.path().join("appcache/appinfo.vdf")).unwrap();
        assert_eq!(types.get(&228_980).map(String::as_str), Some("tool"));
        assert!(types.values().any(|t| t == "game"));
    }

    #[test]
    fn maps_installed_game() {
        let game = &games()[0];
        assert_eq!(game.size_bytes, Some(16_521_526));
        assert_eq!(game.title, "Fixture Shooter");
        assert!(game.installed);
        assert_eq!(
            game.install_dir,
            Some(fixtures().join("steamapps/common/FixtureShooter"))
        );
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
        assert_eq!(
            art.cover.url.as_deref(),
            Some(format!("{CDN}/30/library_600x900.jpg").as_str())
        );
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
        tracing::info!("{} apps with play stats", stats.len());
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
        let now = Instant::now();
        let p = progress(&app(20), &mut None, None, now).unwrap();
        assert_eq!(p.fraction, 0.25);
        assert_eq!(
            (p.action, p.done_bytes, p.total_bytes, p.secs_left),
            (ProgressAction::Download, Some(1000), Some(4000), None)
        );
        // SNUG-style finished manifest: downloaded == to_download.
        assert_eq!(progress(&app(10), &mut None, None, now), None);
        // No byte counters at all.
        assert_eq!(progress(&app(30), &mut None, None, now), None);
    }

    #[test]
    fn steam_writes_move_progress_between_manifest_rewrites() {
        let library = Library::from_dir(&fixtures()).unwrap();
        let mut app = library.app(20).unwrap().unwrap();
        (app.bytes_to_download, app.bytes_downloaded) = (Some(2 << 30), Some(0));
        (app.bytes_to_stage, app.bytes_staged) = (Some(4 << 30), Some(0));
        let mut anchors = None;
        let t0 = Instant::now();
        let at = |s| t0 + Duration::from_secs(s);

        // First sample anchors on steam.exe's counter; no speed yet.
        let p = progress(&app, &mut anchors, Some((7, 5_000)), at(0)).unwrap();
        assert_eq!(
            (p.fraction, p.done_bytes, p.secs_left),
            (0.0, Some(0), None)
        );
        // 1 GiB written in 2 s: a quarter staged, 3 GiB at 0.5 GiB/s left.
        let p = progress(&app, &mut anchors, Some((7, 5_000 + (1 << 30))), at(2)).unwrap();
        assert_eq!(p.fraction, 0.25);
        assert_eq!(
            (p.done_bytes, p.total_bytes, p.secs_left),
            (Some(1 << 29), Some(2 << 30), Some(6))
        );
        // Never 100% while the manifest says it isn't done.
        let p = progress(&app, &mut anchors, Some((7, 5_000 + (9 << 30))), at(4)).unwrap();
        assert!(p.fraction < 1.0);

        // Steam restarted (new pid): start over from the manifest.
        let p = progress(&app, &mut anchors, Some((8, 100)), at(6)).unwrap();
        assert_eq!(p.fraction, 0.0);
        // The manifest was rewritten (paused): anchor on its new BytesStaged.
        app.bytes_staged = Some(2 << 30);
        let p = progress(&app, &mut anchors, Some((8, 1 << 40)), at(8)).unwrap();
        assert_eq!(p.fraction, 0.5);

        // Done: no progress and the anchor is dropped.
        app.bytes_downloaded = app.bytes_to_download;
        assert_eq!(
            progress(&app, &mut anchors, Some((8, 1 << 40)), at(10)),
            None
        );
        assert!(anchors.unwrap().is_empty());
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
        assert!(
            [&art.cover, &art.header, &art.hero]
                .iter()
                .all(|i| i.path.is_none())
        );
    }
}
