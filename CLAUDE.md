# game-launcher

Windows-only launcher for the Steam, Epic, GOG and Amazon Games libraries. Built with Tauri 2, Svelte 5 + TypeScript, and Rust (`src-tauri/src`). Epic goes through the Legendary CLI, GOG through gogdl, Amazon through Nile; Steam through local files and steam-vent.

## Commands
- Frontend:
  - `npm run check` (svelte-check + tsc)
  - `npm test` (node tests in `scripts/`)
- Rust, run from `src-tauri/`:
  - `cargo test`
  - `cargo clippy --all-targets -- -D warnings`
- Release build: `npm run tauri build -- --no-bundle`. The exe is `src-tauri/target/release/launcher.exe`.
- Scroll benchmark: `pwsh scripts/bench.ps1 4000` and `15000`, run against the release build. The bar is 0–2 frames slower than 1.5× the median.
- Every commit must pass check, test, cargo test and clippy.

## Rules that aren't obvious from the code
- **Secrets** live only in Windows Credential Manager (service `dev.noah.gamelauncher`): the Steam refresh token, the IGDB secret and the SteamGridDB API key.
  - Never put them in SQLite, files, logs, error strings or `Debug` output, and never send them to the frontend.
  - Steam passwords and Steam Guard codes are never stored.
- **The database** file is `library.sqlite`; never change the file name. Schema changes are new migration steps at the end of `STEPS` in `cache/db.rs` (`PRAGMA user_version` counts the ones run); never edit a step that shipped.
  - Open it only through `cache::db::open`, which sets WAL and the busy timeout and runs the migrations.
  - The owner's data lives in `settings`, `launches`, `game_prefs` (per-game choices, favorites and hidden games, by copy key), `playtime`, `collections`, `collection_games` (members by copy key) and `custom_games` (games the owner added). Their covers are owner data too: files in `<data_local_dir>/custom/<id>/`, never the cache folder.
  - `info` is cache only: use it with a new `kind` for new cached data, never for anything the owner chose or built up.
- **Keys:**
  - A store copy is keyed `steam-<appid>`, `epic-<app_name>`, `gog-<product_id>` or `amazon-<product id>` (`amzn1.adg.product.…`); a game the owner added is `custom-<id>` and never merges.
  - `entries.rs` groups copies into one card, and merges an Epic, GOG or Amazon copy only on an exact, one-to-one Steam appid match (from GOG GamesDB twins; Amazon by product id only), per store. When several games of one store claim one appid, only the single one whose title equals the Steam title (trimmed, case-insensitive) merges; never on title alone. Epic, GOG and Amazon copies only meet through an owned Steam copy.
  - Launch history stays keyed by copy key.
- **Network** work runs in the background and is throttled, and it fails quietly: a dot in Settings, no banners.
  - Steam CM logons must not fan out. Use the single-flight guards.
- **Commands and their types** reach the UI through the generated `src/lib/bindings.ts` (tauri-specta, `bindings()` in `main.rs`), never hand-written `invoke` calls or types; run `cargo test bindings` after changing them.
- **User-visible backend text** is message codes (`msg!` in `src-tauri/src/msg.rs`), worded in `src/lib/messages.ts`; never English sentences in Rust errors or statuses. Raw OS, SQLite or tool errors go through as `Msg::system`.
- **Logging:** app code logs through the `tracing` macros (`info!`, `warn!` for failures, `error!` for lost data or a broken app); `src-tauri/src/log.rs` writes them to `logs/launcher.log` in the local data folder. Never `eprintln!`/`println!`, except the stdout lines the scroll benchmark reads. No `#[instrument]` on functions that take secrets. The log rule for secrets above applies.
- **Store tools** (Legendary, gogdl, Nile) are pinned in `src-tauri/src/tools.rs` and SHA-256-checked before use; bump one by changing its version, URL and hash together.
- **New OS-specific code** goes behind `src-tauri/src/platform.rs` with `#[cfg(windows)]`. Linux may come later.
- **UI** follows `DESIGN.md` (tokens, components, motion). Update it whenever the UI changes.
- **UI text** only through `t()` with a key in `src/lib/i18n/en.json` (backend codes as `msg.<code>`), never hard-coded.
- **Dependencies:** ask the owner before adding a crate or npm package.
- **Always run both skills on every change.**
  - [ponytail](https://github.com/DietrichGebert/ponytail) for code: take the smallest change that fully solves the task, and use `ponytail-review` before calling it done.
  - [antislop](https://github.com/miqdadbadjuber/anti-slop) for anything users see (`antislop-ui` for UI and copy). Leave out decorative hints, labels that repeat each other, filler copy, and dots or animations that mark no real state.
- **Commit messages** are conventional and never carry Co-Authored-By, Claude-Session or "Generated with" lines.
- **Running the app:**
  - Never drive it with simulated mouse or keyboard input while the owner may be using the PC. Capture it with PrintWindow instead.
  - Never stop a `launcher.exe` you didn't start; the owner often runs `tauri dev`.

## Docs
- Current spec, plan and mockups: `docs/superpowers/specs/2026-11-06-one-library-design.md`, `docs/superpowers/plans/2026-11-06-one-library.md`, and `docs/superpowers/specs/*mockup*.html`.
- Progress ledger for the running plan (git-ignored): `.superpowers/sdd/2026-11-06-one-library/progress.md`.
