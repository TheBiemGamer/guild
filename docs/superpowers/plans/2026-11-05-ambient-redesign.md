# Ambient Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle the launcher in the approved "Ambient" look and add store logos, installed marking, a settings panel, quiet Epic status, and a richer game page (size, store details, IGDB how long to beat).

**Architecture:** Rust (`src-tauri/src`) keeps all data, network and disk work behind Tauri commands; new modules `settings.rs`, `info.rs` and `igdb.rs` sit next to `commands.rs`, with SQLite (schema v3) caching launches, settings and fetched info. The Svelte frontend (`src/`) is split into focused components: shell (`Ambient`, `Toolbar`, `Continue`), grid (`Card`, `InstallPopup`), `Detail`, and `Settings`.

**Tech Stack:** Tauri 2.12, Svelte 5 + TypeScript 6, Vite 8, Rust (reqwest, rusqlite bundled, serde_json, steamlocate, keyvalues-parser, keyring 4.2 for the IGDB secret), Legendary 0.21 CLI.

**Spec:** `docs/superpowers/specs/2026-11-05-ambient-redesign-design.md` (read it first; mockups in `.superpowers/brainstorm/26-1793914734/content/`: `ambient-v3.html`, `installed-marking-v2.html` option C, `hover-install.html` option B, `game-page.html`, `settings-v2.html`).

## Global Constraints

- Windows 10/11 only; Tauri 2 + Svelte 5; no new Rust crates or npm packages without the owner's approval. Approved: `keyring = { version = "4.2", default-features = false, features = ["windows-native-keyring-store"] }`, for the IGDB Client Secret only.
- The IGDB Client Secret lives only in Windows Credential Manager (service `dev.noah.gamelauncher`, user `igdb-client-secret`): never in SQLite, files, logs, or the UI (UI shows "Saved ✓" with Replace and Remove).
- The UI never reads disk or decodes images for display in Rust; local files reach the webview only through `allow(&app, path)` (asset protocol, per file). CSP stays `img-src 'self' asset: http://asset.localhost`, so every remote image is downloaded first (`cache::resolve`).
- Scrolling must stay at the current level: `LAUNCHER_BENCH` 4000 and 15000 px/s over 613 games: 0 frames slower than 1.5× median.
- No status text on every start; Epic refresh is silent. No gradient text, no colored glow halos.
- Store logos: Simple Icons 16.36.0 `steam.svg` and `epicgames.svg`, bundled as files, white on a dark glass circle, cover top-right.
- Installed marking (option C): not-installed covers `filter: saturate(.5) brightness(.75)` plus an "Install" chip bottom-left; installed covers full color.
- Game page tiles: Playtime · Last played · How long to beat (IGDB "normally" big, hastily/completely small). Size only in Details (and next to Install for not-installed Epic games).
- Settings Epic row never shows the Legendary version; that goes under About.
- Missing data hides its tile or row; never show a dash.
- Commits: small, conventional, no Co-Authored-By/Claude-Session/"Generated with" lines. `cargo clippy --all-targets -- -D warnings`, `cargo test`, `npm run check` pass at every commit.

## Review Focus

1. **Offline start:** store details, IGDB and download-size lookups fail quietly (tile/row hidden, no error banner), and cached Epic games stay listed. → test in Task 3 (`epic_result_keeps_cache_when_not_logged_in`) and Task 7 (`cached_info_is_used_when_fetch_fails`).
2. **Wrong or expired IGDB credentials:** "Test" in Settings shows the error text; game pages just hide the tile; the app does not request a new token for every game after a failure. → test in Task 8 (`token_is_reused_until_expiry` and `failed_token_backs_off`).
3. **Last-played game has no hero art** (or nothing was ever played): ambient falls back to the neutral gradient and the Continue banner hides. → test in Task 2 (`featured_ignores_uninstalled_and_unplayed`).
4. **Steam store rate limit (HTTP 429) while opening many game pages:** results already cached are served; a failed fetch is not cached as "no data" for long. → test in Task 7 (`failed_fetch_is_not_cached`).
5. **Download size lookup fails or is slow** in the Epic install popup: popup shows "Size unknown", Install still works. → test in Task 6 (`download_size_from_info_handles_missing_fields`).

