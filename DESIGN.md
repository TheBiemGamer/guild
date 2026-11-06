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
  settings-panel:
    backgroundColor: "rgb(22 22 32 / 0.82)"
    rounded: "{rounded.panel}"
---

# Design System: Game Launcher

## Overview

**Creative North Star: UNCONFIRMED.** No owner document names a metaphor for the system. The owner named the look **"Ambient"** and asked for it to be "**sleek, modern and gradient**" (`docs/superpowers/specs/2026-11-05-ambient-redesign-design.md`, Why). Use "Ambient" as the working name only; do not treat it as a north star until the owner confirms one.

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
- **Signal Violet** (`accent` #7c5cff, hover `accent-2` #a48bff): the "on" state of toggles, links in Settings, the legacy `button.primary` and the progress bar on the game page. Never the main call to action; that is the white pill.

### Neutral
- **Ink Black** (`bg` #0e0e14): window base, text on white pills, the neutral gradient floor.
- **Night Surface** (`surface` #16161f) and **Raised Surface** (`surface-2` #1f1f2a): cover placeholder, default buttons, dropdown options. `line` (#2c2c3a) is their hover/border step.
- **Cloud White** (`text` #eef0f6), `text-2` (white 75%), `text-3` (white 50%), **Dusk Grey** (`muted` #8d90a3): primary text, secondary text, hints and placeholders, and quiet metadata.
- **Glass set** (`glass`, `glass-strong`, `glass-hover`, `glass-line`, `glass-on`, `glass-on-line`): all bar, chip, search, notice and field fills and borders. Use these instead of inventing a new alpha.
- **Pill White** (`pill` #fff, hover #e4e6ee): the primary action only. The code writes these two as literals in `app.css` (`button.pill`), not as custom properties.

### Status
- **Play Green** (`play` #22c55e): running state only (the "Playing" badge, the running-card ring, the live dot). Today it also fills the game page's Play button; that page predates the redesign (see Components).
- **Amber** (`warn` #f5b942): Epic needs attention (settings dot, "Not logged in").
- **Red** (`danger` #ef4444): destructive actions and error notices.
- Settings also uses literal, untokenised tints: success text #4ade80, danger text #fca5a5, error text #fecaca, store tiles #1b2838 (Steam) and #2a2a2a (Epic). Treat these as house values until tokenised.

### Named Rules
**The One Bright Thing Rule.** The solid white pill is the only solid-bright element on a screen. Accent violet and green never compete with it.
**The Tint Comes From The Art Rule.** Chromatic colour on the library comes from the ambient layer. UI chrome stays neutral.

## Typography

**Display / Body / Label Font:** Segoe UI Variable Text (with Segoe UI, system-ui, sans-serif). One family only.

**Character:** The native Windows face, plain and unforced; hierarchy comes from weight, size and uppercase tracking, not from a second font.

### Hierarchy
- **Title** (700, 26px, 1.15): Continue-playing banner game name. The legacy game page title is 40px/700.
- **Headline** (600, 20px): Settings "Accounts" heading; 16px/700 for the Settings panel title.
- **Body** (400, 14px): default text; 13px for hints, 15px/600 for the cover fallback title, 16px for empty states.
- **Label** (600, 12px, 0.08em tracking, uppercase): section labels ("Library 613", Settings sub-headings, "Continue playing"); 11px/600 for chips and the banner kicker. Counts use tabular numerals.

### Named Rules
**The Eleven Pixel Floor Rule.** No UI text below 11px. (Observed in code, and the `.impeccable/config.json` waivers say the real UI keeps text at 11px or more; not an owner statement.)

## Layout

A full-window column: toolbar, optional notices, then a content area that fills the rest. The library is a **virtualised grid** of fixed 180x270 covers with a 16px gap, columns centred in the available width, a 24px page gutter on bars and notices, and the Continue banner (180px high) plus section label scrolling with the covers as the grid header. Only rows near the viewport exist in the DOM. Because of that, card size and gap are constants in code, not fluid.

Settings is a modal panel (max 760px wide, 24px inset from the window) with a 190px section list on the left; under 640px wide it stacks, nav on top. The game page covers the whole content area.

Spacing rhythm observed: 8 / 12 / 16 / 24 px. Controls on bars are 36px high.

## Elevation & Depth

Hybrid: tonal layering and translucency for chrome, soft black shadows for objects that sit over art. Depth over the ambient tint comes from glass fills plus a 1px light border, not from shadow.

### Shadow Vocabulary
- **Cover rest** (`0 4px 12px rgb(0 0 0 / 0.45)`): covers at rest.
- **Cover lift** (`0 16px 34px rgb(0 0 0 / 0.6), 0 0 0 2px rgb(255 255 255 / 0.85)`): hovered or focused cover, with a 4px rise and 1.03 scale. The white 2px ring is the hover outline the owner asked for.
- **Banner** (`0 10px 30px rgb(0 0 0 / 0.45)`).
- **Modal** (`0 30px 80px rgb(0 0 0 / 0.6)`) with `backdrop-filter: blur(24px)` on the Settings panel.

### Named Rules
**The No Halo Rule.** Shadows are black and directional. No coloured glow halos (owner anti-goal). The only coloured ring is the green running ring on a card.
**The Static Backdrop Rule.** Blur is applied once off-screen (canvas) or only on small non-scrolling surfaces. Do not put a live `backdrop-filter` or CSS blur on anything that scrolls. (Derived from the owner's scrolling anti-goal and the code comments in `Ambient.svelte`.)

## Shapes

Rounded, soft-cornered, never sharp: 10px for covers, chips, fields and rows; 8px for plain buttons; 12px notices and logo tiles; 14px banner and account cards; 18px modal; full pills for primary actions, toggles and the cover store logo (26px glass circle, top-right).

Borders are always thin and translucent (`glass-line`, 1px). Cover art is clipped to the card radius. Portrait art fills the cover (`cover`); landscape art stays whole (`contain`).

## Components

### Buttons
- **Primary (pill):** white fill, `bg` text, 700 weight, 999px radius, 9px 20px (small: 6px 14px, 12px). The only primary-action style.
- **Default:** `surface-2` fill, 8px radius, 8px 16px; hover steps to `line`; press scales to 0.97.
- **Glass:** `glass-on` fill with `glass-on-line` border, for secondary actions on glass.
- **Settings button:** 10px radius, white 10% fill, white 14% border; destructive variant uses red text, not a red fill.
- **Focus:** 2px `focus` outline, 2px offset, everywhere. Disabled is 60% opacity.

### Chips (filters, sort, settings button)
36px high, `glass` fill, `glass-line` border, `text-2` text. Selected (`aria-pressed`) = `glass-on` fill, `glass-on-line` border, `text`. Store filters are icon-only chips with the real logo. The settings chip carries an 8px amber dot, ringed in `bg`, when attention is needed.

### Cards / Containers
- **Cover card:** 180x270, 10px radius, `surface` fill with a title fallback while art loads. Store logo top-right. Installed = full colour; not installed = `saturate(0.5) brightness(0.75)` plus a bottom-left glass "Install" chip (owner's option C). Running = "Playing" green badge and green ring; installing = a 6px progress strip.
- **Hover:** lifts, white ring, reveals title and a small Play (installed) or Install (not) pill. The overlay is absent (not transparent) when idle, to keep scrolling cheap.
- **Continue banner:** 14px radius, 180px, hero art cover-fit, left-to-right dark scrim, title and white Play pill bottom-left.
- **Notice:** `glass-strong`, 12px radius, 44px min height, dismiss button; error variant has a red border. Used for Epic logged-out (only on state change), "Playing" and errors.
- **Account card (Settings):** 14px radius, white 5% fill, 40px store-logo tile.

### Inputs / Fields
36px high, 10px radius, `glass` fill, `glass-line` border, `text-3` placeholder, 2px focus outline. The search field's border brightens to `glass-on-line` on focus. Secrets are shown only as "Saved ✓" with Replace and Remove (owner decision).

### Navigation
Top bar only: search, chips, spacer, sort, settings. Settings has a left section list (active = white 10% fill) in the modal. There is no sidebar.

### Toggle
40x22 pill switch, white 18% off, `accent` on, white knob; `role="switch"`.

### Game page (legacy, to be redone)
`src/lib/Detail.svelte` still has the pre-Ambient look (opaque `bg`, 380px hero, green Play button, `surface` bars). The owner-approved target (spec "Game page") is a rounded hero, white Play/Install pill, a `⋯` menu, Playtime / Last played / How long to beat tiles, About and Details panels, and the same ambient tint. Build against the spec, not the current file.

## Do's and Don'ts

### Do:
- **Do** use the CSS custom properties in `src/app.css`; add a token before adding a raw value.
- **Do** keep scrolling at the benchmark level: `LAUNCHER_BENCH` 175 fps, 0 dropped frames over 613 games at 4,000 and 15,000 px/s (spec, Anti-goals).
- **Do** reserve space for late data (covers, store details, how-long-to-beat) so nothing jumps.
- **Do** hide a missing fact's tile or row instead of showing a dash.
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
- Epic install from the grid (option B): a small confirm popup next to the cover with size ("Checking size...") and destination; Steam opens its own dialog. (Popup not yet in code.)
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

- UNCONFIRMED: Creative North Star and overall voice/mood words (no owner doc states them).
- UNCONFIRMED: the exact radii, spacing steps and font sizes above are what the code uses, not owner-stated values.
- The game page and the Epic install popup are specified by the owner but not built yet; this file describes the code as of commit `e13f9e2`, plus the spec where marked.
- No renders were captured (the app was not launched).
