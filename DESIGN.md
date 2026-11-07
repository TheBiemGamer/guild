---
name: Game Launcher
description: "A Windows desktop launcher for Steam and Epic games, in the owner's 'Ambient' look: sleek, modern, gradient."
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

# Design System: Game Launcher

## Overview

**Creative North Star: "Afterglow"** (confirmed by the owner, 2026-11-06). The library glows with whatever you played last. Everything else stays dark, quiet glass, so the covers and that one glow carry all the color. When in doubt, chrome gets no color of its own; color belongs to the games, and white belongs to the one primary action.

The art of the last-played game, "heavily blurred and saturated, tints the whole window. It fades to near-black lower down so content stays readable." On top of that sit translucent dark-glass chrome with thin light borders, covers as the main content, and one solid white pill for the primary action. It is a dark-only, dense, content-first desktop app (Windows 10/11, Tauri webview), not a page. Light theme is an owner non-goal.

**Key Characteristics:**
- One ambient tint layer behind everything, static while scrolling (drawn once into a small canvas, scaled up).
- Dark glass chrome: translucent black fills, 1px light borders, no heavy shadows on bars.
- Solid white pill = the primary action (Play, Install, Log in). Nothing else is that bright.
- Store identity (real Steam/Epic logos) and installed state readable on every cover at a glance.
- Quiet by default: no status text or spinners on every start.

## Colors

A near-black blue-violet base (`bg`, `surface`, `surface-2`, `line`), white text at three opacities, and a single violet accent used sparingly. Colour mostly comes from the game art through the ambient layer, not from the UI.

