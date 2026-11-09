//! Windows games on Linux. Each runs through umu-launcher (`umu-run`) with a Proton build, in its own Wine prefix,
//! `<data>/prefixes/<copy key>`. A prefix holds the game's saves, so uninstalling the game keeps it.
//! umu-launcher and Proton builds (GE-Proton, Proton-CachyOS) download on request into the tools folder: umu pinned
//! like `tools.rs`, a Proton release checked against the `.sha512sum` file published with it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use serde_json::Value;
use sha2::{Sha256, Sha512};

use crate::cache::{Dirs, db};
use crate::msg::Msg;
use crate::platform::{ProtonKind, ProtonView, ReleaseView, RunnerView};
use crate::tools::{Installing, download};

const UMU_URL: &str = "https://github.com/Open-Wine-Components/umu-launcher/releases/download/1.4.4/umu-launcher-1.4.4-zipapp.tar";
const UMU_SHA256: &str = "eb590691841f7fad3fc3ad8fd5db4ccb87849fe7948e62b28ece7a4ee48cc851";
/// `info` kind of a Proton source's recent releases, as `Release` JSON, keyed by `kind_key`.
const RELEASES: &str = "proton-releases";
const RELEASES_MAX_AGE_SECS: i64 = 6 * 60 * 60;
const UMU_DATABASE: &str =
    "https://raw.githubusercontent.com/Open-Wine-Components/umu-database/main/umu-database.csv";
/// `info` key and kind of the umu database, as `{"<store>:<codename>": "umu-<id>"}`.
const UMU_IDS: &str = "umu-ids";
const UMU_IDS_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;
/// A GE-Proton build is over 500 MB.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);

/// GitHub's API refuses requests without a User-Agent.
fn http() -> &'static reqwest::Client {
    static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("Guild")
            .build()
            .expect("default TLS backend is available")
    });
    &HTTP
}

fn managed_umu(tools: &Path) -> PathBuf {
    tools.join("umu").join("umu").join("umu-run")
}

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// umu-run on PATH (a distribution's package), else the one Guild downloaded.
fn umu() -> Option<PathBuf> {
    on_path("umu-run").or_else(|| Some(managed_umu(&Dirs::new().tools)).filter(|p| p.is_file()))
}

/// The Wine prefix of the copy `key`; `None` for a key that isn't a plain folder name.
pub fn prefix(key: &str) -> Option<PathBuf> {
    prefix_in(&Dirs::new().prefixes, key)
}

fn prefix_in(root: &Path, key: &str) -> Option<PathBuf> {
    let plain = !key.is_empty()
        && !key.starts_with('.')
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
    plain.then(|| root.join(key))
}

/// umu's `STORE` for a copy key: its protonfixes are listed per store.
fn store(key: &str) -> Option<&'static str> {
    match key.split_once('-')?.0 {
        "epic" => Some("egs"),
        "gog" => Some("gog"),
        "amazon" => Some("amazon"),
        _ => None,
    }
}

/// The game's id in umu's database: the copy key without its store; Amazon's without `amzn1.adg.product.`.
// shortcut: assumes umu lists an Amazon game by the UUID in its product id, as its rows look; unchecked against Heroic.
fn codename(key: &str) -> Option<String> {
    let id = key.split_once('-')?.1;
    Some(
        id.strip_prefix("amzn1.adg.product.")
            .unwrap_or(id)
            .to_lowercase(),
    )
}

/// umu-database.csv (`TITLE,STORE,CODENAME,UMU_ID,…`, a field quoted when it holds a comma) as
/// `{"<store>:<codename>": umu id}`, codenames lowercased.
fn parse_umu_ids(csv: &str) -> HashMap<String, String> {
    csv.lines()
        .skip(1)
        .filter_map(|line| {
            let f = csv_fields(line);
            let (store, code, id) = (f.get(1)?, f.get(2)?, f.get(3)?);
            id.starts_with("umu-")
                .then(|| (format!("{store}:{}", code.to_lowercase()), id.clone()))
        })
        .collect()
}

fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                fields.last_mut().expect("starts with one").push('"');
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(String::new()),
            c => fields.last_mut().expect("starts with one").push(c),
        }
    }
    fields
}

/// umu's `GAMEID` for a copy: its umu id when umu's database lists the game, else `umu-default`.
// shortcut: offline, every launch tries the database again (5 s at most) until one fetch is cached.
fn game_id(key: &str) -> String {
    let listed = || {
        let wanted = format!("{}:{}", store(key)?, codename(key)?);
        let conn = db::open(&Dirs::new().db).ok();
        let json = crate::info::cached_or_fetch(
            conn.as_ref(),
            UMU_IDS,
            UMU_IDS,
            UMU_IDS_MAX_AGE_SECS,
            crate::library::unix_now(),
            fetch_umu_ids,
        )?;
        serde_json::from_str::<HashMap<String, String>>(&json)
            .ok()?
            .remove(&wanted)
    };
    listed().unwrap_or_else(|| "umu-default".into())
}

