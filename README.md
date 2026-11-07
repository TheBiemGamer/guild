<p align="center">
  <img src="src-tauri/icons/128x128.png" width="96" height="96" alt="">
</p>

<h1 align="center">Guild</h1>

<p align="center">One library for all your PC games: Steam, Epic, GOG, Amazon and your own.</p>

<p align="center">
  <a href="https://github.com/TheBiemGamer/game-launcher/releases/latest"><img src="https://img.shields.io/github/v/release/TheBiemGamer/game-launcher" alt="Latest release"></a>
  <a href="https://github.com/TheBiemGamer/game-launcher/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/TheBiemGamer/game-launcher/ci.yml?branch=main&label=ci" alt="CI status"></a>
  <a href="https://github.com/TheBiemGamer/game-launcher/actions/workflows/release.yml"><img src="https://img.shields.io/github/actions/workflow/status/TheBiemGamer/game-launcher/release.yml?label=release" alt="Release build status"></a>
  <a href="https://github.com/TheBiemGamer/game-launcher/releases"><img src="https://img.shields.io/github/downloads/TheBiemGamer/game-launcher/total" alt="Downloads"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/TheBiemGamer/game-launcher" alt="License: GPL-3.0"></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D4" alt="Platform: Windows 10 and 11">
</p>

![The library grid](docs/screenshot.png)

## Features

- **One library.** Steam, Epic, GOG and Amazon games share one grid. A game you own in several stores is one card, and you pick which store launches it.
- **Your whole Steam library**, including games you haven't installed.
- **Epic games without the Epic Games Launcher:** install with a download queue (pause, resume, reorder), updates, cloud saves, launch options, verify, repair and move installs, and import of games you already installed.
- **GOG games without GOG Galaxy:** play, install, update, verify, repair, move and uninstall them and their DLC in the same download queue, and import games you already installed. Cloud saves sync around play, and GOG's own playtime shows with the time you play here.
- **Amazon Games without the Amazon Games app:** play, install, update, verify, repair, move and uninstall them in the same download queue, and import games you already installed.
- **Your own games:** add them to the library, with covers from SteamGridDB.
- **Game pages** with the description, screenshots, playtime, last played, how long to beat, and links to the store, PCGamingWiki, SteamDB and ProtonDB.
- **Sort and filter** by store, install status, playtime and how long to beat. Mark favorites, hide games and make your own collections.
- **Works offline** from its local cache.
- **Runs in the tray** if you like, starts with Windows, and shows a Windows notification when a download finishes.

## Download and install

Download the `x64-setup.exe` file from the [latest release](https://github.com/TheBiemGamer/game-launcher/releases/latest) and run it. It installs for your Windows user only, so it doesn't ask for admin rights. Guild runs on Windows 10 and 11.

Guild isn't code-signed, so Windows SmartScreen may warn you the first time. Click **More info**, then **Run anyway**.

Guild checks for new versions and installs one when you choose to. To build it yourself, see [DEVELOPMENT.md](DEVELOPMENT.md).

## Getting started

Open **Settings** with the gear button at the top right.

- **Steam:** click **Log in** next to Steam and sign in with your password, or scan the QR code with the Steam mobile app. Steam itself must be installed to play and install Steam games.
- **Epic Games:** click **Log in** next to Epic Games and sign in on the Epic page that opens. Its **Options** hold the install folder, cloud save sync, importing existing installs and syncing with the Epic Games Launcher.
- **GOG:** click **Log in** next to GOG and sign in on the GOG page that opens. Its **Options** hold the install folder, cloud saves and importing existing installs.
- **Amazon Games:** click **Log in** next to Amazon Games and sign in on the Amazon page that opens. Its **Options** hold the install folder and importing existing installs.
- **How long to beat:** create an application in the [Twitch developer console](https://dev.twitch.tv/console/apps), then paste its client ID and secret under **Extra metadata**.

Epic, GOG and Amazon games run through [Legendary](https://github.com/derrod/legendary), [gogdl](https://github.com/Heroic-Games-Launcher/heroic-gogdl) and [Nile](https://github.com/imLinguin/nile). **Settings → Accounts** installs each one when you ask (a pinned release, checked against its SHA-256). You can also use your own `legendary.exe` from your `PATH` or `%LOCALAPPDATA%\Programs\legendary`.

## FAQ

**Why is a game shown twice?**
An Epic, GOG or Amazon copy joins a Steam game only when [GOG GamesDB](https://gamesdb.gog.com) links it to that exact Steam app, which you own. Games with the same title but no such link stay separate cards, and so do Epic, GOG and Amazon copies of a game you don't own on Steam.

**Is my login safe?**
Your Steam login token, IGDB secret and SteamGridDB key are kept only in Windows Credential Manager, never in files. Your Steam password and Steam Guard codes are never stored. Legendary keeps the Epic login in its own folder, gogdl keeps the GOG login in `%LOCALAPPDATA%\Guild\data\gog`, and Nile keeps the Amazon login in `%LOCALAPPDATA%\Guild\data\amazon`.

**Do I need the store apps?**
For Steam games, yes: Guild starts, installs and uninstalls them through Steam. For Epic, GOG and Amazon games, no: Legendary, gogdl and Nile do the work.

**I used Game Launcher 0.1.0. What changed?**
It's the same app under a new name. Your library, settings, logins and Start with Windows carry over the first time Guild starts. The update installs Guild as a new app, so uninstall Game Launcher afterwards in Windows Settings → Apps.

## Languages

English and Nederlands. Guild follows your Windows language; **Settings** lets you pick one.

## Privacy and the services it talks to

Your library, settings, play history and cached art stay on your PC, in `%LOCALAPPDATA%\Guild\`. **Settings → Storage** clears the cache. Guild has no account and sends no usage data.

- **Steam:** login and the Steam network, for owned games and download sizes
- **Epic Games:** through Legendary
- **GOG:** login, and GOG's API for your owned games, their titles and art, your account name, new game builds, cloud save settings and the playtime GOG counted; downloads and cloud saves through gogdl. Play sessions timed here are never sent to GOG.
- **Amazon Games:** login, your owned games, downloads and updates, through Nile
- **GOG GamesDB:** links Epic, GOG and Amazon games to their Steam versions, and has the GOG and Amazon games' covers
- **IGDB (via Twitch):** how long to beat, only if you add credentials
- **SteamGridDB:** covers for games you add yourself, only if you add an API key
- **Store image servers** (Steam, Epic, GOG, Amazon): cover art and screenshots
- **GitHub:** new versions of Guild, and a store tool's official release when you ask Settings to install it

Some of these connections are unofficial and can break when a store changes its API. When one fails, a dot appears on the Settings button.

## Built with

- [Tauri](https://tauri.app) and [Svelte](https://svelte.dev)
- [Legendary](https://github.com/derrod/legendary) for Epic, [gogdl](https://github.com/Heroic-Games-Launcher/heroic-gogdl) for GOG and [Nile](https://github.com/imLinguin/nile) for Amazon (GPL-3.0, downloaded from their official releases only when you ask)
- [Heroic Games Launcher](https://heroicgameslauncher.com), for the research on running the store tools
- [GOG GamesDB](https://gamesdb.gog.com), [IGDB](https://www.igdb.com) and [SteamGridDB](https://www.steamgriddb.com) for game data and art
- [Simple Icons](https://simpleicons.org) for the store logos

## Contributing

Bug reports are welcome: **Settings → Report a problem** opens one with your app and Windows versions filled in. To build Guild and run the checks, see [DEVELOPMENT.md](DEVELOPMENT.md).

## License

[GPL-3.0](LICENSE)
