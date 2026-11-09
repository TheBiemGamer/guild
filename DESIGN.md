---
name: Guild
description: "A Windows launcher for Steam, Epic, GOG and Amazon Games libraries. Dark glass over the last-played game's art."
colors:
  bg: "#0e0e14"
  surface: "#16161f"
  surface-2: "#1f1f2a"
  line: "#2c2c3a"
  text: "#eef0f6"
  text-2: "rgb(255 255 255 / 0.75)"
  text-3: "rgb(255 255 255 / 0.5)"
  muted: "#8d90a3"
  accent: "#7c5cff"
  accent-2: "#a48bff"
  play: "#22c55e"
  warn: "#f5b942"
  danger: "#ef4444"
  danger-text: "#fca5a5"
  error-text: "#fecaca"
  success: "#4ade80"
  scrim: "rgb(10 10 16 / 0.9)"
  focus: "rgb(255 255 255 / 0.85)"
  glass: "rgb(0 0 0 / 0.32)"
  glass-strong: "rgb(10 10 16 / 0.6)"
  glass-hover: "rgb(255 255 255 / 0.09)"
  glass-line: "rgb(255 255 255 / 0.08)"
  glass-on: "rgb(255 255 255 / 0.16)"
  glass-on-line: "rgb(255 255 255 / 0.22)"
  pill: "#ffffff"
  pill-hover: "#e4e6ee"
typography:
  display:
    fontFamily: "'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif"
    fontSize: "34px"
    fontWeight: 700
    lineHeight: 1.1
  title:
    fontFamily: "'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif"
    fontSize: "26px"
    fontWeight: 700
    lineHeight: 1.15
  headline:
    fontFamily: "'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif"
    fontSize: "20px"
    fontWeight: 600
  body:
    fontFamily: "'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 400
  label:
    fontFamily: "'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 600
    letterSpacing: "0.08em"
rounded:
  button: "8px"
  base: "10px"
  notice: "12px"
  banner: "14px"
  panel: "18px"
  modal: "18px"
  pill: "999px"
spacing:
  sm: "8px"
  md: "16px"
  lg: "24px"
components:
  button-pill:
    backgroundColor: "{colors.pill}"
    textColor: "{colors.bg}"
    rounded: "{rounded.pill}"
    padding: "9px 20px"
  button-pill-hover:
    backgroundColor: "{colors.pill-hover}"
  button-pill-large:
    backgroundColor: "{colors.pill}"
    textColor: "{colors.bg}"
    rounded: "{rounded.pill}"
    height: "44px"
    padding: "0 32px"
  button-pill-small:
    backgroundColor: "{colors.pill}"
    textColor: "{colors.bg}"
    rounded: "{rounded.pill}"
    padding: "6px 14px"
  button-default:
    backgroundColor: "{colors.surface-2}"
    textColor: "{colors.text}"
    rounded: "{rounded.button}"
    padding: "8px 16px"
  button-default-hover:
    backgroundColor: "{colors.line}"
  chip:
    backgroundColor: "{colors.glass}"
    textColor: "{colors.text-2}"
    rounded: "{rounded.base}"
    height: "36px"
    padding: "0 12px"
  chip-selected:
    backgroundColor: "{colors.glass-on}"
    textColor: "{colors.text}"
  segmented:
    backgroundColor: "{colors.glass}"
    textColor: "{colors.text-2}"
    rounded: "{rounded.base}"
    height: "36px"
    padding: "2px"
  segmented-selected:
    backgroundColor: "{colors.glass-on}"
    textColor: "{colors.text}"
    rounded: "{rounded.button}"
  filter-toggle:
    backgroundColor: "{colors.glass}"
    textColor: "{colors.text-2}"
    rounded: "{rounded.pill}"
    height: "30px"
    padding: "0 12px"
  popover:
    backgroundColor: "{colors.surface-2}"
    rounded: "{rounded.notice}"
    padding: "12px"
  game-tab:
    textColor: "{colors.text-2}"
    padding: "10px 14px"
  search-field:
    backgroundColor: "{colors.glass}"
    textColor: "{colors.text}"
    rounded: "{rounded.base}"
    height: "36px"
    padding: "0 12px"
  cover-card:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.base}"
    width: "180px"
    height: "270px"
  continue-banner:
    backgroundColor: "{colors.glass-strong}"
    rounded: "{rounded.banner}"
    height: "180px"
  notice:
    backgroundColor: "{colors.glass-strong}"
    textColor: "{colors.text}"
    rounded: "{rounded.notice}"
    padding: "8px 8px 8px 16px"
  game-hero:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.banner}"
    height: "300px"
  fact-tile:
    backgroundColor: "{colors.glass}"
    rounded: "{rounded.notice}"
    padding: "12px 16px"
  info-panel:
    backgroundColor: "{colors.glass}"
    rounded: "{rounded.notice}"
    padding: "16px"
  account-card:
    backgroundColor: "{colors.glass}"
    rounded: "{rounded.notice}"
    padding: "12px 14px"
  settings-panel:
    backgroundColor: "rgb(22 22 32 / 0.82)"
    rounded: "{rounded.panel}"