---

### Task 1: SQLite schema v3: size, launches, settings, info cache

**Files:**
- Modify: `src-tauri/src/cache/db.rs`, `src-tauri/src/cache/mod.rs` (`Dirs::db` → `library-v3.sqlite`), `src-tauri/src/library/mod.rs` (`Game.size_bytes`)
- Modify: every `Game { … }` literal (`view.rs` test helper, `db.rs` tests, `steam.rs::to_game`, `epic.rs::game`) to set `size_bytes: None` for now

**Interfaces:**
- Produces: `Game.size_bytes: Option<u64>`; in `cache::db`:
  `record_launch(conn: &Connection, key: &str, at_secs: i64) -> Result<(), Error>`,
  `launches(conn: &Connection) -> Result<HashMap<String, i64>, Error>` (latest per key),
  `setting(conn: &Connection, name: &str) -> Result<Option<String>, Error>`,
  `set_setting(conn: &Connection, name: &str, value: Option<&str>) -> Result<(), Error>` (`None` deletes),
  `cached_info(conn: &Connection, key: &str, kind: &str, max_age_secs: i64, now_secs: i64) -> Result<Option<String>, Error>`,
  `put_info(conn: &Connection, key: &str, kind: &str, json: &str, now_secs: i64) -> Result<(), Error>`.
- Schema: `games` gains `size_bytes INTEGER`; new tables `launches(key TEXT PRIMARY KEY, at INTEGER NOT NULL)`, `settings(name TEXT PRIMARY KEY, value TEXT NOT NULL)`, `info(key TEXT, kind TEXT, json TEXT NOT NULL, fetched INTEGER NOT NULL, PRIMARY KEY(key, kind))`. Bump the file name (schema shortcut comment says so); update that comment to `library-v4.sqlite`.

- [ ] **Step 1: Write the failing tests** in `db.rs` tests:

```rust
#[test]
fn launches_keep_the_latest_time() {
    let conn = memory();
    record_launch(&conn, "epic-Fortnite", 1_000).unwrap();
    record_launch(&conn, "epic-Fortnite", 2_000).unwrap();
    record_launch(&conn, "epic-Fortnite", 1_500).unwrap();
    assert_eq!(launches(&conn).unwrap().get("epic-Fortnite"), Some(&2_000));
}

#[test]
fn settings_set_read_and_delete() {
    let conn = memory();
    set_setting(&conn, "igdb_client_id", Some("abc")).unwrap();
    assert_eq!(setting(&conn, "igdb_client_id").unwrap().as_deref(), Some("abc"));
    set_setting(&conn, "igdb_client_id", None).unwrap();
    assert_eq!(setting(&conn, "igdb_client_id").unwrap(), None);
}

#[test]
fn cached_info_respects_max_age() {
    let conn = memory();
    put_info(&conn, "steam-1", "details", r#"{"a":1}"#, 100).unwrap();
    assert_eq!(cached_info(&conn, "steam-1", "details", 50, 120).unwrap().as_deref(), Some(r#"{"a":1}"#));
    assert_eq!(cached_info(&conn, "steam-1", "details", 50, 200).unwrap(), None);
    assert_eq!(cached_info(&conn, "steam-1", "ttb", 50, 120).unwrap(), None);
}
```

Also extend the existing `round_trips_and_replaces_per_provider` fixture game with `size_bytes: Some(27_300_000_000)`. `memory()` opens `Connection::open_in_memory()` and runs `SCHEMA`.

