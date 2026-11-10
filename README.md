<p align="center">
  <img src="src-tauri/icons/128x128.png" width="96" height="96" alt="">
</p>

<h1 align="center">Guild</h1>

<p align="center"><b>G</b>ames <b>U</b>nified <b>I</b>n one <b>L</b>ibrary, <b>D</b>uh.</p>

<p align="center">
  <a href="https://github.com/TheBiemGamer/guild/releases/latest"><img src="https://img.shields.io/github/v/release/TheBiemGamer/guild" alt="Latest release"></a>
  <a href="https://github.com/TheBiemGamer/guild/releases"><img src="https://img.shields.io/github/downloads/TheBiemGamer/guild/total" alt="Downloads"></a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20Linux-0078D4" alt="Platform: Windows and Linux">
</p>

Guild puts your Steam, Epic, GOG, Amazon and EA games in one grid, next to any game you add yourself. A game you own in two stores shows up once, and you pick which store starts it.

![The library grid](docs/screenshot.png)

## What it does

Your whole Steam library shows up, installed or not. Epic, GOG and Amazon games install, update, verify and uninstall from Guild itself, through one download queue you can pause and reorder, so you don't need their launchers. Epic, GOG and EA cloud saves sync before and after you play. EA support is experimental: it runs on [Maxima](https://github.com/ArmchairDevelopers/Maxima), which is still early, on Windows and Linux. On Windows, installing or importing an EA game asks once for administrator permission, for the game's own setup. You can import games you already installed with another launcher. On Linux, Windows games run through [umu-launcher](https://github.com/Open-Wine-Components/umu-launcher) and Proton, each in its own Wine prefix, and Settings → Wine installs and manages Proton and the prefixes.

Each game has a page with its description, screenshots, your playtime, when you last played it, how long it takes to beat, and links to its store page, PCGamingWiki, SteamDB and ProtonDB.

You can sort by title, recent play, playtime or length, filter by store and install status, mark favorites, hide games and make your own collections. Guild works offline from its cache, can start when you log in and sit in the tray, and tells you when a download is done.

## Install

### Windows

Download the file ending in `x64-setup.exe` from the [latest release](https://github.com/TheBiemGamer/guild/releases/latest) and run it. It installs for your Windows user only, so it doesn't ask for admin rights.

Guild isn't code-signed yet, so Windows SmartScreen may warn you the first time. Click More info, then Run anyway. After that, Guild tells you when there's a new version and installs it when you say so.

### Linux

Download the file ending in `.AppImage` from the [latest release](https://github.com/TheBiemGamer/guild/releases/latest), make it executable with `chmod +x`, and run it. It updates itself the same way as on Windows.

The `.deb` is for Debian and Ubuntu. It doesn't update itself, so install new versions from the releases page.

On Arch, you can build a package from the release with the PKGBUILD in this repository:

```sh
cd packaging/aur
updpkgsums
makepkg -si
```

Guild needs Python 3 for the Linux builds of Legendary and gogdl, and a keyring such as GNOME Keyring or KWallet to save your logins.

## Getting started

Open Settings with the gear at the top right. Under Accounts, log in to the stores you use:

- Steam: your password, or the QR code in the Steam mobile app. Steam itself has to be installed to play Steam games.
- Epic Games, GOG and Amazon Games: Guild first offers to install the small open-source tool each one needs ([Legendary](https://github.com/derrod/legendary), [gogdl](https://github.com/Heroic-Games-Launcher/heroic-gogdl), [Nile](https://github.com/imLinguin/nile)), then opens the store's own login page. Each store's Options holds its install folder and import.
- EA (experimental): the same, with [Maxima](https://github.com/TheBiemGamer/Maxima/tree/guild) as the tool. On Linux, Maxima runs EA games with its own Proton and Wine prefix, not the ones in Settings → Wine.

For how long a game takes to beat, make a free app in the [Twitch developer console](https://dev.twitch.tv/console/apps) and paste its client ID and secret under Settings → Accounts. For covers of games you add yourself, paste a free [SteamGridDB](https://www.steamgriddb.com) API key there too.

## FAQ

**Why is a game shown twice?**
An Epic, GOG, Amazon or EA copy only joins your Steam copy when [GOG GamesDB](https://gamesdb.gog.com) links it to that exact Steam app. Same title alone isn't enough, because that's how two different games end up merged.

**Where do my logins go?**
Your Steam token, IGDB secret and SteamGridDB key are stored in your system's keyring: Windows Credential Manager on Windows, and GNOME Keyring, KWallet or another Secret Service keyring on Linux. They never go in a file. Guild never stores your Steam password or Steam Guard codes. Legendary, gogdl, Nile and Maxima keep their own logins in their own folders.

**Do I still need the store apps?**
Only Steam. Guild installs and starts Steam games through Steam. Epic, GOG, Amazon and EA games don't need their apps.

**I had Game Launcher 0.1.0. What happened to it?**
That was Guild's old name. Your library, settings and logins move over the first time Guild starts. The update installs Guild next to the old app, so remove Game Launcher afterwards in Windows Settings → Apps.

## Languages

English and Nederlands. Guild follows your system language, and you can change it under Settings → General.

## Privacy

Your library, settings and play history stay on your computer, in `%LOCALAPPDATA%\Guild` on Windows and `~/.local/share/Guild` on Linux. Guild has no account of its own and sends no usage data. It talks to:

- Steam, Epic, GOG, Amazon and EA, for your logins, your games, downloads and cloud saves
- GOG GamesDB, to match games across stores and for some covers
- the stores' image servers, for covers and screenshots
- IGDB and SteamGridDB, only if you add their keys
- GitHub, to check for Guild updates, and to download a store tool, umu-launcher or a Proton build when you ask for it

Some of these connections aren't official and can break when a store changes something. When a store login stops working, a dot shows on the Settings button.

## Thanks

Guild is built on [Tauri](https://tauri.app) and [Svelte](https://svelte.dev), with help from [Claude Code](https://claude.com/claude-code). Epic, GOG, Amazon and EA support exists because of [Legendary](https://github.com/derrod/legendary), [gogdl](https://github.com/Heroic-Games-Launcher/heroic-gogdl), [Nile](https://github.com/imLinguin/nile) and [Maxima](https://github.com/ArmchairDevelopers/Maxima), and how to drive them came from reading [Heroic Games Launcher](https://heroicgameslauncher.com). Windows games run on Linux through [umu-launcher](https://github.com/Open-Wine-Components/umu-launcher), [GE-Proton](https://github.com/GloriousEggroll/proton-ge-custom) and [Proton-CachyOS](https://github.com/CachyOS/proton-cachyos). Game data and art come from [GOG GamesDB](https://gamesdb.gog.com), [IGDB](https://www.igdb.com) and [SteamGridDB](https://www.steamgriddb.com). The store and IGDB logos are from [Simple Icons](https://simpleicons.org), the SteamGridDB logo from [selfh.st icons](https://github.com/selfhst/icons) (CC BY 4.0).

## Contributing

Found a bug? Settings → About → Report a problem opens a GitHub issue with your Guild and system versions filled in. To build Guild yourself, see [DEVELOPMENT.md](DEVELOPMENT.md). To contribute, see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Copyright (C) 2026 TheBiemGamer

[GPL-3.0](LICENSE)