fn fetch_umu_ids() -> Option<String> {
    let csv = tauri::async_runtime::block_on(async {
        http()
            .get(UMU_DATABASE)
            .timeout(Duration::from_secs(5))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await
    })
    .inspect_err(|e| tracing::warn!("umu database: {e}"))
    .ok()?;
    serde_json::to_string(&parse_umu_ids(&csv)).ok()
}

/// A Proton build: its folder's name and the folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Proton {
    pub name: String,
    pub path: PathBuf,
    /// In Guild's tools folder, so Guild may remove it; Steam's are only listed.
    pub removable: bool,
}

/// The Proton builds found, newest first: Guild's, the compatibility tools Steam lists (GE-Proton) and Steam's own in
/// each library. A folder counts when it holds the `proton` script; of two with one name, the first found wins.
pub fn protons() -> Vec<Proton> {
    let mut found = protons_in(&[Dirs::new().tools.join("proton")], |_| true, true);
    if let Ok(steam) = steamlocate::SteamDir::locate() {
        let compat = [steam.path().join("compatibilitytools.d")];
        found.extend(protons_in(&compat, |_| true, false));
        let common: Vec<_> = steam
            .library_paths()
            .unwrap_or_default()
            .into_iter()
            .map(|l| l.join("steamapps").join("common"))
            .collect();
        // Next to the games there, only Steam's own Proton builds.
        found.extend(protons_in(
            &common,
            |name| name.starts_with("Proton"),
            false,
        ));
    }
    newest_first(found)
}

fn protons_in(roots: &[PathBuf], keep: impl Fn(&str) -> bool, removable: bool) -> Vec<Proton> {
    let mut found = Vec::new();
    for root in roots {
        let mut dirs: Vec<_> = fs_err::read_dir(root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("proton").is_file())
            .collect();
        dirs.sort();
        found.extend(dirs.into_iter().filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_owned();
            keep(&name).then_some(Proton {
                name,
                path,
                removable,
            })
        }));
    }
    found
}

/// Without duplicate names, newest first: GE-Proton by version, then Steam's numbered Proton, then the rest.
fn newest_first(found: Vec<Proton>) -> Vec<Proton> {
    let mut unique: Vec<Proton> = Vec::new();
    for p in found {
        if !unique.iter().any(|u| u.name == p.name) {
            unique.push(p);
        }
    }
    let rank = |name: &str| {
        let kind = if name.starts_with("GE-Proton") {
            2
        } else {
            u8::from(name.starts_with("Proton "))
        };
        let numbers: Vec<u32> = name
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|n| n.parse().ok())
            .collect();
        (kind, numbers)
    };
    unique.sort_by_key(|p| std::cmp::Reverse(rank(&p.name)));
    unique
}

/// The first of `wanted` that is found, else the newest found.
fn choose<'a>(protons: &'a [Proton], wanted: &[Option<&str>]) -> Option<&'a Proton> {
    wanted
        .iter()
        .flatten()
        .find_map(|w| protons.iter().find(|p| p.name == *w))
        .or(protons.first())
}

fn default_proton() -> Option<String> {
    let conn = db::open(&Dirs::new().db).ok()?;
    db::setting(&conn, crate::settings::PROTON).ok().flatten()
}

/// `game` (a Windows program with its arguments, folder and variables) run through umu-run with Proton, in the copy's
/// own prefix. `proton` is the game's own choice, before the default in Settings.
fn wrap(game: &Command, key: &str, proton: Option<&str>) -> Result<(Command, Proton), Msg> {
    let umu = umu().ok_or(msg!(noUmu))?;
    let protons = protons();
    let default = default_proton();
    let proton = choose(&protons, &[proton, default.as_deref()])
        .ok_or(msg!(noProton))?
        .clone();
    let prefix = prefix(key).ok_or(msg!(gameNotInLibrary))?;
    fs_err::create_dir_all(&prefix).map_err(Msg::system)?;
    let command = umu_command(&umu, game, &prefix, &proton.path, &game_id(key), store(key));
    Ok((command, proton))
}

/// A game's launch through `wrap`. When umu still has to fetch the Steam Linux Runtime or set the prefix up, the copy
/// counts as setting up (`setting_up`) until the game's own program runs.
pub fn game_command(game: Command, key: &str, proton: Option<&str>) -> Result<Command, Msg> {
    let (command, proton) = wrap(&game, key, proton)?;
    let prefix = prefix(key).ok_or(msg!(gameNotInLibrary))?;
    let mut setups = SETUPS.lock().unwrap_or_else(|e| e.into_inner());
    let exe = Path::new(game.get_program()).file_name();
    match exe.filter(|_| needs_setup(&prefix, &proton.path, &umu_data())) {
        Some(exe) => {
            setups.insert(
                key.to_owned(),
                Setup {
                    exe: exe.to_string_lossy().into_owned(),
                    prefix,
                },
            );
        }
        None => _ = setups.remove(key),
    }
    Ok(command)
}

/// A launch waiting on setup: the game's program file name and its prefix.
struct Setup {
    exe: String,
    prefix: PathBuf,
}