---

# Design System: Guild

The tokens above are the CSS custom properties in `src/app.css`. Use them; add a token before adding a raw value.

## Overview

**North star: "Afterglow"** (owner, 2026-11-06). The library glows with whatever you played last. Everything else is dark, quiet glass, so the covers and that one glow carry the color. Chrome has no color of its own; color belongs to the games, and white belongs to the one primary action.

The last-played game's art, heavily blurred and saturated, tints the whole window and fades to near-black lower down. On top: translucent dark-glass chrome with thin light borders, covers as the main content, and one solid white pill for the primary action. Dark only, dense, content first. A Windows desktop app, not a web page.

## Colors

- **Base:** `bg`, `surface`, `surface-2` and `line`, a near-black blue-violet. Text is `text`, `text-2` (white 75%), `text-3` (white 50%) and `muted`.
- **Glass:** `glass`, `glass-strong`, `glass-hover`, `glass-line`, `glass-on`, `glass-on-line` fill and border every bar, chip, field, notice and panel. Don't invent a new alpha.
- **White pill** (`pill`, `pill-hover`): the primary action only (Play, Install, Log in). Written as literals in `app.css` (`button.pill`).
- **Accent violet** (`accent`, `accent-2`): toggles that are on, links in Settings, progress bars and the Update chip's arrow. Never the main call to action.
- **Status:**
  - `play` green: a running game only (the Playing badge and the card ring).
  - `warn` amber: a store needs attention.
  - `danger` red: destructive actions and error notices. Use `danger-text` for destructive text on glass, because `danger` is too dark at small sizes.
  - `success` green: Saved, Connected, test results. `error-text`: the text of error lines.
  - `scrim`: the bottom stop of gradients over art.
- Store tiles in Settings use literal brand tints: #1b2838 Steam, #2a2a2a Epic, #86328a GOG, #232f3e Amazon Games, #9147ff IGDB.

**One bright thing.** The white pill is the only solid-bright element on a screen. Violet and green never compete with it.

## Typography

One family: Segoe UI Variable Text, falling back to Segoe UI and system-ui. Hierarchy comes from weight, size and uppercase tracking.

| Style    | Use                                                                                                                |
| -------- | ------------------------------------------------------------------------------------------------------------------ |
| Display  | 34px/700, the game page title                                                                                      |
| Title    | 26px/700, the Continue banner game name                                                                            |
| Headline | 20px/700 Settings section titles; 16px/700 dialog titles                                                           |
| Body     | 14px; 13px for hints and secondary lines                                                                           |
| Label    | 12px/600 uppercase, 0.08em tracking: section labels, Settings group headings, tile and tab headings. 11px on chips |

No UI text below 11px. Counts and numbers use tabular figures.

## Layout

- **Window:** toolbar, optional notices, then the content area. 24px gutter on bars and notices. Controls on bars are 36px high. Spacing steps are 8, 12, 16 and 24px.
- **Library:** a virtualised grid of fixed 180×270 covers, 16px gap, columns centred. Only rows near the viewport exist in the DOM, so card size and gap are constants. The Continue banner (180px) and section labels scroll with the grid.
- **Grouping** follows the sort; there is no separate group control. Each group starts on a new row under its own label with a `text-3` count. Empty groups are hidden. Favorites come first as their own group. Hidden games are left out everywhere except the Hidden filter.

  | Sort             | Groups                                                                    |
  | ---------------- | ------------------------------------------------------------------------- |
  | Recently played  | Today · This week · This month · Earlier · Never played                   |
  | Most played      | 500 h+ · 100-500 h · 50-100 h · 20-50 h · 10-20 h · 0-10 h · Never played |
  | Longest to beat  | 100 h+ · 40-100 h · 15-40 h · 5-15 h · Under 5 h · Unknown                |
  | Shortest to beat | the same, reversed                                                        |
  | Title            | none                                                                      |