### Primary
- **Signal Violet** (`accent` #7c5cff, hover `accent-2` #a48bff): the "on" state of toggles, links in Settings, the legacy `button.primary`, the install progress bar in the game page hero and the progress fill in the Downloads popover. Never the main call to action; that is the white pill.

### Neutral
- **Ink Black** (`bg` #0e0e14): window base, text on white pills, the neutral gradient floor.
- **Night Surface** (`surface` #16161f) and **Raised Surface** (`surface-2` #1f1f2a): cover placeholder, default buttons, dropdown options. `line` (#2c2c3a) is their hover/border step.
- **Cloud White** (`text` #eef0f6), `text-2` (white 75%), `text-3` (white 50%), **Dusk Grey** (`muted` #8d90a3): primary text, secondary text, hints and placeholders, and quiet metadata.
- **Glass set** (`glass`, `glass-strong`, `glass-hover`, `glass-line`, `glass-on`, `glass-on-line`): all bar, chip, search, notice and field fills and borders. Use these instead of inventing a new alpha.
- **Pill White** (`pill` #fff, hover #e4e6ee): the primary action only. The code writes these two as literals in `app.css` (`button.pill`), not as custom properties.

### Status
- **Play Green** (`play` #22c55e): running state only (the "Playing" badge, the running-card ring).
- **Amber** (`warn` #f5b942): a store needs attention (settings dot, "Not logged in").
- **Red** (`danger` #ef4444): destructive actions and error notices. **Soft Red** (`danger-text` #fca5a5): destructive text on dark glass (Settings "Remove", the game page's Uninstall); plain `danger` is too dark for small text there.
- **Scrim** (`scrim`, rgb(10 10 16 / 0.9)): the bottom stop of a gradient over art, so titles on photos stay readable (game page hero).
- Settings also uses literal, untokenised tints: success text #4ade80, error text #fecaca, icon tiles white 6%, store tiles #1b2838 (Steam), #2a2a2a (Epic), #86328a (GOG) and #232f3e (Amazon Games). Treat these as house values until tokenised.

### Named Rules
**The One Bright Thing Rule.** The solid white pill is the only solid-bright element on a screen. Accent violet and green never compete with it.
**The Tint Comes From The Art Rule.** Chromatic colour on the library comes from the ambient layer. UI chrome stays neutral.

## Typography

**Display / Body / Label Font:** Segoe UI Variable Text (with Segoe UI, system-ui, sans-serif). One family only.

**Character:** The native Windows face, plain and unforced; hierarchy comes from weight, size and uppercase tracking, not from a second font.

### Hierarchy
- **Display** (700, 34px, 1.1): the game page title, on the hero.
- **Title** (700, 26px, 1.15): Continue-playing banner game name.
- **Headline** (600, 20px): 700 for the Settings page titles (Accounts, General, Services, About); 16px/700 for the Settings panel title.
- **Body** (400, 14px): default text; 13px for hints, 15px/600 for the cover fallback title, 16px for empty states.
- **Label** (600, 12px, 0.08em tracking, uppercase): section labels ("Library 613", Settings group headings in `text-3`, game page tile and panel headings, "Continue playing"); 11px/600 for chips and the banner kicker. Counts use tabular numerals.

### Named Rules
**The Eleven Pixel Floor Rule.** No UI text below 11px. (Observed in code, and the `.impeccable/config.json` waivers say the real UI keeps text at 11px or more; not an owner statement.)

## Layout

A full-window column: toolbar, optional notices, then a content area that fills the rest. The library is a **virtualised grid** of fixed 180x270 covers with a 16px gap, columns centred in the available width, a 24px page gutter on bars and notices, and the Continue banner (180px high) plus section label scrolling with the covers as the grid header. Only rows near the viewport exist in the DOM. Because of that, card size and gap are constants in code, not fluid. The sort sets the grouping (there is no separate Group control). Grouped, the header keeps only the banner and each group starts on a new row under its own section label ("10–20 h 12", the Label style with a `text-3` count): the label row is 18px plus the 16px gap, and 24px of space sits above every label but the first. Groups and labels are rows of the same virtualised list. Empty groups are hidden; within a group the sort order applies. Filters keep each group's cards together.

| Sort | Groups, in order |
|---|---|
| Recently played | Today · This week · This month · Earlier · Never played (local calendar days: This week is the 6 days before today, This month the 23 days before those) |
| Most played | 500 h+ · 100–500 h · 50–100 h · 20–50 h · 10–20 h · 0–10 h · Never played |
| Longest to beat | 100 h+ · 40–100 h · 15–40 h · 5–15 h · Under 5 h · Unknown |
| Shortest to beat | Under 5 h · 5–15 h · 15–40 h · 40–100 h · 100 h+ · Unknown |
| Title | none: one list under the "Library" label |

**Favorites on top** (owner's option A): when a shown game is a favorite, the list starts with a "Favorites" group, then the rest: the sort's own groups without the favorites, or, under Title, one "All games" group. Not when Favorites is the only Lists filter (every game shown is one). **Hidden games** are left out of the library, search, the count and the Continue banner; the Hidden filter shows them. A game that is both is hidden.

Settings is a modal panel (max 760px wide, 24px inset from the window) with a 190px section list on the left; under 640px wide it stacks, nav on top (owner's pick 2026-11-07: settings v4, option C with A's rows). Four sections, in the list and down the page: Accounts · General · Services · About. Each list item has a 16px stroke icon (1.8 stroke, 10px gap, 8px 10px padding, 8px radius, `text-2`; active = white 10% fill and `text`); Accounts carries a 6px amber dot at its right end under the same rule as the settings chip's dot. Each section starts with its name as a page title (20px, 700; 32px between sections), then groups: a Label-style heading (12px, 600, uppercase, 0.08em, `text-3`; 18px above, 8px below) over one rounded group (`glass` fill, `glass-line` border, 12px radius, no clip so menus can hang out of it) whose rows are split by `glass-line` rules, never a box per row. A row: 11px 14px padding, 12px gap, a 30px icon tile (8px radius, white 6% fill, 16px `text-2` stroke icon), the label (subtitle under it in `text-3` 12px, only where it explains something), then its control on the right; at the narrowest the buttons wrap to a second line, right-aligned. General: Startup (Start with Windows; Keep running in the tray when closed, "Downloads keep going."; Notify when a download finishes) and Look (Ambient color from your last-played game; Show the "Continue playing" banner; Language). Services (what Extra metadata held; owner's pick 2026-11-07, services option A): one group with a row per service, folded like the store cards. IGDB and SteamGridDB rows each have a 30px tile with the initials (IG, SG; 11px/700), the name, and one `text-3` line saying what it does ("How long to beat and extra details", "Covers for games you add"), then " · Connected" in the success color once set up (IGDB: Client ID and secret saved; SteamGridDB: key saved), or, while not set up, the hint with its link ("needs a free Twitch application, open the developer console" / "needs a free API key, get one"). On the right a fold button with a chevron: quiet "Edit" when set up, the standard button "Set up" when not. It unfolds the fields under the row inside the group, indented 56px like the Options panel, one line each: the label at 100px on the left, the control filling the rest (it wraps under the label only below 280px). IGDB: Client ID, Client Secret, and Test (its result in the success color beside it); SteamGridDB: API key. Folded each time Settings opens, except SteamGridDB when Add a game's link opened Settings at Services. About: Version (the Version row, see Updates), Storage (Cached covers and art, its size as the subtitle, Clear cache) and Troubleshooting (a "Log file" row with Open log folder and Report a problem, which opens a GitHub bug report with the versions filled in). Each store's own settings live under its account row (see Account card). Settings can open scrolled to a section (the Add a game dialog's link opens Services). The game page covers the whole content area (the library stays mounted underneath, hidden, so its scroll position survives): a column capped at 1360px with a 24px gutter, holding the 300px hero, a tab row (Overview · DLC · Saves · Launch options · Installation), and on Overview a row of three equal fact-tile slots (16px gap; every tile is one head line and one value line, so all tiles are the same height) and a 1.7fr / 1fr About / Details grid.

Spacing rhythm observed: 8 / 12 / 16 / 24 px. Controls on bars are 36px high.

## Elevation & Depth

Hybrid: tonal layering and translucency for chrome, soft black shadows for objects that sit over art. Depth over the ambient tint comes from glass fills plus a 1px light border, not from shadow.

### Shadow Vocabulary
- **Cover rest** (`0 4px 12px rgb(0 0 0 / 0.45)`): covers at rest.
- **Cover lift** (`0 16px 34px rgb(0 0 0 / 0.6), 0 0 0 2px rgb(255 255 255 / 0.85)`): hovered or focused cover, with a 4px rise and 1.03 scale. The white 2px ring is the hover outline the owner asked for.
- **Banner** (`0 10px 30px rgb(0 0 0 / 0.45)`).
- **Modal** (`0 30px 80px rgb(0 0 0 / 0.6)`) with `backdrop-filter: blur(24px)` on the Settings panel. Popovers (⋯ menu, Play with, Filters, Downloads) use the same shadow without the blur.

### Named Rules
**The No Halo Rule.** Shadows are black and directional. No coloured glow halos (owner anti-goal). The only coloured ring is the green running ring on a card.
**The Static Backdrop Rule.** Blur is applied once off-screen (canvas) or only on small non-scrolling surfaces. Do not put a live `backdrop-filter` or CSS blur on anything that scrolls. (Derived from the owner's scrolling anti-goal and the code comments in `Ambient.svelte`.)

## Shapes

Rounded, soft-cornered, never sharp: 10px for covers, chips, fields and rows; 8px for plain buttons; 12px notices, popovers, logo tiles and account cards; 14px banner; 18px modal; full pills for primary actions, toggles and the cover store logo (26px glass circle, top-right).

Borders are always thin and translucent (`glass-line`, 1px). Cover art is clipped to the card radius. Portrait art fills the cover (`cover`); landscape art stays whole (`contain`).

## Components

### Buttons
- **Primary (pill):** white fill, `bg` text, 700 weight, 999px radius, 9px 20px (small: 6px 14px, 12px). The only primary-action style.
- **Default:** `surface-2` fill, 8px radius, 8px 16px; hover steps to `line`; press scales to 0.97.
- **Glass:** `glass-on` fill with `glass-on-line` border, for secondary actions on glass.
- **Settings button:** 10px radius, white 10% fill, white 14% border; destructive variant uses red text, not a red fill.
- **Focus:** 2px `focus` outline, 2px offset, everywhere. Disabled is 60% opacity.

### Chips (filters, sort, downloads, settings button)
36px high, `glass` fill, `glass-line` border, `text-2` text, 6px icon gap. Selected (`aria-pressed`) = `glass-on` fill, `glass-on-line` border, `text`. The settings chip carries an 8px amber dot, ringed in `bg`, only for a real problem: Steam rejected its saved login, or a store that was set up here broke. For Epic, GOG and Amazon Games that is a login that can't be reached, or a logout or missing tool while the store's games are still in the library; a store never set up (no tool, never logged in, no games), or logged out of with Log out until it is logged in again, raises no dot.
- **Store selector:** one segmented control in place of separate chips: All, then the logos (18px, `aria-label` with the store name) of only the stores the library has games from, in the order Steam, Epic, GOG, Amazon Games (`Library::stores`, from the whole library, so searching or filtering never changes it). With one store or none there is no control at all. When the selected store leaves (its games are gone), the filter falls back to All. Chip height and fill (36px, `glass`, `glass-line` border, 10px radius, 2px padding), 36px segments in `text-2` (`text` when selected or hovered; an unselected hovered segment gets `glass-hover`). The selected segment sits on a `glass-on` pill (`glass-on-line` border, 8px radius) that slides to it with a 150ms ease-out transform; no transition under reduced motion (the global rule). `role="radiogroup"` ("Store"), each segment `role="radio"` with `aria-checked`; only the selected segment is in the tab order, and the arrow keys move to the next or previous store and pick it, wrapping. Installed stays a separate toggle chip right after it. Games the owner added show under All only.
- **Active filter chip:** a filter turned on in the Filters popover also shows in the toolbar, after Installed, as a selected chip with its name (a collection's own name) and a `text-3` 11px ✕ (8px gap); clicking it clears exactly that filter (`removeChip` in `src/lib/filters.ts`).
- **Filters chip:** a 15px sliders icon plus "Filters"; opens the Filters popover.
- **Sort:** a chip with a 14px chevron after the text, opening a menu popover (mockup v7). It shows just the current sort: Recently played, Most played, Longest to beat, Shortest to beat, Title; games without the value go last, in title order. The sort also sets the grouping (see Layout). Saved in settings (`library_sort`) and restored at start; an old `library_group` value is ignored. How long to beat always comes from the cache, never from a lookup.
- **Add a game chip:** a 36px icon chip (18px + stroke icon), `aria-label`/`title` "Add a game", left of the settings chip (owner's option A, 2026-11-07). Opens the Add a game dialog.
- **Downloads chip:** a 36px icon chip (17px download arrow), shown only while `poll` reports an install or update, or the download queue (Epic, GOG and Amazon Games) has a job (active, waiting or paused). A 2px `accent-2` ring inset 3px inside its edge fills clockwise with the active job's progress, else the average progress (a conic gradient masked to a ring; it changes with each 2 s poll, never per frame). `aria-label` "Downloads, 42%".

- **Update chip:** a text chip with a 16px up arrow in `accent-2` and "Update", shown only while a check found a newer release, left of Downloads. Opens the Update popover. The app checks quietly once the startup refresh is done, then every 6 h (never in a dev build or the benchmark); a failed check shows nothing here.

### Popovers (Filters, Downloads, Sort)
`surface-2` fill, `glass-on-line` border, 12px radius, modal shadow, 44px below their chip (Filters left-aligned, Downloads right-aligned). Keyboard and pointer (`src/lib/popover.ts`): focus moves to the first control on open, Tab and Shift+Tab stay inside, Escape closes and returns focus to the chip, a click outside closes. Mouse Back/Forward are ignored while one is open, as with Settings. Ctrl K closes it and focuses the search.
- **Filters:** 360px wide, 12px padding. Groups under 11px uppercase `text-3` headings (10px above, 6px below; the first 2px): Status (Not installed, Update available, Downloading), Lists (Favorites, each collection by name, Hidden; a long name ends in an ellipsis), Your playtime (Never played, Under 10 h, 10–50 h, 50 h+; unknown counts as never played), How long to beat (Under 5 h, 5–15 h, 15–40 h, 40–100 h, 100 h+, on the hours the card shows; unknown matches none) and Has (On more than one store; only while the library has games from two or more stores, and switched off when that ends). Filters in one group widen each other, groups narrow. Toggles are 30px pills, 13px, `glass` fill and `glass-line` border, on = `glass-on` / `glass-on-line` / `text`, `aria-pressed`. A footer under a `glass-line` rule (12px above, 10px padding) holds Clear (13px `text-2` text button) on the left and "Manage collections…" (the same text button, `aria-expanded`) on the right, which folds out a list under the footer (8px above, rows 2px apart, 13px): each collection's name (click to rename in a 28px field, `glass` fill, `glass-line` border, 10px radius; Enter or leaving the field saves, Escape cancels) and a 28px round ✕ in `text-3` (`aria-label` "Delete <name>"). ✕ asks in the row: "Delete <name>? Games stay in your library." then Delete (`danger-text`) and Cancel, 28px round chips (`glass-hover` fill, `glass-line` border); focus goes to Cancel. With no collections it says "No collections yet. Add games to one from a game's menu." A failed rename shows as one quiet `text-2` line under the list. Genre (once genres are cached), DLC and Cloud saves join when their data exists.
- **Sort menu** (`ChoiceMenu.svelte`): right-aligned, min 230px wide, 6px padding, `role="menu"`. One `menuitemradio` row per option (8px 10px padding, 8px radius, `text-2`; hover `glass-hover` and `text`), the chosen row in `text` with a trailing ✓ in `text-2` (`aria-checked`). Focus opens on the chosen row; Up/Down/Home/End move; Enter, Space or a click picks and closes, returning focus to the chip.
- **Downloads:** 380px wide, 6px padding (layout F2, chosen by the owner 2026-11-07). Every download is one two-line row, the same for every store: 16px store logo, the title (600, ellipsis) and under it the progress line in `text-2` 12px with tabular numerals, "50% · 0.7 GB · 13 s left" (`progressShort`; Epic's numbers from Legendary, GOG's from gogdl; Steam's from its appmanifest moved on by steam.exe's disk writes, capped at 99% until Steam says it is done; the time shows once known, a repair reads "Repairing 50% · …"). The row is the progress bar: an 8px-radius fill, `accent` at 18% with a 2px `accent` line along its bottom, as wide as the progress (`role="progressbar"`, width eases over 400ms linear). Rows sit 2px apart, 8px 10px padding, no headings or rules. The active job (Epic or GOG) comes first: "Installing…"/"Updating…"/"Verifying…" until Legendary or gogdl reports, then 28px round glass icon buttons, Pause (two bars) or Resume (play triangle), and Cancel (✕, `aria-label` "Cancel <title>"). While the queue is paused its fill turns white 8% with a `text-3` line and the text reads "Paused · 42%" (or "Paused"). Then any Epic Move or GOG DLC uninstall in progress and Steam's own downloads, without buttons. "Up next" (11px uppercase `text-3` heading) lists waiting jobs without a fill: title, the action ("Install", "Update", "Verify") in `text-3`, and 28px round glass ↑, ↓ (`aria-label` "Move <title> up/down"; disabled at the ends, nothing moves ahead of the active job) and ✕ (`aria-label` "Remove <title> from queue"). Pause applies to the whole queue, as in Steam: the active job stops and stays first; Resume starts it again. The queue survives a restart.

- **Notes** (`NotesPopover.svelte`, one component for every release-notes popover; owner-approved mock `updates.html`): 380px wide, 14px padding, fixed to the window so the Settings panel can't clip it: right-aligned under its button, above it when the notes don't fit below and there's more room there; it scrolls when taller than the room. A title line: the version (16px, 600) and, after 8px, a `text-3` 12px aside. Then the CHANGELOG.md section (`parseNotes`): each `###` heading as an 11px uppercase `text-3` Label (12px above, 4px below), its items as a bulleted list in `text-2` (line height 1.55). Keyboard and pointer as the other popovers.
- **Update popover:** Notes for the new version, the aside "you have 0.2.0", then a footer under a `glass-line` rule (12px above, 14px margin, 8px gap, right-aligned): a glass "Later" (closes) and the white pill "Update and restart". While it downloads the pill reads "Downloading…" (`aria-disabled`) and Later is disabled. A failure shows inline above the footer in `danger-text` 13px, and the buttons work again. While a game runs the pill is disabled and a `text-3` 13px line says "Close the game to update." The installer closes the app (the tray icon is hidden first) and starts the new version.

### Cards / Containers
- **Cover card:** 180x270, 10px radius. While the cover loads it shows `surface-2`, then the cover's average colour, then the image fading in over 150ms; the title shows only when there is no art. Store logo top-right; a game the owner added shows a white line gamepad in the same badge (`icons/gamepad.svg`, `custom` in `StoreIcon`). Installed = full colour; not installed = `saturate(0.5) brightness(0.75)` plus a bottom-left glass "Install" chip (owner's option C). Running = "Playing" green badge and green ring; installing = a 6px progress strip. A game waiting in the download queue shows a bottom-left glass "Queued" chip (no icon) in place of Install/Update and, on hover, the busy pill "Queued" (`aria-disabled`) and a small round glass Cancel that takes it out of the queue; the active job while the queue is paused shows a "Paused" chip in place of its strip and, on hover, "Paused" and Resume. The running active job keeps the progress strip. **Stat badge:** while sorted by Most played, Longest to beat or Shortest to beat, every card shows a bottom-left chip in the same glass as Install, in `text-2` with tabular numerals: "38 h played" (whole hours, "45 min played" under an hour), or "~25 h to beat" in the how-long-to-beat sorts. On a not-installed card it takes the Install chip's place (the dimmed cover still says not installed, and the hover keeps the Install pill). Playing, the progress strip, Queued, Paused and Update win over it; never played or unknown shows no badge (a not-installed card then keeps Install). Other sorts show no badge. It hides on hover like the other chips.
- **Hover:** lifts, white ring, reveals title and a small Play (installed) or Install (not) pill. The overlay is absent (not transparent) when idle, to keep scrolling cheap.
- **Favorite heart:** a 28px round button top-left (8px in; `rgb(0 0 0 / 0.5)` fill, `glass-line` border, a 14px white heart outline), shown on hover or focus-within and absent otherwise; a favorite shows it filled, always. A click toggles the favorite without opening the game (`aria-pressed`, `aria-label` "Favorite <title>"). A right-click on a card opens the game menu (see ⋯ menu) at the pointer, kept inside the window; scrolling closes it.
- **Continue banner:** 14px radius, 180px, hero art cover-fit, left-to-right dark scrim, title and white Play pill bottom-left.
- **Notice:** `glass-strong`, 12px radius, 44px min height, dismiss button; error variant has a red border. Used for Epic logged-out (only on state change), errors and "Updated to".
- **Updated notice:** on the first start after an update (owner's option A): "Updated to **0.3.0**" (version in 600), a glass "What's new" that opens the installed version's Notes under it (absent when CHANGELOG.md has no section for it), and ✕. Shown once: `seen_version` in settings remembers the newest version that started; a first start or an older dev build says nothing.
- **Version row (Settings → About):** "Version 0.2.0" with a `text-3` 12px subtitle: "Up to date" (after a check this session), "0.3.0 available", or the check's or install's error. Two Settings buttons: "What's new" (the installed version's Notes; hidden without a section) and "Check for updates" ("Checking…" while it runs), which becomes "Update and restart" ("Downloading…") once an update is found, running the same install as the popover.
- **Accounts heading row:** the "Accounts" Label on the left; on the right "Refreshed 5 min ago" ("Refreshed just now", "Not refreshed yet") in `text-3` 12px, 8px before a 30px round icon button (`glass-on` fill, `glass-on-line` border, 15px ↻ stroke icon, `aria-label`/`title` "Refresh library") that refreshes the library. While a refresh runs the button is disabled and its icon spins (1s linear, only while the refresh runs; it marks real state). Under reduced motion there is no spin and the text reads "Refreshing…" instead.
- **Account card (Settings, Accounts first):** one row per source (Steam, Epic Games, GOG, Amazon Games) in one Accounts group, laid out like every Settings row: a 30px store-logo tile (8px radius, the store's tint, 17px logo) in place of the icon tile, then the store name over the account name in `text-3` 12px (no game count; nothing when Epic has no name; GOG shows "Logged in" until its first refresh has the name; Amazon Games shows the account's first name), and one button: Log out (`danger-text`) when logged in, Log in when not. A problem replaces the account name with short `warn` text, 12px: "Not logged in", "Can't reach Epic", "Can't reach GOG" or "Can't reach Amazon" (the button is Reconnect), "Steam isn't installed". The Refresh button and "Refreshed … ago" sit right of the Accounts title. The cards follow the backend while Settings is open: every library reload re-reads them, so the startup scan, refreshes and polls show without reopening.
- **Epic card without Legendary:** in place of the account name, `text-3` "Epic games need Legendary, a free open-source tool.", and one Settings button "Install Legendary" (no Log in, no Options). While it installs the button is disabled and reads "Installing…", then "Installing… 42%" once the size is known (`aria-live="polite"`). A failure takes the account-name line as the amber problem line with the message ("The download didn't match. Nothing was installed.", "Couldn't download Legendary."), and the button stays to try again. Once installed, the card shows Log in at once (the store's login is read right after the install; the full rescan follows in the background).
- **Epic card Options:** whenever Legendary is installed (logged out too), the Epic card has an "Options" button before its Log in / Log out button: the Settings button shape with a quiet outline (transparent fill, `glass-line` border, `text-2`, `glass-hover` on hover) and a 12px chevron (6px gap) that turns 180° when open (150ms ease-out). `aria-expanded`, `aria-controls`. It folds out a panel inside the Accounts group, right under that store's row and split from it by the group's `glass-line` rule, indented 56px on the left so it lines up with the store name (4px 14px 8px 56px padding), rows with 8px vertical padding, split by `glass-line`: Install folder (the text field, up to 260px, with Reset when set; saved on blur or Enter), Sync cloud saves (toggle), Import an existing install, Sync with Epic Games Launcher ("Adds the games it installed."), and, only when the app's own Legendary is older than the pinned build, "Legendary 0.21.0" over "0.21.1 is available." with a Settings button "Update" ("Updating… 42%" while it runs, `aria-live="polite"`; a failure is one quiet line under it, as the import rows). Folded every time Settings opens; nothing is remembered. Steam's card has no Options.
- **GOG card:** the Epic card's states with gogdl in Legendary's place: without gogdl, "GOG games need gogdl, a free open-source tool." and "Install gogdl" ("Installing… 42%"; a failure is the amber problem line). Then Log in / Log out, and Options: Install folder (as Epic's; empty installs into `~/Games`), Sync cloud saves (a toggle like Epic's, GOG's own setting), Import an existing install (an "Import…" Settings button opens the system folder picker; gogdl reads which game the folder holds, so there is no game select; "Importing…" while it runs; the result is one quiet line under it, "Imported <game>." or the error, as Epic's import rows), and, only when the app's own gogdl is older than the pinned build, a gogdl update row like Legendary's. GOG copies install, update, verify, move, uninstall, change DLC, play and sync cloud saves like Epic copies (the same queue, install popup, busy pill, progress, Downloads rows with the GOG logo, toasts, Saves tab, conflict question, Launch options tab and "Play without updating").
- **Amazon Games card:** the GOG card's states with Nile in gogdl's place: without Nile, "Amazon games need Nile, a free open-source tool." and "Install Nile" ("Installing… 42%"; a failure is the amber problem line). Then Log in / Log out (Log in opens Amazon's sign-in page in its own window), and Options: Install folder (as GOG's; empty installs into `~/Games`), Import an existing install (Epic's rows: "Import…" opens the folder picker, then "Which game is in <folder>?" with a select of the owned Amazon games that aren't installed, Import and Cancel; "Importing…" while Nile checks the files, which can take minutes), and, only when the app's own Nile is older than the pinned build, a Nile update row like Legendary's. There is no cloud saves toggle: Amazon has no cloud saves. Amazon copies install, update, verify, move, uninstall and play like GOG copies (the same queue, install popup without the "Include DLC" box, busy pill, progress, Downloads rows with the Amazon Games logo, toasts, Launch options tab with Extra arguments and Alternative executable, and "Play without updating"), without DLC or a Saves tab. Verify is refused while the game has an update ("Update first, then verify."). Playtime is only the time played from here.
- **Epic import rows (in Epic Options):** "Import an existing install" (an "Import…" Settings button opens the system folder picker; then a stacked row asks "Which game is in <folder>?" with a select of owned, not-installed Epic games, styled like the text fields, and Import / Cancel buttons) and "Sync with Epic Games Launcher" (a "Sync" button, "Syncing…" while it runs; import only, nothing is sent to EGL). Each row's last result is one quiet line under it (13px `text-2`, ellipsis, full text in the tooltip): success ("Imported <game>.", "Synced with the Epic Games Launcher.") or Legendary's error. No banners.

### Inputs / Fields
36px high, 10px radius, `glass` fill, `glass-line` border, `text-3` placeholder, 2px focus outline. The search field's border brightens to `glass-on-line` on focus. It carries a `Ctrl K` hint at the right (11px, `text-2`, 5px radius, `glass-on-line` border), hidden while focused or filled; Ctrl K focuses it (not while Settings is open). Secrets (the IGDB secret, the SteamGridDB API key) are shown only as a filled read-only field with "••••••••" and "Saved" (success color, right) inside it, then Replace and Remove (owner decision); the dots give way first when space runs out.

### Navigation
Top bar only, left to right (mockup v7): search, the store selector (All · the stores in use; absent with one store), Installed, active filter chips, Filters, spacer, Sort, Update (only while an update is ready), Downloads (only while something downloads), Add a game, settings. The bar sits above the content (`z-index: 5`) so its popovers hang over the grid and the game page. Settings has a left section list (active = white 10% fill) in the modal. There is no sidebar.

### Add a game dialog
`src/lib/AddGame.svelte`, per the owner-approved mock `custom-game.html`. A centred panel over the Settings scrim (`rgb(8 8 12 / 0.55)`): min(520px, window − 48px) wide, 18px padding, `surface-2`, `glass-on-line` border, 16px radius, the modal shadow. Title "Add a game" ("Edit game" when editing), 16px/700. Fields, 12px apart, each under a 12px `text-2` label: Program (a read-only field with the path, which opens the picker too, and a glass "Browse…" that opens the system file picker on .exe files), Name (filled from the program's file name without its extension until the owner types one), Arguments (placeholder "Optional"; split on spaces, double quotes keep words together, never through a shell). Fields are the standard 36px glass fields. Cover: four 76x114 slots (8px radius, 8px gap, `surface` while searching or empty, 2px transparent border, `glass-on-line` on hover), filled with SteamGridDB's first four 600x900 covers for the name; the picked one has a white 2px border (`aria-pressed`). Adding, the first is picked; editing keeps the current cover until one is picked. Under them, 12px `text-3`: "From SteamGridDB · Use a file…" (the link in `accent-2`; a picked PNG or JPEG replaces the first words with its file name). The search runs at once on open, then 400 ms after the name stops changing; no spinner or status text. Without a SteamGridDB key the slots are replaced by a note (12px `text-3` on `glass`, 10px radius): "Add a SteamGridDB API key in Settings → Services to pick a cover here.", the Settings part a link that closes the dialog and opens Settings there, then a glass "Use a file…". A failure shows as one error line above the footer (the Settings error style). Footer under a `glass-line` rule (12px above, 16px gap): Cancel (glass) and the white pill "Add game" ("Save"), disabled until there is a program and a name (and, adding without a file, until the cover search answers). Keyboard as the popovers: focus starts in Program, Tab stays inside, Escape closes; a click on the scrim closes. Saved, the dialog closes and the library reloads; the new game's page does not open.

### Language
- **Language row (Settings → General → Look, last):** "Language" on the left; on the right the current choice as an Options-style Settings button (quiet outline, 12px chevron that turns when open, `aria-label` "Language: System"). It opens the Sort menu's `ChoiceMenu` under it, right-aligned: System, English, Nederlands. System follows Windows' display language (Dutch picks Nederlands, anything else English); each language is named in its own language and only System is translated. A pick saves `language` and the whole UI, the tray menu and later toasts and login window titles switch at once, without a reload.
- **Copy lives in language files:** every word the UI shows comes from `src/lib/i18n/<lang>.json` through `t(key, params)` (`src/lib/i18n.ts`): flat dotted keys (`settings.language`), `{name}` placeholders, plurals as `key_one`/`key_other` picked by an `n` param. A key missing from a language shows its English words. Backend message codes are the `msg.<code>` keys. To add a string: add the key to `en.json` (and to `nl.json` once it exists), call `t('the.key')`; text around markup (a link, a bold value) uses `around(key, name)`. Store names, game titles and product names (Steam, Epic Games, GOG, Amazon Games, Legendary, gogdl, Nile, IGDB, SteamGridDB, PCGamingWiki…) stay as they are. CHANGELOG.md release notes stay English in every language.
- **Native text** (tray, toasts, login window titles) is in `src-tauri/src/msg.rs`, one table per language (`EN`, `NL`) with the same words as `msg.<code>`; a test checks them.

### Toggle
40x22 pill switch, white 18% off, `accent` on, white knob; `role="switch"`.

### Tray and notifications
- **Tray icon:** the app icon, tooltip "Game Launcher". Left-click shows and focuses the window. Menu: Open · Downloads (shows the window and opens the Downloads popover when something is downloading) · separator · Quit.
- **Close:** with "Keep running in the tray when closed" on (default) the window's close button hides it to the tray and downloads go on; off, it quits. Started by Windows, the window stays in the tray.
- **Toasts:** a Windows toast titled with the game when a download finishes: "Ready to play" (install, Steam download), "Updated", "Files verified" or "Repaired". None for failures (shown in-app) or cancelled jobs.

### Game page
`src/lib/Detail.svelte`, per the owner-approved mockup `game-page.html` with the owner's change (no size tile).
- **Tint:** the window's ambient layer switches to this game's hero art (its cover when it has none) while the page is open, and back on leaving. Same `Ambient.svelte`, so the Ambient color setting (Settings → General) applies.
- **Hero:** 300px, 14px radius, banner shadow; art cover-fit at 30% from the top, fading in. A gradient darkens the top (`glass`) for the back button and the bottom (`scrim`) for the text. Back button top-left: `glass-strong` with `glass-line`, 10px radius (no backdrop blur: the hero scrolls). Bottom-left: Display title, then a meta line in `text-2` (a logo per store that owns it · "Installed on <Store>" (the chosen copy's, else the first installed copy's) or "Not installed" · developer · year, `·` in `text-3`). Install progress, while it runs, is a 6px `accent` bar under the meta line, with the long progress line under it (`progressLong`): "Downloading 50% · 0.7 of 1.9 GB · 1 min left" for Steam, which knows the total, "Installing 50% · 0.7 GB downloaded · 13 s left" for Epic ("Updating …", "Repairing …"); GOG's reads like Steam's ("Installing 50% · 0.7 of 1.9 GB · …"), as gogdl reports the total; verify reads as in the Downloads popover. No speed.
- **Actions** (bottom-right): a 44px round glass `⋯` button and the large white pill (Play, Install, disabled "Playing", or busy text; the busy pill is `aria-disabled` so it stays focusable). An Epic or GOG job in the download queue drives the busy pill: "Installing…"/"Updating…"/"Verifying…" while it runs, "Queued" while it waits, "Paused" (progress text "Paused · 42%") while the queue is paused, each with a round glass Cancel ("Cancelling…" until it stops); Paused adds Resume before Cancel. Move and Uninstall are refused while the game is in the queue. A game owned on both stores gets the **split pill** instead: one 44px white pill in two segments. The left holds the chosen store's 18px logo and a 14px chevron (`aria-haspopup="menu"`, `aria-expanded`, `aria-label="Choose launcher, now <Store>"`); a 1px divider at `bg` 18% inset 10px; the right is Play (play icon) or Install (download icon) for the chosen copy. The choice is not stored; it resets to the default copy when another game opens. A not-installed Epic or GOG copy shows "Checking size…", then "27.3 GB download", in `text-2` beside Install. Epic and GOG uninstall ask inline (text, then Uninstall in `danger-text` and Cancel, both round glass); Steam asks in its own dialog. An Epic or GOG Play that finds a cloud save conflict asks the same way before launching: "Local save is newer than the cloud save." (or "Cloud save is newer than the local save."), then Keep cloud, Keep local and Cancel (round glass); focus goes to Cancel. It never picks a save by itself. A Play from a card that hits a conflict opens the game page, which asks.
- **Play with menu:** same popover look as the ⋯ menu, 290px wide, under the split pill. A "Play with" Label heading, then one `menuitemradio` per copy: store logo, name, and a `text-2` 12px sub-line ("Installed · 27.3 GB", "Not installed", or "Not installed · 4.3 GB download" for Epic and GOG); a check mark marks the chosen copy. Keyboard as the ⋯ menu: focus opens on the chosen item, arrows/Home/End move, Escape returns focus to the switcher, Tab closes, a click outside closes. Picking a copy closes the menu and returns focus to the switcher.
- **⋯ menu** (`GameMenu.svelte`, shared with the card's right-click): `surface-2` popover with a `glass-on-line` border, 12px radius, 6px padding, min 240px wide and the modal shadow, hanging below the button (always shown, any copy). Rows are 8px radius with 36px left padding, so a leading ✓ (`accent-2`, per the mockup) lines up; hover and focus `glass-hover`. First the game's lists: Favorite (`menuitemcheckbox`, ✓ when on), Add to collection with a `text-3` › (a submenu beside it, on the side with room, max 320px tall: one `menuitemcheckbox` per collection with ✓ when the game is in it, a `glass-line` separator, then "New collection…", which turns into a 34px name field; Enter creates the collection and adds the game, Escape goes back; a taken or empty name leaves the field and shows the error), Hide from library (Show in library when hidden). Then, after a separator, the chosen copy's quick actions, while it isn't busy: Open install folder (installed only), Uninstall (installed only, `danger-text`, last; never for a game the owner added). A game the owner added gets "Edit game…" first (opens the Add a game dialog as Edit) and "Remove from library" last, on the card's right-click too. Remove asks in the menu, like the Epic uninstall: "Remove <name> from your library? The game's files stay on disk." (13px `text-2`, max 280px) over Remove (`danger-text`) and Cancel, 28px round chips (`glass-hover` fill, `glass-line` border); focus goes to Cancel. Removing deletes its row and cover; its launch history and playtime stay. Picking an item closes the menu; lists act on every copy of the game. "Play without updating" is on the Installation tab. Keyboard: focus moves to the first item on open, arrows/Home/End move within the open menu, Right (or Enter) on Add to collection opens the submenu on its first item, Left and Escape close it again, Escape closes the menu and returns focus to the button, Tab closes, and a click outside closes. When an action swaps the buttons, focus moves to what replaced them (the busy pill, or the confirm's Cancel). The page focuses "← Library" on open; Back returns focus to the card or banner that opened it.
- **A game the owner added:** the hero is its cover (cover-fit, as any hero), the meta line reads "Added by you" (plus "Not installed" when its program is gone) with no store logo, the only fact tiles are Playtime and Last played (timed here, like Epic's own sessions), there is no About, Details or links, and the tabs are Overview and Installation, which holds only Folder (Open folder).
- **Tabs:** under the hero (16px above), a `role="tablist"` row with a `glass-line` bottom rule: Overview, DLC (with a count pill: 11px, `glass-on`, 999px), Saves, Launch options, Installation. A tab with nothing for this game is hidden (no DLC; Saves unless the chosen copy is an installed Epic or GOG copy with cloud saves; Launch options and Installation when the chosen copy isn't installed); the row itself always shows, Overview alone included, so a DLC list that arrives late doesn't move the page. Tabs use the Label style (12px/600 uppercase, 0.08em) in `text-2`, 10px 14px padding; the open tab is `text` with a 2px `text` underline. Roving tabindex: Left/Right (wrapping), Home and End move and open. A new game opens on Overview; a tab that disappears falls back to Overview.
- **Overview:** today's page: fact tiles, About and Details. When the last cloud save sync failed, one quiet line above them says so (`text-2`, 13px, ellipsis, full text in the tooltip); no banner.
- **DLC tab:** the DLC list in one `glass` panel (rows as described under About / Details below).
- **Saves tab:** one `glass` panel of rows like the Installation tab, for the chosen Epic or GOG copy. Sync cloud saves: a three-way chip group (Default, which follows that store's Options in Settings; On; Off), the picked chip `glass-on` with a `glass-on-line` border (`aria-pressed`). Last sync: "Synced 5 min ago", "Not synced yet", or the last failure as one quiet line in `text-2` (ellipsis, full text in the tooltip). Sync now: a chip ("Syncing…" while it runs; disabled while the game runs or another action is busy); the newer save wins.
- **Launch options tab:** one `glass` panel of rows like the Installation tab, for the chosen installed copy. Epic: Extra arguments (a 28px text field, `glass` fill, `glass-line` border, 10px radius, 13px; saved on blur or Enter, then a quiet "Saved" in `text-3` 12px for 2 s; split on spaces, double quotes group, never through a shell), Offline mode and Skip version check (one toggle chip each, "On"/"Off", pressed `glass-on` with `aria-pressed`), Alternative executable (the path relative to the install folder, or "Default", in `text-3` 12px; a "Choose…" chip opens the system file picker on .exe files in the install folder, and a Reset chip shows when one is set; an exe outside the install folder is refused). Applied on every Epic Play. GOG: the same Extra arguments and Alternative executable rows, without Offline mode and Skip version check; applied on every GOG Play. Steam: one row, "Steam keeps its own launch options." and an "Open in Steam" chip that opens the game's Steam properties.
- **Installation tab:** one `glass` panel of rows (10px padding, `glass-line` between): Folder (path · size in `text-3` 12px, Open folder chip), Update available (Update and Play without updating chips; Epic and GOG), Verify files (a "Verify files" chip; Epic and GOG verify, then repair bad or missing files, Steam validates in its own window; a GOG game imported from another install has no manifest, so it answers "Imported games can't be verified."), Move (a "Move…" chip; Epic and GOG open the system folder picker and moves the game folder into the picked folder; a folder on another drive is refused with "Moving to another drive isn't supported yet. Pick a folder on the same drive."; Steam opens its storage manager), Uninstall (`danger-text` chip; Epic and GOG ask inline in the hero, Steam in its own dialog). Acts on the chosen copy. Every chip is disabled while the game runs or another action is busy. Epic's and GOG's Verify and Move use the hero's busy pill ("Verifying…", "Moving…"); Verify also uses the progress bar ("Verifying 40% · 120 of 300 files", then "Repairing …" while repair downloads) and Cancel, like Install, and its progress shows in the Downloads popover. Move has no Cancel: stopped halfway, Legendary's record and the folder would disagree.
- **Fact tiles:** Playtime, Last played, How long to beat, in three fixed slots. Every tile has the same two lines, a 24px head line (Label heading) and a single value line (20px/600 tabular, ellipsis), so a tile that answers late is exactly as tall as the ones already shown and the row never changes height. Whether the row exists is decided from the game alone (it has a Playtime or Last played tile), never from how long to beat, so it never collapses once shown and is never empty: Steam games always show Playtime, "Never played" included. Playtime's head line carries "Steam + Epic" at the right (`text-2`, 12px) when both copies counted, and Last played adds " · on Steam" after its value (`text-2`) for a multi-copy game. Each tile: `glass` fill, `glass-line` border, 12px radius. How long to beat shows "normally" as the value with " · N all" (completely) after it in `text-2`, per the mockup; all three figures are in the tooltip. Its head line carries a quiet "IGDB" credit link (`text-3`, underlined, 24px tall target, labelled "Data from IGDB") that opens igdb.com. A tile with nothing to say is absent. Steam's "Never played" is a real value and shows; Epic games get no playtime from Legendary, so their playtime is unknown and that tile hides. Rust decides whether a tile shows (`view::playtime_tile`) and sends minutes and a timestamp; Last played counts whole 24 h periods ("Today", "Yesterday", "12 days ago"; months of 30 days, years of 365).
- **About / Details:** `glass` panels, 16px padding, 12px radius, Label headings. Until the store details arrive, About shows four `glass-hover` skeleton lines and Details skeleton values for Genres, Developer, Publisher and Released; screenshot slots stay reserved (16:9, `surface`) while they download. About: plain-text description (line breaks kept) and up to three 16:9 screenshots. Details: a two-column `dl` (`text-2` terms) with Genres (small `glass-on` pills), Developer, Publisher, Released ("28 Nov 2016"; the meta line shows only the year), Size; each row is absent when unknown (the install folder is on the Installation tab). Below them, **Store pages** (one chip per store that owns the game) and **Info** (PCGamingWiki, SteamDB, ProtonDB; only when the Steam app id is known, else the row is absent). Details sits in the right column, or the left one when the game has no About. When the game has owned DLC, the **DLC tab** lists it, one 28px row per DLC: title, then "Installed"/"Not installed" in `text-2` 12px, then for an Epic or GOG DLC of an installed copy an Install or Uninstall (`danger-text`) link chip. Uninstall asks inline in the row ("Its files will be deleted." in `text-2`, then Uninstall in `danger-text` and Cancel chips; focus goes to Cancel). Steam DLC is listed only; Steam installs it itself. No DLC, no heading.
- **Link chips:** buttons, 28px high, 10px side padding, 999px radius, 13px text, a 14px logo (`currentColor`) with a 6px gap, `glass-hover` fill, `glass-line` border, `glass-on` fill on hover. They open the default browser; the UI sends only a link kind and Rust builds the URL (Epic uses the store page IGDB reported when it is on store.epicgames.com, else a store search by title).

## Do's and Don'ts

### Do:
- **Do** use the CSS custom properties in `src/app.css`; add a token before adding a raw value.
- **Do** format labels from numbers (progress, sizes, times, group names) in `src/lib/format.ts`; Rust sends numbers, ids and timestamps. Error and status messages come from Rust as message codes, worded under `msg.<code>` in the language files (`src/lib/messages.ts` renders them). Every label goes through `t()`; no hard-coded English in components.
- **Do** keep scrolling at the benchmark level: `LAUNCHER_BENCH` 175 fps, 0 dropped frames over 613 games at 4,000 and 15,000 px/s (spec, Anti-goals).
- **Do** reserve space for late data (covers, store details, how-long-to-beat) so nothing jumps.
- **Do** hide a missing (unknown) fact's tile or row instead of showing a dash. A known zero is a fact, not a missing one: Steam's "Never played" shows.
- **Do** show store logos from the bundled Simple Icons SVGs, white, never fetched at runtime.
- **Do** honour `prefers-reduced-motion` (already global in `app.css`).

### Don't:
- **Don't** use gradient text, coloured glow halos, or neon for its own sake (owner anti-goal: generic "AI-made" decoration).
- **Don't** flash status text or spinners on every start; Epic refresh is silent.
- **Don't** add a light theme.
- **Don't** put a live blur or `backdrop-filter` on scrolling content.
- **Don't** show a second solid-bright button next to the white pill.
- **Don't** add UI libraries or packages without the owner's approval (project rule).

## Owner decisions

All from `docs/superpowers/specs/2026-11-05-ambient-redesign-design.md` ("What was decided (by the owner)" unless noted). The owner approved this design on 2026-11-05 ("Approve design", in the design session); the spec's status line records it.

- Look: "The art of the **last-played game**, heavily blurred and saturated, tints the whole window. It fades to near-black lower down so content stays readable."
- "Chrome (bars, chips, buttons) is translucent dark glass with thin light borders. Primary action buttons (Play) are solid white pills with dark text."
- Library: "Each cover carries the **real store logo** ... in the **top-right corner**, white on a small dark glass circle." Covers are 2:3 and rounded. Logos are Simple Icons SVGs, bundled.
- Installed marking, option C: "Installed covers are full color. Covers of games that are not installed are slightly faded ... and show an 'Install' chip with a download icon in the bottom-left corner."
- Hover: "cover lifts slightly with a white outline; shows the title and a small Play (or Install) button."
- Epic install from the grid (option B): a small confirm popup next to the cover with size ("Checking size...") and destination; Steam opens its own dialog. Under the destination, an "Include DLC" checkbox (16px, `accent`, `text-2` 14px label), on by default.
- Epic loading is quiet: cached library shows immediately, rescan runs silently.
- Game page: quick-fact tiles are Playtime, Last played, How long to beat; "**Not** size". Size lives in Details.
- Settings: frosted panel over the library; sections Accounts, General, Services, About with grouped rows (updated by the owner 2026-11-07, settings v4: option C with A's rows; Appearance folded into General, Storage into About, Extra metadata renamed Services). Do not show the Legendary version in Settings.
- Ambient colour on/off: "off = neutral dark gradient".
- Secrets: IGDB Client Secret in Windows Credential Manager; the UI shows only a filled "Saved" field with Replace and Remove.
- Add any game yourself (2026-11-07, mock `custom-game.html`): option A, a "+" chip in the toolbar; covers from SteamGridDB (API key in Credential Manager) or a file.
- Non-goals: "Light theme"; achievements, friends, chat, store browsing, cloud saves, theming Steam, macOS/Linux.
- "Left to the builder": exact colours, blur, radii, shadows, motion (within the anti-goals). Token values in this file therefore come from code, not from the spec.

## Anti-goals

From the same spec ("Anti-goals (would make this a failure even if it works)"):
- "Scrolling that is less smooth than now." 175 fps, 0 dropped frames over 613 games at 4,000 and 15,000 px/s must hold. An open lead on one occasional slow frame is in `docs/perf/2026-11-06-occasional-slow-frame.md` (parked by the owner, not a blocker).
- "Status text or spinners flashing on every start."
- "Layout jumps when data arrives late (covers, store details, how-long-to-beat)."
- "Generic 'AI-made' decoration: gradient text, colored glow halos, neon for its own sake."

## House style (recurring waivers)

No `design: waive` comments exist in `src/`, so no rule has recurred three times. `.impeccable/config.json` holds detector ignores only for throwaway mockups plus one `broken-image` ignore on `Card.svelte` (Svelte `<img {src}>` shorthand false positive). Nothing to carry over.

## Open items

- UNCONFIRMED: the exact radii, spacing steps and font sizes above are what the code uses, not owner-stated values.
- This file describes the code as of the game page redesign, plus the spec where marked. The game page has not been rendered yet; compare it with `game-page.html` on the first run.
- No renders were captured (the app was not launched).
