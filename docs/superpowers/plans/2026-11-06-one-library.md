# One Library Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show every game the owner owns once, across Steam and Epic:
- the full Steam library through a one-time QR login;
- Epic games tied to their Steam twin through GOG GamesDB;
- one card per game, with a launcher switcher on the Play button;
- How long to beat for Epic games;
- store and info links with icons in Details.

**Architecture:**

- **Store copies stay as they are.** Each copy is a `Game`, keyed `steam-<appid>` or `epic-<app_name>`, in `Shared.games` and the `games` table, so launch history, info cache and art cache are untouched.
- **A grouping layer on top.** `entries.rs` groups copies into `Entry`s, using Steam twins cached from GOG GamesDB. The library, filters, featured game and game page all work on entries.
- **New data goes in existing storage, with no schema change.** The Steam owned list, the GamesDB twins and Epic store URLs go in the existing `info` table under new kinds. The refresh token goes in Credential Manager.
- **Steam login.** The QR steps go over HTTPS. The steam-vent CM connection is used only for `GetOwnedGames`: connect, fetch, drop.

**Tech Stack:**
- Tauri 2.12, Svelte 5 + TypeScript 6.
- Rust: reqwest, rusqlite, keyring-core.
- New: `steam-vent` (git, pinned), `steam-vent-proto-steam` 0.5.2, `protobuf` 3.5, `qrcode` 0.14.
- GOG GamesDB and IGDB, both over HTTPS.

**Spec:** `docs/superpowers/specs/2026-11-06-one-library-design.md`. Read it first. The approved look is `docs/superpowers/specs/2026-11-06-one-library-mockup.html`, which follows the app's DESIGN.md tokens.

## Global Constraints

**Platform and dependencies**
- Windows 10/11 only; Tauri 2 + Svelte 5.
- No new crates or npm packages beyond those approved:
  - `steam-vent = { git = "https://codeberg.org/steam-vent/steam-vent", rev = "b0e40109dd7a1b5101e1546ed27f62663dbe5c4b" }` (Codeberg `main` on 2026-11-06), with `steam-vent-proto-steam = "0.5.2"` and `protobuf = "3.5"` (its own protobuf types);
  - `qrcode = { version = "0.14", default-features = false, features = ["svg"] }`.
  - **Base64 for the QR request:** write a small private encoder in `steam_login.rs` (standard alphabet, padded) rather than adding the `base64` crate.

**The Steam refresh token**
- It lives only in Windows Credential Manager (service `dev.noah.gamelauncher`, user `steam-refresh-token`).
- It must never be in SQLite, files, logs, the UI or error strings, and no struct that holds it may derive `Debug`.
- It is sent only to Steam (`api.steampowered.com` IAuthenticationService and the CM logon).
- The Steam device name is `Game Launcher`.

**Data and storage**
- **No database file bump.** This plan adds no table or column. It uses `info` kinds `owned`, `twin` and `epic-url`, so `settings` and `launches` are never at risk.
- **Merge only on an exact, one-to-one appid match** (spec part 3). Never merge on title.

**Network and performance**
- Network work never blocks startup or scrolling. Background jobs run after `load_cache` and are throttled:
  - GamesDB at most 1 request per 500 ms;
  - `GetOwnedGames` at most every 6 h.
- Scrolling stays at today's level: `LAUNCHER_BENCH` 4000 and 15000 px/s give 0 frames slower than 1.5× median, and idle CPU stays near 0.03%.

**UI**
- Missing data hides its tile, row or chip; never show a dash.
- No status text on start. Steam and GamesDB failures are quiet: a dot on the Settings Steam row, no banners.

**Commits and live checks**
- Small, conventional commits with no Co-Authored-By, Claude-Session or "Generated with" lines.
- `cargo clippy --all-targets -- -D warnings`, `cargo test` and `npm run check` pass at every commit.
- At most 10 app launches per task. Live steps that need the owner (QR scan, watching the Steam client) go in the ledger's VERIFY-LATER list.

## Review Focus

1. **The Steam token is rejected or expired, or Steam changes its protocol.** The cached owned list stays, the Settings Steam row gets a dot, there are no banners, and the app doesn't retry logon on every scan. → Task 3 `rejected_token_keeps_owned_and_flags_attention`.
2. **Offline start while Steam is connected.** Startup lists the cached owned games without waiting on the network. → Task 3 `scan_merges_cached_owned_without_network`.
3. **A game title with `"` or `\` sent to IGDB.** The query text is escaped and can't break or widen the query. → Task 9 `epic_title_query_escapes_quotes_and_backslashes`.
4. **A merge disappears** (Epic logs out and its copies vanish, or a twin turns out ambiguous). Every old key still resolves to an entry, and the Steam key keeps its history. → Task 6 `every_copy_key_resolves_after_a_merge_disappears`.
5. **The QR dialog is closed mid-poll, or a poll arrives after cancel.** Nothing is saved and polling reports "not started". → Task 4 `cancel_drops_the_session`.

---

### Task 1: Split `commands.rs` into a module (no behavior change)

**Files:**
- Delete: `src-tauri/src/commands.rs`
- Create:
  - `src-tauri/src/commands/mod.rs`: `AppState`, `Shared`, `EpicWatch`, `Res`, `blocking`, `allow`, `unix_now`, `open_db`.
  - `commands/library.rs`: `library`, `load_cache`, `refresh`, `rescan`, `scan`, `merge`, `epic_result` and the login helpers, `cover`, `featured`, `poll` with its helpers, `bench_*`.
  - `commands/game.rs`: `game`, `detail_art`, `act`, `download_size`, `game_info`, `time_to_beat`, `test_igdb`, `open_folder`, `open_store_page`, `store_url`, `cancel_install`, `record_play`.
  - `commands/settings.rs`: `settings_view`, `update_setting`, `set_igdb_secret`, `clear_cache`, `open_steam`, `open_twitch_console`, `open_igdb`, `epic_log_in`, `epic_log_out`, `dismiss_epic_notice`.
- Modify: `src-tauri/src/main.rs` (handler paths become `commands::library::library` and so on).
- Each test moves with the function it tests.

**Interfaces:**
- Produces: the same commands with the same names and signatures. `pub(super)` helpers in `mod.rs` are shared by the submodules.

- [ ] **Step 1: Move code without editing it.** Fix visibility (`pub(super)`/`pub(crate)`) and `use` lines only.
- [ ] **Step 2: Verify:** `cargo test` (70 passed, 3 ignored, the same counts as before), `cargo clippy --all-targets -- -D warnings`, `npm run check`.
- [ ] **Step 3: Commit:** `refactor(commands): split into library, game and settings modules`.

### Task 2: Steam QR login over HTTPS (`steam_login.rs`)

**Files:**
- Modify: `src-tauri/Cargo.toml`. Add the approved crates from Global Constraints.
- Create: `src-tauri/src/steam_login.rs`.
- Modify: `src-tauri/src/settings.rs`. Add a token entry next to the IGDB secret.
- Modify: `src-tauri/src/main.rs` (`mod steam_login;`).

**Interfaces:**
- Produces, in `steam_login.rs`:
  - `pub struct QrSession { client_id: u64, request_id: Vec<u8>, pub interval: Duration, pub challenge_url: String }`
  - `pub enum PollStep { Waiting, Rotated, Scanned, Done(Approved) }`
  - `pub struct Approved { pub account: String, pub refresh_token: String }`. It has a manual `Debug` that prints `Approved { account, refresh_token: "<redacted>" }`.
  - `pub fn apply_poll(session: &mut QrSession, resp: &CAuthentication_PollAuthSessionStatus_Response) -> PollStep`. It is pure: it applies `new_client_id`, sets `challenge_url` from `new_challenge_url` (returns `Rotated`), returns `Done` when `refresh_token` is non-empty, `Scanned` when `had_remote_interaction`, else `Waiting`.
  - `pub fn eresult_ok(header: Option<&str>) -> Result<(), String>`. `Some("1")` is Ok. Anything else gives `Err("Steam refused the login request (code N).")`, with no body text.
  - `pub fn qr_svg(url: &str) -> Result<String, String>`. It returns an SVG with a quiet zone, dark modules `#000`, light `#fff`, `min_dimensions(240, 240)`.
  - `pub async fn begin(http: &reqwest::Client) -> Result<QrSession, String>`. It posts `BeginAuthSessionViaQR` with `website_id "Client"`, `device_details { device_friendly_name "Game Launcher", platform_type SteamClient }`.
  - `pub async fn poll(http: &reqwest::Client, session: &mut QrSession) -> Result<PollStep, String>`
  - Transport: `POST https://api.steampowered.com/IAuthenticationService/<Method>/v1/`, form field `input_protobuf_encoded` = base64(protobuf). The response body is protobuf; check `x-eresult` first.
- Produces, in `settings.rs`:
  - `pub const STEAM_TOKEN_USER: &str = "steam-refresh-token";`
  - `pub fn steam_token() -> Option<String>`
  - `pub fn set_steam_token(value: Option<&str>) -> Result<(), String>`
  - `pub const STEAM_ACCOUNT: &str = "steam_account";` This is a plain setting holding the account name, which is not a secret.
  - `pub fn validate` accepts `STEAM_ACCOUNT` as non-empty text.

