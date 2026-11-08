# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

A Tauri 2 desktop app for Windows 10 and 11. The UI is a webview with its own visual system, not native Windows controls. Linux may come later; nothing is built for it yet.

## Users

PC gamers on Windows who own games in more than one store (Steam, Epic Games, GOG, Amazon Games) and want one place to see, install and start all of them. Libraries range from a handful of games to many hundreds; the owner's Steam library alone has 679.

They open Guild to pick something to play, start it, and get out of the way. Less often, they install or update games, manage downloads, sort their library into collections, or connect a store account.

## Product Purpose

Guild puts every game a person owns, across stores, into one library: one grid, one card per game, one game page. A game owned in two stores appears once, and the person picks which store starts it.

Success means someone with games in several stores keeps only Steam installed and uses Guild for everything else: browsing, installing, updating, cloud saves and launching.

## Positioning

Two things set Guild apart from Playnite, GOG Galaxy and Heroic:

- **It replaces the store apps.** Epic, GOG and Amazon games install, update, verify, repair, move and uninstall from Guild itself, through bundled open-source tools (Legendary, gogdl, Nile), with Epic and GOG cloud saves synced around play. Only Steam's own client stays, because Steam games install and start through it.
- **It is fast and quiet.** The library scrolls smoothly at any size (the benchmark holds 0 slow frames at 4,000 and 15,000 px/s), works offline from its cache, and does network work in the background. Failures show as a dot in Settings, never as banners, spinners or status text on every start.

## Operating Context

- Runs on the player's own Windows PC, often started with Windows and kept in the tray. A notification says when a download finishes.
- Store logins: Steam by password or QR code in the Steam mobile app; Epic, GOG and Amazon through each store's own login page, after Guild offers to install the tool it needs.
- Optional services the player sets up themselves: an IGDB app (Twitch developer console) for how long to beat, and a SteamGridDB API key for covers of games they add.
- Game pages link out to the stores the game is owned in, PCGamingWiki, SteamDB and ProtonDB.

## Capabilities and Constraints

- **Library:** the full Steam library (installed or not) plus Epic, GOG and Amazon libraries and games the player adds. Sort by title, recent play, playtime or length; filter by store and install status; favorites, hidden games and collections.
- **Merging:** an Epic, GOG or Amazon copy joins a Steam copy only on an exact, one-to-one Steam appid match (from GOG GamesDB). Never on title alone. A duplicate card is acceptable; two different games shown as one is a failure.
- **Downloads:** one queue for Epic, GOG and Amazon that can be paused, resumed and reordered.
- **Game page:** description, screenshots, playtime, last played, how long to beat, store links, and a launcher switcher on the Play button when a game is owned in more than one store.
- **Privacy:** library, settings and play history stay on the PC. No Guild account, no usage data. Store logins and API keys live in Windows Credential Manager; Steam passwords and Steam Guard codes are never stored.
- **Reliability:** some store connections are unofficial and can break when a store changes. Guild fails quietly and keeps working from cache.
- **Languages:** English and Dutch. Guild follows the Windows language and can be switched in Settings.
- **Not in scope:** achievements, friends, chat, and a light theme.

## Brand Commitments

- **Name:** Guild, short for "Games Unified In one Library, Duh." Formerly Game Launcher (0.1.0).
- **Voice:** plain, direct sentences addressed to the player ("you"). Says what happened and what to do, without hype. No decorative hints, filler copy or labels that repeat each other.
- **Store identity:** real store logos (from Simple Icons) mark each copy. Credits for Tauri, Svelte, the store tools and the data sources stay in the README.

## Evidence on Hand

- Library screenshot: `docs/screenshot.png`.
- Public releases and download counts on GitHub (`TheBiemGamer/guild`).
- Scroll benchmark: `scripts/bench.ps1`, with the targets above.
- There are no testimonials, user counts, reviews or press. Do not invent any.
- Guild is not code-signed yet; SmartScreen warns on first install.

## Product Principles

1. **One library, never a wrong one.** Show every game once, but only merge what is provably the same game.
2. **Guild does the store's job.** If a store app is still needed for anything besides Steam, that is a gap to close.
3. **Quiet until it matters.** Background work stays in the background; problems show where the player will look for them, not in their face.
4. **Speed is a feature.** No change may make the library scroll, start or respond slower.
5. **The player's data is theirs.** It stays on their PC, secrets stay in Credential Manager, and nothing they built up is lost to an update.
