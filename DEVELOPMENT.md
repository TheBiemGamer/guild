# Development

Guild is built with Tauri 2, Svelte 5 + TypeScript, and Rust.

## Build from source

You need [Node.js](https://nodejs.org) 24 or later, [Rust](https://rustup.rs) stable (edition 2024), and the [Tauri prerequisites for Windows](https://tauri.app/start/prerequisites/) (WebView2 and the MSVC build tools).

```bash
npm install
npm run tauri dev                       # run in development
npm run tauri build -- --no-bundle      # release exe: src-tauri/target/release/Guild.exe
npm run tauri build                     # installer (needs the signing key, see Releasing)
```

## Checks

Every commit must pass these:

```bash
npm run check                                        # svelte-check + tsc
npm test                                             # frontend unit tests
npm run format:check                                 # prettier (npm run format to fix)
cd src-tauri && cargo fmt --check                    # rustfmt (cargo fmt to fix)
cd src-tauri && cargo test                           # Rust tests
cd src-tauri && cargo clippy --all-targets -- -D warnings
cd src-tauri && cargo deny check                    # licenses, advisories, sources (also in CI)
```

The scroll benchmark runs against the release build:

```bash
pwsh scripts/bench.ps1 4000
pwsh scripts/bench.ps1 15000
```

It passes with 0-2 frames slower than 1.5× the median frame time.

### Snapshot tests

Some Rust tests compare their output with a saved snapshot, using [insta](https://insta.rs). These tests cover what the store tools print (Legendary, gogdl and Nile, parsed from the files in `src-tauri/tests/fixtures/`) and the JSON the commands send to the UI. Snapshots live in `snapshots/` folders next to the code, as `.snap` files. Paths, times and versions in them are replaced with placeholders like `[path]`.

When a change alters a snapshot, `cargo test` fails and writes the new output next to the old one as a `.snap.new` file. To accept the new output:

```bash
cd src-tauri && cargo insta review              # if cargo-insta is installed: step through each change
cd src-tauri && INSTA_UPDATE=always cargo test  # without it: overwrite the snapshots
git diff -- '*.snap'                            # then read what changed
```

On PowerShell, set the variable first with `$env:INSTA_UPDATE='always'`. Commit the changed `.snap` files with the code. In review, a changed command snapshot means the UI now gets different JSON, so `src/lib/bindings.ts` and the UI code that reads it may need changes too.

## Code layout

```
src/                  Svelte UI (App.svelte, src/lib/*)
src-tauri/src/        Rust backend
  commands/           Tauri commands (library, game page, settings)
  library/            store integrations (steam.rs, epic.rs, gog.rs, amazon.rs, custom.rs)
  cache/              SQLite cache, migrations, art cache
  entries.rs          merges store copies into one card
  gamesdb.rs          GOG GamesDB lookups (Steam ↔ Epic, GOG and Amazon)
  igdb.rs             how long to beat
scripts/              benchmark and frontend tests
PRODUCT.md            who Guild is for and what it must stay
DESIGN.md             visual design system: tokens, components, rules
CLAUDE.md             rules for AI coding agents working in this repo
```

`src/lib/bindings.ts` holds the commands and the types they send, generated with tauri-specta from the commands listed in `bindings()` in `src-tauri/src/main.rs`. A new command needs `#[specta::specta]` under `#[tauri::command]`, and the types it takes or returns need `specta::Type`. A debug build rewrites the file at startup; to regenerate it without starting the app:

```bash
cd src-tauri && cargo test bindings
```

`cargo test` fails while the file is stale (and rewrites it), so commit it with the Rust change. `src/lib/api.ts` only adds what needs logic on top: remembered answers, cover batching, asset URLs and pickers.

The database is `library.sqlite`. Schema changes are new migration steps in `src-tauri/src/cache/db.rs`.

## Releasing

Pushing a `v*` tag runs `.github/workflows/release.yml`: it runs the checks, builds the NSIS installer, signs it for the updater and publishes a GitHub release with `latest.json`. The installed app checks `releases/latest/download/latest.json` for updates. Running the workflow by hand only builds and signs, and keeps the files as workflow artifacts.

### One-time setup

1. Create the updater signing key and choose a password:

   ```powershell
   npm run tauri signer generate -- -w "$env:USERPROFILE\.tauri\game-launcher.key"
   ```

2. Back up the key file and its password somewhere safe. Without them, installed copies can't be updated: they only accept updates signed with this key.
3. In the GitHub repository, under **Settings → Secrets and variables → Actions**, add `TAURI_SIGNING_PRIVATE_KEY` (the content of the `.key` file) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
4. Paste the content of `game-launcher.key.pub` into `plugins.updater.pubkey` in `src-tauri/tauri.conf.json` and commit it. Until then, installer builds stop with "Missing comment in public key".

### Each release

1. `npm run bump X.Y.Z` sets the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/Cargo.lock`.
2. Add a `## X.Y.Z, YYYY-MM-DD` section to `CHANGELOG.md`, with `### New`, `### Changed` or `### Fixed` groups of `- ` lines. It becomes the release notes and what the app shows before updating.
3. Commit, then tag and push the tag:

   ```bash
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

The workflow stops if the tag, `package.json` and `Cargo.toml` versions differ, or if `CHANGELOG.md` has no section for the version.

### Local installer builds

`npm run tauri build` signs the updater files, so it needs `TAURI_SIGNING_PRIVATE_KEY` (and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`) set. To build an installer without the key:

```powershell
npm run tauri build -- --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

`npm run tauri build -- --no-bundle` and the scroll benchmark don't need the key.

## Contributing

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org).
- UI changes follow [DESIGN.md](DESIGN.md), and update it when the UI changes.
- [CLAUDE.md](CLAUDE.md) lists the project rules that aren't obvious from the code: secrets, the database, merge keys, network behavior.
