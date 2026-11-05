# Ambient redesign: library, game page, settings

Status: reviewed, awaiting owner approval · 2026-11-05 · Tauri 2 + Svelte 5 app on branch `feat/tauri-rewrite`

Mockups (throwaway, for reference): `.superpowers/brainstorm/26-1793914734/content/`
`ambient-v3.html` (library), `installed-marking-v2.html` (option C), `game-page.html`, `settings-v2.html`.

## Why

The launcher works but looks plain, and three things bother the owner in daily use:

1. You can't see at a glance which games are Steam and which are Epic.
2. There is nowhere to reconnect or log out of Epic.
3. "Loading Epic games…" shows on every start, although the saved library is already on screen.

He wants it to look **sleek, modern and gradient**, and the game page to carry useful facts:
how big the game is, the store description, how long it takes to beat.

## Goals

- A distinct, polished look ("Ambient") across the library, the game page and settings.
- Store identity on every cover; installed vs not installed obvious at a glance.
- A settings panel with account management (Epic reconnect / log out).
- Quiet background refreshes: no loading message on every start.
- A game page with size, description, details and how long to beat (IGDB).

## Non-goals

- Anything from the project's standing non-goals: achievements, friends, chat, store browsing,
  cloud saves, theming or injecting into Steam, macOS/Linux, auto-updater, installer.
- Owned-but-not-installed Steam games (that is milestone M5, decided separately).
- Light theme.

## Anti-goals (would make this a failure even if it works)

- Scrolling that is less smooth than now. The `LAUNCHER_BENCH` run currently shows 175 fps
  with 0 dropped frames over 613 games at 4,000 and 15,000 px/s; that must hold.
- Status text or spinners flashing on every start.
- Layout jumps when data arrives late (covers, store details, how-long-to-beat).
- Generic "AI-made" decoration: gradient text, colored glow halos, neon for its own sake.

## Constraints

- Stack stays as is: Tauri 2, Svelte 5 + TypeScript, Rust backend in `src-tauri`. Windows 10/11 only.
- The frontend never touches the disk or decodes images in Rust for display; the webview loads
  files through the asset protocol (`allow_file` per path), as now.
- New Rust crates or npm packages need the owner's approval first (project rule).
- Steam and Epic stay the ones that install, update and launch (Epic via Legendary).
- Small conventional commits, `cargo clippy -D warnings`, tests for logic with branches,
  `/ponytail-review` before calling it done.

## What was decided (by the owner)

### Overall look: "Ambient"

- The art of the **last-played game**, heavily blurred and saturated, tints the whole window.
  It fades to near-black lower down so content stays readable. It changes when the
  last-played game changes.
- Chrome (bars, chips, buttons) is translucent dark glass with thin light borders.
- Primary action buttons (Play) are solid white pills with dark text.

### Library screen (mockup `ambient-v3.html`)

- **Top bar:** search box; filter chips **All · Steam logo · Epic logo · Installed**; a sort menu;
  a ⚙ button that opens settings.
- **"Continue playing" banner** under the bar: last-played game's hero art in a wide rounded
  card with the title and a white Play button.
- **Grid of covers** (2:3, rounded). Each cover carries the **real store logo**
  (Steam or Epic Games) in the **top-right corner**, white on a small dark glass circle.
  The logos come from Simple Icons (SVG, bundled in the app, never fetched at runtime).
- **Installed vs not installed: option C.** Installed covers are full color. Covers of games that
  are not installed are slightly faded (lower saturation and brightness) and show an
  **"Install" chip** with a download icon in the bottom-left corner.
- **Hover:** cover lifts slightly with a white outline; shows the title and a small Play
  (or Install) button, so launching doesn't require opening the game page.