static SETUPS: LazyLock<Mutex<HashMap<String, Setup>>> = LazyLock::new(Mutex::default);

/// Whether the copy's launch is still setting up: asked while its umu-run runs. Ends once a process of the game's
/// program runs in its prefix (Proton starts that program itself, so it is always seen); a failed setup ends with
/// umu-run. Not the prefix's `version` file: Proton writes it before the game starts.
pub fn setting_up(key: &str) -> bool {
    let mut setups = SETUPS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(setup) = setups.get(key) else {
        return false;
    };
    if game_runs(&setup.exe, &setup.prefix) {
        setups.remove(key);
        return false;
    }
    true
}

/// umu's own folder, where it keeps the Steam Linux Runtime.
fn umu_data() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.data_dir().join("umu"))
        .unwrap_or_default()
}

/// Whether a launch must wait: the prefix hasn't been set up (Proton writes `version` once it has), or umu still has to
/// fetch the runtime this Proton build asks for in its `toolmanifest.vdf`.
fn needs_setup(prefix: &Path, proton: &Path, umu_data: &Path) -> bool {
    let manifest = fs_err::read_to_string(proton.join("toolmanifest.vdf")).unwrap_or_default();
    !prefix.join("version").is_file()
        || runtime(&manifest)
            .is_some_and(|dir| !umu_data.join(dir).join("_v2-entry-point").exists())
}

/// umu's folder for the runtime a `toolmanifest.vdf` requires (`require_tool_appid`, umu's `RUNTIME_NAMES`).
fn runtime(manifest: &str) -> Option<&'static str> {
    let line = manifest
        .lines()
        .find(|l| l.contains("\"require_tool_appid\""))?;
    match line.split('"').nth(3)? {
        "1628350" => Some("steamrt3"),
        "4183110" => Some("steamrt4"),
        _ => None,
    }
}

/// Whether a process of `exe` runs in `prefix`. Under Wine a game's first argument is its Windows path
/// (`X:\…\game.exe`) and it carries `WINEPREFIX`; umu's own processes only name the program further on.
fn game_runs(exe: &str, prefix: &Path) -> bool {
    let wanted = prefix.as_os_str().as_encoded_bytes();
    fs_err::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_digit())
        })
        .any(|e| {
            let read = |name| std::fs::read(e.path().join(name)).unwrap_or_default();
            is_game_process(&read("cmdline"), &read("environ"), exe, wanted)
        })
}

/// Proton runs the game with `WINEPREFIX=<prefix>/pfx/`, `pfx` being umu's link back to the prefix itself.
fn is_game_process(cmdline: &[u8], environ: &[u8], exe: &str, prefix: &[u8]) -> bool {
    let first = cmdline.split(|&b| b == 0).next().unwrap_or_default();
    let name = first
        .rsplit(|&b| b == b'/' || b == b'\\')
        .next()
        .unwrap_or_default();
    fn trim(mut v: &[u8]) -> &[u8] {
        while let Some(rest) = v.strip_suffix(b"/") {
            v = rest;
        }
        v
    }
    let in_prefix = environ.split(|&b| b == 0).any(|var| {
        var.strip_prefix(b"WINEPREFIX=").map(trim).is_some_and(|v| {
            let v = v.strip_suffix(b"/pfx").map_or(v, trim);
            v == trim(prefix)
        })
    });
    name.eq_ignore_ascii_case(exe.as_bytes()) && in_prefix
}

/// Sets up the copy's prefix if it isn't yet, before a store tool looks inside it: Legendary reads the prefix's
/// `user.reg` to find the save folder, and without it skips the download before play, so the upload after would
/// replace the cloud save. umu-run with an empty program only sets the prefix up. Quiet when it can't.
pub fn ready_prefix(key: &str) {
    let Some(prefix) = prefix(key) else { return };
    let ready = || prefix.join("user.reg").is_file();
    if ready() {
        return;
    }
    // Exits 1 for the empty program once the prefix is made, so the prefix is the outcome.
    let ran = wrap(&Command::new(""), key, None).and_then(|(mut command, _)| {
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(Msg::system)
    });
    match ran {
        Ok(_) if ready() => tracing::info!("wine prefix {key} set up"),
        Ok(status) => tracing::warn!("wine prefix {key}: umu-run exited with {status}"),
        Err(e) => tracing::warn!("wine prefix {key}: {e}"),
    }
}

