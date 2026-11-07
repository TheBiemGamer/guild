//! GOG games through gogdl (https://github.com/Heroic-Games-Launcher/heroic-gogdl, 1.4.0), Heroic's open-source
//! GOG client. gogdl keeps the login in `<data>/gog/auth.json`, plain JSON like Legendary's `user.json`. Its
//! tokens are read into memory only: never logged, stored elsewhere, put in an error or sent to the UI.
//! The library comes from GOG's endpoints: owned ids from `embed.gog.com`, titles and art from
//! `api.gog.com/products`, cached in `info` as `gog-product`. Installs, updates, repairs and DLC changes are gogdl
//! jobs run by the download queue (`commands::game`); each install folder is recorded in `gog_installs`. A game
//! starts from its `goggame-<id>.info` play task, its cloud saves sync through gogdl's `save-sync`, and GOG's own
//! playtime figure is read from `gameplay.gog.com` (sessions timed here are never sent back).

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::de::IgnoredAny;
use serde_json::{Value, json};

use super::epic::{InstallEvent, LaunchOptions, SaveSync, SyncOutcome, split_args};
use super::{Art, Dlc, Error, Game, Image, Progress, ProgressAction, Provider, ProviderKind};
use crate::cache::{Dirs, db};
use crate::tools;

/// GOG Galaxy's client id: gogdl's default, and the key of the main login in `auth.json`.
const CLIENT_ID: &str = "46899977096215655";

/// Where GOG's login page sends the browser after login, with a `code` for `gogdl auth --code`.
pub const LOGIN_URL: &str = "https://auth.gog.com/auth?client_id=46899977096215655&redirect_uri=https%3A%2F%2Fembed.gog.com%2Fon_login_success%3Forigin%3Dclient&response_type=code&layout=galaxy";

const OWNED_URL: &str = "https://embed.gog.com/user/data/games";
const PRODUCTS_URL: &str = "https://api.gog.com/products";
const USERS_URL: &str = "https://users.gog.com/users";
/// `<BUILDS_URL>/<id>/os/windows/builds?generation=2`: a game's builds, newest first. Public.
const BUILDS_URL: &str = "https://content-system.gog.com/products";

/// `info` kind of a product's trimmed JSON (`trim`), or `null` for an id GOG didn't answer for.
const PRODUCT: &str = "gog-product";
const PRODUCT_MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;
/// `info` kind of the logged-in account's name, under the key `gog`.
const ACCOUNT: &str = "gog-account";
const ACCOUNT_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;
/// `info` kind of an installed game's newest public build id: a JSON string, or `null` when it has none.
const LATEST_BUILD: &str = "gog-build";
const LATEST_BUILD_MAX_AGE_SECS: i64 = 6 * 60 * 60;
/// `api.gog.com/products` answers at most 50 ids per request.
const BATCH: usize = 50;
const BATCH_GAP: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Login {
    /// `gogdl.exe` is in none of the places `tools::GOGDL.exe()` looks.
    NoGogdl,
    LoggedOut,
    LoggedIn,
    /// gogdl couldn't refresh the login, e.g. offline: keep the last known games.
    Unreachable,
}

/// The main login's access token, in memory only. No `Debug`, so it can't reach a log by accident.
#[derive(Clone)]
pub struct Creds {
    access_token: String,
    user_id: String,
    /// Unix seconds when the access token runs out.
    expires_at: f64,
}

impl Creds {
    /// From gogdl's token JSON. `loginTime` is when gogdl got the token; `now` stands in when it's missing.
    fn parse(v: &Value, now: f64) -> Option<Self> {
        let user_id = id_text(&v["user_id"])?;
        Some(Self {
            access_token: v["access_token"].as_str().filter(|t| !t.is_empty())?.to_owned(),
            user_id,
            expires_at: v["loginTime"].as_f64().unwrap_or(now) + v["expires_in"].as_f64()?,
        })
    }

    /// Good for at least another minute.
    fn fresh(&self, now: f64) -> bool {
        now < self.expires_at - 60.0
    }

    fn bearer(&self) -> String {
        format!("Bearer {}", self.access_token)
    }
}

/// What `gogdl auth` said about the login.
enum Auth {
    In(Creds),
    Out,
    Unreachable,
}

/// The last good login, until it runs out. Held across every gogdl `auth`, so only one runs at a time:
/// concurrent refreshes rewrite `auth.json` non-atomically.
static AUTH: Mutex<Option<Creds>> = Mutex::new(None);

fn lock_auth() -> std::sync::MutexGuard<'static, Option<Creds>> {
    AUTH.lock().unwrap_or_else(|e| e.into_inner())
}

/// The login state, and the access token when logged in. Runs gogdl (which refreshes an expired token)
/// only when the token in memory is missing or about to run out.
pub fn login() -> (Login, Option<Creds>) {
    if tools::version(&tools::GOGDL).is_none() {
        return (Login::NoGogdl, None);
    }
    match auth() {
        Auth::In(creds) => (Login::LoggedIn, Some(creds)),
        Auth::Out => (Login::LoggedOut, None),
        Auth::Unreachable => (Login::Unreachable, None),
    }
}

fn auth() -> Auth {
    let mut cached = lock_auth();
    let now = now_secs();
    if let Some(creds) = cached.as_ref().filter(|c| c.fresh(now)) {
        return Auth::In(creds.clone());
    }
    *cached = None;
    if !has_login(&auth_file()) {
        return Auth::Out;
    }
    let output = match gogdl(["auth"]).output() {
        Ok(output) => output,
        Err(e) => {
            log!("gogdl auth: {e}");
            return Auth::Unreachable;
        }
    };
    let auth = classify(output.status.success(), &output.stdout, &String::from_utf8_lossy(&output.stderr), now);
    match &auth {
        Auth::In(creds) => *cached = Some(creds.clone()),
        // stdout and stderr stay unread here: stdout carries the tokens.
        Auth::Unreachable => log!("gogdl auth: couldn't refresh the login ({})", output.status),
        Auth::Out => log!("gogdl auth: the login expired"),
    }
    auth
}

/// Reads `gogdl auth`: the credentials on stdout, or `null` when it has none. `null` with the refresh error
/// means GOG couldn't be reached; without it the session expired. Serde errors are dropped unread.
fn classify(success: bool, stdout: &[u8], stderr: &str, now: f64) -> Auth {
    if !success {
        return Auth::Unreachable;
    }
    match serde_json::from_slice::<Value>(stdout) {
        Ok(Value::Null) if stderr.contains("Failed to refresh credentials") => Auth::Unreachable,
        Ok(Value::Null) => Auth::Out,
        Ok(v) if v["error"] != true => Creds::parse(&v, now).map_or(Auth::Unreachable, Auth::In),
        // shortcut: `{"error": true}` and unreadable output (a crash reading `auth.json` mid-write) both keep the cache.
        _ => Auth::Unreachable,
    }
}

/// `auth.json` has the main login. Parsed into ignored values, so no token is held.
fn has_login(file: &Path) -> bool {
    let read = std::fs::read(file).ok().and_then(|b| serde_json::from_slice::<HashMap<String, IgnoredAny>>(&b).ok());
    read.is_some_and(|logins| logins.contains_key(CLIENT_ID))
}

/// Hands the code from the login window to gogdl, which stores the session. Its answer is only checked for
/// `error`, then kept in memory as the login.
pub fn log_in(code: &str) -> Result<(), Error> {
    let mut cached = lock_auth();
    std::fs::create_dir_all(dir())?;
    let output = gogdl(["auth", "--code", code]).output()?;
    let answer = serde_json::from_slice::<Value>(&output.stdout).ok().filter(|v| v["error"] != true);
    match answer.and_then(|v| Creds::parse(&v, now_secs())) {
        Some(creds) => {
            *cached = Some(creds);
            Ok(())
        }
        None => Err("GOG didn't accept the login. Try again.".into()),
    }
}

/// gogdl has no logout: forgetting the login is deleting its file.
pub fn log_out() -> Result<(), Error> {
    let mut cached = lock_auth();
    *cached = None;
    match std::fs::remove_file(auth_file()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Couldn't log out of GOG: {e}").into()),
        _ => Ok(()),
    }
}

/// `<data>/gog`: gogdl's config folder (its manifests go in `heroic_gogdl` under it) and `auth.json`.
fn dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| Dirs::new().gog)
}

fn auth_file() -> PathBuf {
    dir().join("auth.json")
}

/// gogdl with its global flags before the subcommand. `--auth-config-path` always: without it `auth --code`
/// crashes writing the file. Never `-d`: its debug log prints the arguments, a login code among them.
/// `PYTHONUTF8` keeps piped output UTF-8 instead of cp1252.
fn gogdl(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    let mut command = Command::new(tools::GOGDL.exe());
    command
        .arg("--auth-config-path")
        .arg(auth_file())
        .args(args)
        .env("GOGDL_CONFIG_PATH", dir())
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::null());
    crate::platform::no_window(&mut command);
    command
}

fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// GOG writes ids as strings or numbers.
fn id_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub struct Gog;

/// Listed by `list`, which knows the app's folders and HTTP client; installed and uninstalled through `commands::game`.
const NOT_HERE: &str = "GOG games go through the download queue.";

impl Provider for Gog {
    fn list_games(&self) -> Result<Vec<Game>, Error> {
        Err(NOT_HERE.into())
    }

    fn launch(&self, game: &Game) -> Result<(), Error> {
        let dir = game.install_dir.as_deref().filter(|_| game.installed).ok_or("This game isn't installed.")?;
        launch(&game.external_id, dir, &LaunchOptions::default()).map(drop)
    }

    fn install(&self, _game: &Game) -> Result<(), Error> {
        Err(NOT_HERE.into())
    }

    fn uninstall(&self, _game: &Game) -> Result<(), Error> {
        Err(NOT_HERE.into())
    }

    fn running_game(&self) -> Option<String> {
        None
    }

    fn download_progress(&self, _game: &Game) -> Option<Progress> {
        None
    }
}

