<p align="center">
  <img src="src-tauri/icons/128x128.png" width="96" height="96" alt="">
</p>

<h1 align="center">Guild</h1>

<p align="center"><b>G</b>ames <b>U</b>nified <b>I</b>n one <b>L</b>ibrary, <b>D</b>uh.</p>

<p align="center">
  <a href="https://github.com/TheBiemGamer/game-launcher/releases/latest"><img src="https://img.shields.io/github/v/release/TheBiemGamer/game-launcher" alt="Latest release"></a>
  <a href="https://github.com/TheBiemGamer/game-launcher/releases"><img src="https://img.shields.io/github/downloads/TheBiemGamer/game-launcher/total" alt="Downloads"></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D4" alt="Platform: Windows 10 and 11">
</p>

Guild puts your Steam, Epic, GOG and Amazon games in one grid, next to any game you add yourself. A game you own in two stores shows up once, and you pick which store starts it.

![The library grid](docs/screenshot.png)

## What it does

Your whole Steam library shows up, installed or not. Epic, GOG and Amazon games install, update, verify and uninstall from Guild itself, through one download queue you can pause and reorder, so you don't need their launchers. Epic and GOG cloud saves sync before and after you play. You can import games you already installed with another launcher.

Each game has a page with its description, screenshots, your playtime, when you last played it, how long it takes to beat, and links to its store page, PCGamingWiki, SteamDB and ProtonDB.

You can sort by title, recent play, playtime or length, filter by store and install status, mark favorites, hide games and make your own collections. Guild works offline from its cache, can start with Windows and sit in the tray, and tells you when a download is done.

## Install

Download the file ending in `x64-setup.exe` from the [latest release](https://github.com/TheBiemGamer/game-launcher/releases/latest) and run it. It installs for your Windows user only, so it doesn't ask for admin rights.

Guild isn't code-signed yet, so Windows SmartScreen may warn you the first time. Click More info, then Run anyway. After that, Guild tells you when there's a new version and installs it when you say so.

## Getting started

Open Settings with the gear at the top right. Under Accounts, log in to the stores you use:

- Steam: your password, or the QR code in the Steam mobile app. Steam itself has to be installed to play Steam games.
- Epic Games, GOG and Amazon Games: Guild first offers to install the small open-source tool each one needs ([Legendary](https://github.com/derrod/legendary), [gogdl](https://github.com/Heroic-Games-Launcher/heroic-gogdl), [Nile](https://github.com/imLinguin/nile)), then opens the store's own login page. Each store's Options holds its install folder and import.

For how long a game takes to beat, make a free app in the [Twitch developer console](https://dev.twitch.tv/console/apps) and paste its client ID and secret under Services. For covers of games you add yourself, paste a free [SteamGridDB](https://www.steamgriddb.com) API key there too.

## FAQ

**Why is a game shown twice?**
An Epic, GOG or Amazon copy only joins your Steam copy when [GOG GamesDB](https://gamesdb.gog.com) links it to that exact Steam app. Same title alone isn't enough, because that's how two different games end up merged.

**Where do my logins go?**
Your Steam token, IGDB secret and SteamGridDB key are stored in Windows Credential Manager, not in a file. Guild never stores your Steam password or Steam Guard codes. Legendary, gogdl and Nile keep their own logins in their own folders.

**Do I still need the store apps?**
Only Steam. Guild installs and starts Steam games through Steam. Epic, GOG and Amazon games don't need their apps.

**I had Game Launcher 0.1.0. What happened to it?**
That was Guild's old name. Your library, settings and logins move over the first time Guild starts. The update installs Guild next to the old app, so remove Game Launcher afterwards in Windows Settings → Apps.

## Languages

English and Nederlands. Guild follows your Windows language, and you can change it under Settings → General.

## Privacy

Your library, settings and play history stay on your PC in `%LOCALAPPDATA%\Guild`. Guild has no account of its own and sends no usage data. It talks to:

- Steam, Epic, GOG and Amazon, for your logins, your games, downloads and cloud saves
- GOG GamesDB, to match games across stores and for some covers
- the stores' image servers, for covers and screenshots
- IGDB and SteamGridDB, only if you add their keys
- GitHub, to check for Guild updates and to download a store tool when you ask for it

Some of these connections aren't official and can break when a store changes something. When a store login stops working, a dot shows on the Settings button.

## Thanks

Guild is built on [Tauri](https://tauri.app) and [Svelte](https://svelte.dev), with help from [Claude Code](https://claude.com/claude-code). Epic, GOG and Amazon support exists because of [Legendary](https://github.com/derrod/legendary), [gogdl](https://github.com/Heroic-Games-Launcher/heroic-gogdl) and [Nile](https://github.com/imLinguin/nile), and how to drive them came from reading [Heroic Games Launcher](https://heroicgameslauncher.com). Game data and art come from [GOG GamesDB](https://gamesdb.gog.com), [IGDB](https://www.igdb.com) and [SteamGridDB](https://www.steamgriddb.com), and the store logos from [Simple Icons](https://simpleicons.org).

## Contributing

Found a bug? Settings → About → Report a problem opens a GitHub issue with your app and Windows versions filled in. To build Guild yourself, see [DEVELOPMENT.md](DEVELOPMENT.md).

## License

[GPL-3.0](LICENSE)
