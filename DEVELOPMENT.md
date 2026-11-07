# Development

Game Launcher is built with Tauri 2, Svelte 5 + TypeScript, and Rust.

## Build from source

You need [Node.js](https://nodejs.org) 24 or later, [Rust](https://rustup.rs) stable (edition 2024), and the [Tauri prerequisites for Windows](https://tauri.app/start/prerequisites/) (WebView2 and the MSVC build tools).

```bash
npm install
npm run tauri dev                       # run in development
npm run tauri build -- --no-bundle      # release exe: src-tauri/target/release/launcher.exe
npm run tauri build                     # installer
```

## Checks

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

## Code layout

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

The database is `library.sqlite`. Schema changes are new migration steps in `src-tauri/src/cache/db.rs`.

## Contributing

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org).
- UI changes follow [DESIGN.md](DESIGN.md), and update it when the UI changes.
- [CLAUDE.md](CLAUDE.md) lists the project rules that aren't obvious from the code: secrets, the database, merge keys, network behavior.
