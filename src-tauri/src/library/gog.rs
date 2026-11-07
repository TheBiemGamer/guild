//! GOG games through gogdl (https://github.com/Heroic-Games-Launcher/heroic-gogdl, 1.4.0), Heroic's open-source
//! GOG client. gogdl keeps the login in `<data>/gog/auth.json`, plain JSON like Legendary's `user.json`. Its
//! tokens are read into memory only: never logged, stored elsewhere, put in an error or sent to the UI.
//! The library comes from GOG's endpoints: owned ids from `embed.gog.com`, titles and art from
//! `api.gog.com/products`, cached in `info` as `gog-product`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::de::IgnoredAny;
use serde_json::{Value, json};

use super::{Art, Dlc, Error, Game, Image, Progress, Provider, ProviderKind};
use crate::cache::{Dirs, db};
use crate::tools;

/// GOG Galaxy's client id: gogdl's default, and the key of the main login in `auth.json`.
const CLIENT_ID: &str = "46899977096215655";

/// Where GOG's login page sends the browser after login, with a `code` for `gogdl auth --code`.
pub const LOGIN_URL: &str = "https://auth.gog.com/auth?client_id=46899977096215655&redirect_uri=https%3A%2F%2Fembed.gog.com%2Fon_login_success%3Forigin%3Dclient&response_type=code&layout=galaxy";

const OWNED_URL: &str = "https://embed.gog.com/user/data/games";
const PRODUCTS_URL: &str = "https://api.gog.com/products";
const USERS_URL: &str = "https://users.gog.com/users";

/// `info` kind of a product's trimmed JSON (`trim`), or `null` for an id GOG didn't answer for.
const PRODUCT: &str = "gog-product";
const PRODUCT_MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;
/// `info` kind of the logged-in account's name, under the key `gog`.
const ACCOUNT: &str = "gog-account";
const ACCOUNT_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;
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
        let user_id = match &v["user_id"] {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => return None,
        };
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
    let output = match gogdl(&["auth"]).output() {
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
    let output = gogdl(&["auth", "--code", code]).output()?;
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
fn gogdl(args: &[&str]) -> Command {
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

pub struct Gog;

/// Listed by `list`, which knows the app's folders and HTTP client.
const NOT_HERE: &str = "GOG games can't be installed or played here yet.";

impl Provider for Gog {
    fn list_games(&self) -> Result<Vec<Game>, Error> {
        Err(NOT_HERE.into())
    }

    fn launch(&self, _game: &Game) -> Result<(), Error> {
        Err(NOT_HERE.into())
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
/// refreshes the cached account name. Blocks: call from a blocking thread.
pub fn list(http: &reqwest::Client, dirs: &Dirs, creds: &Creds) -> Result<Vec<Game>, Error> {
    let conn = db::open(&dirs.db)?;
    let now = now_secs() as i64;
    let (owned, products) = tauri::async_runtime::block_on(async {
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
        Ok::<_, Error>((owned, products))
    })?;
    let installs = db::gog_installs(&conn)?;
    let covers = crate::gamesdb::covers(&conn, now);
    Ok(games(&owned, &products, &installs, &covers))
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
/// when it has an install folder that holds its `goggame-<id>.info`; so is each owned DLC. `covers` are
/// GamesDB's portrait covers by key; without one the grid shows the landscape logo art.
fn games(
    owned: &[u64],
    products: &HashMap<u64, Value>,
    installs: &HashMap<String, PathBuf>,
    covers: &HashMap<String, String>,
) -> Vec<Game> {
    let owned_set: HashSet<u64> = owned.iter().copied().collect();
    let title = |p: &Value| p["title"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned);
    owned
        .iter()
        .filter_map(|&id| {
            let p = products.get(&id).filter(|p| p["game_type"] == "game")?;
            let key = key(id);
            let has_info = |dir: &Path, id: u64| dir.join(format!("goggame-{id}.info")).is_file();
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
                // shortcut: playtime, size, updates and cloud saves come with the GOG install and launch.
                playtime_minutes: None,
                last_played: None,
                size_bytes: None,
                art: Art {
                    cover: Image { path: None, url: covers.get(&key).cloned() },
                    header: image("logo"),
                    hero: image("background"),
                },
                dlc,
                update_available: false,
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
        let command = gogdl(&["auth", "--code", "abc"]);
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
        let all = games(&[1207664663, 1425609977, 1495134320], &products, &HashMap::new(), &HashMap::new());
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
        assert!(games(&[1], &products, &HashMap::new(), &HashMap::new()).is_empty());
        // An id GOG didn't answer for (`null`) is left out.
        assert!(games(&[9], &HashMap::from([(9, Value::Null)]), &HashMap::new(), &HashMap::new()).is_empty());
    }

    #[test]
    fn owned_dlc_go_under_their_game() {
        let mut products = products("products2.json");
        // Cyberpunk 2077 owns two of its DLC; one of them has a cached product with a title.
        products.insert(1445725644, json!({"title": " Phantom Liberty ", "game_type": "dlc", "images": {}, "dlcs": []}));
        let owned = [1423049311, 1445725644, 2116847748];
        let all = games(&owned, &products, &HashMap::new(), &HashMap::new());
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
        let list = || games(&owned, &products, &installs, &HashMap::new()).remove(0);
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
        assert!(!games(&owned, &products, &HashMap::new(), &HashMap::new())[0].installed);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn gamesdb_cover_is_the_portrait_art() {
        let covers = HashMap::from([("gog-1425609977".to_string(), "https://images.gog.com/c.jpg".to_string())]);
        let all = games(&[1425609977], &products("products.json"), &HashMap::new(), &covers);
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