fn umu_command(
    umu: &Path,
    game: &Command,
    prefix: &Path,
    proton: &Path,
    game_id: &str,
    store: Option<&str>,
) -> Command {
    let mut command = Command::new(umu);
    command.arg(game.get_program()).args(game.get_args());
    if let Some(dir) = game.get_current_dir() {
        command.current_dir(dir);
    }
    for (name, value) in game.get_envs() {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    command
        .env("WINEPREFIX", prefix)
        .env("PROTONPATH", proton)
        .env("GAMEID", game_id);
    if let Some(store) = store {
        command.env("STORE", store);
    }
    command
}

/// A GOG save folder variable inside a Windows game's prefix, where Proton's user is `steamuser`.
pub fn windows_folder(prefix: &Path, name: &str) -> Option<PathBuf> {
    let user = prefix.join("drive_c").join("users").join("steamuser");
    Some(match name {
        "SAVED_GAMES" => user.join("Saved Games"),
        "APPLICATION_DATA_LOCAL" => user.join("AppData").join("Local"),
        "APPLICATION_DATA_LOCAL_LOW" => user.join("AppData").join("LocalLow"),
        "APPLICATION_DATA_ROAMING" => user.join("AppData").join("Roaming"),
        "DOCUMENTS" => user.join("Documents"),
        _ => return None,
    })
}

pub fn view() -> RunnerView {
    RunnerView {
        umu: umu().is_some(),
        protons: protons()
            .into_iter()
            .map(|p| ProtonView {
                name: p.name,
                removable: p.removable,
            })
            .collect(),
        proton: default_proton(),
        winetricks: on_path("winetricks").is_some(),
    }
}

pub fn proton_names() -> Vec<String> {
    protons().into_iter().map(|p| p.name).collect()
}

/// Deletes a Proton build Guild installed; Steam's are left alone.
pub fn remove_proton(name: &str) -> Result<(), Msg> {
    let proton = protons()
        .into_iter()
        .find(|p| p.name == name && p.removable)
        .ok_or(msg!(protonNotRemovable))?;
    fs_err::remove_dir_all(&proton.path).map_err(Msg::system)?;
    tracing::info!("removed {name}");
    Ok(())
}

/// The copy keys that have a prefix.
pub fn prefix_keys() -> Vec<String> {
    let root = Dirs::new().prefixes;
    let mut keys: Vec<String> = fs_err::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|k| prefix_in(&root, k).is_some())
        .collect();
    keys.sort();
    keys
}

/// Deletes a copy's prefix, saves and all.
pub fn remove_prefix(key: &str) -> Result<(), Msg> {
    let prefix = prefix(key).ok_or(msg!(gameNotInLibrary))?;
    match fs_err::remove_dir_all(&prefix) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Msg::system(e)),
        _ => {
            tracing::info!("removed wine prefix {key}");
            Ok(())
        }
    }
}

/// Starts winecfg or winetricks in a copy's prefix with its Proton build, without waiting. umu-run takes `winetricks`
/// as its own word: the one the Proton build bundles, else the system's.
pub fn run_in_prefix(key: &str, program: &str, proton: Option<&str>) -> Result<(), Msg> {
    if program == "winetricks" && on_path("winetricks").is_none() {
        return Err(msg!(noWinetricks));
    }
    let (mut command, _) = wrap(&Command::new(program), key, proton)?;
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(Msg::system)?;
    // Reaped off-thread so it never lingers as a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Downloads the pinned umu-launcher into the tools folder.
pub async fn install_umu(on_percent: impl FnMut(u8)) -> Result<(), Msg> {
    let _busy = Installing::start("umu")
        .ok_or_else(|| msg!(toolAlreadyInstalling, tool = "umu-launcher"))?;
    let dir = Dirs::new().tools.join("umu");
    let part = dir.join("umu.tar.part");
    let fetched = download::<Sha256>(http(), UMU_URL, DOWNLOAD_TIMEOUT, &part, on_percent).await;
    unpack(fetched, UMU_SHA256, &part, &dir, "umu-launcher")?;
    tracing::info!("installed umu-launcher");
    Ok(())
}

fn repo(kind: ProtonKind) -> &'static str {
    match kind {
        ProtonKind::GeProton => "GloriousEggroll/proton-ge-custom",
        ProtonKind::ProtonCachyos => "CachyOS/proton-cachyos",
    }
}

fn title(kind: ProtonKind) -> &'static str {
    match kind {
        ProtonKind::GeProton => "GE-Proton",
        ProtonKind::ProtonCachyos => "Proton-CachyOS",
    }
}

/// The builds to take, by archive suffix, best first: Proton-CachyOS has an x86-64-v3 build for CPUs that run it.
/// `""` is an archive without one (older GE-Proton releases).
fn arches(kind: ProtonKind) -> &'static [&'static str] {
    match kind {
        ProtonKind::GeProton => &["x86_64", ""],
        ProtonKind::ProtonCachyos
            if x86_64_v3(&std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default()) =>
        {
            &["x86_64_v3", "x86_64"]
        }
        ProtonKind::ProtonCachyos => &["x86_64"],
    }
}

/// Whether the CPU's flags in `/proc/cpuinfo` cover x86-64-v3: v2's (cx16, lahf_lm, popcnt, sse4_1, sse4_2, ssse3)
/// plus avx, avx2, bmi1, bmi2, f16c, fma, abm (lzcnt), movbe and xsave.
fn x86_64_v3(cpuinfo: &str) -> bool {
    let flags: HashSet<&str> = cpuinfo
        .lines()
        .find_map(|l| l.strip_prefix("flags")?.split_once(':'))
        .map(|(_, f)| f.split_whitespace().collect())
        .unwrap_or_default();
    [
        "cx16", "lahf_lm", "popcnt", "sse4_1", "sse4_2", "ssse3", "avx", "avx2", "bmi1", "bmi2",
        "f16c", "fma", "abm", "movbe", "xsave",
    ]
    .iter()
    .all(|f| flags.contains(f))
}