- [ ] **Step 2:** `cd src-tauri && cargo test db::` → FAIL (functions not defined).
- [ ] **Step 3:** Implement the schema and the six functions; `record_launch` uses `INSERT … ON CONFLICT(key) DO UPDATE SET at = max(at, excluded.at)`.
- [ ] **Step 4:** `cargo test` and `cargo clippy --all-targets -- -D warnings` → PASS.
- [ ] **Step 5:** Commit `feat(cache): schema v3 with sizes, launches, settings and info cache`.

### Task 2: Sizes, last played across stores, featured game

**Files:**
- Modify: `src-tauri/src/library/steam.rs` (`to_game`: `size_bytes = app.size_on_disk` when installed), `src-tauri/src/library/epic.rs` (`game()`: `install_size` from `list-installed`), `src-tauri/src/view.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/main.rs`

**Interfaces:**
- Consumes: Task 1 `record_launch`, `launches`.
- Produces: `view::size_label(bytes: u64) -> String` (`"27.3 GB"`, 1 decimal, GB = 1024³; under 1 GB → `"812 MB"`);
  `view::merge_last_played(games: &mut [Game], launches: &HashMap<String, i64>)` (keeps the later of the two);
  `view::featured(games: &[Game]) -> Option<&Game>` (installed game with the latest `last_played`; `None` if none);
  command `featured(app) -> Option<Featured>` with `Featured { key: String, title: String, hero: Option<String> }` (hero = `allow`ed local path, cover art as fallback, `None` if neither);
  `act(Play)` on success calls `record_launch(key, now)` and updates that game's `last_played` in `Shared`; `rescan` and `load_cache` apply `merge_last_played` before storing games.

- [ ] **Step 1: Failing tests** in `view.rs`:

```rust
#[test]
fn size_labels() {
    assert_eq!(size_label(812 * 1024 * 1024), "812 MB");
    assert_eq!(size_label(27_300 * 1024 * 1024 * 1024 / 1000), "27.3 GB");
}

#[test]
fn launches_override_older_last_played() {
    let mut games = vec![game("a", true, None, Some(100)), game("b", true, None, None)];
    let launches = HashMap::from([("steam-a".to_string(), 50), ("steam-b".to_string(), 300)]);
    merge_last_played(&mut games, &launches);
    assert_eq!(games[0].last_played, Some(UNIX_EPOCH + Duration::from_secs(100)));
    assert_eq!(games[1].last_played, Some(UNIX_EPOCH + Duration::from_secs(300)));
}

#[test]
fn featured_ignores_uninstalled_and_unplayed() {
    let games = vec![game("old", true, None, Some(10)), game("gone", false, None, Some(99)), game("never", true, None, None)];
    assert_eq!(featured(&games).map(|g| g.title.as_str()), Some("old"));
    assert!(featured(&[game("never", true, None, None)]).is_none());
}
```

Steam fixture test in `steam.rs`: `assert_eq!(games()[0].size_bytes, Some(16_521_526));` (fixture `SizeOnDisk`) and `None` for the not-installed game 20. Epic test in `epic.rs`: add `"install_size": 1234` to the Fortnite entry of `tests/fixtures/legendary/list-installed.json` and assert `fortnite.size_bytes == Some(1234)`.
- [ ] **Step 2:** `cargo test` → FAIL.
- [ ] **Step 3:** Implement; register `commands::featured` in `main.rs`.
- [ ] **Step 4:** `cargo test`, clippy → PASS.
- [ ] **Step 5:** Commit `feat: game sizes, last played across stores, featured game`.

### Task 3: Quiet Epic status that keeps the library

**Files:** Modify `src-tauri/src/commands.rs`, `src-tauri/src/library/epic.rs`, `src-tauri/src/main.rs`