- [ ] **Step 1: Write failing tests** in `steam_login.rs`:

```rust
#[test] fn poll_rotates_challenge_and_switches_client_id() {
    // new_client_id=7 and new_challenge_url="https://s.team/q/1/new" → Rotated; session.client_id == 7; challenge_url updated
}
#[test] fn poll_done_carries_account_and_token() { /* refresh_token "r", account_name "acc" → Done(Approved{account:"acc",..}) */ }
#[test] fn poll_scanned_then_waiting() { /* had_remote_interaction → Scanned; empty response → Waiting */ }
#[test] fn approved_debug_hides_the_token() { assert!(!format!("{:?}", approved).contains("secret-token")); }
#[test] fn eresult_ok_only_for_1() { assert!(eresult_ok(Some("1")).is_ok()); assert!(eresult_ok(Some("84")).unwrap_err().contains("84")); assert!(eresult_ok(None).is_err()); }
#[test] fn qr_svg_is_an_svg() { let s = qr_svg("https://s.team/q/1/abc").unwrap(); assert!(s.starts_with("<?xml") || s.starts_with("<svg")); }
#[test] fn base64_matches_rfc4648_vectors() { /* "" "", "f" "Zg==", "fo" "Zm8=", "foo" "Zm9v", "foobar" "Zm9vYmFy" */ }
```

In `settings.rs`, add `#[ignore = "touches Windows Credential Manager"] steam_token_round_trips`, mirroring the existing IGDB secret test.

- [ ] **Step 2:** `cargo test steam_login` must FAIL because the module and functions don't exist yet.
- [ ] **Step 3: Implement** the interfaces above. `begin` and `poll` are thin wrappers around one private `call<Req, Resp>`.
- [ ] **Step 4:** `cargo test` and `cargo clippy --all-targets -- -D warnings` pass. Confirm that `cargo tree -e normal -i aws-lc-sys` builds on this machine. The spike did, with CMake and NASM present.
- [ ] **Step 5: Commit:** `feat(steam): QR login steps over Steam's web API`.

### Task 3: Owned Steam games (`steam_owned.rs`) merged into the Steam list

**Files:**
- Create: `src-tauri/src/steam_owned.rs`.
- Modify:
  - `src-tauri/src/library/steam.rs`: `with_owned`; make `art` `pub(crate)`.
  - `commands/library.rs`: `scan` merges the cached owned list; a background `refresh_owned`.
  - `commands/mod.rs`: `Shared.steam_attention: bool`.

**Interfaces:**
- Consumes: `settings::steam_token()` (Task 2), `cache::db::{cached_info, put_info}`.
- Produces:
  - `#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)] pub struct OwnedGame { pub appid: u32, pub name: String, pub playtime_minutes: u64 }`
  - `pub const OWNED: &str = "owned";`, `pub const OWNED_KEY: &str = "steam";`, `pub const OWNED_MAX_AGE_SECS: i64 = 6 * 60 * 60;`
  - `pub enum OwnedError { TokenRejected, Network(String) }`. `TokenRejected` covers an expired `RefreshToken`, a logon `LoginError`, and an `AccessDenied`/`InvalidPassword`/`Expired` EResult.
  - `pub async fn fetch(token: &str) -> Result<Vec<OwnedGame>, OwnedError>`: `RefreshToken::new`, `ServerList::discover`, `Connection::login_with_refresh_token`, `GetOwnedGames { steamid, include_appinfo: true, include_played_free_games: true }`, then drop the connection.
  - `pub fn cached(conn: &Connection) -> Option<(Vec<OwnedGame>, i64)>` returns the list and the fetched timestamp, at any age.
  - `pub fn remember(conn: &Connection, games: &[OwnedGame], now_secs: i64)`
  - `pub fn forget(conn: &Connection)`
  - `pub enum OwnedOutcome { Store(Vec<OwnedGame>), KeepAndFlag, KeepQuiet }`
  - `pub fn outcome(result: Result<Vec<OwnedGame>, OwnedError>) -> OwnedOutcome`
  - In `library/steam.rs`: `pub fn with_owned(installed: Vec<Game>, owned: &[OwnedGame], librarycache: &Path) -> Vec<Game>`.
    - Owned appids missing from `installed` become `installed: false`, `install_dir: None`, `size_bytes: None`, with art from `art(appid, librarycache/<appid>)` and title from `name`.
    - `playtime_minutes` is set from `owned` for every matching appid, installed or not.
  - In `commands/library.rs`: `pub(crate) fn refresh_owned(app: AppHandle, force: bool)`.
    - It spawns a background job that does nothing without a token, or when the cache is younger than `OWNED_MAX_AGE_SECS` (unless forced by `force: bool`).
    - On `Store` it remembers the list, clears `steam_attention` and calls `rescan(app, false)`.
    - On `KeepAndFlag` it sets `steam_attention = true` and never retries until the next connect or app start.
    - On `KeepQuiet` it does nothing.
    - It is called at the end of `load_cache` and of `refresh`.
    - `refresh` gains a parameter: `refresh(app, epic: bool, owned: bool)`, with TS `api.refresh(epic, owned)`.
      - The toolbar Refresh button calls `refresh(true, true)`, which is `refresh_owned(app, force = true)`: a fresh Steam list every time the owner clicks (owner decision, 2026-11-06).
      - Startup and the 60 s timer pass `owned: false`, so the 6 h rule applies.
  - **Background full refresh** (owner decision, 2026-11-06), done in `App.svelte`:
    - **Triggers:**
      - every 6 h while the app is open;
      - when the window becomes visible or focused again (`visibilitychange` / window `focus`) after at least 1 h since the last full refresh.
    - **What it calls:** `api.refresh(true, true)`, with the same quiet behaviour as startup: no status text, failures only as the Settings dot.
    - **Never two at once:** reuse the existing `refreshing` flag. A trigger while a refresh runs is dropped.
    - `refreshedAt` already records the last full refresh, so use it for the 1 h check.
    - Pure helper in `App.svelte`'s module script: `export function dueForRefresh(lastMs: number | null, nowMs: number, minGapMs: number): boolean`. `null` means due.
    - Constants: `FULL_REFRESH_EVERY_MS = 6 * 60 * 60 * 1000` and `REFRESH_ON_RETURN_AFTER_MS = 60 * 60 * 1000`.
- `Library` gains `steam_attention: bool`, which the toolbar's settings dot ORs with `epic_attention`.

- [ ] **Step 1: Write failing tests:**

```rust
// library/steam.rs
#[test] fn with_owned_adds_uninstalled_games_and_keeps_installed() {
    // installed: appid 400 installed; owned: 400 "Portal" 51, 70 "Half-Life" 0
    // → 2 games; 70 has installed=false, install_dir None, title "Half-Life", art.cover.url ends "/70/library_600x900.jpg"
    // → 400 keeps installed=true and gets playtime_minutes Some(51)
}
// steam_owned.rs
#[test] fn owned_round_trips_through_the_info_cache() { /* remember then cached == Some((games, now)) ; forget → None */ }
#[test] fn rejected_token_keeps_owned_and_flags_attention() { assert!(matches!(outcome(Err(OwnedError::TokenRejected)), OwnedOutcome::KeepAndFlag)); assert!(matches!(outcome(Err(OwnedError::Network("x".into()))), OwnedOutcome::KeepQuiet)); }
// commands/library.rs
#[test] fn scan_merges_cached_owned_without_network() { /* scan's merge helper takes (installed, cached owned) → contains the uninstalled appid; no async/network involved */ }
```

- [ ] **Step 2:** `cargo test` FAILS on the new tests.
- [ ] **Step 3: Implement.** In `scan`, after Steam's `list_games`, apply `with_owned` with `steam_owned::cached` (a missing cache means installed only).
- [ ] **Step 4:** `cargo test` and clippy pass.
- [ ] **Step 5: Token renewal (find out, then decide).**
  - Try `Authentication.GenerateAccessTokenForApp` (`refresh_token`, `steamid`, `renewal_type = Allow`) over the same HTTPS transport as Task 2.
  - **If Steam answers with a new refresh token:** `refresh_owned` renews when `RefreshToken::remaining_lifetime()` is under 30 days and saves the new token with `set_steam_token`. Test the decision as `renews_only_when_under_30_days`.
  - **If it isn't accepted over HTTPS:** skip renewal and record that in the ledger. The owner reconnects after about 200 days.
- [ ] **Step 6: Commit:** `feat(steam): full owned library from GetOwnedGames, cached`.

### Task 4: Connect Steam in Settings

**Files:**
- Modify: `commands/settings.rs` (connect commands, `SettingsView`), `commands/mod.rs` (`AppState.steam_qr`), `main.rs` (register), `src/lib/api.ts`, `src/lib/Settings.svelte`.
- Create: `src/lib/SteamConnect.svelte` (the QR dialog).