fn kind_key(kind: ProtonKind) -> &'static str {
    match kind {
        ProtonKind::GeProton => "ge-proton",
        ProtonKind::ProtonCachyos => "proton-cachyos",
    }
}

/// A source's recent releases that have a build for this CPU and a checksum file, newest first; cached 6 h. `None`
/// when GitHub couldn't be asked (logged). Blocking.
fn releases(kind: ProtonKind) -> Option<Vec<Release>> {
    let conn = db::open(&Dirs::new().db).ok();
    let json = crate::info::cached_or_fetch(
        conn.as_ref(),
        kind_key(kind),
        RELEASES,
        RELEASES_MAX_AGE_SECS,
        crate::library::unix_now(),
        || {
            let url = format!(
                "https://api.github.com/repos/{}/releases?per_page=10",
                repo(kind)
            );
            let text = tauri::async_runtime::block_on(get(&url))
                .inspect_err(|e| tracing::warn!("{} releases: {e}", title(kind)))
                .ok()?;
            let list: Value = serde_json::from_str(&text).ok()?;
            let found: Vec<Release> = list
                .as_array()?
                .iter()
                .filter(|r| r["draft"] != true)
                .filter_map(|r| parse_release(r, arches(kind)))
                .collect();
            serde_json::to_string(&found).ok()
        },
    )?;
    serde_json::from_str(&json).ok()
}

/// The releases for Settings, each marked when its build is installed. Blocking.
pub fn release_views(kind: ProtonKind) -> Option<Vec<ReleaseView>> {
    let installed = proton_names();
    Some(
        releases(kind)?
            .into_iter()
            .map(|r| ReleaseView {
                installed: installed.contains(&r.stem),
                tag: r.tag,
                name: r.stem,
            })
            .collect(),
    )
}

/// Downloads one release of a Proton source into the tools folder, checked against its `.sha512sum`. One Proton
/// install at a time.
pub async fn install_proton(
    kind: ProtonKind,
    tag: String,
    on_percent: impl FnMut(u8),
) -> Result<(), Msg> {
    let title = title(kind);
    let _busy =
        Installing::start("proton").ok_or_else(|| msg!(toolAlreadyInstalling, tool = title))?;
    let cant = |e: &dyn std::fmt::Display| {
        tracing::warn!("install {title} {tag}: {e}");
        msg!(cantDownloadTool, tool = title)
    };
    let found = tauri::async_runtime::spawn_blocking(move || releases(kind))
        .await
        .map_err(|e| cant(&e))?;
    let release = found
        .into_iter()
        .flatten()
        .find(|r| r.tag == tag)
        .ok_or_else(|| cant(&"no such release"))?;
    let sums = get(&release.sha512sum).await.map_err(|e| cant(&e))?;
    let expected = parse_sha512sum(&sums, &release.file).ok_or_else(|| cant(&"no checksum"))?;
    let root = Dirs::new().tools.join("proton");
    let part = root.join(format!("{}.part", release.file));
    // Unpacked aside and then moved, so a half-unpacked build is never listed.
    let unpacking = root.join(".unpacking");
    _ = fs_err::remove_dir_all(&unpacking);
    let fetched =
        download::<Sha512>(http(), &release.tar, DOWNLOAD_TIMEOUT, &part, on_percent).await;
    unpack(fetched, &expected, &part, &unpacking, title)?;
    let moved = fs_err::read_dir(&unpacking).and_then(|entries| {
        for entry in entries.flatten() {
            let dest = root.join(entry.file_name());
            _ = fs_err::remove_dir_all(&dest);
            fs_err::rename(entry.path(), dest)?;
        }
        fs_err::remove_dir_all(&unpacking)
    });
    moved.map_err(|e| cant(&e))?;
    tracing::info!("installed {}", release.file);
    Ok(())
}

async fn get(url: &str) -> Result<String, crate::library::Error> {
    Ok(http()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?)
}

/// Checks a downloaded archive's hash, then unpacks it into `dir` with the system's `tar`. The archive is deleted
/// either way.
fn unpack(
    fetched: Result<String, crate::library::Error>,
    expected: &str,
    part: &Path,
    dir: &Path,
    title: &str,
) -> Result<(), Msg> {
    let cant = |e: &dyn std::fmt::Display| {
        tracing::warn!("install {title}: {e}");
        _ = fs_err::remove_file(part);
        msg!(cantDownloadTool, tool = title)
    };
    let digest = fetched.map_err(|e| cant(&e))?;
    if digest != expected {
        _ = fs_err::remove_file(part);
        return Err(msg!(downloadMismatch));
    }
    fs_err::create_dir_all(dir).map_err(|e| cant(&e))?;
    let status = Command::new("tar")
        .arg("-xf")
        .arg(part)
        .arg("-C")
        .arg(dir)
        .status()
        .map_err(|e| cant(&e))?;
    if !status.success() {
        return Err(cant(&format!("tar exited with {status}")));
    }
    _ = fs_err::remove_file(part);
    Ok(())
}