/// The owned GOG games. Products come from the cache; only uncached ids are asked, 50 per request. Also
/// refreshes the cached account name, and marks the installed games with cloud saves. Blocks: call from a blocking thread.
pub fn list(http: &reqwest::Client, dirs: &Dirs, creds: &Creds) -> Result<Vec<Game>, Error> {
    let conn = db::open(&dirs.db)?;
    let now = now_secs() as i64;
    let installs = db::gog_installs(&conn)?;
    let installed: Vec<u64> = installs
        .iter()
        .filter_map(|(key, dir)| key.strip_prefix("gog-")?.parse().ok().filter(|&id| has_info(dir, id)))
        .collect();
    let (owned, products, latest, saves) = tauri::async_runtime::block_on(async {
        let owned = owned_ids(&get_json(http.get(OWNED_URL).header(reqwest::header::AUTHORIZATION, creds.bearer())).await?);
        if let Err(e) = remember_account(http, &conn, creds, now).await {
            log!("gog account: {e}");
        }
        let mut products = HashMap::new();
        let mut missing = Vec::new();
        for &id in &owned {
            match db::cached_info(&conn, &key(id), PRODUCT, PRODUCT_MAX_AGE_SECS, now)? {
                Some(json) => _ = products.insert(id, serde_json::from_str(&json).unwrap_or(Value::Null)),
                None => missing.push(id),
            }
        }
        for (i, batch) in missing.chunks(BATCH).enumerate() {
            if i > 0 {
                tokio::time::sleep(BATCH_GAP).await;
            }
            let ids: Vec<String> = batch.iter().map(u64::to_string).collect();
            let answer = get_json(http.get(format!("{PRODUCTS_URL}?ids={}", ids.join(",")))).await?;
            let mut found: HashMap<u64, Value> =
                answer.as_array().into_iter().flatten().filter_map(|p| Some((p["id"].as_u64()?, trim(p)))).collect();
            // An id GOG doesn't answer for is remembered as `null`, so it isn't asked every scan.
            // shortcut: such a game (gone from the catalog) stays out of the library until its entry expires.
            for &id in batch {
                let product = found.remove(&id).unwrap_or(Value::Null);
                db::put_info(&conn, &key(id), PRODUCT, &product.to_string(), now)?;
                products.insert(id, product);
            }
        }
        let latest = latest_builds(http, &conn, &installed, now).await;
        let saves = with_cloud_saves(http, &conn, &installs, &installed, now).await;
        Ok::<_, Error>((owned, products, latest, saves))
    })?;
    let covers = crate::gamesdb::covers(&conn, now);
    let mut games = games(&owned, &products, &installs, &latest, &covers);
    for g in &mut games {
        g.cloud_saves = g.installed && g.external_id.parse().is_ok_and(|id| saves.contains(&id));
    }
    Ok(games)
}

/// The newest public build of each installed game (GOG's builds list needs no login), cached 6 h and asked one game
/// at a time. A failure is logged and leaves that game without an update mark.
async fn latest_builds(http: &reqwest::Client, conn: &Connection, ids: &[u64], now: i64) -> HashMap<u64, String> {
    let mut latest = HashMap::new();
    let mut asked = false;
    for &id in ids {
        let json = match db::cached_info(conn, &key(id), LATEST_BUILD, LATEST_BUILD_MAX_AGE_SECS, now).ok().flatten() {
            Some(json) => json,
            None => {
                if std::mem::replace(&mut asked, true) {
                    tokio::time::sleep(BATCH_GAP).await;
                }
                match get_json(http.get(format!("{BUILDS_URL}/{id}/os/windows/builds?generation=2"))).await {
                    Ok(body) => {
                        let json = default_build(&body).map_or(Value::Null, Value::String).to_string();
                        if let Err(e) = db::put_info(conn, &key(id), LATEST_BUILD, &json, now) {
                            log!("gog builds cache: {e}");
                        }
                        json
                    }
                    Err(e) => {
                        log!("gog builds {id}: {e}");
                        continue;
                    }
                }
            }
        };
        if let Ok(Value::String(build)) = serde_json::from_str(&json) {
            latest.insert(id, build);
        }
    }
    latest
}

/// The build a plain update installs: the first one on no branch.
fn default_build(builds: &Value) -> Option<String> {
    builds["items"].as_array()?.iter().find(|b| b["branch"].is_null()).and_then(|b| id_text(&b["build_id"]))
}

async fn get_json(request: reqwest::RequestBuilder) -> Result<Value, Error> {
    let body = request.send().await?.error_for_status()?.text().await?;
    Ok(serde_json::from_str(&body)?)
}

fn key(id: u64) -> String {
    ProviderKind::Gog.key(&id.to_string())
}

/// `{"owned": [ids]}`, sorted and without repeats.
fn owned_ids(body: &Value) -> Vec<u64> {
    let mut ids: Vec<u64> = body["owned"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// What the library keeps of a product: its DLC as plain ids (`dlcs` is `[]` without any, else `{products}`).
fn trim(p: &Value) -> Value {
    let dlcs: Vec<u64> =
        p["dlcs"]["products"].as_array().into_iter().flatten().filter_map(|d| d["id"].as_u64()).collect();
    json!({
        "title": p["title"],
        "game_type": p["game_type"],
        "images": { "logo": p["images"]["logo"], "background": p["images"]["background"] },
        "dlcs": dlcs,
    })
}

/// Owned products that are games (packs and DLC are left out), by trimmed product (`trim`). A game is installed
/// when it has an install folder that holds its `goggame-<id>.info`; so is each owned DLC. An installed game has an
/// update when its folder's build isn't the `latest` one. `covers` are GamesDB's portrait covers by key; without
/// one the grid shows the landscape logo art.
fn games(
    owned: &[u64],
    products: &HashMap<u64, Value>,
    installs: &HashMap<String, PathBuf>,
    latest: &HashMap<u64, String>,
    covers: &HashMap<String, String>,
) -> Vec<Game> {
    let owned_set: HashSet<u64> = owned.iter().copied().collect();
    let title = |p: &Value| p["title"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned);
    owned
        .iter()
        .filter_map(|&id| {
            let p = products.get(&id).filter(|p| p["game_type"] == "game")?;
            let key = key(id);
            let dir = installs.get(&key).filter(|dir| has_info(dir, id));
            // GOG's image URLs are protocol-relative.
            let image = |field: &str| Image {
                path: None,
                url: p["images"][field].as_str().and_then(|u| u.strip_prefix("//")).map(|u| format!("https://{u}")),
            };
            let mut dlc: Vec<Dlc> = p["dlcs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .filter(|d| owned_set.contains(d))
                .map(|d| Dlc {
                    id: d.to_string(),
                    title: products.get(&d).and_then(title).unwrap_or_else(|| d.to_string()),
                    installed: dir.is_some_and(|dir| has_info(dir, d)),
                })
                .collect();
            dlc.sort_by(|a, b| a.title.cmp(&b.title));
            Some(Game {
                provider: ProviderKind::Gog,
                external_id: id.to_string(),
                title: title(p)?,
                installed: dir.is_some(),
                install_dir: dir.cloned(),
                // Playtime comes from `epic_playtime::merge`, cloud saves from `list`.
                // shortcut: the size on disk isn't measured.
                playtime_minutes: None,
                last_played: None,
                size_bytes: None,
                art: Art {
                    cover: Image { path: None, url: covers.get(&key).cloned() },
                    header: image("logo"),
                    hero: image("background"),
                },
                dlc,
                update_available: dir.zip(latest.get(&id)).is_some_and(|(dir, build)| {
                    installed_build(dir, &id.to_string()).is_some_and(|local| local != *build)
                }),
                cloud_saves: false,
            })
        })
        .collect()
}

/// Caches the account's name (`users.gog.com`) unless the cached one is this account's and recent.
async fn remember_account(http: &reqwest::Client, conn: &rusqlite::Connection, creds: &Creds, now: i64) -> Result<(), Error> {
    let cached = db::cached_info(conn, "gog", ACCOUNT, ACCOUNT_MAX_AGE_SECS, now)?;
    if cached.and_then(|json| serde_json::from_str::<Value>(&json).ok()).is_some_and(|a| a["user_id"] == creds.user_id.as_str()) {
        return Ok(());
    }
    let user = get_json(http.get(format!("{USERS_URL}/{}", creds.user_id)).header(reqwest::header::AUTHORIZATION, creds.bearer())).await?;
    let name = user["username"].as_str().ok_or("no username")?;
    db::put_info(conn, "gog", ACCOUNT, &json!({ "user_id": creds.user_id, "username": name }).to_string(), now)?;
    Ok(())
}

/// The cached name of the account logged in now, if the scan has fetched it. `None` while gogdl is refreshing
/// the login, rather than wait for it.
pub fn account_name(conn: &rusqlite::Connection) -> Option<String> {
    let user_id = AUTH.try_lock().ok()?.as_ref()?.user_id.clone();
    let json = db::cached_info(conn, "gog", ACCOUNT, i64::MAX / 2, 0).ok()??;
    let account: Value = serde_json::from_str(&json).ok()?;
    (account["user_id"] == user_id.as_str()).then(|| account["username"].as_str().map(str::to_owned)).flatten()
}

// Installing and managing games. Commands and flags are from gogdl 1.4.0's `args.py`; the lines read from its log
// are from its source's format strings.

/// `info` kind of `gogdl info`'s answer for a game (sizes, languages, DLC, folder name), kept a week.
const INSTALL_INFO: &str = "gog-install-info";
const INSTALL_INFO_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;
/// `info` kind of a game's cloud save timestamps, one row per game: `{location name: timestamp}`. Forgotten with the install.
const SAVE_TS: &str = "gog-save-ts";
/// gogdl retries forever without a connection, so a job that sends no progress this long is stopped.
const STALL: Duration = Duration::from_secs(5 * 60);
const STALLED: &str = "No progress for 5 minutes, so the download was stopped. Check your connection and try again.";
const UNREACHABLE: &str = "Can't reach GOG. Try again later.";
const NO_MANIFEST: &str = "Imported games can't be verified.";
const DEFAULT_LANG: &str = "en-US";

/// Which DLC a job installs; a DLC left out of `Only` is deleted by an update.
#[derive(Debug, Clone, PartialEq)]
pub enum Dlcs {
    All,
    Only(Vec<String>),
}

/// Where installs go without an install folder setting: `%USERPROFILE%\Games`, writable without admin.
pub fn default_base() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().join("Games"))
}

fn has_info(dir: &Path, id: impl std::fmt::Display) -> bool {
    dir.join(format!("goggame-{id}.info")).is_file()
}

/// Where gogdl keeps a game's manifest, which `repair` needs; an imported game has none.
fn manifest(id: &str) -> PathBuf {
    dir().join("heroic_gogdl").join("manifests").join(id)
}

/// The game's support files (its install scripts and redistributables), which this app doesn't run.
fn support(id: &str) -> PathBuf {
    dir().join("support").join(id)
}

/// A JSON file, with or without a byte order mark.
fn read_json(path: &Path) -> Option<Value> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes)).ok()
}