- **Installing from the grid (owner's decision, mockup `hover-install.html`, option B):**
  - Steam: the hover Install button opens Steam's own install dialog, as today.
  - Epic: the hover Install button opens a **small confirm popup next to the cover**:
    "Install *title*?", the download size and the destination folder, with Install and Cancel.
    The user stays on the grid.
  - The popup appears at once with "Checking size…" while Legendary looks the size up (a slow
    online call); the number fills in when it arrives. Install is usable straight away.
- **Epic loading is quiet:** the cached library shows immediately; the Epic rescan runs in the
  background with no visible message. A message only appears on a first-ever load with nothing
  cached, or when the user refreshes manually.

### Game page (mockup `game-page.html`, with the owner's change)

- The page uses the same ambient tint, from this game's art.
- **Hero banner** (rounded, wide): back button; title; a meta line (store · installed state ·
  developer · year); a big white **Play** (or **Install**) button and a **⋯** menu.
- **Quick-fact tiles:** Playtime · Last played · **How long to beat** (from IGDB: "normally" as the
  main number, with hastily / completely as smaller text). **Not** size.
- **Size lives in Details.** For a game that is not installed, its download size also sits next
  to the Install button.
- **About panel:** store description and a few screenshots.
- **Details panel:** genres, developer, publisher, release date, size, install folder.

### Settings (mockup `settings-v2.html`, with the owner's change)

- A frosted panel over the library (the library stays underneath, scroll position kept).
  Section list on the left: Accounts, Library, Downloads, Appearance, Storage, About.
- **Accounts:**
  - Steam: connected state, account name, install path, "Open Steam".
  - Epic Games: logged-in state and game count, **Reconnect** and **Log out**.
    Do **not** show "via Legendary 0.21.1" here; the Legendary version goes under **About**.
- **Library:** "Refresh now" with last-refreshed time; "Show the Continue playing banner".
- **Downloads:** Epic install folder (currently Legendary's default).
- **Game info:** IGDB (Twitch) Client ID and Client Secret fields, with a link to the Twitch developer
  console and a "Test" button that confirms the credentials work.
- **Appearance:** ambient color on/off (off = neutral dark gradient).
- **Storage** (cache size, clear cache) and **About** (app version, Legendary version):
  listed in the mockup but not designed in detail.

## Where the game-page data comes from (findings, not yet built)

| Data | Steam | Epic |
|---|---|---|
| Playtime, last played | `localconfig.vdf` (already read) | not available from Legendary |
| Size on disk (installed) | appmanifest `SizeOnDisk` (steamlocate has it) | `legendary list-installed --json` → `install_size` |
| Download size (not installed) | not available publicly (see open questions) | `legendary info <app> --json` |
| Description, genres, developer, publisher, release date, screenshots | Steam store `appdetails` endpoint: public, undocumented, rate-limited | Epic catalog metadata that `legendary list --json` already returns (fields to verify) |
| How long to beat | IGDB `game_time_to_beats` | IGDB `game_time_to_beats` |

**How long to beat comes from IGDB (owner's decision).** He weighed IGDB against RAWG (simpler key,
but only an average-hours-played figure) and HowLongToBeat (unofficial, title-only matching).
- IGDB is Twitch's official game database. Access needs a free **Twitch developer application**:
  the owner pastes its **Client ID and Client Secret** into Settings (a secret can't ship inside the
  binary). The app exchanges them for an app access token (client-credentials flow) and renews it
  when it expires. No credentials: the tile hides.
- Match games by store ID, not title: IGDB links games to store pages ("external games"), so a
  Steam app ID maps exactly. Epic should map the same way; **verify** before relying on it, and
  fall back to hiding the tile rather than guessing by title.
- Time-to-beat values come in three flavors (hastily, normally, completely); verify the units
  (expected: seconds) and the exact field names against the official IGDB docs.
- Free tier allows about 4 requests per second (verify); batch lookups and cache results in SQLite
  so each game is fetched at most once per cache lifetime.
- Check IGDB's terms for attribution and show it where IGDB data appears if required.
- IGDB also has descriptions, genres, developers, release dates and screenshots. The builder may use
  it to fill gaps where Epic's catalog data is thin.

## Claude's calls (owner can override)

- **"Hide software and tools" is dropped from this redesign.** It was Claude's addition, and nothing
  the app reads today tells a tool from a game reliably (it would need Steam store data for every
  game, or parsing the binary `appinfo.vdf`). Revisit later on its own.
- **Last played includes Epic.** The app records its own launches (both stores) in SQLite and takes
  the later of that and Steam's `localconfig.vdf` value. This drives the ambient tint, the
  Continue playing banner and the Last played sort. With no last-played game at all, the banner
  hides and the background uses the neutral dark gradient.
- **Epic problems are quiet but never silent:**
  - When Legendary reports logged out, unreachable, or missing, the **cached Epic games stay**
    (today a logged-out scan wipes them; that changes).
  - A small dot appears on the ⚙ button, and Settings → Accounts shows what's wrong with the fix
    (Log in / Reconnect / install Legendary).
  - A one-line dismissible banner appears **only** when the state changed to logged out since the
    last run, never on every start.
  - First-time Epic login lives in Settings → Accounts; the Epic filter chip, when there are no
    Epic games, shows an empty state with a "Log in to Epic" button.
- **Store descriptions are shown as plain text** (markup stripped, paragraphs kept). No HTML
  sanitizer dependency, and no remote images inside descriptions (the CSP stays as is).
- **No new dependencies are expected.** The Simple Icons logos are copied in as two SVG files;
  Steam `appdetails` and IGDB are JSON over the existing `reqwest` + `serde_json`.
  Anything beyond that still needs the owner's approval.
- The ⋯ menu on the game page: Uninstall, Open install folder, Open store page.
- Missing facts hide their tile or row instead of showing a dash.
- Store details and IGDB results are cached in the existing SQLite database so the
  game page renders instantly on a second visit and the app stays within rate limits.

## Left to the builder

- Exact colors, blur strengths, radii, shadows and motion (within the anti-goals).
- Cache lifetimes for store details and how-long-to-beat.
- Component structure, naming, how the ambient background is rendered efficiently.
- Whether the "Install" chip and hover Play button share one component.