/// A Proton release's archive for this CPU and its checksum file. `stem` is the archive's name without `.tar.*`,
/// which is also the folder it unpacks to.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Release {
    tag: String,
    stem: String,
    file: String,
    tar: String,
    sha512sum: String,
}

/// From a GitHub release: the first of `arches` among its archives, like `GE-Proton11-12-x86_64.tar.gz` or
/// `proton-cachyos-11.0-20261015-slr-x86_64_v3.tar.xz`, with `<stem>.sha512sum` next to it. No checksum file, no
/// release: nothing installs unchecked.
fn parse_release(release: &Value, arches: &[&str]) -> Option<Release> {
    let assets = release["assets"].as_array()?;
    let url = |name: &str| {
        assets.iter().find(|a| a["name"] == name)?["browser_download_url"]
            .as_str()
            .map(str::to_owned)
    };
    let stem = |name: &str| {
        name.strip_suffix(".tar.gz")
            .or_else(|| name.strip_suffix(".tar.xz"))
            .map(str::to_owned)
    };
    let arch = |stem: &str| {
        ["x86_64_v3", "x86_64", "aarch64", "arm64"]
            .into_iter()
            .find(|a| stem.ends_with(&format!("-{a}")))
            .unwrap_or("")
    };
    let names: Vec<&str> = assets.iter().filter_map(|a| a["name"].as_str()).collect();
    let (file, stem) = arches.iter().find_map(|wanted| {
        names
            .iter()
            .find_map(|n| stem(n).filter(|s| arch(s) == *wanted).map(|s| (*n, s)))
    })?;
    Some(Release {
        tag: release["tag_name"].as_str()?.to_owned(),
        tar: url(file)?,
        sha512sum: url(&format!("{stem}.sha512sum"))?,
        file: file.to_owned(),
        stem,
    })
}