**Interfaces:**
- Consumes: Task 1 `setting`/`set_setting` (key `"epic_last_login"`, values `"loggedIn"|"loggedOut"|"noLegendary"|"unreachable"`).
- Produces: `fn epic_result(login: Login, list: impl FnOnce() -> Result<Vec<Game>, String>) -> Result<Vec<Game>, String>` (only `LoggedIn` lists; every other state returns `Err` so `merge` keeps cached Epic games);
  `fn logged_out_since_last_run(previous: Option<&str>, now: Login) -> bool` (true only for `Some("loggedIn")` → `LoggedOut`);
  `Library` gains `epic_attention: bool` (Epic state known and not `LoggedIn`) and `epic_notice: bool` (set by the scan when `logged_out_since_last_run`, cleared by command `dismiss_epic_notice()`);
  `epic::log_out() -> Result<(), Error>` (`legendary auth --delete`) and command `epic_log_out(app) -> Res<()>` (then rescans Epic).

- [ ] **Step 1: Failing tests** in `commands.rs`:

```rust
#[test]
fn epic_result_keeps_cache_when_not_logged_in() {
    for login in [Login::LoggedOut, Login::NoLegendary, Login::Unreachable] {
        assert!(epic_result(login, || panic!("must not list")).is_err());
    }
    assert_eq!(epic_result(Login::LoggedIn, || Ok(vec![])).unwrap().len(), 0);
}

#[test]
fn logged_out_notice_only_on_change() {
    assert!(logged_out_since_last_run(Some("loggedIn"), Login::LoggedOut));
    assert!(!logged_out_since_last_run(Some("loggedOut"), Login::LoggedOut));
    assert!(!logged_out_since_last_run(None, Login::LoggedOut));
    assert!(!logged_out_since_last_run(Some("loggedIn"), Login::Unreachable));
}
```
- [ ] **Step 2:** `cargo test` → FAIL.
- [ ] **Step 3:** Implement; `scan` uses `epic_result`; after an Epic scan store the new state with `set_setting`; register new commands.
- [ ] **Step 4:** `cargo test`, clippy → PASS.
- [ ] **Step 5:** Commit `fix(epic): keep cached Epic games when Legendary isn't logged in; quiet status flags`.

### Task 4: Ambient shell, logos and cards

**Files:**
- Create: `src/lib/icons/steam.svg`, `src/lib/icons/epicgames.svg` (copy from `https://cdn.jsdelivr.net/npm/simple-icons@16.36.0/icons/<name>.svg`), `src/lib/StoreIcon.svelte`, `src/lib/Ambient.svelte`, `src/lib/Toolbar.svelte`, `src/lib/Continue.svelte`
- Modify: `src/App.svelte`, `src/lib/Card.svelte`, `src/lib/Grid.svelte`, `src/lib/api.ts`, `src/app.css`

**Interfaces:**
- Consumes: Task 2 `featured`; Task 3 `Library.epicAttention`, `Library.epicNotice`, `dismiss_epic_notice`; `Card` needs `provider` → add `provider: ProviderKind` to the Rust `Card` struct and `api.ts` `Card`.
- Produces: `api.featured(): Promise<{ key; title; hero: string | null } | null>` (hero already converted with `convertFileSrc`); components: `<Ambient hero={string | null} enabled={boolean} />`, `<Toolbar … onsettings={() => void} attention={boolean} />`, `<Continue featured={…} onplay onopen />`, `<StoreIcon provider={'steam'|'epic'} size={number} />`; `Card` props gain `provider`, `onplay(key)`, `oninstall(key, anchor: DOMRect)`.

Behavior (from the spec, mockup `ambient-v3.html`):
- `Ambient`: fixed full-window layer; hero image blurred (≈70px) and saturated, ≈0.6 opacity, under a vertical darkening gradient; neutral dark gradient when `hero` is null or `enabled` is false. Render it once as a static layer (no animation) so scrolling cost doesn't change.
- `Toolbar`: search, chips **All / Steam logo / Epic logo / Installed**, sort select, ⚙ button with a small dot when `attention`.
- No "Loading Epic games…" text any more; on a first run with an empty library show the existing status text in the empty state only.
- `epicNotice` shows one dismissible banner: "You were logged out of Epic Games. Log in again in Settings." with a Settings button.
- Epic chip selected with no Epic games: empty state "Log in to Epic to see your Epic games" with a **Log in to Epic** button (`epic_log_in`); if `lib.epic === 'noLegendary'`, say Legendary isn't installed instead.
- `Card`: logo top-right (26px circle, dark glass); not-installed: option C fade + "Install" chip bottom-left; hover: lift, white outline, title + white Play (installed) or Install pill. Steam Install → `act('install')`; Epic Install → `oninstall` (popup in Task 6; until then call `act('install')`).