/// The first line of stdout that is a JSON object: gogdl prints its answers on one line.
fn json_line(stdout: &[u8]) -> Option<Value> {
    String::from_utf8_lossy(stdout).lines().find(|l| l.starts_with('{')).and_then(|l| serde_json::from_str(l).ok())
}

/// A job needs a fresh login: gogdl would otherwise refresh an expired one itself, racing the scan's refresh.
fn ensure_login() -> Result<(), Error> {
    match login().0 {
        Login::LoggedIn => Ok(()),
        Login::NoGogdl => Err("Install gogdl in Settings first.".into()),
        Login::LoggedOut => Err("Log in to GOG in Settings first.".into()),
        Login::Unreachable => Err(UNREACHABLE.into()),
    }
}

/// `en-US` when the game has it, else the first of its languages in order.
fn pick_lang(info: &Value) -> String {
    let mut langs: Vec<&str> = info["languages"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    langs.sort_unstable();
    if langs.contains(&DEFAULT_LANG) { DEFAULT_LANG } else { langs.first().copied().unwrap_or(DEFAULT_LANG) }.to_owned()
}

/// `download_size` of a `size` map: the files every language shares plus `lang`'s.
fn size_of(size: &Value, lang: &str) -> Option<u64> {
    match (size["*"]["download_size"].as_u64(), size[lang]["download_size"].as_u64()) {
        (None, None) => None,
        (all, own) => Some(all.unwrap_or(0) + own.unwrap_or(0)),
    }
}

/// Bytes an install downloads: the game, plus the DLC among `dlcs` (an install takes the owned DLC by default).
pub fn download_size(info: &Value, dlcs: &[String]) -> Option<u64> {
    let lang = pick_lang(info);
    let extra: u64 = info["dlcs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| id_text(&d["id"]).is_some_and(|id| dlcs.contains(&id)))
        .filter_map(|d| size_of(&d["size"], &lang))
        .sum();
    Some(size_of(&info["size"], &lang)? + extra)
}

/// `gogdl info` for a game, from the cache while under a week old. `None` when gogdl couldn't tell (logged).
pub fn install_info(conn: Option<&Connection>, id: &str) -> Option<Value> {
    let key = ProviderKind::Gog.key(id);
    let json = crate::info::cached_or_fetch(conn, &key, INSTALL_INFO, INSTALL_INFO_MAX_AGE_SECS, now_secs() as i64, || {
        // A fresh login first, so gogdl doesn't refresh it itself (a game with DLC asks GOG what is owned).
        _ = login();
        let output = gogdl(["info", id, "--os", "windows"]).output().inspect_err(|e| log!("gogdl info {id}: {e}")).ok()?;
        let info = json_line(&output.stdout).filter(|v| v["size"].is_object());
        if info.is_none() {
            log!("gogdl info {id}: {} {}", output.status, last_error(&String::from_utf8_lossy(&output.stderr)));
        }
        Some(info?.to_string())
    })?;
    serde_json::from_str(&json).ok()
}

/// The build in an install folder: `buildId` of its `goggame-<id>.info`, else of its `goggame-<id>.id`.
fn installed_build(dir: &Path, id: &str) -> Option<String> {
    let build = |ext: &str| read_json(&dir.join(format!("goggame-{id}.{ext}"))).and_then(|v| id_text(&v["buildId"]));
    build("info").or_else(|| build("id"))
}

/// What an install folder says about the game `id`: its language and the DLC installed with it (the
/// `goggame-*.info` files of another game with `id` as their root).
#[derive(Debug, PartialEq)]
struct Local {
    lang: String,
    dlcs: Vec<String>,
}

fn local(dir: &Path, id: &str) -> Local {
    let root = read_json(&dir.join(format!("goggame-{id}.info"))).unwrap_or(Value::Null);
    let mut dlcs: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.starts_with("goggame-") && n.ends_with(".info")))
        .filter_map(|e| read_json(&e.path()))
        .filter(|v| id_text(&v["rootGameId"]).as_deref() == Some(id))
        .filter_map(|v| id_text(&v["gameId"]).filter(|game| game != id))
        .collect();
    dlcs.sort_unstable();
    Local { lang: root["languages"][0].as_str().unwrap_or(DEFAULT_LANG).to_owned(), dlcs }
}

/// gogdl's `download`, `update` or `repair` arguments. `download` puts the game in a folder named after it inside
/// `path`; `update` and `repair` take the game's own folder.
fn job_args(verb: &str, id: &str, path: &Path, lang: &str, dlcs: &Dlcs) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![verb.into(), id.into(), "--platform".into(), "windows".into(), "--path".into()];
    args.extend([path.into(), "--support".into(), support(id).into(), "--lang".into(), lang.into()]);
    match dlcs {
        Dlcs::All => args.push("--with-dlcs".into()),
        Dlcs::Only(ids) if !ids.is_empty() => args.extend(["--with-dlcs".into(), "--dlcs".into(), ids.join(",").into()]),
        Dlcs::Only(_) => args.push("--skip-dlcs".into()),
    }
    args
}

/// `[PROGRESS] INFO: = Progress: 12.34 123456/1000000, Running for: 00:00:10, ETA: 00:01:00` (stderr, about once
/// a second): bytes written of the bytes the install takes, with no `%`.
fn parse_progress(line: &str, action: ProgressAction) -> Option<Progress> {
    let (_, rest) = line.split_once("= Progress: ")?;
    let mut words = rest.split_whitespace();
    let percent: f32 = words.next()?.parse().ok()?;
    let (done, total) = words.next()?.trim_end_matches(',').split_once('/')?;
    let (done, total): (u64, u64) = (done.parse().ok()?, total.parse().ok()?);
    let secs_left = rest.split_once("ETA: ").and_then(|(_, eta)| {
        eta.trim().split(':').try_fold(0u64, |secs, part| Some(secs * 60 + part.parse::<u64>().ok()?))
    });
    let fraction = if total > 0 { done as f32 / total as f32 } else { percent / 100.0 };
    Some(Progress {
        fraction: fraction.clamp(0.0, 1.0),
        action,
        done_bytes: Some(done),
        total_bytes: Some(total),
        secs_left,
        done_files: None,
        total_files: None,
    })
}

/// `repair` found bad files and goes on to download them: `[V2] INFO: Found 3 broken files, repairing...`.
fn repairing(line: &str) -> bool {
    line.contains("broken files, repairing")
}

#[derive(Debug, PartialEq)]
enum Fed {
    Ended,
    Stalled,
}

/// Hands each line to `on_line` until the sender is gone. Once armed (from the start, or by the first line
/// `on_line` says was progress), `stall` without progress ends it early.
fn feed(lines: &Receiver<String>, stall: Duration, mut armed: bool, mut on_line: impl FnMut(&str) -> bool) -> Fed {
    let mut last = Instant::now();
    loop {
        let line = if armed {
            match lines.recv_timeout(stall.saturating_sub(last.elapsed())) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => return Fed::Stalled,
                Err(RecvTimeoutError::Disconnected) => return Fed::Ended,
            }
        } else {
            match lines.recv() {
                Ok(line) => line,
                Err(_) => return Fed::Ended,
            }
        };
        if on_line(&line) {
            armed = true;
            last = Instant::now();
        }
    }
}

/// How a gogdl job ended.
struct Run {
    success: bool,
    stdout: String,
    /// stderr, where gogdl logs.
    log: String,
}

