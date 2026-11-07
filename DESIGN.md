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
    padding: "14px 16px"
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
- **Signal Violet** (`accent` #7c5cff, hover `accent-2` #a48bff): the "on" state of toggles, links in Settings, the legacy `button.primary` and the install progress bar in the game page hero. Never the main call to action; that is the white pill.

### Neutral
- **Ink Black** (`bg` #0e0e14): window base, text on white pills, the neutral gradient floor.
- **Night Surface** (`surface` #16161f) and **Raised Surface** (`surface-2` #1f1f2a): cover placeholder, default buttons, dropdown options. `line` (#2c2c3a) is their hover/border step.
- **Cloud White** (`text` #eef0f6), `text-2` (white 75%), `text-3` (white 50%), **Dusk Grey** (`muted` #8d90a3): primary text, secondary text, hints and placeholders, and quiet metadata.
- **Glass set** (`glass`, `glass-strong`, `glass-hover`, `glass-line`, `glass-on`, `glass-on-line`): all bar, chip, search, notice and field fills and borders. Use these instead of inventing a new alpha.
- **Pill White** (`pill` #fff, hover #e4e6ee): the primary action only. The code writes these two as literals in `app.css` (`button.pill`), not as custom properties.

### Status
- **Play Green** (`play` #22c55e): running state only (the "Playing" badge, the running-card ring, the live dot).
- **Amber** (`warn` #f5b942): Epic needs attention (settings dot, "Not logged in").
- **Red** (`danger` #ef4444): destructive actions and error notices. **Soft Red** (`danger-text` #fca5a5): destructive text on dark glass (Settings "Remove", the game page's Uninstall); plain `danger` is too dark for small text there.
- **Scrim** (`scrim`, rgb(10 10 16 / 0.9)): the bottom stop of a gradient over art, so titles on photos stay readable (game page hero).
- Settings also uses literal, untokenised tints: success text #4ade80, error text #fecaca, store tiles #1b2838 (Steam) and #2a2a2a (Epic). Treat these as house values until tokenised.

### Named Rules
**The One Bright Thing Rule.** The solid white pill is the only solid-bright element on a screen. Accent violet and green never compete with it.
**The Tint Comes From The Art Rule.** Chromatic colour on the library comes from the ambient layer. UI chrome stays neutral.

## Typography

**Display / Body / Label Font:** Segoe UI Variable Text (with Segoe UI, system-ui, sans-serif). One family only.

**Character:** The native Windows face, plain and unforced; hierarchy comes from weight, size and uppercase tracking, not from a second font.

### Hierarchy
- **Display** (700, 34px, 1.1): the game page title, on the hero.
- **Title** (700, 26px, 1.15): Continue-playing banner game name.
- **Headline** (600, 20px): Settings "Accounts" heading; 16px/700 for the Settings panel title.
- **Body** (400, 14px): default text; 13px for hints, 15px/600 for the cover fallback title, 16px for empty states.
- **Label** (600, 12px, 0.08em tracking, uppercase): section labels ("Library 613", Settings sub-headings, game page tile and panel headings, "Continue playing"); 11px/600 for chips and the banner kicker. Counts use tabular numerals.

### Named Rules
**The Eleven Pixel Floor Rule.** No UI text below 11px. (Observed in code, and the `.impeccable/config.json` waivers say the real UI keeps text at 11px or more; not an owner statement.)

## Layout

A full-window column: toolbar, optional notices, then a content area that fills the rest. The library is a **virtualised grid** of fixed 180x270 covers with a 16px gap, columns centred in the available width, a 24px page gutter on bars and notices, and the Continue banner (180px high) plus section label scrolling with the covers as the grid header. Only rows near the viewport exist in the DOM. Because of that, card size and gap are constants in code, not fluid.

Settings is a modal panel (max 760px wide, 24px inset from the window) with a 190px section list on the left; under 640px wide it stacks, nav on top. The game page covers the whole content area (the library stays mounted underneath, hidden, so its scroll position survives): a column capped at 1360px with a 24px gutter, holding the 300px hero, a tab row (Overview · DLC · Saves · Installation; Launch options joins with its feature), and on Overview a row of three equal fact-tile slots (16px gap; every tile is one head line and one value line, so all tiles are the same height) and a 1.7fr / 1fr About / Details grid.

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
36px high, `glass` fill, `glass-line` border, `text-2` text, 6px icon gap. Selected (`aria-pressed`) = `glass-on` fill, `glass-on-line` border, `text`. Store filters are icon-only chips with the real logo. The settings chip carries an 8px amber dot, ringed in `bg`, when attention is needed.
- **Active filter chip:** a filter turned on in the Filters popover also shows in the toolbar, after Installed, as a selected chip with its name and a `text-3` 11px ✕ (8px gap); clicking it clears exactly that filter (`removeChip` in `src/lib/filters.ts`).
- **Filters chip:** a 15px sliders icon plus "Filters"; opens the Filters popover.
- **Downloads chip:** a 36px icon chip (17px download arrow), shown only while `poll` reports an install or update. A 2px `accent-2` ring inset 3px inside its edge fills clockwise with the average progress (a conic gradient masked to a ring; it changes with each 2 s poll, never per frame). `aria-label` "Downloads, 42%".

### Popovers (Filters, Downloads)
`surface-2` fill, `glass-on-line` border, 12px radius, modal shadow, 44px below their chip (Filters left-aligned, Downloads right-aligned). Keyboard and pointer (`src/lib/popover.ts`): focus moves to the first control on open, Tab and Shift+Tab stay inside, Escape closes and returns focus to the chip, a click outside closes. Mouse Back/Forward are ignored while one is open, as with Settings. Ctrl K closes it and focuses the search.
- **Filters:** 360px wide, 12px padding. Groups under 11px uppercase `text-3` headings (10px above, 6px below; the first 2px): Status (Not installed, Update available, Downloading) and Has (On both stores). Filters in one group widen each other, groups narrow. Toggles are 30px pills, 13px, `glass` fill and `glass-line` border, on = `glass-on` / `glass-on-line` / `text`, `aria-pressed`. A footer under a `glass-line` rule (12px above, 10px padding) holds Clear (13px `text-2` text button) on the left; the right slot is kept for collections. Playtime and how-long-to-beat ranges (Task 27), Genre (once genres are cached), DLC and Cloud saves join when their data exists.
- **Downloads:** 380px wide, 6px padding. "Downloading" (Epic installs and updates: title 600, Legendary's progress text in `text-3` 12px, a 6px `accent` bar, a 32px round glass Cancel) then "In Steam" (Steam's own downloads, read-only, "In Steam · 1.2 of 4 GB"). Each row has its 16px store logo; rows are split by `glass-line`. Pause, "Up next" and reordering come with the download queue (Task 24).

### Cards / Containers
- **Cover card:** 180x270, 10px radius. While the cover loads it shows `surface-2`, then the cover's average colour, then the image fading in over 150ms; the title shows only when there is no art. Store logo top-right. Installed = full colour; not installed = `saturate(0.5) brightness(0.75)` plus a bottom-left glass "Install" chip (owner's option C). Running = "Playing" green badge and green ring; installing = a 6px progress strip.
- **Hover:** lifts, white ring, reveals title and a small Play (installed) or Install (not) pill. The overlay is absent (not transparent) when idle, to keep scrolling cheap.
- **Continue banner:** 14px radius, 180px, hero art cover-fit, left-to-right dark scrim, title and white Play pill bottom-left.
- **Notice:** `glass-strong`, 12px radius, 44px min height, dismiss button; error variant has a red border. Used for Epic logged-out (only on state change), "Playing" and errors.
- **Account card (Settings, Accounts first):** one per source (Steam, Epic Games), per mockup v5: `glass` fill, `glass-line` border, 12px radius, 14px 16px padding, 14px gap; a 40px store-logo tile, then the name, a status line (a green or amber `●`, "Logged in as …"), and Log in / Log out buttons.

### Inputs / Fields
36px high, 10px radius, `glass` fill, `glass-line` border, `text-3` placeholder, 2px focus outline. The search field's border brightens to `glass-on-line` on focus. It carries a `Ctrl K` hint at the right (11px, `text-2`, 5px radius, `glass-on-line` border), hidden while focused or filled; Ctrl K focuses it (not while Settings is open). Secrets are shown only as "Saved ✓" with Replace and Remove (owner decision).

### Navigation
Top bar only, left to right (mockup v7): search, quick chips (All, Steam, Epic, Installed), active filter chips, Filters, spacer, sort (Group joins it with Task 27), Downloads (only while something downloads), settings. The bar sits above the content (`z-index: 5`) so its popovers hang over the grid and the game page. Settings has a left section list (active = white 10% fill) in the modal. There is no sidebar.

### Toggle
40x22 pill switch, white 18% off, `accent` on, white knob; `role="switch"`.

### Game page
`src/lib/Detail.svelte`, per the owner-approved mockup `game-page.html` with the owner's change (no size tile).
- **Tint:** the window's ambient layer switches to this game's hero art (its cover when it has none) while the page is open, and back on leaving. Same `Ambient.svelte`, so the Appearance on/off setting applies.
- **Hero:** 300px, 14px radius, banner shadow; art cover-fit at 30% from the top, fading in. A gradient darkens the top (`glass`) for the back button and the bottom (`scrim`) for the text. Back button top-left: `glass-strong` with `glass-line`, 10px radius (no backdrop blur: the hero scrolls). Bottom-left: Display title, then a meta line in `text-2` (a logo per store that owns it · "Installed on <Store>" (the chosen copy's, else the first installed copy's) or "Not installed" · developer · year, `·` in `text-3`). Install progress, while it runs, is a 6px `accent` bar under the meta line.
- **Actions** (bottom-right): a 44px round glass `⋯` button and the large white pill (Play, Install, disabled "Playing", or busy text; the busy pill is `aria-disabled` so it stays focusable). A game owned on both stores gets the **split pill** instead: one 44px white pill in two segments. The left holds the chosen store's 18px logo and a 14px chevron (`aria-haspopup="menu"`, `aria-expanded`, `aria-label="Choose launcher, now <Store>"`); a 1px divider at `bg` 18% inset 10px; the right is Play (play icon) or Install (download icon) for the chosen copy. The choice is not stored; it resets to the default copy when another game opens. A not-installed Epic copy shows "Checking size…", then "27.3 GB download", in `text-2` beside Install. Epic uninstall asks inline (text, then Uninstall in `danger-text` and Cancel, both round glass); Steam asks in its own dialog. An Epic Play that finds a cloud save conflict asks the same way before launching: "Local save is newer than the cloud save." (or "Cloud save is newer than the local save."), then Keep cloud, Keep local and Cancel (round glass); focus goes to Cancel. It never picks a save by itself. A Play from a card that hits a conflict opens the game page, which asks.
- **Play with menu:** same popover look as the ⋯ menu, 290px wide, under the split pill. A "Play with" Label heading, then one `menuitemradio` per copy: store logo, name, and a `text-2` 12px sub-line ("Installed · 27.3 GB", "Not installed", or "Not installed · 4.3 GB download" for Epic); a check mark marks the chosen copy. Keyboard as the ⋯ menu: focus opens on the chosen item, arrows/Home/End move, Escape returns focus to the switcher, Tab closes, a click outside closes. Picking a copy closes the menu and returns focus to the switcher.
- **⋯ menu:** `surface-2` popover with a `glass-on-line` border, 12px radius and the modal shadow, hanging below the button. It acts on the chosen copy and keeps only quick actions: Open install folder (installed only), Uninstall (installed only, `danger-text`, last). "Play without updating" is on the Installation tab. Keyboard: focus moves to the first item on open, arrows/Home/End move, Escape closes and returns focus to the button, Tab closes, and a click outside closes. When an action swaps the buttons, focus moves to what replaced them (the busy pill, or the confirm's Cancel). The page focuses "← Library" on open; Back returns focus to the card or banner that opened it.
- **Tabs:** under the hero (16px above), a `role="tablist"` row with a `glass-line` bottom rule: Overview, DLC (with a count pill: 11px, `glass-on`, 999px), Saves, Installation. Launch options joins when Task 25 builds it. A tab with nothing for this game is hidden (no DLC; Saves unless the chosen copy is an installed Epic copy with cloud saves; Installation when the chosen copy isn't installed); the row itself always shows, Overview alone included, so a DLC list that arrives late doesn't move the page. Tabs use the Label style (12px/600 uppercase, 0.08em) in `text-2`, 10px 14px padding; the open tab is `text` with a 2px `text` underline. Roving tabindex: Left/Right (wrapping), Home and End move and open. A new game opens on Overview; a tab that disappears falls back to Overview.
- **Overview:** today's page: fact tiles, About and Details. When the last cloud save sync failed, one quiet line above them says so (`text-2`, 13px, ellipsis, full text in the tooltip); no banner.
- **DLC tab:** the DLC list in one `glass` panel (rows as described under About / Details below).
- **Saves tab:** one `glass` panel of rows like the Installation tab, for the chosen Epic copy. Sync cloud saves: a three-way chip group (Default, which follows Settings → Library; On; Off), the picked chip `glass-on` with a `glass-on-line` border (`aria-pressed`). Last sync: "Synced 5 min ago", "Not synced yet", or the last failure as one quiet line in `text-2` (ellipsis, full text in the tooltip). Sync now: a chip ("Syncing…" while it runs; disabled while the game runs or another action is busy); the newer save wins.
- **Installation tab:** one `glass` panel of rows (10px padding, `glass-line` between): Folder (path · size in `text-3` 12px, Open folder chip), Update available (Update and Play without updating chips; Epic only), Uninstall (`danger-text` chip; Epic asks inline in the hero, Steam in its own dialog). Acts on the chosen copy. Verify and Move come with Task 23.
- **Fact tiles:** Playtime, Last played, How long to beat, in three fixed slots. Every tile has the same two lines, a 24px head line (Label heading) and a single value line (20px/600 tabular, ellipsis), so a tile that answers late is exactly as tall as the ones already shown and the row never changes height. Whether the row exists is decided from the game alone (it has a Playtime or Last played tile), never from how long to beat, so it never collapses once shown and is never empty: Steam games always show Playtime, "Never played" included. Playtime's head line carries "Steam + Epic" at the right (`text-2`, 12px) when both copies counted, and Last played adds " · on Steam" after its value (`text-2`) for a multi-copy game. Each tile: `glass` fill, `glass-line` border, 12px radius. How long to beat shows "normally" as the value with " · N all" (completely) after it in `text-2`, per the mockup; all three figures are in the tooltip. Its head line carries a quiet "IGDB" credit link (`text-3`, underlined, 24px tall target, labelled "Data from IGDB") that opens igdb.com. A tile with nothing to say is absent. Steam's "Never played" is a real value and shows; Epic games get no playtime from Legendary, so their playtime is unknown and that tile hides. The tile text comes ready from Rust (`view::playtime_tile`, `view::last_played_tile`); the UI doesn't reword it.
- **About / Details:** `glass` panels, 16px padding, 12px radius, Label headings. Until the store details arrive, About shows four `glass-hover` skeleton lines and Details skeleton values for Genres, Developer, Publisher and Released; screenshot slots stay reserved (16:9, `surface`) while they download. About: plain-text description (line breaks kept) and up to three 16:9 screenshots. Details: a two-column `dl` (`text-2` terms) with Genres (small `glass-on` pills), Developer, Publisher, Released ("28 Nov 2016"; the meta line shows only the year), Size; each row is absent when unknown (the install folder is on the Installation tab). Below them, **Store pages** (one chip per store that owns the game) and **Info** (PCGamingWiki, SteamDB, ProtonDB; only when the Steam app id is known, else the row is absent). Details sits in the right column, or the left one when the game has no About. When the game has owned DLC, the **DLC tab** lists it, one 28px row per DLC: title, then "Installed"/"Not installed" in `text-2` 12px, then for an Epic DLC of an installed copy an Install or Uninstall (`danger-text`) link chip. Uninstall asks inline in the row ("Its files will be deleted." in `text-2`, then Uninstall in `danger-text` and Cancel chips; focus goes to Cancel). Steam DLC is listed only; Steam installs it itself. No DLC, no heading.
- **Link chips:** buttons, 28px high, 10px side padding, 999px radius, 13px text, a 14px logo (`currentColor`) with a 6px gap, `glass-hover` fill, `glass-line` border, `glass-on` fill on hover. They open the default browser; the UI sends only a link kind and Rust builds the URL (Epic uses the store page IGDB reported when it is on store.epicgames.com, else a store search by title).

## Do's and Don'ts

### Do:
- **Do** use the CSS custom properties in `src/app.css`; add a token before adding a raw value.
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
- Settings: frosted panel over the library; sections Accounts, Library, Downloads, Appearance, Storage, About. Do not show the Legendary version under Accounts.
- Ambient colour on/off: "off = neutral dark gradient".
- Secrets: IGDB Client Secret in Windows Credential Manager; the UI shows only "Saved ✓" with Replace and Remove.
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