- [ ] **Step 1:** Add the two SVGs and `StoreIcon`; extend Rust `Card` with `provider` (and its test-free serde change); `npm run check` and `cargo test` pass.
- [ ] **Step 2:** Build `Ambient`, `Toolbar`, `Continue`; wire them in `App.svelte`; remove the loading text.
- [ ] **Step 3:** Restyle `Card` (logo, chip, hover overlay) and pass the new props from `Grid`.
- [ ] **Step 4:** Verify: `npm run check` → 0 errors; `npx tauri build`; run the exe and capture the window (`PrintWindow`) next to `ambient-v3.html`: logos visible on both stores, faded covers with chips, banner shows last played game.
- [ ] **Step 5:** Add `scripts/bench.ps1`: starts `src-tauri/target/release/launcher.exe` with `LAUNCHER_BENCH=<px/s>`, redirects stdout (the app prints one stats line and exits), and samples the summed private memory of the process tree (the exe plus its `msedgewebview2` children, found through `Win32_Process.ParentProcessId`) every 250 ms; prints the stats line and peak private MB. Run it at 4000 and 15000 → 0 frames >1.5× median. If not, drop the blur from the scrolled layer first.
- [ ] **Step 6:** Commit `feat(ui): ambient shell, store logos and option C cards`.

### Task 5: Settings panel

**Files:**
- Create: `src-tauri/src/settings.rs`, `src/lib/Settings.svelte`
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/main.rs`, `src-tauri/src/library/steam.rs`, `src-tauri/src/library/epic.rs`, `src/App.svelte`, `src/lib/api.ts`

**Interfaces:**
- Consumes: Task 1 settings table; Task 3 `epic_log_out`.
- Produces:
  `steam::persona_name(steam_root: &Path) -> Option<String>` (`UserLocalConfigStore/friends/PersonaName` from the newest `localconfig.vdf`, same file `play_stats` picks);
  `epic::legendary_version() -> Option<String>` (parses `legendary --version`: `legendary version "0.21.1", codename …` → `"0.21.1"`);
  `epic::install(game, base_path: Option<&Path>, on_event)` passes `--base-path <dir>` when set (verify the flag with `legendary install --help` first);
  `settings::Settings { show_banner: bool, ambient: bool, epic_install_dir: Option<PathBuf>, igdb_client_id: Option<String> }` with `load(conn) -> Settings` (defaults: banner and ambient on) and setters through `set_setting`;
  `settings::secret() -> Option<String>`, `settings::set_secret(value: Option<&str>) -> Result<(), String>` (`None` deletes) on Windows Credential Manager through `keyring` (service `dev.noah.gamelauncher`, user `igdb-client-secret`). keyring 4.x splits stores into separate crates: check docs.rs/keyring/4.2.0 for how the Windows store is selected before writing this;
  command `set_igdb_secret(app, value: Option<String>) -> Res<()>`; `update_setting` rejects `igdb_secret` (it must go through `set_igdb_secret`);
  command `settings_view(app) -> SettingsView { steam: Option<SteamAccount { name: Option<String>, path: String }>, epic: Option<Login>, epic_games: usize, epic_install_dir: Option<String>, igdb_client_id: Option<String>, igdb_secret_set: bool, show_banner: bool, ambient: bool, app_version: String, legendary_version: Option<String>, cache_bytes: u64 }`;
  command `update_setting(app, name: String, value: Option<String>) -> Res<()>` (rejects unknown names; `epic_install_dir` must be an existing folder);
  command `clear_cache(app) -> Res<()>` (deletes `Dirs::thumbs` and `Dirs::art`, keeps the database).
  Folder entry is a text field with validation (no dialog plugin: that would be a new dependency).

- [ ] **Step 0:** `cargo add keyring@4.2 --no-default-features --features windows-native-keyring-store` (approved).
- [ ] **Step 1: Failing tests:** `validate("igdb_secret", Some("x"))` is rejected; an ignored test (`#[ignore = "touches Windows Credential Manager"]`) round-trips `set_secret(Some("test"))` → `secret() == Some("test")` → `set_secret(None)` → `secret() == None` under a test-only user name; `persona_name` reads `"fixture"` from `tests/fixtures/steam` (`assert_eq!(persona_name(&fixtures()).as_deref(), Some("fixture"))`); `parse_legendary_version("legendary version \"0.21.1\", codename \"Lowlife\"") == Some("0.21.1")`; `settings::load` on an empty db returns `show_banner == true && ambient == true`; `update_setting` with name `"nope"` is rejected (test the validator `settings::validate(name, value) -> Result<(), String>`).
- [ ] **Step 2:** `cargo test` → FAIL.
- [ ] **Step 3:** Implement backend; build `Settings.svelte` per `settings-v2.html` (frosted panel over the library, sections Accounts, Library, Downloads, Appearance, Game info, Storage, About; Epic row shows state + game count, Reconnect = `epic_log_in`, Log out = `epic_log_out`; Library: Refresh now with last-refreshed time, banner toggle; Appearance: ambient toggle; Game info: IGDB Client ID + Secret fields, link to `https://dev.twitch.tv/console/apps`, Test button (wired in Task 8); About: app and Legendary versions). `App.svelte` reads `settings_view` on start and after every change, and passes `show_banner` to `Continue` and `ambient` to `Ambient`.
- [ ] **Step 4:** `cargo test`, clippy, `npm run check` → PASS; open the panel in the built app and capture it.
- [ ] **Step 5:** Commit `feat: settings panel with accounts, library, downloads, appearance and about`.

