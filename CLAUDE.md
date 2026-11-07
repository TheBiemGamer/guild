# game-launcher

Windows-only launcher for the Steam and Epic libraries. Built with Tauri 2, Svelte 5 + TypeScript, and Rust (`src-tauri/src`). Epic goes through the Legendary CLI; Steam through local files and steam-vent.

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
- **Secrets** live only in Windows Credential Manager (service `dev.noah.gamelauncher`): the Steam refresh token and the IGDB secret.
  - Never put them in SQLite, files, logs, error strings or `Debug` output, and never send them to the frontend.
  - Steam passwords and Steam Guard codes are never stored.
- **The database** file is `library.sqlite`; never change the file name. Schema changes are new migration steps at the end of `STEPS` in `cache/db.rs` (`PRAGMA user_version` counts the ones run); never edit a step that shipped.
  - Open it only through `cache::db::open`, which sets WAL and the busy timeout and runs the migrations.
  - The owner's data lives in `settings`, `launches`, `game_prefs` (per-game choices) and `playtime`.
  - `info` is cache only: use it with a new `kind` for new cached data, never for anything the owner chose or built up.
- **Keys:**
  - A store copy is keyed `steam-<appid>` or `epic-<app_name>`.
  - `entries.rs` groups copies into one card, and merges only on an exact, one-to-one Steam appid match (from GOG GamesDB twins).
  - Launch history stays keyed by copy key.
- **Network** work runs in the background and is throttled, and it fails quietly: a dot in Settings, no banners.
  - Steam CM logons must not fan out. Use the single-flight guards.
- **New OS-specific code** goes behind `src-tauri/src/platform.rs` with `#[cfg(windows)]`. Linux may come later.
- **UI** follows `DESIGN.md` (tokens, components, motion). Update it whenever the UI changes.
- **Dependencies:** ask the owner before adding a crate or npm package.
- **Style:** take the smallest change that fully solves the task (ponytail).
- **Commit messages** are conventional and never carry Co-Authored-By, Claude-Session or "Generated with" lines.
- **Running the app:**
  - Never drive it with simulated mouse or keyboard input while the owner may be using the PC. Capture it with PrintWindow instead.
  - Never stop a `launcher.exe` you didn't start; the owner often runs `tauri dev`.

## Docs
- Current spec, plan and mockups: `docs/superpowers/specs/2026-11-06-one-library-design.md`, `docs/superpowers/plans/2026-11-06-one-library.md`, and `docs/superpowers/specs/*mockup*.html`.
- Progress ledger for the running plan (git-ignored): `.superpowers/sdd/2026-11-06-one-library/progress.md`.
