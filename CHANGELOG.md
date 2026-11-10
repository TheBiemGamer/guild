# Changelog

## Unreleased

### New

- EA games, through Maxima: log in, then install, update, uninstall, import and play your EA games from Guild, on Windows and Linux. EA is marked Experimental, because Maxima is still early.

## 0.2.0, 2026-11-09

### New

- Guild runs on Linux, as an AppImage or a .deb. Steam works from a regular or a Flatpak install, and your Steam login and API keys are kept in your desktop's keyring.
- On Linux, Epic, GOG, Amazon and added Windows games run through umu-launcher and Proton, each game in its own Wine prefix. GOG installs a game's Linux version when GOG has one.
- Settings → Wine on Linux: install umu-launcher, GE-Proton and Proton-CachyOS, choose the default Proton, and open, configure or remove each game's prefix. A game's Launch tab can pick its own Proton.
- Uninstalling a Windows game on Linux keeps its prefix, and with it your saves, unless you tick the box to remove it.
- Installing an Epic, GOG or Amazon game opens a panel with the download and disk size, the folder it goes to and the space left on that drive. Pick another folder, choose which DLC to install, and for GOG the language and the Linux or Windows version.
- Importing an installed Epic or Amazon game lets you search for it by name.

### Changed

- Install is turned off when the drive doesn't have room for the game.
- The folder you install a game to becomes that store's install folder.
- With no Epic install folder set, games go to Legendary's own default folder, and the install panel shows that path.
- In a menu, typing a letter jumps to the first item that starts with it.
- Escape closes only the topmost menu or popup.
- Hints on buttons are tooltips now, and they also show when you reach the button with the keyboard. A cut-off path or message shows in full when you hover it.
- While Settings or another dialog is open, screen readers and the keyboard stay inside it.
- The mouse Back and Forward buttons do nothing while a menu or popup is open.

### Fixed

- A game shows as installed by the time its Ready to play notification appears.

## 0.1.1, 2026-11-08

### New

- The Manage tab on a game page groups its actions under Folder, Files and Saves.
- An empty library offers to open Settings or add a game. A search or filter that matches nothing says so and lets you clear it.
- Settings shows the folder new games install to by default.

### Changed

- The app is now called Guild. Your library, settings, logins and Start with Windows carry over the first time it starts.
- Updating from 0.1.0 installs Guild as a new app. Afterwards, uninstall Game Launcher in Windows Settings → Apps; your library stays.
- Settings lists each service as a row you can fold open, with the IGDB and SteamGridDB logos.
- Report a problem opens an issue on the Guild repository.
- Plainer wording throughout, and Dutch that reads naturally.

### Fixed

- You can't uninstall a game while it runs, or start a game while another one is still uploading its cloud saves.
- GOG, Amazon and added games are tracked by the process Guild started, so another program with the same name no longer counts as the game running.
- Legendary, gogdl and Nile close with Guild, even when Guild is ended from Task Manager. Quitting during a download pauses the queue and keeps it for next time.
- A slow library rescan no longer overwrites the result of a newer one.
- Hovering over covers no longer starts Legendary, gogdl or Nile for each game.
- Clearing the cache no longer forgets which cloud saves are in sync.
- Last played counts by your local calendar day.
- Removing an added game deletes only that game's own folder.
- Search has a label and a visible focus ring, and covers tell screen readers which game they open.

## 0.1.0, 2026-11-07

### New

- Your Steam and Epic Games libraries in one grid. A game you own in both stores is one card, and you pick which store launches it.
- Your whole Steam library, including games you haven't installed, after logging in with your password or a QR code.
- Epic games through Legendary, without the Epic Games Launcher: install, update, verify, repair and move games, and import games you already installed.
- A download queue for Epic games that you can pause, resume and reorder.
- Cloud saves and per-game launch options for Epic games.
- GOG games in the same library, through gogdl: play, install, update, verify, repair, move and uninstall them and their DLC in the same download queue, and import games you already installed. Cloud saves, launch options and GOG's playtime too.
- Amazon Games in the same library, through Nile: play, install, update, verify, repair, move and uninstall them in the same download queue, and import games you already installed. Launch options too.
- Game pages with the description, screenshots, playtime, last played, how long to beat, and links to the store, PCGamingWiki, SteamDB and ProtonDB.
- Sort and filter by store, install status, playtime and how long to beat.
- Favorites, hidden games and your own collections.
- Add your own games, with covers from SteamGridDB.
- Start with Windows, keep running in the tray, and get a notification when a download finishes.
- Works offline from its local cache.
- Settings → About → Report a problem opens a GitHub bug report with your app and Windows versions filled in.
- A Windows installer, and updates that install themselves.