### Task 6: Epic download size and the install popup

**Files:**
- Create: `src/lib/InstallPopup.svelte`, `src-tauri/tests/fixtures/legendary/info.json` (captured)
- Modify: `src-tauri/src/library/epic.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/main.rs`, `src/lib/Grid.svelte`, `src/lib/api.ts`

**Interfaces:**
- Produces: `epic::info_json(game: &Game) -> Result<Value, Error>` (`legendary info <app> --json`, hidden window); `epic::download_size_from_info(info: &Value) -> Option<u64>`; command `download_size(key) -> Option<u64>` (cached in `info` with kind `"size"`, 7 days); `api.downloadSize(key)`; `<InstallPopup game anchor onconfirm oncancel />`.

- [ ] **Step 1:** Capture real output: `legendary info <a small owned not-installed AppName> --json > tests/fixtures/legendary/info.json`; remove account-specific fields; note in the test which JSON path holds the download size (verify; do not guess).
- [ ] **Step 2: Failing test** `download_size_from_info_reads_the_fixture` (expects the captured number) and `download_size_from_info_handles_missing_fields` (`download_size_from_info(&json!({})) == None`).
- [ ] **Step 3:** Implement; popup per `hover-install.html` B: opens next to the cover with "Checking size…", fills in `size_label`, shows the install folder (`epic_install_dir` or "Legendary's default folder"), Install usable immediately; Escape and outside click cancel.
- [ ] **Step 4:** `cargo test`, clippy, `npm run check` → PASS.
- [ ] **Step 5:** Commit `feat(epic): confirm installs on the grid with size and folder`.

### Task 7: Store details (description, genres, developer, screenshots)