- **Toolbar,** left to right: search, store selector, Installed, active filter chips, Filters, spacer, Sort, Update (only when one is ready), Downloads (only while something downloads), Add a game, Settings. No sidebar.
- **Settings:** a modal panel, max 760px wide, with a 190px section list on the left (stacked under 640px). Sections: Accounts, General, Services, About. Each section is a title, then Label headings over one rounded `glass` group per heading, its rows split by `glass-line` rules. Never a box per row. On Linux a Wine section sits between General and Services (Windows never shows it): umu-launcher, with Install only while it is missing; Proton, one row per build found with Default and From Steam as the only labels, Make default (Use newest on an explicit default), and Remove with an inline confirm for builds Guild installed only; Install Proton with GE-Proton and Proton-CachyOS chips that fold out their releases, each with Install (its percent while it runs) or Installed; Prefixes, one row per game prefix with its name (the copy key once the game is gone), its size once measured, Open folder, winecfg, winetricks (disabled with a one-line reason when the system has none) and Remove, whose inline confirm says saves in it are lost unless cloud saves are on.
- **Game page:** covers the content area while the library stays mounted underneath, so its scroll position survives. A column up to 1360px: the 300px hero, a tab row (Overview, DLC, Saves, Launch options, Installation); on Linux, Launch options adds a Proton menu chip (Default follows Settings), also for added games, where it is the only row, and never for a native Linux GOG build. A Windows game's first launch on Linux shows Setting up… in the busy pill, the card badge and Continue until its program runs. Its uninstall confirm offers an Also remove its Wine prefix checkbox, off by default, then three fact tiles and an About / Details grid (1.7fr / 1fr).

## Elevation and shapes

- Depth over the tint comes from glass fills and a 1px light border, not shadows. Bars have no shadow.
- Shadows are black and directional:
  - cover at rest: `0 4px 12px rgb(0 0 0 / 0.45)`
  - cover hover: `0 16px 34px rgb(0 0 0 / 0.6)`, a 2px white ring, a 4px rise and 1.03 scale
  - banner: `0 10px 30px rgb(0 0 0 / 0.45)`
  - dialogs and popovers: `--shadow-modal`
- The only colored ring is the green one on a running game.
- Radii: 8px plain buttons and menu rows, 10px covers, chips, fields and rows, 12px popovers, notices, tiles and groups, 14px banner and hero, 18px dialogs, full pills for primary actions, toggles and small round buttons.
- Borders are always 1px and translucent.

## Components

- **Buttons:**
  - Primary is the white pill, 700 weight (large 44px on the game page).
  - Default is `surface-2`, stepping to `line` on hover.
  - Glass is `glass-on` with `glass-on-line`, for secondary actions on glass.
  - Destructive actions use `danger-text`, never a red fill.
  - Focus is a 2px `focus` outline with 2px offset, everywhere. Disabled is 60% opacity. A busy button ("Downloading…", "Queued") is `aria-disabled` so it keeps focus.
- **Chips:** 36px, `glass` fill, `glass-line` border, `text-2`. Selected (`aria-pressed`) is `glass-on`, `glass-on-line`, `text`.
  - The Settings chip carries an 8px amber dot only for a real problem: a saved login that stopped working, or a store that was set up and broke. A store never set up raises nothing.
  - The store selector is one segmented control (`role="radiogroup"`) listing only the stores the library has games from. It is absent with one store.
  - The Downloads chip shows progress as a 2px `accent-2` ring that fills clockwise and updates per poll, never per frame.
- **Popovers** (Filters, Sort, Downloads, Notes, the ⋯ menu, Play with): `surface-2`, `glass-on-line` border, 12px radius, modal shadow.
  - Keyboard (`src/lib/popover.ts`): focus moves in on open, Tab stays inside, Escape closes and returns focus, a click outside closes.
  - Chosen rows carry the same check mark (`Check.svelte`).
  - Menus use `menuitemradio` or `menuitemcheckbox`, with arrows, Home and End.
  - Destructive confirms ask inline in the menu or row, and focus goes to Cancel.
