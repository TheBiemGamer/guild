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