**Files:**
- Create: `src-tauri/src/info.rs`, `src-tauri/tests/fixtures/steam/appdetails-447040.json` (captured), reuse `tests/fixtures/legendary/info.json`
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/main.rs`, `src/lib/api.ts`

**Interfaces:**
- Produces: `info::GameInfo { description: String, genres: Vec<String>, developer: Option<String>, publisher: Option<String>, released: Option<String>, screenshots: Vec<String> }` (`Serialize, Deserialize`);
  `info::parse_steam_appdetails(json: &str, appid: &str) -> Option<GameInfo>` (from `short_description`, falling back to stripped `about_the_game`; `genres[].description`; first of `developers`/`publishers`; `release_date.date`; up to 3 `screenshots[].path_thumbnail`);
  `info::parse_epic_info(info: &Value) -> Option<GameInfo>` (from the Legendary `info` metadata: description, developer, release date, keyImages screenshots; verify field names against the fixture);
  `info::strip_html(s: &str) -> String` (drops tags, decodes `&amp; &lt; &gt; &quot; &#39; &nbsp;`, `<br>`/`</p>` become newlines, collapses blank lines);
  command `game_info(app, key) -> Option<GameInfoView>`: cached JSON (kind `"details"`, 7 days); Steam fetch `https://store.steampowered.com/api/appdetails?appids={id}&l=english`; screenshots downloaded through `cache::resolve` to `Dirs::art/<key>/shot{n}` and returned as `allow`ed paths.