**Interfaces:**
- Consumes: `steam_login::{begin, poll, qr_svg, PollStep}`, `settings::{set_steam_token, STEAM_ACCOUNT}`, `refresh_owned` (Task 3), `steam_owned::forget`.
- Produces (Rust):
  - `AppState.steam_qr: tokio::sync::Mutex<Option<QrSession>>`
  - `#[tauri::command] steam_connect_start(app) -> Res<String>` returns the SVG and replaces any running session.
  - `#[tauri::command] steam_connect_poll(app) -> Res<ConnectStep>`
    - `#[derive(Serialize)] #[serde(tag = "step", rename_all = "camelCase")] pub enum ConnectStep { Waiting, Rotated { svg: String }, Scanned, Connected { account: String } }`
    - With no session: `Err("Not connecting.")`.
    - On `Done`: save the token, save `STEAM_ACCOUNT`, clear the session and `steam_attention`, then `refresh_owned(app, force = true)`.
  - `#[tauri::command] steam_connect_cancel(app)` sets the session to `None`.
  - `#[tauri::command] steam_disconnect(app) -> Res<()>`: `set_steam_token(None)`, delete `STEAM_ACCOUNT`, `steam_owned::forget`, then `rescan(app, false)`.
  - `SettingsView.steam` becomes `Option<SteamLocal { name: Option<String>, path: String }>`, plus a new `steam_library: SteamLibrary { account: Option<String>, attention: bool }`.
- Produces (TS):
  - `steamConnectStart(): Promise<string>`, `steamConnectPoll(): Promise<ConnectStep>`, `steamConnectCancel()`, `steamDisconnect()`.
  - `type ConnectStep = {step:'waiting'} | {step:'rotated'; svg:string} | {step:'scanned'} | {step:'connected'; account:string}`.
- **UI** (spec part 1):
  - The Steam row's first line reads "● Found · <persona> · <path>" (renamed from "Connected").
  - The second line is "Library: [Connect]" or "Library: <account> · [Disconnect]", with a dot when `attention`.
  - `SteamConnect.svelte` is a modal like the install popup, with the `alive` flag pattern and Escape or the close button to cancel.
    - It shows the SVG (inserted with `{@html}`; it is generated in Rust from a URL, never user text) and the line "Open the Steam app → Steam Guard → scan this code."
    - After `scanned` it shows "Approve the login in the Steam app".
    - It polls every 5 s and stops on close or unmount by calling cancel.

- [ ] **Step 1: Write failing tests:**

```rust
#[test] fn cancel_drops_the_session() { /* put a QrSession in the slot, run the cancel helper, assert slot is None; poll helper on empty slot → Err("Not connecting.") */ }
#[test] fn connect_step_serializes_with_a_step_tag() { assert_eq!(serde_json::to_string(&ConnectStep::Connected{account:"a".into()}).unwrap(), r#"{"step":"connected","account":"a"}"#); }
```

- [ ] **Step 2:** `cargo test` FAILS.
- [ ] **Step 3: Implement** the Rust commands, then the UI.
- [ ] **Step 4:** `cargo test`, clippy and `npm run check` pass.
  - **One app launch:** Settings shows "Found" and "Library: Connect", and Connect opens a QR. Capture it with PrintWindow; don't scan.
  - Add "owner scans QR; desktop Steam stays signed in; ~679 games appear" to VERIFY-LATER.
- [ ] **Step 5: Commit:** `feat(settings): connect a Steam account with a QR code`.

### Task 5: Steam twins from GOG GamesDB (`gamesdb.rs`)

**Files:**
- Create: `src-tauri/src/gamesdb.rs`, `src-tauri/tests/fixtures/gamesdb/pumpkin.json`.
  - Capture the fixture with one `curl https://gamesdb.gog.com/platforms/epic/external_releases/Pumpkin`, trimmed to the fields used.
- Modify: `commands/mod.rs` (`Shared.twins: HashMap<String, u32>`), `commands/library.rs` (load twins in `load_cache`; background `fill_twins`).

**Interfaces:**
- Produces:
  - `pub const TWIN: &str = "twin";`, `pub const TWIN_MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;`, `pub const TWIN_GAP: Duration = Duration::from_millis(500);`
  - `pub fn parse_steam_twin(json: &str) -> Result<Option<u32>, String>` returns `Some` only when `game.releases` has **exactly one** entry with `platform_id == "steam"` and a numeric `external_id`.
  - `pub async fn look_up(http: &reqwest::Client, app_name: &str) -> Result<Option<u32>, String>`
    - The `app_name` is percent-encoded with `library::percent_encode`.
    - A 404 gives `Ok(None)`. Another non-2xx gives `Err`.
  - `pub fn cached(conn: &Connection, epic_key: &str, now_secs: i64) -> Option<Option<u32>>` stores `{"steam":635320}` or `{"steam":null}`.
  - `pub fn remember(conn: &Connection, epic_key: &str, twin: Option<u32>, now_secs: i64)`
  - `pub fn all_cached(conn: &Connection) -> HashMap<String, u32>` returns fresh non-null twins only.
  - `pub(crate) fn fill_twins(app: AppHandle)` (in `commands/library.rs`):
    - For each Epic game without a fresh cache entry, it calls `look_up` and sleeps `TWIN_GAP` between calls.
    - It remembers `Ok` answers and skips `Err` without caching it.
    - It inserts into `Shared.twins` and calls `regroup` (Task 6).
    - Only one runs at a time, through an `AtomicBool`.

- [ ] **Step 1: Write failing tests:**

```rust
#[test] fn pumpkin_twin_is_last_day_of_june() { assert_eq!(parse_steam_twin(&fixture("pumpkin.json")).unwrap(), Some(635320)); }
#[test] fn no_steam_release_means_no_twin() { /* releases with only gog/epic → None */ }
#[test] fn two_steam_releases_mean_no_twin() { /* two steam rows → None */ }
#[test] fn twin_cache_keeps_misses_and_expires() { /* remember None → cached Some(None); after 30 days → None; all_cached skips nulls */ }
```

- [ ] **Step 2:** `cargo test` FAILS.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** `cargo test` and clippy pass.
- [ ] **Step 5: Commit:** `feat(gamesdb): find each Epic game's Steam twin, cached 30 days`.

### Task 6: Entries: one card per game (`entries.rs`)

**Files:**
- Create: `src-tauri/src/entries.rs`.
- Modify:
  - `src-tauri/src/view.rs`: `arrange` and `featured` take entries.
  - `commands/mod.rs`: `Shared.entries`, `regroup`, `AppState::entry`.
  - `commands/library.rs`: `library`, `featured`, `cover`; `Card.stores`.
  - `src/lib/api.ts`, `src/lib/Card.svelte`: show every store logo.

**Interfaces:**
- Consumes: `Shared.games`, `Shared.twins` (Task 5), launches (already merged into `Game.last_played` by `view::merge_last_played`).
- Produces:
  - `#[derive(Clone, Debug, PartialEq)] pub struct Entry { pub key: String, pub copies: Vec<usize> }`
    - `copies` are indices into `games`, with the Steam copy first.
    - `key` is the Steam copy's key when merged, else the only copy's key.
  - `pub fn group(games: &[Game], twins: &HashMap<String, u32>) -> Vec<Entry>`. An Epic game merges with `steam-<appid>` only when that Steam game exists in `games` and no other Epic game in `games` has the same twin.
  - `pub fn find<'a>(entries: &'a [Entry], games: &[Game], key: &str) -> Option<&'a Entry>` matches the entry key or any copy key.
  - `pub fn default_copy(entry: &Entry, games: &[Game]) -> usize`:
    - the only installed copy;
    - else the copy with the latest `last_played`;
    - else the first (Steam).
  - `pub fn playtime(entry: &Entry, games: &[Game]) -> (Option<u64>, bool)` returns the sum of known minutes, and whether more than one copy contributed.
  - `pub fn last_played(entry: &Entry, games: &[Game]) -> Option<(SystemTime, ProviderKind)>`
  - `pub fn installed(entry: &Entry, games: &[Game]) -> bool`
  - `pub fn stores(entry: &Entry, games: &[Game]) -> Vec<ProviderKind>`
  - `pub fn steam_appid(entry: &Entry, games: &[Game], twins: &HashMap<String, u32>) -> Option<u32>` takes the Steam copy's id, else the Epic copy's twin.
  - `view::arrange(games: &[Game], entries: &[Entry], query: &str, store: Option<ProviderKind>, installed_only: bool, sort: Sort) -> Vec<usize>` returns entry indices.
    - The store filter matches any copy, and installed means any copy.
    - Sort uses `entries::playtime` and `last_played`.
    - Title comes from the first copy.
  - `view::featured(games, entries) -> Option<&Entry>` keeps today's rule (installed and played), over entries.
  - `commands::regroup(s: &mut Shared)` sets `s.entries = entries::group(&s.games, &s.twins)`. It is called wherever `s.games` or `s.twins` change.
  - `AppState::entry(&self, key: &str) -> Option<(Entry, Vec<Game>)>` returns the entry plus clones of its copies in copy order.
  - `Card { key, title, stores: Vec<ProviderKind>, installed }` replaces `provider`. The cover comes from the first copy. TS: `Card.stores: Store[]`.

- [ ] **Step 1: Write failing tests** in `entries.rs`:

```rust
#[test] fn merges_epic_with_owned_steam_twin() { /* steam-635320 + epic-Pumpkin, twins {epic-Pumpkin: 635320} → 1 entry, key "steam-635320", copies [steam, epic] */ }
#[test] fn no_merge_when_two_epic_games_share_a_twin() { /* epic-A and epic-B both → 635320 → 3 entries */ }
#[test] fn no_merge_without_the_steam_game() { /* twin 635320 but no steam-635320 in games → separate */ }
#[test] fn same_title_without_twin_stays_separate() { /* identical titles, empty twins → 2 entries */ }
#[test] fn default_copy_prefers_installed_then_last_played_then_steam() { /* three cases */ }
#[test] fn playtime_sums_and_flags_both_stores() { /* steam 120 + epic Some(30) → (Some(150), true); epic None → (Some(120), false) */ }
#[test] fn every_copy_key_resolves_after_a_merge_disappears() { /* group with twin, then without: find("epic-Pumpkin") and find("steam-635320") both Some each time */ }
// view.rs
#[test] fn store_filter_matches_any_copy() { /* merged entry listed under Steam and under Epic */ }
```

- [ ] **Step 2:** `cargo test` FAILS.
- [ ] **Step 3: Implement.** Update the existing `arrange` and `featured` tests to build entries with `entries::group(&games, &HashMap::new())`.
- [ ] **Step 4:** `cargo test`, clippy and `npm run check` pass. The card shows one logo per store in the existing top-right glass circle style, stacked with a 4px gap.
- [ ] **Step 5: Commit:** `feat(library): one entry per game across Steam and Epic`.

### Task 7: Game page data for entries

**Files:**
- Modify: `commands/game.rs` (`game` returns `Detail` for an entry), `src-tauri/src/view.rs` (tile suffix helpers), `src/lib/api.ts`.

**Interfaces:**
- Consumes: `AppState::entry`, `entries::{default_copy, playtime, last_played, installed, stores}`.
- Produces:
  - `Detail` becomes:

```rust
pub struct Detail {
    key: String,                 // entry key
    title: String,
    stores: Vec<ProviderKind>,
    installed: bool,             // any copy
    copies: Vec<CopyView>,
    chosen: String,              // copy key from entries::default_copy
    playtime_tile: Option<String>,
    playtime_note: Option<String>,     // "Steam + Epic" when both contribute
    last_played_tile: Option<String>,
    last_played_store: Option<String>, // "on Steam" / "on Epic Games", only when the entry has 2+ copies
}
pub struct CopyView { key: String, provider: ProviderKind, installed: bool, size: Option<String>, install_dir: Option<String> }
```

  - `view::playtime_tile` keeps its signature, called with the summed minutes and the provider of the copy that has known playtime: Steam when any Steam copy exists, else the default copy.
  - `act`, `download_size`, `open_folder`, `cancel_install` and `detail_art` keep taking a **copy key**. The page passes `chosen` or the switched copy's key.
  - `detail_art` for an entry key uses the first copy's art. `AppState::entry` resolves either key.
  - TS gets the `Detail` and `CopyView` interfaces to match.

- [ ] **Step 1: Write failing tests:**

```rust
#[test] fn detail_for_a_merged_entry() { /* build Shared with steam(installed, 120 min) + epic(not installed); detail("epic-Pumpkin") → key "steam-635320", chosen "steam-635320", copies.len()==2, playtime_note None (epic unknown) */ }
#[test] fn last_played_store_only_for_merged_entries() { /* single copy → None; merged, epic played last → Some("on Epic Games") */ }
```

Factor `game`'s body into `fn detail(entry: &Entry, games: &[Game], now: SystemTime) -> Detail` so it can be tested without Tauri.

- [ ] **Step 2:** `cargo test` FAILS.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** `cargo test`, clippy and `npm run check` pass. `Detail.svelte` compiles against the new fields (temporary: use `copies` of `chosen` where it used `provider`, `installed`, `size` and `installDir`).
- [ ] **Step 5: Commit:** `feat(game-page): entry details with copies and the default launcher`.

### Task 8: Play button with launcher switcher

**Files:**
- Create: `src/lib/PlayButton.svelte`.
- Modify: `src/lib/Detail.svelte` (use it; meta line logos; tile suffixes; ⋯ menu acts on the chosen copy and drops "Open … store page"), `DESIGN.md` (split pill, "Play with" menu).

**Interfaces:**
- Consumes: `Detail.copies`, `Detail.chosen` (Task 7), the existing `api.act` and `api.downloadSize`, `StoreIcon`.
- Produces:
  - `PlayButton.svelte` props: `{ copies: CopyView[]; chosen: string; running: boolean; busy: string | null; onchoose: (key: string) => void; onplay: () => void; oninstall: () => void }`.
    - With 1 copy it renders today's plain pill: Play, Install or Playing.
    - With 2 or more copies it renders the split pill and the menu.
  - **Look** (mockup v4):
    - The split pill is 44px high, `#fff`, with a dark label.
    - The left segment holds the 18px store logo and a 14px chevron, with `aria-haspopup="menu"`, `aria-expanded`, and `aria-label="Choose launcher, now <Store>"`.
    - A 1px divider at `rgb(14 14 20 / 0.18)` sits inset 10px.
    - The right segment is Play (play icon) or Install (download icon).
  - **Menu** (mockup v4):
    - A "Play with" label, then one `menuitemradio` per copy: logo, name, and a sub-line "Installed · <size>" or "Not installed".
    - A not-installed Epic copy shows "Not installed · <size> download" from `downloadSize`.
    - A check mark marks the chosen copy.
    - Keyboard is the same as the ⋯ menu: arrows, Home/End, Escape returns focus to the switcher, Tab closes, clicking outside closes.
  - **Detail.svelte:**
    - Keeps `chosenKey` state, reset to `detail.chosen` when the entry key changes. It is not stored.
    - The meta line shows a `StoreIcon` per store, then "Installed on <Store of chosen copy, if installed>", else "Installed on <first installed copy's store>" or "Not installed".
    - The playtime tile head shows `playtimeNote` at the right in `--text-2` 12px. The last-played value gets ` · <lastPlayedStore>`.

- [ ] **Step 1:** `npm run check` stays green as each piece lands. There is no Svelte unit test harness in this repo. Behaviour is checked live in Step 3.
- [ ] **Step 2: Implement** `PlayButton.svelte` and wire it into `Detail.svelte`. Update DESIGN.md.
- [ ] **Step 3: Verify.**
  - `npm run check`, `cargo test`, clippy.
  - **One app launch:** open a merged game (Last Day of June, once Tasks 3–5 have data, or any merged entry) and capture it with PrintWindow next to the mockup.
  - Add "switcher keyboard + click-outside, Install path on not-installed copy" to VERIFY-LATER.
- [ ] **Step 4: Commit:** `feat(game-page): split Play button to choose the launcher`.

### Task 9: How long to beat for Epic games

**Files:**
- Modify: `src-tauri/src/igdb.rs`, `commands/game.rs` (`look_up_time_to_beat`, `time_to_beat`), `src-tauri/tests/fixtures/igdb/epic_title.json`.
  - The fixture holds the owner's live answer: two rows with `game: 36543`, `name: "Last Day of June"`, uids `3f9b7d87d6ea4ac1bb26d49683bd7064` and `0792ef92-1e18-48d3-8e97-89f0e7d1a70a`. Add `url` fields as IGDB returns them (`https://store.epicgames.com/en-US/p/last-day-of-june`).