/// The hash `sha512sum` printed for `file`: lines of `<hex>  <file>`.
fn parse_sha512sum(text: &str, file: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (hash, name) = line.split_once(char::is_whitespace)?;
        let name = name.trim().trim_start_matches('*');
        (name == file && hash.len() == 128 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prefixes_are_plain_folders_per_copy() {
        let root = Path::new("/data/prefixes");
        assert_eq!(
            prefix_in(root, "gog-1207658883"),
            Some(root.join("gog-1207658883"))
        );
        assert_eq!(
            prefix_in(root, "amazon-amzn1.adg.product.0127edd7-4a35"),
            Some(root.join("amazon-amzn1.adg.product.0127edd7-4a35"))
        );
        for bad in ["", "..", ".hidden", "epic-a/b", "custom-../x", "epic-a b"] {
            assert_eq!(prefix_in(root, bad), None, "{bad}");
        }
    }

    #[test]
    fn umu_ids_come_from_the_database_rows() {
        let csv = "TITLE,STORE,CODENAME,UMU_ID,COMMON ACRONYM (Optional),NOTE (Optional)\n\
            Borderlands 3,egs,Catnip,umu-397540,bl3,,\n\
            \"Warhammer 40,000: Gladius\",gog,1207658883,umu-489630,,\"a \"\"quoted\"\" note, here\"\n\
            Star Wars,amazon,0127EDD7-4a35,umu-6020,,,\n\
            Broken,egs,x,none,,\n";
        let ids = parse_umu_ids(csv);
        assert_eq!(
            ids.get("egs:catnip").map(String::as_str),
            Some("umu-397540")
        );
        assert_eq!(
            ids.get("gog:1207658883").map(String::as_str),
            Some("umu-489630")
        );
        assert_eq!(ids.len(), 3);
        assert_eq!(
            format!(
                "{}:{}",
                store("amazon-amzn1.adg.product.0127edd7-4a35").unwrap(),
                codename("amazon-amzn1.adg.product.0127edd7-4a35").unwrap()
            ),
            "amazon:0127edd7-4a35"
        );
        assert!(ids.contains_key("amazon:0127edd7-4a35"));
        assert_eq!(store("custom-18f2a"), None);
        assert_eq!(csv_fields(r#"a,"b,""c""",d"#), ["a", r#"b,"c""#, "d"]);
    }

    #[test]
    fn umu_runs_the_game_with_its_prefix_proton_and_ids() {
        let mut game = Command::new("/games/June/june.exe");
        game.args(["-skipintro", "a b"])
            .current_dir("/games/June/bin")
            .env("FUEL_DIR", "/fuel");
        let command = umu_command(
            Path::new("/usr/bin/umu-run"),
            &game,
            Path::new("/data/prefixes/gog-1"),
            Path::new("/tools/proton/GE-Proton11-12"),
            "umu-61500",
            Some("gog"),
        );
        assert_eq!(command.get_program(), "/usr/bin/umu-run");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["/games/June/june.exe", "-skipintro", "a b"]);
        assert_eq!(
            command.get_current_dir(),
            Some(Path::new("/games/June/bin"))
        );
        let envs: HashMap<_, _> = command
            .get_envs()
            .map(|(k, v)| (k.to_str().unwrap(), v.unwrap().to_str().unwrap()))
            .collect();
        assert_eq!(
            envs,
            HashMap::from([
                ("FUEL_DIR", "/fuel"),
                ("WINEPREFIX", "/data/prefixes/gog-1"),
                ("PROTONPATH", "/tools/proton/GE-Proton11-12"),
                ("GAMEID", "umu-61500"),
                ("STORE", "gog"),
            ])
        );
        let custom = umu_command(
            Path::new("umu-run"),
            &Command::new("a.exe"),
            Path::new("/p"),
            Path::new("/q"),
            "umu-default",
            None,
        );
        assert!(custom.get_envs().all(|(k, _)| k != "STORE"));
    }

    #[test]
    fn protons_are_found_newest_first_and_chosen_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        let (ours, steam, common) = (
            tmp.path().join("ours"),
            tmp.path().join("compat"),
            tmp.path().join("common"),
        );
        for dir in [
            ours.join("GE-Proton9-27"),
            steam.join("GE-Proton11-12-x86_64"),
            steam.join("GE-Proton9-27"),
            common.join("Proton 9.0 (Beta)"),
            common.join("Proton - Experimental"),
            common.join("Proton 10.0"),
            common.join("Some Game"),
        ] {
            fs_err::create_dir_all(&dir).unwrap();
            fs_err::write(dir.join("proton"), "").unwrap();
        }
        fs_err::create_dir_all(steam.join("Half unpacked")).unwrap();
        let mut found = protons_in(std::slice::from_ref(&ours), |_| true, true);
        found.extend(protons_in(&[steam], |_| true, false));
        found.extend(protons_in(&[common], |n| n.starts_with("Proton"), false));
        let found = newest_first(found);
        let names: Vec<_> = found.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "GE-Proton11-12-x86_64",
                "GE-Proton9-27",
                "Proton 10.0",
                "Proton 9.0 (Beta)",
                "Proton - Experimental"
            ]
        );
        assert_eq!(found[1].path, ours.join("GE-Proton9-27"), "ours wins");
        assert!(found[1].removable && !found[0].removable && !found[2].removable);
        assert_eq!(
            choose(&found, &[Some("Proton 10.0"), Some("GE-Proton9-27")])
                .unwrap()
                .name,
            "Proton 10.0"
        );
        assert_eq!(
            choose(&found, &[Some("Gone"), Some("GE-Proton9-27")])
                .unwrap()
                .name,
            "GE-Proton9-27"
        );
        assert_eq!(
            choose(&found, &[None, Some("Gone")]).unwrap().name,
            "GE-Proton11-12-x86_64"
        );
        assert_eq!(choose(&[], &[Some("GE-Proton9-27")]), None);
    }

    #[test]
    fn gog_save_folders_map_into_the_prefix() {
        let user = Path::new("/p/drive_c/users/steamuser");
        let at = |name| windows_folder(Path::new("/p"), name);
        assert_eq!(at("SAVED_GAMES"), Some(user.join("Saved Games")));
        assert_eq!(
            at("APPLICATION_DATA_LOCAL"),
            Some(user.join("AppData/Local"))
        );
        assert_eq!(
            at("APPLICATION_DATA_LOCAL_LOW"),
            Some(user.join("AppData/LocalLow"))
        );
        assert_eq!(
            at("APPLICATION_DATA_ROAMING"),
            Some(user.join("AppData/Roaming"))
        );
        assert_eq!(at("DOCUMENTS"), Some(user.join("Documents")));
        assert_eq!(at("INSTALL"), None);
    }

    #[test]
    fn ge_proton_release_and_checksum_parse() {
        let base =
            "https://github.com/GloriousEggroll/proton-ge-custom/releases/download/GE-Proton11-12";
        let asset =
            |name: &str| json!({"name": name, "browser_download_url": format!("{base}/{name}")});
        let release = json!({"tag_name": "GE-Proton11-12", "assets": [
            asset("GE-Proton11-12-aarch64.sha512sum"),
            asset("GE-Proton11-12-aarch64.tar.gz"),
            asset("GE-Proton11-12-x86_64.sha512sum"),
            asset("GE-Proton11-12-x86_64.tar.gz"),
        ]});
        let ge = ["x86_64", ""];
        assert_eq!(
            parse_release(&release, &ge),
            Some(Release {
                tag: "GE-Proton11-12".into(),
                stem: "GE-Proton11-12-x86_64".into(),
                file: "GE-Proton11-12-x86_64.tar.gz".into(),
                tar: format!("{base}/GE-Proton11-12-x86_64.tar.gz"),
                sha512sum: format!("{base}/GE-Proton11-12-x86_64.sha512sum"),
            })
        );
        let old = json!({"tag_name": "GE-Proton9-27", "assets": [asset("GE-Proton9-27.tar.gz"), asset("GE-Proton9-27.sha512sum")]});
        assert_eq!(
            parse_release(&old, &ge).unwrap().sha512sum,
            format!("{base}/GE-Proton9-27.sha512sum")
        );
        // No checksum file: not offered.
        assert_eq!(
            parse_release(
                &json!({"tag_name": "x", "assets": [asset("GE-Proton9-27.tar.gz")]}),
                &ge
            ),
            None
        );
        let cachy = json!({"tag_name": "cachyos-11.0-20261015-slr", "assets": [
            asset("proton-cachyos-11.0-20261015-slr-arm64.sha512sum"),
            asset("proton-cachyos-11.0-20261015-slr-arm64.tar.xz"),
            asset("proton-cachyos-11.0-20261015-slr-x86_64.sha512sum"),
            asset("proton-cachyos-11.0-20261015-slr-x86_64.tar.xz"),
            asset("proton-cachyos-11.0-20261015-slr-x86_64_v3.sha512sum"),
            asset("proton-cachyos-11.0-20261015-slr-x86_64_v3.tar.xz"),
        ]});
        let v3 = parse_release(&cachy, &["x86_64_v3", "x86_64"]).unwrap();
        assert_eq!(v3.file, "proton-cachyos-11.0-20261015-slr-x86_64_v3.tar.xz");
        assert_eq!(
            v3.sha512sum,
            format!("{base}/proton-cachyos-11.0-20261015-slr-x86_64_v3.sha512sum")
        );
        assert_eq!(
            parse_release(&cachy, &["x86_64"]).unwrap().stem,
            "proton-cachyos-11.0-20261015-slr-x86_64"
        );

        let hash = "2e37447902529c9fc39138dc0b3f24aed3513b59a190b825e6b4a668e49e2033bdf6b8a7998e8686e067bbb3f61f3a6633e2408f0cc96f6dad983bd18e2db31e";
        let sums = format!("{hash}  GE-Proton11-12-x86_64.tar.gz\n");
        assert_eq!(
            parse_sha512sum(&sums, "GE-Proton11-12-x86_64.tar.gz").as_deref(),
            Some(hash)
        );
        assert_eq!(
            parse_sha512sum(&sums, "GE-Proton11-12-aarch64.tar.gz"),
            None
        );
        assert_eq!(parse_sha512sum("abc  x.tar.gz", "x.tar.gz"), None);
    }

    #[test]
    fn cpu_level_comes_from_its_flags() {
        let v3 = "processor\t: 0\nflags\t\t: fpu cx16 lahf_lm popcnt sse4_1 sse4_2 ssse3 avx avx2 bmi1 bmi2 f16c fma abm movbe xsave\n";
        assert!(x86_64_v3(v3));
        assert!(!x86_64_v3(&v3.replace(" avx2", "")));
        assert!(!x86_64_v3(""));
    }

    #[test]
    fn a_launch_sets_up_until_the_prefix_and_runtime_are_there() {
        let tmp = tempfile::tempdir().unwrap();
        let (prefix, proton, umu) = (
            tmp.path().join("p"),
            tmp.path().join("GE"),
            tmp.path().join("umu"),
        );
        fs_err::create_dir_all(&prefix).unwrap();
        fs_err::create_dir_all(&proton).unwrap();
        let manifest = "\"manifest\"\n{\n  \"require_tool_appid\" \"4183110\"\n}\n";
        assert_eq!(runtime(manifest), Some("steamrt4"));
        assert_eq!(
            runtime("\"require_tool_appid\" \"1628350\""),
            Some("steamrt3")
        );
        assert_eq!(runtime(""), None);
        fs_err::write(proton.join("toolmanifest.vdf"), manifest).unwrap();
        assert!(needs_setup(&prefix, &proton, &umu), "a new prefix");
        fs_err::write(prefix.join("version"), "11.0-100").unwrap();
        assert!(needs_setup(&prefix, &proton, &umu), "no runtime yet");
        fs_err::create_dir_all(umu.join("steamrt4")).unwrap();
        fs_err::write(umu.join("steamrt4/_v2-entry-point"), "").unwrap();
        assert!(!needs_setup(&prefix, &proton, &umu));
    }

    #[test]
    fn the_game_process_is_its_windows_path_in_its_prefix() {
        let var = b"/data/prefixes/gog-1";
        let env = b"HOME=/home/a\0WINEPREFIX=/data/prefixes/gog-1/pfx/\0";
        let game = b"X:\\games\\June\\June.exe\0-skipintro\0";
        assert!(is_game_process(game, env, "june.exe", var));
        assert!(!is_game_process(
            game,
            b"WINEPREFIX=/data/prefixes/gog-10/pfx/\0",
            "june.exe",
            var
        ));
        // umu-run and the runtime name the program only in later arguments.
        let umu = b"/usr/bin/python3\0/usr/bin/umu-run\0/games/June/June.exe\0";
        assert!(!is_game_process(umu, env, "June.exe", var));
        assert!(is_game_process(
            b"/games/june/June.exe\0",
            env,
            "June.exe",
            var
        ));
        assert!(is_game_process(
            game,
            b"WINEPREFIX=/data/prefixes/gog-1\0",
            "june.exe",
            var
        ));
    }
}