- [ ] **Step 1:** Capture fixture: `curl "https://store.steampowered.com/api/appdetails?appids=447040&l=english" > tests/fixtures/steam/appdetails-447040.json`.
- [ ] **Step 2: Failing tests:** `parse_steam_appdetails` on the fixture returns non-empty description, `genres` containing `"Action"`, developer `Some("Ubisoft Montréal")` (or whatever the fixture says: assert the fixture's actual values), ≤3 screenshots; `parse_steam_appdetails(r#"{"447040":{"success":false}}"#, "447040") == None`; `strip_html("<p>A &amp; B</p><br>C") == "A & B\nC"`; `failed_fetch_is_not_cached`: a `None`/error result is never written with `put_info`; `cached_info_is_used_when_fetch_fails`: with a cached row, the command path returns it without fetching (test the helper `fn pick(cached: Option<String>, fetch: impl FnOnce() -> Option<String>) -> Option<String>`).
- [ ] **Step 3:** `cargo test` → FAIL; implement; → PASS; clippy.
- [ ] **Step 4:** Commit `feat: store details for Steam and Epic games`.

### Task 8: IGDB how long to beat

**Files:**
- Create: `src-tauri/src/igdb.rs`, fixtures `src-tauri/tests/fixtures/igdb/{token,external_games,time_to_beat}.json` (captured with the owner's credentials; if not yet available, shaped from the official docs at `https://api-docs.igdb.com` and marked unverified in the test)
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/main.rs`, `src/lib/Settings.svelte`, `src/lib/api.ts`

**Interfaces:**
- Consumes: Task 5 `Settings.igdb_client_id` and `settings::secret()`; Task 1 info cache (kind `"ttb"`, 30 days for found, 7 days for "not found").
- Produces: `igdb::Token { value: String, expires_at: Instant }`;
  `igdb::TokenCache` in `AppState` (`Mutex<Option<Token>>` plus `failed_until: Option<Instant>` for a 10-minute back-off after a failed token request);
  `async fn igdb::token(http, client_id, secret) -> Result<Token, String>` (POST `https://id.twitch.tv/oauth2/token?client_id=…&client_secret=…&grant_type=client_credentials`; renew 60 s before `expires_in`);
  `async fn igdb::game_id(http, token, client_id, store: ProviderKind, uid: &str) -> Result<Option<u64>, String>` (POST `/v4/external_games` with Apicalypse body; Steam source and the Epic source id: **verify the current field (`external_game_source` vs older `category`) and the Epic uid format in the docs**; if Epic can't be matched by ID, return `Ok(None)` for Epic);
  `async fn igdb::time_to_beat(http, token, client_id, game_id) -> Result<Option<TimeToBeat>, String>` (POST `/v4/game_time_to_beats`, `fields hastily,normally,completely; where game_id = {id};`, values in seconds: verify);
  `igdb::TimeToBeat { hastily: Option<u64>, normally: Option<u64>, completely: Option<u64> }`;
  `view::hours_label(secs: u64) -> String` (`"32 h"`, under 1 h → `"45 min"`);
  commands `time_to_beat(app, key) -> Option<TimeToBeatView { normally, hastily, completely: Option<String> }>` and `test_igdb(app) -> Res<()>`.

- [ ] **Step 1: Failing tests:** parse fixtures (`parse_token`, `parse_game_id`, `parse_time_to_beat`); `hours_label(115_200) == "32 h"`, `hours_label(2_700) == "45 min"`; `token_is_reused_until_expiry` (cache returns the stored token while `expires_at` is ≥60 s away); `failed_token_backs_off` (after a failure, no new request until `failed_until`).
- [ ] **Step 2:** `cargo test` → FAIL; implement; → PASS; clippy.
- [ ] **Step 3:** Read IGDB's API terms (`https://api-docs.igdb.com`, terms of use) for attribution; if required, show it on the game page next to the tile (small "Data from IGDB" link) and note it in the commit.
- [ ] **Step 4:** Wire the Settings Test button (`test_igdb`: fetch a token and look up Steam app 447040; show OK or the error text).
- [ ] **Step 5:** Commit `feat: how long to beat from IGDB`.

### Task 9: Game page redesign

**Files:** Modify `src/lib/Detail.svelte`, `src/lib/api.ts`, `src-tauri/src/commands.rs` (`Detail` gains `size: Option<String>`, `install_dir: Option<String>`, `store_url: String`; commands `open_folder(key)` via `explorer.exe <dir>` and `open_store_page(key)` via `open_uri` with `https://store.steampowered.com/app/{id}` or `https://store.epicgames.com/` + the product slug if Legendary metadata has one, else the store search URL)

**Interfaces:** Consumes Task 2 `size_label`, Task 5 settings (`ambient`), Task 6 popup/size, Task 7 `game_info`, Task 8 `time_to_beat`.

Layout per `game-page.html` with the owner's change: ambient tint from this game; rounded hero with back button, title, meta line (store logo · Installed/Not installed · developer · year), white Play/Install pill, ⋯ menu (Uninstall, Open install folder, Open store page); tiles Playtime · Last played · How long to beat (each hidden when empty); About (description, up to 3 screenshots); Details (genres, developer, publisher, released, size, install folder: rows hidden when empty). Not-installed Epic: download size beside Install. Keep the existing busy/cancel/confirm-uninstall logic.

- [ ] **Step 1:** Rust: extend `Detail` and add the two commands; test `store_url` for a Steam key (`"https://store.steampowered.com/app/447040"`).
- [ ] **Step 2:** Svelte: rebuild the page; data loads in parallel (`game`, `detailArt`, `gameInfo`, `timeToBeat`) and each section appears when its data arrives without moving what's above it (reserve the tile row height).
- [ ] **Step 3:** Verify: `npm run check`, `cargo test`, clippy; build; capture a Steam and an Epic game page and compare with the mockup.
- [ ] **Step 4:** Commit `feat(ui): ambient game page with details, size and how long to beat`.

### Task 10: Final verification

- [ ] **Step 1:** `cargo test`, `cargo clippy --all-targets -- -D warnings`, `npm run check`, `npx tauri build` → all pass.
- [ ] **Step 2:** Benchmarks at 4000 and 15000 px/s plus idle (60 s settle): 0 slow frames; idle CPU ≤ 0.1%; record numbers next to the earlier ones.
- [ ] **Step 3:** Offline check: disconnect network (or point `legendary` at an unreachable state), start the app: cached library shows, no error banners, game page tiles hide.
- [ ] **Step 4:** Run `/ponytail-review` on the branch; fix must-fix findings; commit.
