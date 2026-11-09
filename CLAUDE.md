# Guild

Guild is a launcher for the Steam, Epic, GOG and Amazon Games libraries, for Windows and Linux. On Linux it runs the store tools' Linux builds; running Windows games through Wine is a later phase. Built with Tauri 2, Svelte 5 + TypeScript, and Rust (`src-tauri/src`). Epic goes through the Legendary CLI, GOG through gogdl, Amazon through Nile; Steam through local files and steam-vent.

## Commands

- Frontend:
  - `npm run check` (svelte-check + tsc)
  - `npm test` (node tests in `scripts/`)
  - `npx prettier --write .` (with `prettier-plugin-svelte`) before committing frontend changes
- Rust, run from `src-tauri/`:
  - `cargo fmt` before committing Rust changes
  - `cargo test`
  - `cargo clippy --all-targets -- -D warnings`
- Release build: `npm run tauri build -- --no-bundle`. The exe is `src-tauri/target/release/Guild.exe`.
- Scroll benchmark: `pwsh scripts/bench.ps1 4000` and `15000`, run against the release build. The bar is 0–2 frames slower than 1.5× the median.
- Every commit must pass check, test, cargo test and clippy, and be formatted (`cargo fmt --check`, `npx prettier --check .`).

## Rules that aren't obvious from the code

- **Secrets** live only in the OS keyring (Windows Credential Manager, or the Secret Service on Linux; service `cloud.biem.guild`): the Steam refresh token, the IGDB secret and the SteamGridDB API key.
  - Never put them in SQLite, files, logs, error strings or `Debug` output, and never send them to the frontend.
  - Steam passwords and Steam Guard codes are never stored.
- **The database** file is `library.sqlite`; never change the file name. Schema changes are new migration steps at the end of `STEPS` in `cache/db.rs` (`PRAGMA user_version` counts the ones run); never edit a step that shipped.
  - Open it only through `cache::db::open`, which sets WAL and the busy timeout and runs the migrations.
  - The owner's data lives in `settings`, `launches`, `game_prefs` (per-game choices, favorites, hidden games and cloud save sync state, by copy key), `playtime`, `collections`, `collection_games` (members by copy key), `custom_games` (games the owner added) and `gog_installs` (where each GOG game is installed). Their covers are owner data too: files in `<data_local_dir>/custom/<id>/`, never the cache folder.
  - `info` is cache only: use it with a new `kind` for new cached data, never for anything the owner chose or built up.
- **Keys:**
  - A store copy is keyed `steam-<appid>`, `epic-<app_name>`, `gog-<product_id>` or `amazon-<product id>` (`amzn1.adg.product.…`); a game the owner added is `custom-<id>` and never merges.
  - `entries.rs` groups copies into one card, and merges an Epic, GOG or Amazon copy only on an exact, one-to-one Steam appid match (from GOG GamesDB twins; Amazon by product id only), per store. When several games of one store claim one appid, only the single one whose title equals the Steam title (trimmed, case-insensitive) merges; never on title alone. Epic, GOG and Amazon copies only meet through an owned Steam copy.
  - Launch history stays keyed by copy key.
- **Network** work runs in the background and is throttled, and it fails quietly: a dot in Settings, no banners.
  - Steam CM logons must not fan out. Use the single-flight guards.
- **Commands and their types** reach the UI through the generated `src/lib/bindings.ts` (tauri-specta, `bindings()` in `main.rs`), never hand-written `invoke` calls or types; run `cargo test bindings` after changing them.
- **User-visible backend text** is message codes (`msg!` in `src-tauri/src/msg.rs`), worded under `msg.<code>` in `src/lib/i18n/en.json` (and the other language files) and rendered by `src/lib/messages.ts`; never English sentences in Rust errors or statuses. Raw OS, SQLite or tool errors go through as `Msg::system`.
- **Logging:** app code logs through the `tracing` macros (`info!`, `warn!` for failures, `error!` for lost data or a broken app); `src-tauri/src/log.rs` writes them to `logs/launcher.log` in the local data folder. Never `eprintln!`/`println!`, except the stdout lines the scroll benchmark reads. No `#[instrument]` on functions that take secrets. The log rule for secrets above applies.
- **Store tools** (Legendary, gogdl, Nile) are pinned per OS in `src-tauri/src/tools.rs` and SHA-256-checked before use; bump one by changing its version and each OS's URL and hash together.
- **New OS-specific code** goes behind `src-tauri/src/platform.rs`, with a `#[cfg(windows)]` and a `#[cfg(target_os = "linux")]` version of each function. Windows behavior must not change for Linux work.
- **UI** follows `DESIGN.md` (tokens, components, motion). Update it whenever the UI changes.
- **UI text** only through `t()` with a key in `src/lib/i18n/en.json` (backend codes as `msg.<code>`), never hard-coded.
- **Dependencies:** ask the owner before adding a crate or npm package.
- **Always run both skills on every change**, also when you write it yourself instead of a worker, and load them before you start, not after the owner asks.
  - [ponytail](https://github.com/DietrichGebert/ponytail) for code: take the smallest change that fully solves the task, and use `ponytail-review` before calling it done.
  - [antislop](https://github.com/miqdadbadjuber/anti-slop) for anything users see: app UI and copy (`antislop-ui`), and README, docs, CHANGELOG notes, release text and issue templates (`antislop-copywriting`). Leave out decorative hints, labels that repeat each other, filler copy, and dots or animations that mark no real state.
  - Icons, logos and mockups go through a design skill (`impeccable`) as well as antislop: look at how comparable products do it first, and check the result at the sizes it ships at.
- **Commit messages** are conventional and never carry Co-Authored-By, Claude-Session or "Generated with" lines.
- **Running the app:**
  - Never drive it with simulated mouse or keyboard input while the owner may be using the PC. Capture it with PrintWindow instead.
  - Never stop a `Guild.exe` or `launcher.exe` you didn't start; the owner often runs `tauri dev`.

## Docs

- `PRODUCT.md`: who Guild is for and what it must stay. `DESIGN.md`: the visual system. `DEVELOPMENT.md`: build, checks and releasing.
- Specs, plans, briefs, mockups and progress notes for work in progress go in `.superpowers/` (git-ignored), never in `docs/`. When the work ships, fold any lasting decision into `DESIGN.md`, `PRODUCT.md` or this file.