- **Cover card:**
  - States: installed is full color. Not installed is `saturate(0.5) brightness(0.75)` with a bottom-left glass Install chip. Running gets the green badge and ring. Installing gets a 6px progress strip. Queued and Paused get glass chips.
  - In the playtime and length sorts, a stat chip ("38 h played", "~25 h to beat") takes the same spot.
  - Loading: while the cover loads the card shows `surface-2`, then the cover's average color, then the image fading in over 150ms.
  - The store logo sits top-right in a 26px glass circle.
  - On hover: lift, white ring, title, and a small Play or Install pill. The hover overlay is absent when idle, not transparent, to keep scrolling cheap.
- **Notices:** `glass-strong`, 12px radius, with a dismiss button. Errors get a red border and `error-text` and are `role="alert"`; the others are `role="status"`.
- **Fields:** 36px, `glass` fill, `glass-line` border, `text-3` placeholder. Saved secrets show only as a read-only "••••••••" field with "Saved", Replace and Remove; the real value never reaches the UI.
- **Toggle:** 40×22 pill, white 18% off, `accent` on, `role="switch"`.
- **Game page:**
  - The window tint switches to this game's art while the page is open.
  - The hero has a gradient top (`glass`) and bottom (`scrim`), the title and a meta line bottom-left, and actions bottom-right: a round ⋯ button and the large white pill.
  - A game owned in more than one store gets a split pill: the left half picks the store, the right half plays or installs. The choice isn't stored.
  - Fact tiles (Playtime, Last played, How long to beat) are all exactly two lines tall, so a late answer never changes the row height. A tile with nothing to say is absent.
  - How long to beat carries a small "IGDB" credit link; the attribution is required.
  - Tabs with nothing for the game are hidden, but the tab row always shows.
- **Link chips:** 28px pills with a 14px logo, `glass-hover` fill. The UI sends a link kind and Rust builds the URL.
- **Text:**
  - Every UI string goes through `t()` from `src/lib/i18n/<lang>.json`.
  - Backend messages arrive as codes, worded under `msg.<code>`.
  - Store, tool and game names are never translated.
  - Release notes in `CHANGELOG.md` stay English.

## Do

- Format numbers into labels in `src/lib/format.ts`. Rust sends numbers, ids and timestamps.
- Reserve space for late data (covers, store details, how long to beat) so nothing jumps.
- Hide an unknown fact instead of showing a dash. A known zero, like Steam's "Never played", is a fact and shows.
- Use the bundled Simple Icons store logos, white. Never fetch them at runtime.
- Respect `prefers-reduced-motion` (global in `app.css`).

## Don't

- Gradient text, colored glow halos, or neon for its own sake.
- Status text or spinners on every start. Store refreshes are silent.
- A light theme.
- A live blur or `backdrop-filter` on anything that scrolls. Blur once into a canvas (`Ambient.svelte`) or only on small surfaces that don't scroll.
- A second solid-bright button next to the white pill.
- New UI libraries or packages without the owner's approval.

## Owner decisions

- The look: last-played art blurred and saturated behind everything, dark glass chrome, white pill for the primary action (2026-11-05).
- Covers carry the real store logo top-right. Not installed means faded, with an Install chip bottom-left (option C).
- The game page's fact tiles are Playtime, Last played and How long to beat, not size. Size lives in Details.
- Settings is a frosted panel with Accounts, General, Services and About, as grouped rows (settings v4, 2026-11-07). Don't show tool versions there, except when an update is available.
- Ambient color can be turned off, which gives a neutral dark gradient.
- Add a game is a chip in the toolbar. Covers come from SteamGridDB or a file (2026-11-07).
- Downloads popover layout F2: one two-line row per download where the row itself is the progress bar (2026-11-07).
- Installation tab layout C: rows named after what they're about, words only, no icons (2026-11-08).
- Non-goals: a light theme, achievements, friends, chat, store browsing, macOS.

## Anti-goals

These make the design a failure even when everything works:

- Scrolling less smooth than now. `scripts/bench.ps1` must hold at 4,000 and 15,000 px/s.
- Status text or spinners flashing on every start.
- Layout jumps when data arrives late.
- Generic AI-made decoration: gradient text, glow halos, neon.
