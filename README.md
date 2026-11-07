# Game Launcher

A desktop launcher for Windows that shows your Steam and Epic Games libraries as one library. A game you own in both stores becomes a single card, and you pick which store launches it.

Built with Tauri 2, Svelte 5 + TypeScript, and Rust.

## Features

- **One library.** Steam and Epic games share one grid. A game owned in both stores is merged, but only on an exact Steam app id match (from GOG GamesDB), never on title.
- **Your full Steam library**, including games you haven't installed, after a one-time login with a password or QR code.
- **Epic through [Legendary](https://github.com/derrod/legendary):**
  - install with a download queue (pause, resume, reorder);
  - updates;
  - cloud saves;
  - verify, repair and move installs;
  - import existing installs and sync with the Epic Games Launcher;
  - DLC listed under its game;
  - per-game launch options.
- **Game pages** with the store description, screenshots, playtime, last played, how long to beat (IGDB), and links to the store pages, PCGamingWiki, SteamDB and ProtonDB.
- **Sort and filter.** Sorting by playtime or how long to beat also groups the cards. Filters cover status, playtime and how long to beat.
- **Works offline** from its local cache.
- **Runs in the tray** if you like, starts with Windows, and shows a Windows notification when a download finishes.

## Requirements

- Windows 10 or 11
- [Steam](https://store.steampowered.com/about/), for Steam games
- [Legendary](https://github.com/derrod/legendary/releases), for Epic games. It must be on `PATH` or in `%LOCALAPPDATA%\Programs\legendary`.
- Optional: a free [Twitch developer application](https://dev.twitch.tv/console/apps), whose client ID and secret enable how long to beat (IGDB).

## Build from source

You need [Node.js](https://nodejs.org) 24 or later, [Rust](https://rustup.rs) stable (edition 2024), and the [Tauri prerequisites for Windows](https://tauri.app/start/prerequisites/) (WebView2 and the MSVC build tools).

```bash
npm install
npm run tauri dev                       # run in development
npm run tauri build -- --no-bundle      # release exe: src-tauri/target/release/launcher.exe
npm run tauri build                     # installer
```

### Checks

Every commit must pass these:

```bash
npm run check                                        # svelte-check + tsc
npm test                                             # frontend unit tests
cd src-tauri && cargo test                           # Rust tests
cd src-tauri && cargo clippy --all-targets -- -D warnings
```

The scroll benchmark runs against the release build:

```bash
pwsh scripts/bench.ps1 4000
pwsh scripts/bench.ps1 15000
```

It passes with 0–2 frames slower than 1.5× the median frame time.

## Where data lives

- **Library cache, settings and play history:** `%LOCALAPPDATA%\launcher\data\library.sqlite`. Schema changes are migrations inside this file (`src-tauri/src/cache/db.rs`).
- **Art and thumbnails:** `%LOCALAPPDATA%\launcher\data\`. Settings → Storage clears them.
- **Secrets:** the Steam login token and the IGDB client secret are kept only in Windows Credential Manager, under `dev.noah.gamelauncher`. Your Steam password and Steam Guard codes are never stored.
- **Epic login:** handled by Legendary in its own config folder.

## Project layout

```
src/                  Svelte UI (App.svelte, src/lib/*)
src-tauri/src/        Rust backend
  commands/           Tauri commands (library, game page, settings)
  library/            store integrations (steam.rs, epic.rs)
  cache/              SQLite cache, migrations, art cache
  entries.rs          merges store copies into one card
  gamesdb.rs          GOG GamesDB lookups (Steam ↔ Epic)
  igdb.rs             how long to beat
scripts/              benchmark and frontend tests
docs/superpowers/     design specs, plans and mockups
DESIGN.md             visual design system: tokens, components, motion
CLAUDE.md             rules for AI coding agents working in this repo
```

## Third-party services

- **Steam:** Steam's web API (login) and the Steam client network via [steam-vent](https://codeberg.org/steam-vent/steam-vent) (owned games, download sizes).
- **Epic Games:** [Legendary](https://github.com/derrod/legendary). Some features are unofficial, so they may break when a store changes its API.
- **[GOG GamesDB](https://gamesdb.gog.com):** links Epic games to their Steam versions.
- **[IGDB](https://www.igdb.com):** how long to beat, credited on the game page.

Failures fail quietly: an attention dot appears in Settings, and there are no error banners.

## License

Private project. No license has been granted yet.
