# Game Launcher

A Windows launcher that shows your Steam and Epic Games libraries as one library.

![The library grid](docs/screenshot.png)

## What it does

- **One library.** Steam and Epic games share one grid. A game you own in both stores becomes one card, and you pick which store launches it.
- **Your whole Steam library**, including games you haven't installed.
- **Epic games without the Epic Games Launcher:** install with a download queue (pause, resume, reorder), updates, cloud saves, verify, repair and move installs, and import of games you already installed.
- **Game pages** with the description, screenshots, playtime, last played, how long to beat, and links to the store, PCGamingWiki, SteamDB and ProtonDB.
- **Sort and filter** by store, install status, playtime and how long to beat.
- **Works offline** from its local cache.

## Requirements

- Windows 10 or 11
- [Steam](https://store.steampowered.com/about/), for Steam games
- [Legendary](https://github.com/derrod/legendary/releases), for Epic games. Put `legendary.exe` on your `PATH` or in `%LOCALAPPDATA%\Programs\legendary`.
- Optional, for how long to beat: a free Twitch developer application (see below)

## Install

Download the `x64-setup.exe` file from the [latest release](https://github.com/TheBiemGamer/game-launcher/releases/latest) and run it. It installs for your Windows user only, so it doesn't ask for admin rights.

The app isn't code-signed, so Windows SmartScreen may warn you the first time. Click **More info**, then **Run anyway**.

Updates download and install themselves. To build it yourself, see [DEVELOPMENT.md](DEVELOPMENT.md).

## Getting started

Open **Settings** with the gear button at the top right.

- **Steam:** click **Log in** next to Steam and sign in with your password, or scan the QR code with the Steam mobile app.
- **Epic Games:** click **Log in** next to Epic Games and sign in on the Epic page that opens. Its **Options** hold the install folder, cloud save sync, importing existing installs and syncing with the Epic Games Launcher.
- **How long to beat:** create an application in the [Twitch developer console](https://dev.twitch.tv/console/apps), then paste its client ID and secret under **Extra metadata**.

## Why is a game shown twice?

A Steam and an Epic copy merge only when [GOG GamesDB](https://gamesdb.gog.com) links the Epic game to that exact Steam app. Games with the same title but no such link stay separate cards.

## Your data

- Your library, settings, play history and cached art live in `%LOCALAPPDATA%\launcher\`. **Settings → Storage** clears the cache.
- Your Steam login token and IGDB secret are kept only in Windows Credential Manager. Your Steam password and Steam Guard codes are never stored. Legendary keeps the Epic login in its own folder.

## Services it connects to

- **Steam:** login and the Steam network, for owned games and download sizes
- **Epic Games:** through Legendary
- **GOG GamesDB:** links Epic games to their Steam versions
- **IGDB (via Twitch):** how long to beat, only if you add credentials
- **SteamGridDB:** covers for games you add yourself, only if you add an API key
- **Store image servers** (Steam, Epic, GOG): cover art and screenshots

Some of these connections are unofficial and can break when a store changes its API. When one fails, a dot appears on the Settings button.

## License

[GPL-3.0](LICENSE)