**Interfaces:**
- Consumes: `entries::steam_appid` (Task 6), `AppState::entry`.
- Produces:
  - Remove `can_match`. Add `const EPIC_SOURCE: u32 = 26;`.
  - `pub async fn game_id_for_steam(http, token, client_id, appid: u32) -> Result<Option<u64>, String>` (today's `game_id` for source 1).
  - `pub fn epic_title_query(title: &str) -> String` builds `fields game,url; where external_game_source = 26 & name = "<escaped>"; limit 10;`, escaping `\` as `\\` and `"` as `\"`.
  - `pub struct EpicMatch { pub game: u64, pub url: Option<String> }`
  - `pub fn parse_epic_title_match(body: &str) -> Result<Option<EpicMatch>, String>`
    - `Some` only when there is at least one row and every row has the same `game`.
    - `url` is the first row's `url` that starts with `https://store.epicgames.com/`.
  - `pub async fn epic_by_title(http, token, client_id, title: &str) -> Result<Option<EpicMatch>, String>`
  - `pub const EPIC_URL: &str = "epic-url";` is an `info` kind holding the URL string, keyed by the Epic copy key. It has no expiry and is overwritten when re-learned.
  - `pub fn remember_epic_url(conn: &Connection, epic_key: &str, url: &str)` and `pub fn epic_url(conn: &Connection, epic_key: &str) -> Option<String>`, both in `igdb.rs`.
  - `look_up_time_to_beat(state, client_id, secret, appid: Option<u32>, title: &str, epic_key: Option<&str>)`:
    1. If `appid`, try `game_id_for_steam`.
    2. If that gives no id and the entry has an Epic copy, use `epic_by_title(title)`. When it returns a `url`, remember it with `remember_epic_url` for `epic_key`.
    3. Fetch `time_to_beat(id)`.
    - The cache key stays the entry key.

- [ ] **Step 1: Write failing tests:**

```rust
#[test] fn epic_title_match_accepts_rows_of_one_game() { let m = parse_epic_title_match(&fixture("epic_title.json")).unwrap().unwrap(); assert_eq!(m.game, 36543); assert_eq!(m.url.as_deref(), Some("https://store.epicgames.com/en-US/p/last-day-of-june")); }
#[test] fn epic_title_match_rejects_mixed_games_and_empty() { /* rows game 1 and 2 → None; [] → None */ }
#[test] fn epic_title_query_escapes_quotes_and_backslashes() { assert_eq!(epic_title_query(r#"Say "Hi" \o/"#), r#"fields game,url; where external_game_source = 26 & name = "Say \"Hi\" \\o/"; limit 10;"#); }
#[test] fn epic_match_ignores_non_store_urls() { /* url "https://example.com/x" → url None, game kept */ }
```

- [ ] **Step 2:** `cargo test` FAILS.
- [ ] **Step 3: Implement.** `time_to_beat` resolves the entry, computes `steam_appid`, and calls the chain. It no longer returns `None` just because the copy is from Epic.
- [ ] **Step 4:** `cargo test` and clippy pass. Add "Epic-only game shows How long to beat" to VERIFY-LATER (needs IGDB credentials).
- [ ] **Step 5: Commit:** `feat(igdb): how long to beat for Epic games via Steam twin or exact title`.

### Task 10: Store and info links in Details

**Files:**
- Create: `src-tauri/src/links.rs`, `src/lib/LinkIcon.svelte`.
- Modify: `commands/game.rs` (add `Detail.links`, command `open_link`; remove `open_store_page` and `store_url`, moving their test into `links.rs`), `main.rs`, `src/lib/api.ts`, `src/lib/Detail.svelte` (two rows in Details), `DESIGN.md` (link chips).
- Icons: `src/lib/icons/{steam,epicgames,pcgamingwiki,steamdb,protondb}.svg` are already in the repo.

**Interfaces:**
- Consumes: `entries::{stores, steam_appid}`, `igdb::epic_url` (Task 9).
- Produces:
  - `#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)] #[serde(rename_all = "camelCase")] pub enum LinkKind { SteamStore, EpicStore, PcGamingWiki, SteamDb, ProtonDb }`
  - `pub fn kinds(stores: &[ProviderKind], steam_appid: Option<u32>) -> Vec<LinkKind>` returns the store kinds in store order, then `[PcGamingWiki, SteamDb, ProtonDb]` when `steam_appid` is `Some`.
  - `pub fn url(kind: LinkKind, steam_appid: Option<u32>, epic_url: Option<&str>, title: &str) -> Option<String>`:
    - SteamStore: `https://store.steampowered.com/app/<appid>`
    - EpicStore: `epic_url` if it starts with `https://store.epicgames.com/`, else `https://store.epicgames.com/browse?q=<percent_encode(title)>`
    - PcGamingWiki: `https://www.pcgamingwiki.com/api/appid.php?appid=<appid>`
    - SteamDb: `https://steamdb.info/app/<appid>/`
    - ProtonDb: `https://www.protondb.com/app/<appid>`
    - It returns `None` when a required appid is missing.
  - `#[tauri::command] open_link(app, key: String, kind: LinkKind) -> Res<()>` resolves the entry, builds the URL in Rust, and opens it with `library::open_uri`. The frontend never sends a URL.
  - `Detail.links: Vec<LinkKind>`. TS: `type LinkKind = 'steamStore' | 'epicStore' | 'pcGamingWiki' | 'steamDb' | 'protonDb'`.
- **UI** (mockup v4):
  - Details gets two last rows:
    - "Store pages": the store chips.
    - "Info": the PCGamingWiki, SteamDB and ProtonDB chips. The row is hidden when it has none.
  - Each chip is an `<a>`-styled button: 28px high, 10px side padding, 13px text, a 14px icon with a 6px gap, `--glass-hover` background, `--glass-line` border, and `--glass-on` on hover.
  - `LinkIcon.svelte` reads the single `d` path from each SVG with `?raw`, the way `StoreIcon.svelte` does.

- [ ] **Step 1: Write failing tests:**

```rust
#[test] fn kinds_for_merged_entry_with_appid() { assert_eq!(kinds(&[Steam, Epic], Some(635320)), vec![SteamStore, EpicStore, PcGamingWiki, SteamDb, ProtonDb]); }
#[test] fn epic_only_without_appid_has_just_its_store() { assert_eq!(kinds(&[Epic], None), vec![EpicStore]); }
#[test] fn urls_have_the_exact_shapes() { /* each kind with 635320 → the exact strings above */ }
#[test] fn epic_store_url_falls_back_to_search_and_rejects_other_hosts() { /* None → browse?q=Last%20Day%20of%20June ; "https://evil.example/p/x" → search */ }
```

- [ ] **Step 2:** `cargo test` FAILS.
- [ ] **Step 3: Implement** the Rust side, then the UI.
- [ ] **Step 4:** `cargo test`, clippy and `npm run check` pass. **One app launch:** capture Details for a merged game with PrintWindow and compare it with the mockup.
- [ ] **Step 5: Commit:** `feat(game-page): store and info links with icons`.

### Task 11: Final verification

**Files:**
- Modify: the ledger only. Fix anything found in its own commit.

- [ ] **Step 1: Checks.**
  - `cargo test` (all pass), `cargo clippy --all-targets -- -D warnings`, `npm run check` (0 errors, 0 warnings).
  - `npm run tauri build` (or the release build used before).
- [ ] **Step 2: Benchmark** with the full library. Steam connected means about 1,300 entries instead of 613.
  - `scripts/bench.ps1 4000` and `scripts/bench.ps1 15000`: expect 0 frames slower than 1.5× median.
  - The idle script: expect around 0.03% CPU. Record the private MB next to the old numbers (582 and 542 MB under the bench, 214 MB idle).
  - If a number regresses, A/B against `774b952` before blaming the code. Background apps caused a false alarm last time.
- [ ] **Step 3: Offline check.** Set `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` to `http://127.0.0.1:9` for the app process only. The cached library, including owned Steam games and merges, appears, and there are no banners.
- [ ] **Step 4: Ledger.** List VERIFY-LATER items for the owner:
  - QR connect, and Steam staying signed in;
  - the switcher's keyboard behaviour and Install path;
  - Epic How long to beat;
  - removing the "Game Launcher (test)" device.
  - Also record the total number of launches.
- [ ] **Step 5: Commit** the ledger, if it is tracked; otherwise just report.

---

## Addendum tasks (owner feedback, 2026-11-07)

These implement the spec's "Addendum, 2026-11-07". Global Constraints still apply. The tasks run after the final-review fix wave. Task 11's live checks are re-run at the end as Task 18.

### Task 12: Twins accept a duplicated Steam row

**Files:**
- Modify: `src-tauri/src/gamesdb.rs`.
- Fixture: `src-tauri/tests/fixtures/gamesdb/condor.json`. Capture it once with `curl https://gamesdb.gog.com/platforms/epic/external_releases/Condor` and trim it to the fields used. It has the Steam rows `202750` and `steam_202750`.

**Interfaces:**
- `parse_steam_twin(json) -> Result<Option<u32>, String>` collects the **distinct** appids from `platform_id == "steam"` releases.
  - It accepts an `external_id` that is all digits, or `steam_<digits>`.
  - It returns `Some` only when exactly one distinct appid remains.
- `TWIN` becomes `"twin2"`, so answers cached under the old rule, including the 30-day misses, are looked up again.

- [ ] **Step 1: Write failing tests.**
  - `duplicated_steam_row_is_one_twin`: the condor fixture gives `Some(202750)`.
  - `two_different_steam_apps_are_no_twin`.
  - The existing tests stay green.
- [ ] **Step 2: Implement.** Then run `cargo test` and clippy.
- [ ] **Step 3: Commit:** `fix(gamesdb): a Steam app listed twice is still one twin`.

### Task 13: Links open in the browser

**Files:**
- Modify: `src-tauri/src/library/mod.rs`, plus the callers of https openers.

**Interfaces:**
- Add `pub fn open_url(url: &str) -> Result<(), Error>` for `https://` URLs. It runs `rundll32.exe url.dll,FileProtocolHandler <url>` with `CREATE_NO_WINDOW`. No cmd.exe is involved, so `&` and `?` are safe.
  - It refuses anything that doesn't start with `https://`.
  - `open_link`, the Twitch console, IGDB and every other https opener use it.
  - `open_uri` stays for `steam://` and `com.epicgames.launcher://`.
- Test: `open_url_refuses_non_https`.
- VERIFY-LATER: each link chip opens the browser.

- [ ] **Step 1: Write the failing test, implement, then run `cargo test` and clippy.**
- [ ] **Step 2: Commit:** `fix(links): open web links in the browser, not Explorer`.

### Task 14: Fuller metadata for Epic games

**Files:**
- Modify: `src-tauri/src/info.rs`, `src-tauri/src/gamesdb.rs`, `commands/game.rs` (`game_info`).
- Fixture: `src-tauri/tests/fixtures/gamesdb/pumpkin.json`. Extend it with the GamesDB game fields used, from a fresh one-time capture.

**Interfaces:**
- `pub fn parse_gamesdb_info(json: &str) -> Option<GameInfo>` reads these fields:
  - `game.summary` (English, through `strip_html`);
  - `game.genres[].name` (English);
  - the first of `game.developers[].name` and of `game.publishers[].name`;
  - `game.first_release_date`;
  - screenshot URLs. These are built from GamesDB's URL template; read the real fixture to find the formatter placeholder, and pick a size the existing art downloader can fetch.
- `pub fn fill_missing(base: GameInfo, extra: GameInfo) -> GameInfo` keeps each non-empty field of `base` and fills empty ones from `extra`.
- `game_info` for an entry with an Epic copy:
  - Start from Legendary's info.
  - Fill it from the Steam twin's appdetails when `entries::steam_appid` is known. Reuse the Steam path and its cache, keyed `steam-<appid>`.
  - Otherwise fill it from GamesDB's game data. Cache that as kind `details-gamesdb`: 30 days when found, 7 days for misses.
- Merged entries keep using the Steam copy's details first.
- Tests: `gamesdb_info_reads_summary_genres_and_screenshots` and `fill_missing_keeps_base_and_fills_gaps`.

- [ ] **Step 1: Write the failing tests, implement, then run the full suite and clippy.**
- [ ] **Step 2: Commit:** `feat(info): fill Epic metadata from the Steam twin or GOG GamesDB`.

### Task 15: Steam login window and Settings wording

**Files:**
- Modify: `commands/settings.rs`, `src/lib/Settings.svelte`, `src/lib/api.ts`.
- Replace `src/lib/SteamConnect.svelte` with `src/lib/SteamLogin.svelte`.

**Interfaces:**
- `steam_password_login(app, account: String, password: String) -> Res<LoginStep>`.
  - `#[serde(tag="step", rename_all="camelCase")] enum LoginStep { Approve, Code { kind: String }, Connected { account: String }, Failed { message: String } }`.
  - It uses `steam_vent::Connection::login` with:
    - a no-op `GuardDataStore` that stores nothing;
    - a confirmation handler that reports Approve or Code to the UI, then waits for `steam_password_code(app, code: String) -> Res<LoginStep>` or `steam_login_cancel`;
    - `ClientInfo` named "Game Launcher".
  - On success it saves `connection.refresh_token()` through the same `save_login` as the QR flow, then calls `refresh_owned(app, true)`.
  - The password and code are never stored, logged, returned, put in an error string, or held in a Debug-derived struct.
  - Cancel follows the QR generation rule: a login that completes after cancel is not saved.
  - Steam's error kinds map to short messages: wrong password, rate limited, code mismatch.
- UI:
  - **Steam row:** built like the Epic row. It has a dot, status text ("Logged in as <account>" or "Not logged in"), and a **Log in** / **Log out** button; Log out is the existing disconnect. The local "Found · persona · path" stays as a detail line.
  - **`SteamLogin.svelte`:** a modal in the install popup's look.
    - On the left: account name and password fields (type=password, autocomplete off) and a Sign in button, followed by the Approve or code state.
    - On the right: the existing QR flow (start, poll every 5 s, rotate).
    - Whichever finishes first wins. Closing the modal cancels both.
- Wording:
  - Remove the "Kept in Windows Credential Manager…" hint.
  - The IGDB section heading becomes "Extra metadata". Its intro says IGDB fills how long to beat and extra details for Steam and Epic games.
- Tests:
  - `login_step_serializes_with_a_step_tag`;
  - an approved-after-cancel password login is not saved (reuse `should_save`);
  - the no-op guard store stores nothing.

- [ ] **Step 1: Write the failing tests, implement, then run the full suite, clippy and `npm run check`.**
- [ ] **Step 2: VERIFY-LATER:** password login with Steam Guard approve and with a code, and that the QR flow still works.
- [ ] **Step 3: Commit:** `feat(settings): Steam log in window with password or QR, Epic-style row`.

### Task 16: Steam download size

**Files:**
- Create: `src-tauri/src/steam_size.rs`.
- Modify: `commands/game.rs` (`download_size`), `src/lib/PlayButton.svelte`.
- Fixture: `src-tauri/tests/fixtures/steam/appinfo-202750.vdf`. Capture it once with an anonymous steam-vent PICS request.

**Interfaces:**
- `pub fn parse_download_size(appinfo_vdf: &str) -> Option<u64>` sums `depots.<id>.manifests.public.download`, falling back to `.size`. It counts only depots that meet all of these:
  - `config.oslist` is absent or contains `windows`;
  - no `dlcappid`;
  - no `config.language`, or the language is English.
- `pub async fn fetch_appinfo(appid: u32) -> Result<String, String>`:
  - It opens an anonymous steam-vent connection (`Connection::anonymous`), sends `CMsgClientPICSProductInfoRequest` for the app, returns the text VDF, then drops the connection.
  - No login or token is involved.
- `download_size(key)` for a not-installed Steam copy uses it. The result is cached 7 days like Epic's size; failures are not cached.
  - The Play-with menu's Steam sub-line becomes "Not installed · <size> download".
  - A single-copy Steam game shows the size beside Install, as Epic does.
- Tests: `download_size_sums_windows_depots` and `download_size_skips_dlc_and_other_os_depots`.

- [ ] **Step 1: Write the failing tests, implement, then run the full suite and clippy.**
- [ ] **Step 2: Commit:** `feat(steam): download size for games not installed`.

### Task 17: Mouse back and forward

**Files:**
- Modify: `src/App.svelte`.

**Interfaces:**
- A window `mouseup` / `auxclick` handler, which calls preventDefault so the webview doesn't navigate on its own:
  - **Button 3 (Back):** while a game page is open, closes it the way "← Library" does, with the same focus return.
  - **Button 4 (Forward):** reopens the last game page closed this session, if that game still exists.
- A pure helper in the module script decides the step: `export function navStep(button: number, open: string | null, last: string | null): { open: string | null; last: string | null } | null`.

- [ ] **Step 1: Implement, then run `npm run check`.**
- [ ] **Step 2: VERIFY-LATER:** mouse back and forward.
- [ ] **Step 3: Commit:** `feat(app): mouse back and forward buttons navigate`.

### Task 18: Re-run the live checks

Repeat Task 11 Steps 2–4: release build, bench 4000 and 15000, idle, and the offline check.
- Steam is now connected, so the bench runs with the full library.
- If idle regresses, A/B against `c520547`.
- Record the results in the ledger.

### Task 19: Smoother loading (no pop-in)

Owner request, 2026-11-07. Today:
- the window opens on an empty library and the grid then jumps in;
- covers fill in one by one on blank cards;
- the game page's info, how long to beat and size arrive separately and push content down.

**Files:**
- Modify: `src-tauri/tauri.conf.json` (window `visible: false`) and `src-tauri/src/commands/library.rs` (a `ready` command that shows the window).
- Modify: `src-tauri/src/cache/mod.rs` (store each thumbnail's average colour next to it).
- Modify: `src/App.svelte`, `src/lib/Card.svelte`, `src/lib/Detail.svelte`.

**Interfaces:**
- **Window shown when ready.**
  - The window starts hidden.
  - `App.svelte` calls `api.ready()` after `loadCache()`, the first `reload()`, and the covers for the first screen (the cards visible at start) have resolved.
  - A 300 ms cap applies. Whatever is still missing at that point fills in after the window shows.
  - `ready` is idempotent and also fires from Rust after 2 s as a safety net, so the app can never stay invisible.
- **Cover colour placeholder.**
  - `cache::thumbnail` also writes the average RGB of the thumbnail, either in a `<key>.color` file or in the info table, the builder's choice.
  - `cover(key)` returns `{ src, color }`.
  - `Card.svelte` paints `color` immediately, then fades the image in over 150 ms.
  - Cards with no colour yet use `--surface-2`.
- **Cover order.** Cover requests are issued in on-screen order, top to bottom and left to right, not in order of arrival.
- **Game page.**
  - About and Details reserve their space with skeleton lines in DESIGN.md tokens that fill in place. Nothing pushes content down when data arrives.
  - `gameInfo`, `timeToBeat` and `downloadSize` are prefetched when the pointer has rested on a card for 150 ms, or a card is focused. Results are kept in a small in-memory map (last 20 keys), so opening the page usually shows them at once.
- **Motion.** All fades respect `prefers-reduced-motion` (no fade).

**Tests:**
- Rust: `thumbnail_writes_average_color` (a solid-colour fixture image gives that colour).
- Rust: `ready_is_idempotent`.
- Frontend: `npm run check`.

- [ ] **Step 1:** Write the failing Rust tests, implement, then run the full suite, clippy and `npm run check`.
- [ ] **Step 2:** Benchmark 4000 and 15000. Both must stay at 0 slow frames, and idle must stay at today's level.
- [ ] **Step 3:** VERIFY-LATER: the start shows no empty flash, covers fade in on their colour, and the game page doesn't jump.
- [ ] **Step 4: Commit:** `feat(ui): smoother loading without pop-in`.

### Task 20: Only games in the Epic library

Owner request, 2026-11-07. Today `legendary list --json` gives every catalog item the account owns, and the launcher lists all of them. In the owner's library of 656 items, 547 are games. The rest are:
- 64 Unreal Engine assets, plugins and engines (for example "Async Loading Screen" several times, "City Sample", "ARK ModKit (UE4)");
- 36 DLC add-ons (for example "Train Sim World 5: Sand Patch Grade");
- 4 apps (Spotify, Brave, Discord, itch.io);
- 2 hidden content items (Fortnite content);
- 2 dedicated servers;
- 1 bonus pack.

**Files:**
- Modify: `src-tauri/src/library/epic.rs` (the parser behind `list --json`) and a fixture under `src-tauri/tests/fixtures/legendary/`.

**Interfaces:**
- `fn is_game(item: &Value) -> bool` returns true when `metadata.categories[].path` contains `"games"`. The epic list parser keeps only those items.
- Installed items always stay, even without the `games` category, so nothing the owner installed disappears.
- The scan replaces the Epic rows in SQLite, so the filtered-out items disappear on the next Epic refresh. Their launch history and info rows stay harmless.

**Tests:**
- `list_keeps_only_games`: a fixture with one game, one UE asset (`plugins`/`asset-format`), one addon, one `engines` item and one software app gives only the game.
- `installed_non_game_is_kept`.

- [ ] **Step 1:** Write the failing tests (trim real entries from `~/.config/legendary/metadata/` for the fixture), implement, then run the full suite, clippy and `npm run check`.
- [ ] **Step 2: Commit:** `fix(epic): list only games, not engine assets, DLC or apps`.

### Task 20b: DLC under its base game (amends Task 20)

Owner decision, 2026-11-07: DLC must be handled, not just hidden. Task 20 drops DLC from the card list. This task attaches each DLC to its base game.

**Files:**
- Modify: `library/epic.rs`, `library/mod.rs` (`Game.dlc`), `library/steam.rs`, `commands/game.rs`, `src/lib/Detail.svelte`, `src/lib/InstallPopup.svelte`.

**Interfaces:**
- **Epic.**
  - A Legendary item whose categories contain `addons` is a DLC. Its base is `metadata.mainGameItem.id` (catalog id), matched to the base game's catalog id.
  - `Game { dlc: Vec<Dlc> }` with `Dlc { id: String, title: String, installed: bool }`.
  - Install and uninstall go through Legendary: `legendary install <base> --dlcs-only` / `--skip-dlcs`, or the DLC's own app_name, depending on what the Legendary version supports (check `legendary install --help`).
  - The install popup gets a "Include DLC" checkbox, on by default.
- **Steam.**
  - List the owned DLC: GetOwnedGames doesn't return DLC; use PICS app info `extended.listofdlc` intersected with the account's licenses, or the store appdetails `dlc` list filtered to owned. Pick the one that works anonymously or with the existing token, and record which.
  - Steam DLC is shown only. Steam installs DLC itself.
- **Game page.** A "DLC" section under Details lists each DLC with its installed state. For Epic it adds Install / Uninstall per DLC. It is hidden when the game has no DLC.

**Tests:**
- `addon_attaches_to_its_base_game` (fixture from real Legendary metadata).
- `dlc_without_base_in_library_is_hidden`.

- [ ] **Step 1:** Write the failing tests, implement, then run the full suite, clippy and `npm run check`.
- [ ] **Step 2:** VERIFY-LATER: install a game with and without DLC; install and uninstall one DLC.
- [ ] **Step 3: Commit:** `feat(dlc): show and manage DLC under its game`.

## Feature parity tasks (owner, 2026-11-07)

These tasks bring over Heroic features the owner chose: updates, cloud saves, install management, a download queue, and launch options.

**Linux later:** the owner may want Linux support one day. New OS-specific code (process spawning flags, opening URLs and folders, registry and paths) goes behind small functions in one place (`src-tauri/src/platform.rs`, created by the first task that needs it), with the Windows implementation behind `#[cfg(windows)]`. No Windows-only calls are scattered through feature code. Don't build Linux support now.

### Task 21: Epic game updates

**Files:** `library/epic.rs`, `commands/game.rs` and `commands/library.rs`, `src/lib/Card.svelte`, `src/lib/Detail.svelte`, `src/lib/PlayButton.svelte`.

**Interfaces:**
- **Finding updates.** After each Epic refresh, `legendary list-installed --check-updates --json` (or the equivalent in the installed Legendary version) marks installed games that have an update. The result goes in `Game.update_available: bool`.
- **Game page.**
  - The Play button becomes **Update** when an update is available.
  - Update runs `legendary update <app>` with the same progress, cancel and popup flow as Install.
  - The ⋯ menu offers "Play without updating".
- **Card.** A small "Update" chip like the existing "Install" chip.
- Steam is left alone: Steam updates games itself.

**Tests:** `parses_update_available_from_list_installed` (fixture).

- [ ] **Steps:** Write the failing test, implement, run the full suite, clippy and `npm run check`. VERIFY-LATER: update a game with a pending update. **Commit:** `feat(epic): find and install game updates`.

### Task 22: Epic cloud saves

**Files:** `library/epic.rs`, `commands/game.rs`, settings (per-game toggle and a global default), `src/lib/Detail.svelte`, `src/lib/Settings.svelte`.

**Interfaces:**
- **When it syncs.** For Epic games with cloud saves (the catalog's `customAttributes.CloudSaveFolder` is present):
  - `legendary sync-saves <app> --skip-upload` runs before launch;
  - `legendary sync-saves <app> --skip-download` runs after the game exits. The existing Epic process watch knows when it exits.
- **Conflicts.** When Legendary reports one, show "Cloud save is newer / local save is newer" with Keep cloud / Keep local, and never pick silently.
- **Setting.** "Sync cloud saves", on by default, plus a per-game override in the ⋯ menu.
- **Failure.** A sync failure shows a quiet note on the game page and never blocks launching.

**Tests:**
- `cloud_save_folder_detected` (fixture);
- `sync_args_before_and_after_launch`.

- [ ] **Steps:** Write the failing tests, implement, run the full suite, clippy and `npm run check`. VERIFY-LATER: a save made in a game appears after a reinstall or on another PC. **Commit:** `feat(epic): sync cloud saves around play`.

### Task 23: Manage installs (verify, move, import)

**Files:** `library/epic.rs`, `commands/game.rs`, `src/lib/Detail.svelte` (⋯ menu), `src/lib/Settings.svelte` (Import).

**Interfaces:**
- **Verify / repair.**
  - Epic: `legendary verify <app>`, then `legendary repair <app>` if files are bad, with progress like Install.
  - Steam: `steam://validate/<appid>`.
- **Move** (Epic): pick a folder, then run `legendary move <app> <dest>` with progress. Steam: open Steam's storage manager (`steam://open/storage`).
- **Import** (Epic):
  - "Import existing install": pick the game folder, then `legendary import <app> <path>`.
  - Settings also offers "Sync with Epic Games Launcher" (`legendary egl-sync`), so games installed by EGL show up.
- The ⋯ menu lists Verify, Move and Uninstall for the chosen copy.

**Tests:** argument builders for verify, move and import (pure); `import_requires_existing_folder`.

- [ ] **Steps:** Write the failing tests, implement, run the full suite, clippy and `npm run check`. VERIFY-LATER: verify one game, move one game, import one folder. **Commit:** `feat(install): verify, move and import games`.

### Task 24: Download queue

**Files:** `commands/game.rs` / a new `src-tauri/src/downloads.rs`, `src/App.svelte` (a small downloads indicator), `src/lib/Downloads.svelte`.

**Interfaces:**
- **What the queue does.** Epic installs, updates and repairs go through one queue:
  - one active Legendary job at a time;
  - the rest wait in order;
  - the owner can reorder, pause (stop the Legendary process; Legendary resumes partial downloads), resume and cancel.
- **Where it shows.** A toolbar indicator shows the active job's progress. Clicking it opens a Downloads panel with the active job and the queue.
- **Steam.** Steam jobs aren't queued (Steam has its own), but they are shown in the panel as "in Steam" with their progress.
- **Restarts.** The queue survives an app restart: queued jobs are stored as a `settings` value (JSON list of `{key, action}`) and resumed on start.

**Tests:** queue order, pause and resume, cancel, and restore from the stored list (pure queue model).

- [ ] **Steps:** Write the failing tests, implement, run the full suite, clippy and `npm run check`. VERIFY-LATER: queue two installs, pause and resume, restart the app mid-queue. **Commit:** `feat(downloads): queue Epic installs with pause and resume`.

### Task 25: Launch options per game

**Files:** `library/epic.rs`, `library/steam.rs`, `commands/game.rs`, `src/lib/Detail.svelte` (a "Launch options" dialog from the ⋯ menu).

**Interfaces:**
- **Per-game options** (stored in `settings` as JSON keyed by the copy key):
  - extra arguments;
  - offline mode (Epic: `--offline`);
  - "skip version check" (Epic);
  - an alternative executable (Epic: `--override-exe`).
- **Steam.** Steam keeps its own launch options. The dialog shows a short note and a button that opens the game's Steam properties (`steam://gameproperties/<appid>`), because Steam has no URI that accepts arguments for every game.
- **Validation.** Arguments are passed as separate arguments, never through a shell. The alternative executable must exist inside the install folder.

**Tests:** argument building from options; `override_exe_must_be_inside_install_dir`.

- [ ] **Steps:** Write the failing tests, implement, run the full suite, clippy and `npm run check`. VERIFY-LATER: launch with extra arguments and in offline mode. **Commit:** `feat(launch): per-game launch options`.

### Task 26: Epic playtime

Owner request, 2026-11-07. Steam games have playtime and Epic games don't. Playtime is needed for sorting and grouping (Task 27).

**Files:** `src-tauri/src/library/epic.rs`, a new `src-tauri/src/epic_playtime.rs`, `src-tauri/src/commands/library.rs`, `cache/db.rs` (only if a table is unavoidable; prefer the `info` table or the existing `launches`), and `platform.rs` for process timing if it exists by then.

**Interfaces:**
- **Epic's playtime service (try first).**
  - `GET https://library-service.live.use1a.on.epicgames.com/library/api/public/playtime/account/<accountId>/all` with the Epic access token. This is the service the Epic Games Launcher uses. The endpoint is UNVERIFIED: verify it once with the owner's session before relying on it.
  - The token and account id come from Legendary's own session (`legendary` keeps them in its `user.json`). Read them in Rust only for this call and never log or store them anywhere else. If the token is expired, let Legendary refresh it (for example by running `legendary status`) and read it again.
  - Parse the result into `HashMap<String /*artifactId / app_name*/, u64 /*minutes*/>`. Cache it in the `info` table (kind `epic-playtime`) and refresh it with each Epic refresh.
- **Own tracking (always on, fallback and supplement).**
  - When an Epic game started from the launcher exits (the existing Epic process watch), add the session length to a per-game total kept in the `info` table, kind `own-playtime`.
- **Combining.** `Game.playtime_minutes` for Epic is the service value when it exists, otherwise the own total, otherwise `None`. Merged entries keep summing copies (Task 6).

**Tests:**
- `parses_epic_playtime_response` (fixture shaped after one real, trimmed response);
- `own_playtime_adds_session_length`;
- `epic_playtime_prefers_service_then_own`.

- [ ] **Step 1:** Write the failing tests, implement, then run the full suite, clippy and `npm run check`.
- [ ] **Step 2:** VERIFY-LATER: Epic games show playtime in the tile and the list.
- [ ] **Step 3: Commit:** `feat(epic): playtime from Epic's service and own tracking`.

### Task 27: Group, sort and filter by playtime and how long to beat

Owner request, 2026-11-07. Approved look: mockup v7 in the brainstorm companion folder (`.superpowers/brainstorm/6468-1794010770/content/layout-v7.html`). Copy it to `docs/superpowers/specs/` when this task starts.

**Files:** `src-tauri/src/view.rs`, `src-tauri/src/commands/library.rs`, `src/lib/Toolbar.svelte`, `src/lib/Grid.svelte`, `src/lib/Card.svelte`, `src/lib/api.ts`.

**Interfaces:**
- **Sort** gains `mostPlayed`, `longestToBeat` and `shortestToBeat`, next to the existing title, last played and playtime options.
  - Entries without the value go last, with ties broken by title.
  - How long to beat uses the cached IGDB `normally` value (in hours). Sorting never triggers network lookups.
- **Group** is a new toolbar dropdown: None, Store, Your playtime, How long to beat. Genre comes later, once genres are cached for every game.
  - Your playtime buckets, in this order: `500 h+`, `100–500 h`, `50–100 h`, `20–50 h`, `10–20 h`, `0–10 h`, `Never played`.
  - How long to beat buckets: `Short · under 5 h`, `Medium · 5–15 h`, `Long · 15–40 h`, `Very long · 40–100 h`, `Endless · 100 h+`, and `Unknown` last.
  - Empty buckets are hidden.
  - `view::group(entries, games, by) -> Vec<(String, Vec<usize>)>`.
- **The grid with groups.** Each group gets a section label in the existing `.label` style ("10–20 h 12"). Virtualisation still works: groups are flattened into rows with label rows. The benchmark must stay at 0 slow frames.
- **Filters** (the Filters chip popover from the v5/v7 direction, or plain chips if that popover isn't built yet): playtime ranges (Never played, Under 10 h, 10–50 h, 50 h+) and how-long-to-beat ranges (Under 5 h, 5–15 h, 15–40 h, 40–100 h, 100 h+).
- **Card stat badge.** While grouping or sorting by playtime or how long to beat, each card shows a small bottom-left badge ("38 h played" / "~25 h to beat"). The Install and Update chips win over it when both apply.
- **Persistence.** The chosen group and sort are kept in settings across restarts.

**Tests:**
- `playtime_buckets_edges` (0, 9, 10, 499 and 500 h land in the right bucket);
- `hltb_buckets_edges`;
- `group_hides_empty_buckets`;
- `sort_longest_to_beat_puts_unknown_last`.

- [ ] **Step 1:** Write the failing tests, implement, then run the full suite, clippy and `npm run check`.
- [ ] **Step 2:** Benchmark 4000 and 15000 with grouping on: 0 slow frames.
- [ ] **Step 3:** VERIFY-LATER: each grouping and sort works on the full library.
- [ ] **Step 4: Commit:** `feat(library): group and sort by playtime and how long to beat`.

### Task 28: Library toolbar and game page tabs (layout v5/v7)

Owner request, 2026-11-07. The owner approved the layout direction in two mockups:
- `docs/superpowers/specs/2026-11-07-layout-mockup-v5.html`
- `docs/superpowers/specs/2026-11-07-grouping-mockup.html` (v7)

The direction stays in DESIGN.md's "Afterglow": quiet dark glass, chips, colour only from the art, and the white pill as the one bright thing. There is no Home page and no sidebar. The library stays the start page, with the Continue-playing banner unchanged.

**Order:** run this task before Tasks 20b–25 where possible, so their UI lands in the tabs and the toolbar. If some of those tasks already landed, move their UI into the tabs here.

**Files:** `src/lib/Toolbar.svelte`, `src/App.svelte`, `src/lib/Grid.svelte`, `src/lib/Detail.svelte`, and a new `src/lib/FilterPopover.svelte`, `src/lib/LibraryList.svelte` and `src/lib/DownloadsPopover.svelte` (or Task 24's panel), plus `DESIGN.md`.

**Interfaces and look (match the mockups; DESIGN.md tokens win on values):**
- **Toolbar, left to right:**
  - the search field (36px, glass) with a `Ctrl K` hint;
  - the quick chips All / Steam / Epic / Installed, as today;
  - the active extra filters as glass-on chips with ✕;
  - a **Filters** chip;
  - a spacer;
  - Group (from Task 27) and Sort;
  - a list/grid toggle chip;
  - a **Downloads** chip;
  - the Settings gear with its attention dot, as today.
- **Filters chip:** opens a glass popover (`surface-2`, radius 12, the same shadow as the ⋯ menu) with groups of toggle chips:
  - Status: Not installed, Update available, Downloading;
  - Your playtime and How long to beat ranges (from Task 27);
  - Has: DLC, Cloud saves, On both stores;
  - Genre (once genres are cached).
  - Each chip turned on there also appears in the toolbar as an active chip with ✕.
  - The popover footer has **Clear**.
  - Collections and "Save as collection" are left for later; leave the footer slot for them.
  - The popover closes on Escape and on a click outside, and keeps keyboard focus inside while open.
- **List view:** a dense table with Name, Stores (logos), Size, Playtime, How long to beat, Last played and Status. Rows open the game page.
  - It must stay virtualised for about 1,300 rows.
  - The choice of list or grid is kept in settings.
- **Downloads chip:**
  - It shows **only while something is downloading, updating or queued**, as a 36px icon chip with a progress ring (accent-2) around the icon.
  - Clicking it opens a glass popover: the active job with its progress, size and time left, Pause and Cancel; then "Up next" with reorder and remove; then Steam downloads, read-only, as "in Steam".
  - It uses Task 24's queue model. If Task 24 hasn't landed yet, it shows the existing per-game progress only.
- **Game page tabs:** under the hero, a tab row: **Overview · DLC · Saves · Installation · Launch options**.
  - Overview is exactly today's page: tiles, About and Details.
  - DLC, Saves, Installation and Launch options hold the UI from Tasks 20b, 22, 23 and 25. A tab whose feature doesn't exist yet, or doesn't apply to this game (no DLC, or no cloud saves for a Steam-only game), is hidden.
  - The tab row uses the label style. The active tab gets a 2px `text` underline.
  - Keyboard: arrow keys move between tabs (roving tabindex).
  - The ⋯ menu keeps only quick actions: Open folder, Uninstall.
- **Settings** stays a modal with its section list. "Accounts" comes first, with one card per source: Steam and Epic, each with status, the account name and Log in / Log out. Other sections stay as they are.
- **Mouse Back** (Task 17) closes the game page, as before.

**Tests:**
- `npm run check`;
- a pure helper for filter state ↔ toolbar chips (`filterChips(state) -> Chip[]`, removing one chip clears exactly that filter);
- the Rust side is unchanged, apart from any sort or group fields from Task 27.

- [ ] **Step 1:** Implement, then run `npm run check`, `cargo test` and clippy.
- [ ] **Step 2:** Benchmark 4000 and 15000 in grid view, and in list view with 1,300 entries: 0 slow frames.
- [ ] **Step 3:** VERIFY-LATER: the Filters popover, active chips, list view, the Downloads chip appearing only during downloads, the tabs and their keyboard behaviour.
- [ ] **Step 4:** Update DESIGN.md: the toolbar, filter popover, downloads chip, list view and game page tabs.
- [ ] **Step 5: Commit:** `feat(ui): library toolbar with filters, list view, downloads chip and game page tabs`.