/// Runs a gogdl job, reporting its pid and progress. A `Verify` job is a `repair`: it hashes the files without
/// progress, then reports `Repair` progress once it found bad files. A job silent for `STALL` once progress is
/// expected (from the start, for a download) is killed.
fn run(args: Vec<OsString>, mut action: ProgressAction, mut on_event: impl FnMut(InstallEvent)) -> Result<Run, Error> {
    let mut child = gogdl(args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let pid = child.id();
    on_event(InstallEvent::Started(pid));
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        _ = stdout.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let stderr = child.stderr.take().expect("stderr is piped");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut log = String::new();
    let fed = feed(&rx, STALL, action != ProgressAction::Verify, |line| {
        log.push_str(line);
        log.push('\n');
        if action == ProgressAction::Verify && repairing(line) {
            action = ProgressAction::Repair;
            on_event(InstallEvent::Progress(Progress {
                fraction: 0.0,
                action,
                done_bytes: None,
                total_bytes: None,
                secs_left: None,
                done_files: None,
                total_files: None,
            }));
        }
        let progress = parse_progress(line, action);
        let found = progress.is_some();
        progress.into_iter().for_each(|p| on_event(InstallEvent::Progress(p)));
        found
    });
    if fed == Fed::Stalled {
        if let Err(e) = crate::platform::kill_tree(pid) {
            log!("stop stalled gogdl: {e}");
            _ = child.kill();
        }
        _ = child.wait();
        return Err(STALLED.into());
    }
    let status = child.wait()?;
    Ok(Run { success: status.success(), stdout: out.join().unwrap_or_default(), log })
}

/// Whether a job worked. gogdl exits 0 after some failures, so it also needs no `CRITICAL:` line and the game's
/// info file in its folder (`done`).
fn outcome(run: &Run, done: bool) -> Result<(), Error> {
    if run.stdout.contains("Not enough disk space") {
        return Err("There isn't enough disk space.".into());
    }
    if run.log.contains("doesn't support content system api") {
        // Also what an invalid login looks like.
        let unreachable = run.log.contains("Failed to refresh credentials");
        return Err(if unreachable { UNREACHABLE } else { "GOG doesn't offer this game for download here." }.into());
    }
    if run.log.contains("No manifest stored locally") {
        return Err(NO_MANIFEST.into());
    }
    if run.success && done && !run.log.contains("CRITICAL:") {
        return Ok(());
    }
    Err(last_error(&run.log).into())
}

/// The last error in gogdl's log without its `[NAME] LEVEL: ` head, else its last line.
fn last_error(log: &str) -> String {
    let lines = || log.lines().rev().map(str::trim).filter(|l| !l.is_empty());
    let is_error = |l: &&str| l.contains("] ERROR: ") || l.contains("] CRITICAL: ");
    let Some(line) = lines().find(is_error).or_else(|| lines().next()) else {
        return "gogdl stopped without saying why.".into();
    };
    line.split_once(": ").filter(|(head, _)| head.starts_with('[')).map_or(line, |(_, m)| m).to_owned()
}

/// Downloads a game into a folder named after it inside `base`, with its owned DLC or none. Returns that folder.
pub fn install(
    conn: Option<&Connection>,
    id: &str,
    base: &Path,
    with_dlc: bool,
    on_event: impl FnMut(InstallEvent),
) -> Result<PathBuf, Error> {
    ensure_login()?;
    let info = install_info(conn, id).ok_or(UNREACHABLE)?;
    let folder = info["folder_name"].as_str().filter(|f| !f.is_empty()).ok_or("GOG didn't name the game's folder.")?;
    std::fs::create_dir_all(base)?;
    let dlcs = if with_dlc { Dlcs::All } else { Dlcs::Only(Vec::new()) };
    let run = run(job_args("download", id, base, &pick_lang(&info), &dlcs), ProgressAction::Install, on_event)?;
    let dir = base.join(folder);
    outcome(&run, has_info(&dir, id))?;
    Ok(dir)
}

/// Brings an installed game to GOG's current build with `dlcs`; their diff also downloads or deletes DLC files.
/// `action` is how its progress reads: a DLC install is an `Install`.
fn update_to(
    id: &str,
    dir: &Path,
    lang: &str,
    dlcs: Vec<String>,
    action: ProgressAction,
    on_event: impl FnMut(InstallEvent),
) -> Result<(), Error> {
    ensure_login()?;
    let run = run(job_args("update", id, dir, lang, &Dlcs::Only(dlcs)), action, on_event)?;
    outcome(&run, has_info(dir, id))
}

/// Updates an installed game, keeping its installed DLC.
pub fn update(id: &str, dir: &Path, on_event: impl FnMut(InstallEvent)) -> Result<(), Error> {
    let Local { lang, dlcs } = local(dir, id);
    update_to(id, dir, &lang, dlcs, ProgressAction::Update, on_event)
}

/// Adds or removes (`add`) one DLC of an installed game.
pub fn change_dlc(id: &str, dir: &Path, dlc: &str, add: bool, on_event: impl FnMut(InstallEvent)) -> Result<(), Error> {
    let Local { lang, mut dlcs } = local(dir, id);
    dlcs.retain(|d| d != dlc);
    if add {
        dlcs.push(dlc.to_owned());
    }
    let action = if add { ProgressAction::Install } else { ProgressAction::Update };
    update_to(id, dir, &lang, dlcs, action, on_event)
}

/// Checks an installed game's files against its manifest and downloads the bad or missing ones.
pub fn verify(id: &str, dir: &Path, on_event: impl FnMut(InstallEvent)) -> Result<(), Error> {
    if !manifest(id).is_file() {
        return Err(NO_MANIFEST.into());
    }
    ensure_login()?;
    let Local { lang, dlcs } = local(dir, id);
    let run = run(job_args("repair", id, dir, &lang, &Dlcs::Only(dlcs)), ProgressAction::Verify, on_event)?;
    outcome(&run, has_info(dir, id))
}

/// Records where a game is installed. GOG's own setup steps (an install script, redistributables) don't run here,
/// so a game that has one is logged.
pub fn record_install(conn: &Connection, id: &str, dir: &Path) -> Result<(), Error> {
    db::set_gog_install(conn, &ProviderKind::Gog.key(id), Some(dir))?;
    if read_json(&manifest(id)).is_some_and(|m| m["scriptInterpreter"] == true) {
        log!("gog {id}: its install script was skipped");
    }
    Ok(())
}

/// Deletes an installed game's folder, gogdl's files for it and its record. Only a folder holding the game's
/// `goggame-<id>.info` is deleted, so a wrong record can't take another folder with it.
// shortcut: GOG's uninstaller (unins000.exe) doesn't run, so registry keys from its setup stay behind.
pub fn uninstall(conn: &Connection, id: &str, dir: &Path) -> Result<(), Error> {
    if !has_info(dir, id) {
        return Err(format!("{} doesn't hold this game, so nothing was deleted.", dir.display()).into());
    }
    std::fs::remove_dir_all(dir)?;
    _ = std::fs::remove_file(manifest(id));
    _ = std::fs::remove_dir_all(support(id));
    let key = ProviderKind::Gog.key(id);
    db::set_gog_install(conn, &key, None)?;
    db::delete_info(conn, &key, SAVE_TS)?;
    Ok(())
}

/// Reads an existing install at `path` with gogdl. Returns the game's product id.
pub fn import(path: &Path) -> Result<String, Error> {
    if !path.is_dir() {
        return Err(format!("Not a folder: {}", path.display()).into());
    }
    let output = gogdl([OsStr::new("import"), path.as_os_str()]).output()?;
    match json_line(&output.stdout).and_then(|v| id_text(&v["appName"])) {
        Some(id) => Ok(id),
        None => {
            log!("gogdl import: {} {}", output.status, last_error(&String::from_utf8_lossy(&output.stderr)));
            Err(format!("No GOG game found in {}.", path.display()).into())
        }
    }
}

/// Moves an installed game's folder into `dest` and updates its record. A rename, so only on the same drive.
pub fn move_to(conn: &Connection, id: &str, from: &Path, dest: &Path) -> Result<(), Error> {
    if !super::epic::same_drive(from, dest) {
        return Err("Moving to another drive isn't supported yet. Pick a folder on the same drive.".into());
    }
    let to = dest.join(from.file_name().ok_or("The game's folder has no name.")?);
    if to.exists() {
        return Err(format!("{} already exists.", to.display()).into());
    }
    std::fs::rename(from, &to)?;
    if let Err(e) = db::set_gog_install(conn, &ProviderKind::Gog.key(id), Some(&to)) {
        _ = std::fs::rename(&to, from);
        return Err(e);
    }
    Ok(())
}

// Playing. Play runs the primary play task of `goggame-<id>.info` directly: `gogdl launch` hands it to a shell
// (so cmd parses its arguments) and waits until the game exits.

/// A play task: a program inside the install folder (GOG writes `/` or `\`), the folder it runs in and its arguments.
#[derive(Debug, PartialEq)]
struct Task {
    path: String,
    working_dir: String,
    arguments: String,
}

/// The primary task, when it runs a program (a `URLTask` opens a web page).
// shortcut: a primary `launcher` task (Prison Architect's `Launcher/dowser.exe`) is what gets watched although it hands
// over to the game, so the session is timed only while it runs, like an Epic bootstrapper's.
fn play_task(info: &Value) -> Option<Task> {
    let task = info["playTasks"].as_array()?.iter().find(|t| t["isPrimary"] == true && t["type"] == "FileTask")?;
    let text = |field: &str| task[field].as_str().unwrap_or_default().to_owned();
    Some(Task { path: text("path"), working_dir: text("workingDir"), arguments: text("arguments") }).filter(|t| !t.path.is_empty())
}

/// `relative` inside `dir`, whichever slashes it uses.
fn inside(dir: &Path, relative: &str) -> PathBuf {
    relative.split(['/', '\\']).filter(|p| !p.is_empty()).fold(dir.to_path_buf(), |path, part| path.join(part))
}

/// What Play runs: the task's program in its working folder with its arguments, then the owner's extra arguments.
/// An alternative executable (relative to `dir`, checked by the caller) runs in its own folder with only the extra ones.
fn launch_command(dir: &Path, task: &Task, options: &LaunchOptions) -> Command {
    let (exe, cwd, mut args) = match &options.exe {
        Some(exe) => {
            let exe = inside(dir, exe);
            let cwd = exe.parent().unwrap_or(dir).to_path_buf();
            (exe, cwd, Vec::new())
        }
        None => (inside(dir, &task.path), inside(dir, &task.working_dir), split_args(&task.arguments)),
    };
    args.extend(split_args(&options.args));
    let mut command = Command::new(exe);
    command.args(args).current_dir(cwd);
    command
}

/// Starts an installed game without waiting for it. Returns its program's file name, to watch its process.
pub fn launch(id: &str, dir: &Path, options: &LaunchOptions) -> Result<String, Error> {
    let info = read_json(&dir.join(format!("goggame-{id}.info"))).ok_or("The game's files are missing. Verify or reinstall it.")?;
    let task = play_task(&info).ok_or("GOG doesn't say how to start this game.")?;
    let mut command = launch_command(dir, &task, options);
    let exe = PathBuf::from(command.get_program());
    if !exe.is_file() {
        return Err(format!("The game's program is missing: {}", exe.display()).into());
    }
    command.spawn()?;
    Ok(exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
}

// Playtime: GOG's own figure per game. Sessions timed here are added on top (`epic_playtime`) and never posted to GOG,
// which would count them twice.

/// `<GAMEPLAY_URL>/<id>/users/<user_id>/sessions`: `time_sum`, the minutes GOG counted for the game.
const GAMEPLAY_URL: &str = "https://gameplay.gog.com/games";
/// `info` kind of GOG's minutes for a game, or `null` when GOG answered without them.
pub const PLAYTIME: &str = "gog-playtime";
const PLAYTIME_MAX_AGE_SECS: i64 = 24 * 60 * 60;
/// Between two playtime requests.
pub const PLAYTIME_GAP: Duration = Duration::from_secs(1);

/// The game has no recent playtime answer.
pub fn playtime_due(conn: &Connection, key: &str, now: i64) -> bool {
    db::cached_info(conn, key, PLAYTIME, PLAYTIME_MAX_AGE_SECS, now).ok().flatten().is_none()
}

// shortcut: UNVERIFIED field, from Heroic's GOG playtime code, not a captured response.
fn parse_playtime(body: &Value) -> Option<u64> {
    body["time_sum"].as_u64()
}

/// GOG's minutes for a game, `None` when GOG answered without them (an error status or another shape). `Err` only
/// when GOG can't be reached.
pub async fn fetch_playtime(http: &reqwest::Client, creds: &Creds, id: &str) -> Result<Option<u64>, Error> {
    let url = format!("{GAMEPLAY_URL}/{id}/users/{}/sessions", creds.user_id);
    let response = http.get(url).header(reqwest::header::AUTHORIZATION, creds.bearer()).send().await?;
    if !response.status().is_success() {
        return Ok(None);
    }
    Ok(serde_json::from_str(&response.text().await?).ok().as_ref().and_then(parse_playtime))
}

// Cloud saves, through gogdl's `save-sync`, one run per save location.

/// `<SAVES_URL>/<clientId>?component_version=2.0.45`: a game's Galaxy settings, cloud saves among them. Public.
const SAVES_URL: &str = "https://remote-config.gog.com/components/galaxy_client/clients";
/// `info` kind of an installed game's cloud save locations (`[{name, location}]`), or `null` when it has none.
const SAVES: &str = "gog-saves";
const SAVES_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;

/// The game's Galaxy client id, from its `goggame-<id>.info`.
fn client_id(dir: &Path, id: impl std::fmt::Display) -> Option<String> {
    read_json(&dir.join(format!("goggame-{id}.info"))).and_then(|v| id_text(&v["clientId"]))
}

/// The installed games (`ids`, in `installs`) whose remote config turns on cloud saves. Cached a week and asked one game
/// at a time; a failure is logged and leaves that game without cloud saves until a later scan.
async fn with_cloud_saves(
    http: &reqwest::Client,
    conn: &Connection,
    installs: &HashMap<String, PathBuf>,
    ids: &[u64],
    now: i64,
) -> HashSet<u64> {
    let mut found = HashSet::new();
    let mut asked = false;
    for &id in ids {
        let json = match db::cached_info(conn, &key(id), SAVES, SAVES_MAX_AGE_SECS, now).ok().flatten() {
            Some(json) => json,
            None => {
                let Some(client) = installs.get(&key(id)).and_then(|dir| client_id(dir, id)) else { continue };
                if std::mem::replace(&mut asked, true) {
                    tokio::time::sleep(BATCH_GAP).await;
                }
                match get_json(http.get(format!("{SAVES_URL}/{client}?component_version=2.0.45"))).await {
                    Ok(config) => {
                        let json = save_locations(&config).to_string();
                        if let Err(e) = db::put_info(conn, &key(id), SAVES, &json, now) {
                            log!("gog saves cache: {e}");
                        }
                        json
                    }
                    Err(e) => {
                        log!("gog remote config {id}: {e}");
                        continue;
                    }
                }
            }
        };
        if json.starts_with('[') {
            found.insert(id);
        }
    }
    found
}

/// A remote config's Windows save locations, `null` when cloud saves are off.
fn save_locations(config: &Value) -> Value {
    let storage = &config["content"]["Windows"]["cloudStorage"];
    if storage["enabled"] != true {
        return Value::Null;
    }
    Value::Array(storage["locations"].as_array().cloned().unwrap_or_default())
}

/// A save location with its `<?NAME?>` variables resolved (`platform::gog_folder`); `None` when one is unknown.
fn resolve(location: &str, install: &Path) -> Option<PathBuf> {
    let mut path = String::new();
    let mut rest = location;
    while let Some(start) = rest.find("<?") {
        let end = start + rest[start..].find("?>")?;
        path.push_str(&rest[..start]);
        path.push_str(&crate::platform::gog_folder(&rest[start + 2..end], install)?.to_string_lossy());
        rest = &rest[end + 2..];
    }
    path.push_str(rest);
    // The remote config doubles its backslashes.
    Some(PathBuf::from(path.split(['/', '\\']).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\\")))
}

/// Each save location's name and folder. No locations means Galaxy's default folder for the game's client id. A
/// location with a variable this app doesn't know is left out, quietly.
fn save_folders(locations: &Value, client: &str, install: &Path) -> Vec<(String, PathBuf)> {
    let default = [json!({
        "name": "__default",
        "location": format!("<?APPLICATION_DATA_LOCAL?>/GOG.com/Galaxy/Applications/{client}/Storage/Shared/Files"),
    })];
    let list = locations.as_array().filter(|l| !l.is_empty()).map_or(&default[..], Vec::as_slice);
    list.iter()
        .filter_map(|l| {
            let (name, location) = (l["name"].as_str()?, l["location"].as_str()?);
            let folder = resolve(location, install);
            if folder.is_none() {
                log!("gog saves: unknown folder in {location}");
            }
            Some((name.to_owned(), folder?))
        })
        .collect()
}

/// gogdl's `save-sync` arguments for one location. `ts` is when that location last synced, `0` for never.
fn sync_args(folder: &Path, id: &str, name: &str, ts: &str, sync: SaveSync) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["save-sync".into(), folder.into(), id.into(), "--os".into(), "windows".into()];
    args.extend(["--ts".into(), ts.into(), "--name".into(), name.into()]);
    let flag = match sync {
        SaveSync::Download => Some("--skip-upload"),
        SaveSync::Upload => Some("--skip-download"),
        SaveSync::Both => None,
        SaveSync::KeepCloud => Some("--force-download"),
        SaveSync::KeepLocal => Some("--force-upload"),
    };
    args.extend(flag.map(OsString::from));
    args
}

/// Reads a `save-sync` log (stderr, `[SAVES] …` lines).
fn sync_outcome(success: bool, log: &str) -> SyncOutcome {
    if !success {
        SyncOutcome::Failed(last_error(log))
    } else if log.contains("Files in conflict") {
        SyncOutcome::Conflict
    } else if log.contains("Refused to download files, newer files locally") {
        SyncOutcome::LocalNewer
    } else if log.contains("Refused to upload files, newer files in the cloud") {
        SyncOutcome::CloudNewer
    } else {
        SyncOutcome::Synced
    }
}

/// The outcome to report of several locations': a question first, then a failure.
fn first_to_report(outcomes: Vec<SyncOutcome>) -> SyncOutcome {
    let rank = |o: &SyncOutcome| match o {
        SyncOutcome::Conflict => 0,
        SyncOutcome::LocalNewer => 1,
        SyncOutcome::CloudNewer => 2,
        SyncOutcome::Failed(_) => 3,
        SyncOutcome::Synced => 4,
    };
    outcomes.into_iter().min_by_key(rank).unwrap_or(SyncOutcome::Synced)
}

/// When each save location of the game `key` last synced, as gogdl printed it.
fn save_stamps(conn: &Connection, key: &str) -> HashMap<String, String> {
    let json = db::cached_info(conn, key, SAVE_TS, i64::MAX / 2, 0).ok().flatten();
    json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}

fn store_save_stamps(conn: &Connection, key: &str, stamps: &HashMap<String, String>) {
    let json = serde_json::to_string(stamps).unwrap_or_default();
    if let Err(e) = db::put_info(conn, key, SAVE_TS, &json, now_secs() as i64) {
        log!("gog save timestamps: {e}");
    }
}

/// Keeps the timestamp gogdl printed (stdout, a float) as the location's last sync, only when it synced: after a
/// refusal or a conflict the old one keeps the difference to ask about.
fn keep_stamp(stamps: &mut HashMap<String, String>, name: &str, outcome: &SyncOutcome, stdout: &str) {
    let stamp = stdout.trim();
    if *outcome == SyncOutcome::Synced && stamp.parse::<f64>().is_ok() {
        stamps.insert(name.to_owned(), stamp.to_owned());
    }
}

/// Syncs an installed game's cloud saves, each location from the timestamp of its last sync (`keep_stamp`).
/// Never fails: a failure is an outcome.
pub fn sync_saves(conn: Option<&Connection>, id: &str, dir: &Path, sync: SaveSync) -> SyncOutcome {
    let key = ProviderKind::Gog.key(id);
    let Some(client) = client_id(dir, id) else { return SyncOutcome::Failed("The game's files are missing.".into()) };
    let cached = conn.and_then(|c| db::cached_info(c, &key, SAVES, i64::MAX / 2, 0).ok().flatten());
    let locations = cached.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or(Value::Null);
    let folders = save_folders(&locations, &client, dir);
    if folders.is_empty() {
        return SyncOutcome::Failed("Can't tell where this game keeps its saves.".into());
    }
    if let Err(e) = ensure_login() {
        return SyncOutcome::Failed(e.to_string());
    }
    // gogdl writes the game's own token into `auth.json`, so no other gogdl auth may run meanwhile. Held over the
    // timestamps too, so two syncs of one game (Sync now while the upload after play runs) don't undo each other's.
    let _auth = lock_auth();
    let mut stamps = conn.map(|c| save_stamps(c, &key)).unwrap_or_default();
    let outcomes = folders
        .iter()
        .map(|(name, folder)| {
            _ = std::fs::create_dir_all(folder);
            let ts = stamps.get(name).map_or("0", String::as_str);
            let command = gogdl(sync_args(folder, id, name, ts, sync));
            match super::epic::run_with_timeout(command, b"", super::epic::SYNC_TIMEOUT) {
                Ok(Some((success, stdout, log))) => {
                    let outcome = sync_outcome(success, &log);
                    keep_stamp(&mut stamps, name, &outcome, &stdout);
                    outcome
                }
                Ok(None) => SyncOutcome::Failed("Cloud saves timed out.".into()),
                Err(e) => SyncOutcome::Failed(e.to_string()),
            }
        })
        .collect();
    if let Some(conn) = conn {
        store_save_stamps(conn, &key, &stamps);
    }
    first_to_report(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gog").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Every product in the fixture, trimmed as the cache keeps it.
    fn products(name: &str) -> HashMap<u64, Value> {
        fixture(name).as_array().unwrap().iter().map(|p| (p["id"].as_u64().unwrap(), trim(p))).collect()
    }

    const FAKE: &str = r#"{"access_token":"test-access-token","expires_in":3600,"refresh_token":"test-refresh-token","user_id":"0","session_id":"s","loginTime":1000.5}"#;

    fn state(auth: Auth) -> &'static str {
        match auth {
            Auth::In(_) => "in",
            Auth::Out => "out",
            Auth::Unreachable => "unreachable",
        }
    }

    #[test]
    fn auth_output_is_classified() {
        let refresh_failed = "[AUTH] ERROR: Failed to refresh credentials\n";
        assert_eq!(state(classify(true, FAKE.as_bytes(), "", 1000.0)), "in");
        assert_eq!(state(classify(true, b"null\n", refresh_failed, 1000.0)), "unreachable");
        assert_eq!(state(classify(true, b"null\n", "", 1000.0)), "out");
        assert_eq!(state(classify(true, br#"{"error": true}"#, "", 1000.0)), "unreachable");
        assert_eq!(state(classify(true, b"", "Traceback ...", 1000.0)), "unreachable");
        assert_eq!(state(classify(false, FAKE.as_bytes(), "", 1000.0)), "unreachable");
        // Credentials without a token aren't a login.
        assert_eq!(state(classify(true, br#"{"user_id":"0","expires_in":3600}"#, "", 1000.0)), "unreachable");
    }

    #[test]
    fn creds_expire_a_minute_early() {
        let Auth::In(creds) = classify(true, FAKE.as_bytes(), "", 0.0) else { panic!("not logged in") };
        assert_eq!(creds.user_id, "0");
        assert_eq!(creds.bearer(), "Bearer test-access-token");
        assert!(creds.fresh(1000.5 + 3600.0 - 61.0));
        assert!(!creds.fresh(1000.5 + 3600.0 - 59.0));
        // `auth --code` may print no `loginTime`: the token counts from now.
        let numeric = Creds::parse(&json!({"access_token": "t", "user_id": 7, "expires_in": 10}), 50.0).unwrap();
        assert_eq!((numeric.user_id.as_str(), numeric.expires_at), ("7", 60.0));
    }

    #[test]
    fn login_file_needs_the_galaxy_client() {
        let dir = std::env::temp_dir().join(format!("launcher-gog-auth-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("auth.json");
        assert!(!has_login(&file));
        std::fs::write(&file, format!(r#"{{"{CLIENT_ID}": {FAKE}, "123": {{}}}}"#)).unwrap();
        assert!(has_login(&file));
        std::fs::write(&file, r#"{"123": {}}"#).unwrap();
        assert!(!has_login(&file));
        std::fs::write(&file, "{not json").unwrap();
        assert!(!has_login(&file));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn gogdl_flags_go_before_the_subcommand() {
        let command = gogdl(["auth", "--code", "abc"]);
        let args: Vec<String> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args[0], "--auth-config-path");
        assert!(args[1].ends_with("auth.json"));
        assert_eq!(&args[2..], ["auth", "--code", "abc"]);
        assert!(!args.iter().any(|a| a == "-d" || a == "--debug"));
        let envs: HashMap<String, String> = command
            .get_envs()
            .filter_map(|(k, v)| Some((k.to_string_lossy().into_owned(), v?.to_string_lossy().into_owned())))
            .collect();
        assert_eq!(envs["PYTHONUTF8"], "1");
        assert_eq!(Path::new(&envs["GOGDL_CONFIG_PATH"]), dir());
    }

    #[test]
    fn owned_ids_are_sorted_once() {
        assert_eq!(owned_ids(&json!({"owned": [3, 1, 3, "x", 2]})), [1, 2, 3]);
        assert!(owned_ids(&json!({})).is_empty());
    }

    #[test]
    fn games_keep_only_owned_games() {
        let products = products("products.json");
        // Last Day of June (a game), The Witcher 3 Complete Edition (a pack), The Witcher 3 (a game).
        let all = games(&[1207664663, 1425609977, 1495134320], &products, &HashMap::new(), &HashMap::new(), &HashMap::new());
        let ids: Vec<&str> = all.iter().map(|g| g.external_id.as_str()).collect();
        assert_eq!(ids, ["1425609977", "1495134320"]);
        let june = &all[0];
        assert_eq!(june.key(), "gog-1425609977");
        assert_eq!(june.title, "Last Day of June");
        assert!(!june.installed);
        assert_eq!(june.install_dir, None);
        assert_eq!(
            june.art.header.url.as_deref(),
            Some("https://images-1.gog-statics.com/5f8705f07a91568cdd622e0961e6a4526e7035bc4bfef5ce440d41c4e2e7f563_glx_logo.jpg")
        );
        assert!(june.art.hero.url.as_deref().is_some_and(|u| u.starts_with("https://images-4.gog-statics.com/")));
        assert_eq!(june.art.cover, Image::default());
        // Not owned: not listed, even with a cached product.
        assert!(games(&[1], &products, &HashMap::new(), &HashMap::new(), &HashMap::new()).is_empty());
        // An id GOG didn't answer for (`null`) is left out.
        assert!(games(&[9], &HashMap::from([(9, Value::Null)]), &HashMap::new(), &HashMap::new(), &HashMap::new()).is_empty());
    }

    #[test]
    fn owned_dlc_go_under_their_game() {
        let mut products = products("products2.json");
        // Cyberpunk 2077 owns two of its DLC; one of them has a cached product with a title.
        products.insert(1445725644, json!({"title": " Phantom Liberty ", "game_type": "dlc", "images": {}, "dlcs": []}));
        let owned = [1423049311, 1445725644, 2116847748];
        let all = games(&owned, &products, &HashMap::new(), &HashMap::new(), &HashMap::new());
        assert_eq!(all.len(), 1, "a DLC is no game of its own");
        let titles: Vec<&str> = all[0].dlc.iter().map(|d| d.title.as_str()).collect();
        assert_eq!(titles, ["2116847748", "Phantom Liberty"]);
        assert!(all[0].dlc.iter().all(|d| !d.installed));
    }

    #[test]
    fn installed_needs_the_record_and_the_info_file() {
        let dir = std::env::temp_dir().join(format!("launcher-gog-installed-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut products = products("products2.json");
        products.insert(1445725644, json!({"title": "Phantom Liberty", "game_type": "dlc", "images": {}, "dlcs": []}));
        let owned = [1423049311, 1445725644];
        let installs = HashMap::from([("gog-1423049311".to_string(), dir.clone())]);
        let list = || games(&owned, &products, &installs, &HashMap::new(), &HashMap::new()).remove(0);
        // A record whose folder lacks the info file (deleted by hand) isn't installed.
        assert!(!list().installed);
        std::fs::write(dir.join("goggame-1423049311.info"), "{}").unwrap();
        let game = list();
        assert!(game.installed);
        assert_eq!(game.install_dir.as_deref(), Some(dir.as_path()));
        assert!(!game.dlc[0].installed);
        std::fs::write(dir.join("goggame-1445725644.info"), "{}").unwrap();
        assert!(list().dlc[0].installed);
        // The info file alone, without a record, installs nothing.
        assert!(!games(&owned, &products, &HashMap::new(), &HashMap::new(), &HashMap::new())[0].installed);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn gamesdb_cover_is_the_portrait_art() {
        let covers = HashMap::from([("gog-1425609977".to_string(), "https://images.gog.com/c.jpg".to_string())]);
        let all = games(&[1425609977], &products("products.json"), &HashMap::new(), &HashMap::new(), &covers);
        assert_eq!(all[0].art.cover.url.as_deref(), Some("https://images.gog.com/c.jpg"));
    }

    #[test]
    fn a_full_batch_trims_to_what_the_library_needs() {
        let batch = fixture("products50.json");
        let list = batch.as_array().unwrap();
        assert_eq!(list.len(), BATCH);
        for p in list {
            let t = trim(p);
            assert_eq!(t["title"], p["title"]);
            assert!(t["dlcs"].is_array());
            assert_eq!(t.as_object().unwrap().len(), 4);
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("launcher-gog-{name}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn job_arguments_name_the_folder_language_and_dlc() {
        let support = support("1425609977").to_string_lossy().into_owned();
        let args = strings(&job_args("download", "1425609977", Path::new(r"C:\Games"), "en-US", &Dlcs::All));
        assert_eq!(
            args,
            ["download", "1425609977", "--platform", "windows", "--path", r"C:\Games", "--support", &support, "--lang", "en-US", "--with-dlcs"]
        );
        assert!(support.ends_with(r"gog\support\1425609977"), "{support}");
        let none = strings(&job_args("download", "1", Path::new(r"C:\Games"), "de-DE", &Dlcs::Only(vec![])));
        assert_eq!(none[8..], ["--lang", "de-DE", "--skip-dlcs"]);
        let update = strings(&job_args("update", "1", Path::new(r"C:\Games\A"), "en-US", &Dlcs::Only(vec!["2".into(), "3".into()])));
        assert_eq!((update[0].as_str(), update[5].as_str()), ("update", r"C:\Games\A"));
        assert_eq!(update[10..], ["--with-dlcs", "--dlcs", "2,3"]);
        // No DLC left: skip them, so an update deletes the last one's files.
        let repair = strings(&job_args("repair", "1", Path::new(r"C:\Games\A"), "en-US", &Dlcs::Only(vec![])));
        assert_eq!((repair[0].as_str(), repair[10].as_str(), repair.len()), ("repair", "--skip-dlcs", 11));
        // Never debug mode: it logs the arguments.
        let command = gogdl(job_args("download", "1", Path::new(r"C:\Games"), "en-US", &Dlcs::All));
        assert!(!command.get_args().any(|a| a == "-d" || a == "--debug"));
    }

    #[test]
    fn progress_lines_carry_bytes_and_time_left() {
        let line = "[PROGRESS] INFO: = Progress: 12.34 123456/1000000, Running for: 00:00:10, ETA: 01:02:03";
        let p = parse_progress(line, ProgressAction::Install).unwrap();
        assert_eq!((p.done_bytes, p.total_bytes, p.secs_left), (Some(123456), Some(1000000), Some(3723)));
        assert!((p.fraction - 0.123456).abs() < 1e-6);
        assert_eq!(p.action, ProgressAction::Install);
        // No total yet: the percent stands in.
        let early = parse_progress("[PROGRESS] INFO: = Progress: 50.00 0/0, Running for: 00:00:01, ETA: 00:00:00", ProgressAction::Update).unwrap();
        assert_eq!(early.fraction, 0.5);
        assert_eq!(parse_progress("[PROGRESS] INFO: = Downloaded: 1.18 MiB, Written: 2.00 MiB", ProgressAction::Install), None);
        // Legendary's line has a `%`; it isn't gogdl's.
        assert_eq!(parse_progress("[DLManager] INFO: = Progress: 12.34% (123/456), Running for 00:00:10, ETA: 00:01:00", ProgressAction::Install), None);
        assert!(repairing("[V2] INFO: Found 3 broken files, repairing..."));
        assert!(!repairing("[V2] INFO: All files look good"));
    }

    #[test]
    fn the_watchdog_stops_a_silent_job_once_progress_is_expected() {
        let progress = |l: &str| l.starts_with('=');
        // Lines until gogdl exits.
        let (tx, rx) = mpsc::channel();
        for line in ["log", "= 1", "= 2"] {
            tx.send(line.to_string()).unwrap();
        }
        drop(tx);
        let mut seen = Vec::new();
        assert_eq!(feed(&rx, Duration::from_secs(5), true, |l| { seen.push(l.to_string()); progress(l) }), Fed::Ended);
        assert_eq!(seen, ["log", "= 1", "= 2"]);
        // Armed from the start and silent: stalled.
        let (tx, rx) = mpsc::channel::<String>();
        assert_eq!(feed(&rx, Duration::from_millis(30), true, progress), Fed::Stalled);
        // Log lines that aren't progress don't keep it alive.
        tx.send("log".into()).unwrap();
        assert_eq!(feed(&rx, Duration::from_millis(30), true, progress), Fed::Stalled);
        // Not armed (a repair hashing files): it waits, until progress arms it.
        let (tx, rx) = mpsc::channel::<String>();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            tx.send("= 1".into()).unwrap();
            std::thread::sleep(Duration::from_millis(200));
            drop(tx);
        });
        let mut lines = 0;
        let fed = feed(&rx, Duration::from_millis(30), false, |l| {
            lines += 1;
            progress(l)
        });
        assert_eq!((fed, lines), (Fed::Stalled, 1), "waited out the silence before the first progress");
        sender.join().unwrap();
    }

    fn ran(success: bool, stdout: &str, log: &str) -> Run {
        Run { success, stdout: stdout.into(), log: log.into() }
    }

    fn said(result: Result<(), Error>) -> String {
        result.map_or_else(|e| e.to_string(), |()| "ok".into())
    }

    #[test]
    fn success_needs_exit_zero_no_critical_line_and_the_info_file() {
        assert_eq!(said(outcome(&ran(true, "", "[PROGRESS] INFO: = Progress: 100.00 1/1\n"), true)), "ok");
        assert_eq!(said(outcome(&ran(true, "", "[V2] INFO: All files look good\n"), true)), "ok");
        // Exits 0 with nothing done.
        assert_eq!(said(outcome(&ran(true, "", "[TASK_EXEC] CRITICAL: Task writer failed\n"), true)), "Task writer failed");
        assert_eq!(said(outcome(&ran(true, "", "[V2] INFO: done\n"), false)), "done");
        assert_eq!(
            said(outcome(&ran(false, "", "[V2] INFO: start\n[DOWNLOAD] ERROR: Connection reset\nTraceback (most recent call last):\n"), true)),
            "Connection reset"
        );
        assert_eq!(said(outcome(&ran(false, "", ""), false)), "gogdl stopped without saying why.");
        let full = "Download size: 1.00 GiB\nUnable to proceed, Not enough disk space\n";
        assert_eq!(said(outcome(&ran(false, full, ""), false)), "There isn't enough disk space.");
        let unsupported = "[GENERIC DOWNLOAD_MANAGER] ERROR: Game doesn't support content system api, unable to proceed\n";
        assert_eq!(said(outcome(&ran(true, "", unsupported), false)), "GOG doesn't offer this game for download here.");
        let offline = format!("[AUTH] ERROR: Failed to refresh credentials\n{unsupported}");
        assert_eq!(said(outcome(&ran(true, "", &offline), false)), UNREACHABLE);
        let imported = "Traceback (most recent call last):\nException: No manifest stored locally, unable to verify\n";
        assert_eq!(said(outcome(&ran(false, "", imported), true)), NO_MANIFEST);
    }

    #[test]
    fn download_size_is_the_shared_files_plus_the_language_and_dlc() {
        let info = fixture("info-1425609977.json");
        assert_eq!(pick_lang(&info), "en-US");
        assert_eq!(download_size(&info, &[]), Some(310 + 1694805181));
        let mut with_dlc = info.clone();
        with_dlc["dlcs"] = json!([
            {"title": "Owned", "id": "7", "size": {"*": {"download_size": 5}, "en-US": {"download_size": 10}}},
            {"title": "Not owned", "id": 8, "size": {"*": {"download_size": 1000}}},
        ]);
        assert_eq!(download_size(&with_dlc, &["7".into()]), Some(310 + 1694805181 + 15));
        assert_eq!(download_size(&with_dlc, &["7".into(), "8".into()]), Some(310 + 1694805181 + 1015));
        assert_eq!(download_size(&json!({"languages": ["en-US"]}), &[]), None);
    }

    #[test]
    fn language_is_english_else_the_first_in_order() {
        assert_eq!(pick_lang(&json!({"languages": ["fr-FR", "en-US", "de-DE"]})), "en-US");
        assert_eq!(pick_lang(&json!({"languages": ["fr-FR", "de-DE"]})), "de-DE");
        assert_eq!(pick_lang(&json!({})), "en-US");
    }

    #[test]
    fn the_default_build_is_the_first_on_no_branch() {
        assert_eq!(default_build(&fixture("builds.json")).as_deref(), Some("51019539443704905"));
        let branches = json!({"items": [{"build_id": "2", "branch": "beta"}, {"build_id": 1, "branch": null}]});
        assert_eq!(default_build(&branches).as_deref(), Some("1"));
        assert_eq!(default_build(&json!({"items": []})), None);
    }

    /// An install folder like the Prison Architect sample in Heroic's `GOGImportData`: the game and one DLC. Its primary
    /// play task is a launcher that hands over to a hidden game task.
    fn prison_architect(dir: &Path) {
        let game = r#"{"buildId": "55136646198962890", "clientId": "1", "gameId": "1441974651", "language": "English",
            "languages": ["en-US"], "name": "Prison Architect", "rootGameId": "1441974651", "version": 1, "playTasks": [
                {"category": "launcher", "isPrimary": true, "languages": ["en-US"], "name": "Prison Architect",
                 "osBitness": ["64"], "path": "Launcher/dowser.exe", "type": "FileTask"},
                {"category": "game", "isHidden": true, "languages": ["en-US"], "name": "Prison Architect - launcher process Prison Architect64.exe",
                 "osBitness": ["64"], "path": "Prison Architect64.exe", "type": "FileTask"},
                {"category": "document", "isPrimary": false, "link": "https://www.gog.com/forum", "name": "Forum", "type": "URLTask"}
            ]}"#;
        // Written with a byte order mark, as some GOG files are.
        std::fs::write(dir.join("goggame-1441974651.info"), [b"\xEF\xBB\xBF".as_slice(), game.as_bytes()].concat()).unwrap();
        let dlc = r#"{"gameId": "1716751705", "rootGameId": "1441974651", "name": "Aficionado"}"#;
        std::fs::write(dir.join("goggame-1716751705.info"), dlc).unwrap();
        std::fs::write(dir.join("goggame-9.info"), r#"{"gameId": "9", "rootGameId": "9"}"#).unwrap();
    }

    #[test]
    fn the_install_folder_tells_language_build_and_dlc() {
        let dir = temp("local");
        prison_architect(&dir);
        assert_eq!(local(&dir, "1441974651"), Local { lang: "en-US".into(), dlcs: vec!["1716751705".into()] });
        assert_eq!(installed_build(&dir, "1441974651").as_deref(), Some("55136646198962890"));
        // Without a buildId in the info file, the `.id` file has it; with neither, no build.
        std::fs::write(dir.join("goggame-1716751705.id"), r#"{"buildId": "77"}"#).unwrap();
        assert_eq!(installed_build(&dir, "1716751705").as_deref(), Some("77"));
        assert_eq!(installed_build(&dir, "9"), None);
        assert_eq!(local(&temp("empty"), "1"), Local { lang: "en-US".into(), dlcs: vec![] });
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_update_shows_when_the_folder_has_another_build() {
        let dir = temp("update");
        prison_architect(&dir);
        let owned = [1441974651];
        let products = HashMap::from([(1441974651, json!({"title": "Prison Architect", "game_type": "game", "images": {}, "dlcs": []}))]);
        let installs = HashMap::from([("gog-1441974651".to_string(), dir.clone())]);
        let update = |latest: &[(u64, &str)]| {
            let latest = latest.iter().map(|&(id, b)| (id, b.to_string())).collect();
            games(&owned, &products, &installs, &latest, &HashMap::new())[0].update_available
        };
        assert!(update(&[(1441974651, "99")]));
        assert!(!update(&[(1441974651, "55136646198962890")]));
        // Builds unknown (offline, or never asked): no update.
        assert!(!update(&[]));
        // Not installed: never an update.
        let missing = games(&owned, &products, &HashMap::new(), &HashMap::from([(1441974651, "99".into())]), &HashMap::new());
        assert!(!missing[0].update_available);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn test_db(name: &str) -> (PathBuf, Connection) {
        let dir = temp(name);
        let conn = db::open(&dir.join("library.sqlite")).unwrap();
        (dir, conn)
    }

    #[test]
    fn uninstall_deletes_only_a_folder_holding_the_game() {
        // Id 0 is no real product, so gogdl's real manifest and support folders have nothing of it to delete.
        let (root, conn) = test_db("uninstall");
        let dir = root.join("Game");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("goggame-0.info"), r#"{"gameId": "0", "rootGameId": "0"}"#).unwrap();
        record_install(&conn, "0", &dir).unwrap();
        store_save_stamps(&conn, "gog-0", &HashMap::from([("saves".to_string(), "1.5".to_string()), ("__default".to_string(), "2".to_string())]));
        assert_eq!(save_stamps(&conn, "gog-0").len(), 2);
        // Another game's folder (a wrong record) is refused and kept.
        assert!(uninstall(&conn, "1", &dir).unwrap_err().to_string().contains("nothing was deleted"));
        assert!(dir.is_dir());
        uninstall(&conn, "0", &dir).unwrap();
        assert!(!dir.exists());
        assert!(db::gog_installs(&conn).unwrap().is_empty());
        assert_eq!(db::cached_info(&conn, "gog-0", SAVE_TS, i64::MAX / 2, 0).unwrap(), None);
        assert!(save_stamps(&conn, "gog-0").is_empty());
        drop(conn);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn move_renames_on_the_same_drive_and_updates_the_record() {
        let (root, conn) = test_db("move");
        let from = root.join("a").join("Prison Architect");
        std::fs::create_dir_all(&from).unwrap();
        prison_architect(&from);
        std::fs::create_dir_all(root.join("b")).unwrap();
        record_install(&conn, "1441974651", &from).unwrap();
        move_to(&conn, "1441974651", &from, &root.join("b")).unwrap();
        let to = root.join("b").join("Prison Architect");
        assert!(has_info(&to, 1441974651) && !from.exists());
        assert_eq!(db::gog_installs(&conn).unwrap()["gog-1441974651"], to);
        // Something there already, or another drive: refused before anything moves.
        std::fs::create_dir_all(root.join("c").join("Prison Architect")).unwrap();
        assert!(move_to(&conn, "1441974651", &to, &root.join("c")).unwrap_err().to_string().contains("already exists"));
        let drive = if to.to_string_lossy().starts_with('Z') { r"Y:\Games" } else { r"Z:\Games" };
        assert!(move_to(&conn, "1441974651", &to, Path::new(drive)).unwrap_err().to_string().contains("another drive"));
        assert!(to.is_dir());
        drop(conn);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn play_starts_the_primary_task() {
        let dir = temp("play");
        prison_architect(&dir);
        let info = read_json(&dir.join("goggame-1441974651.info")).unwrap();
        // The launcher task is primary, so it runs (and is watched), not the hidden game task.
        let task = play_task(&info).unwrap();
        assert_eq!(task, Task { path: "Launcher/dowser.exe".into(), working_dir: String::new(), arguments: String::new() });
        let command = launch_command(Path::new(r"C:\Games\PA"), &task, &LaunchOptions::default());
        assert_eq!(command.get_program(), r"C:\Games\PA\Launcher\dowser.exe");
        assert_eq!(command.get_current_dir(), Some(Path::new(r"C:\Games\PA")));
        assert_eq!(command.get_args().count(), 0);
        // A plain game: its working folder and arguments, then the owner's extra arguments.
        let plain = json!({"playTasks": [
            {"isPrimary": true, "type": "FileTask", "path": "bin\\game.exe", "workingDir": "bin", "arguments": "-lang en \"-name x\""},
        ]});
        let task = play_task(&plain).unwrap();
        let options = LaunchOptions { args: "-windowed".into(), ..LaunchOptions::default() };
        let command = launch_command(Path::new(r"D:\G"), &task, &options);
        assert_eq!(command.get_program(), r"D:\G\bin\game.exe");
        assert_eq!(command.get_current_dir(), Some(Path::new(r"D:\G\bin")));
        let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["-lang", "en", "-name x", "-windowed"]);
        // An alternative executable runs in its own folder, with only the extra arguments.
        let alt = LaunchOptions { exe: Some(r"tools\editor.exe".into()), ..options };
        let command = launch_command(Path::new(r"D:\G"), &task, &alt);
        assert_eq!(command.get_program(), r"D:\G\tools\editor.exe");
        assert_eq!(command.get_current_dir(), Some(Path::new(r"D:\G\tools")));
        assert_eq!(command.get_args().count(), 1);
        // No primary task, or a primary web page: nothing to start.
        assert_eq!(play_task(&json!({"playTasks": [{"type": "FileTask", "path": "a.exe"}]})), None);
        assert_eq!(play_task(&json!({"playTasks": [{"isPrimary": true, "type": "URLTask", "link": "https://gog.com"}]})), None);
        assert_eq!(play_task(&json!({})), None);
        // The program missing from the folder: refused before anything starts.
        assert!(launch("1441974651", &dir, &LaunchOptions::default()).unwrap_err().to_string().contains("missing"));
        assert!(launch("1", &dir, &LaunchOptions::default()).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_locations_resolve_their_folder_variables() {
        let install = Path::new(r"C:\Games\June");
        let low = std::env::var("USERPROFILE").unwrap() + r"\AppData\LocalLow";
        let locations = save_locations(&fixture("remote-config-1425609977.json"));
        let folders = save_folders(&locations, "1", install);
        // The remote config doubles its backslashes; the folder has single ones.
        assert_eq!(folders, [("saves".to_string(), PathBuf::from(format!(r"{low}\Ovosonico\Last Day of June")))]);
        assert_eq!(resolve("<?INSTALL?>/saves", install), Some(PathBuf::from(r"C:\Games\June\saves")));
        let local = std::env::var("LOCALAPPDATA").unwrap();
        assert_eq!(resolve("<?APPLICATION_DATA_LOCAL?>", install), Some(PathBuf::from(&local)));
        // An unknown variable, or one left open, leaves that location out.
        assert_eq!(resolve("<?STEAM_CLOUD?>/x", install), None);
        assert_eq!(resolve("<?INSTALL", install), None);
        let mixed = json!([{"name": "a", "location": "<?NOPE?>\\x"}, {"name": "b", "location": "<?INSTALL?>\\b"}]);
        assert_eq!(save_folders(&mixed, "1", install), [("b".to_string(), PathBuf::from(r"C:\Games\June\b"))]);
        // No locations: Galaxy's default folder for the client id.
        assert_eq!(
            save_folders(&json!([]), "5", install),
            [("__default".to_string(), PathBuf::from(format!(r"{local}\GOG.com\Galaxy\Applications\5\Storage\Shared\Files")))]
        );
        // Cloud saves off: no locations at all.
        let off = json!({"content": {"Windows": {"cloudStorage": {"enabled": false, "locations": []}}}});
        assert_eq!(save_locations(&off), Value::Null);
        assert_eq!(save_locations(&json!({})), Value::Null);
    }

    #[test]
    fn save_sync_arguments_follow_the_direction() {
        let args = |sync| strings(&sync_args(Path::new(r"C:\S"), "7", "saves", "1730000000.5", sync));
        assert_eq!(
            args(SaveSync::Both),
            ["save-sync", r"C:\S", "7", "--os", "windows", "--ts", "1730000000.5", "--name", "saves"]
        );
        assert_eq!(args(SaveSync::Download)[9..], ["--skip-upload"]);
        assert_eq!(args(SaveSync::Upload)[9..], ["--skip-download"]);
        assert_eq!(args(SaveSync::KeepCloud)[9..], ["--force-download"]);
        assert_eq!(args(SaveSync::KeepLocal)[9..], ["--force-upload"]);
    }

    #[test]
    fn save_sync_log_decides_the_outcome() {
        use SyncOutcome::*;
        assert_eq!(sync_outcome(true, "[SAVES] INFO: Uploading 2 files\n"), Synced);
        assert_eq!(sync_outcome(true, "[SAVES] WARNING: Refused to download files, newer files locally\n"), LocalNewer);
        assert_eq!(sync_outcome(true, "[SAVES] WARNING: Refused to upload files, newer files in the cloud\n"), CloudNewer);
        assert_eq!(sync_outcome(true, "[SAVES] WARNING: Files in conflict force downloading or uploading of files\n"), Conflict);
        assert_eq!(sync_outcome(false, "[SAVES] ERROR: Failed to get credentials\nTraceback\n"), Failed("Failed to get credentials".into()));
        // A question wins over a failure, a failure over a sync.
        assert_eq!(first_to_report(vec![Synced, Failed("x".into()), CloudNewer]), CloudNewer);
        assert_eq!(first_to_report(vec![LocalNewer, Conflict]), Conflict);
        assert_eq!(first_to_report(vec![Synced, Failed("x".into())]), Failed("x".into()));
        assert_eq!(first_to_report(vec![]), Synced);
    }

    #[test]
    fn only_a_synced_location_keeps_the_new_timestamp() {
        let mut stamps = HashMap::from([("saves".to_string(), "100.5".to_string())]);
        keep_stamp(&mut stamps, "saves", &SyncOutcome::Conflict, "200.0");
        keep_stamp(&mut stamps, "saves", &SyncOutcome::LocalNewer, "200.0\n");
        keep_stamp(&mut stamps, "saves", &SyncOutcome::Failed("x".into()), "200.0");
        assert_eq!(stamps["saves"], "100.5");
        // gogdl may print no newline; junk isn't a timestamp.
        keep_stamp(&mut stamps, "saves", &SyncOutcome::Synced, "Traceback");
        keep_stamp(&mut stamps, "other", &SyncOutcome::Synced, "300.25");
        keep_stamp(&mut stamps, "saves", &SyncOutcome::Synced, "200.0\r\n");
        assert_eq!(stamps, HashMap::from([("saves".to_string(), "200.0".to_string()), ("other".to_string(), "300.25".to_string())]));
    }

    #[test]
    fn playtime_is_the_time_sum_in_minutes() {
        let sessions = json!({"time_sum": 754, "sessions": [{"date": "2026-11-01T20:00:00+0000", "time": 120}]});
        assert_eq!(parse_playtime(&sessions), Some(754));
        assert_eq!(parse_playtime(&json!({"error": "not_found"})), None);
        assert_eq!(parse_playtime(&json!({"time_sum": "754"})), None);
    }

    /// Runs the real gogdl with no login: `gogdl info` needs only public endpoints for a game without DLC. Network;
    /// installs gogdl into a temp folder first. Its answer is `tests/fixtures/gog/info-1425609977.json`.
    /// `cargo test real_gogdl_info -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_gogdl_info_without_login() {
        let root = std::env::temp_dir().join(format!("launcher-gogdl-{}", std::process::id()));
        let exe = root.join("gogdl").join("gogdl.exe");
        let http = reqwest::Client::new();
        tauri::async_runtime::block_on(crate::tools::install_to(&http, &tools::GOGDL, &root)).unwrap();
        let data = root.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let mut command = Command::new(&exe);
        command
            .arg("--auth-config-path")
            .arg(data.join("auth.json"))
            .args(["info", "1425609977", "--os", "windows"])
            .env("GOGDL_CONFIG_PATH", &data)
            .env("PYTHONUTF8", "1");
        let output = crate::platform::no_window(&mut command).output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        println!("exit {}\nstdout: {stdout}\nstderr: {}", output.status, String::from_utf8_lossy(&output.stderr));
        let line = stdout.lines().find(|l| l.starts_with('{')).expect("a JSON line");
        let info: Value = serde_json::from_str(line).unwrap();
        assert!(info["size"]["en-US"]["download_size"].is_u64());
        assert_eq!(info["folder_name"], "Last Day of June");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
