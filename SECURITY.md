# Security

## Supported versions

Only the latest release gets security fixes. Guild updates itself, so the fix reaches you through the update prompt. The `.deb` doesn't update itself: install the new version from the [releases page](https://github.com/TheBiemGamer/guild/releases).

## Reporting a vulnerability

Report it privately through [GitHub's vulnerability reporting](https://github.com/TheBiemGamer/guild/security/advisories/new). Don't open a public issue for it.

Say which Guild version and OS you found it on, what an attacker can do with it, and how to reproduce it. I'll reply in the advisory, and credit you in the release notes unless you'd rather not.

Guild is maintained by one person, so a fix can take a few days.

## What counts

These are the problems I most want to hear about:

- A secret leaving the OS keyring: the Steam refresh token, the IGDB secret or the SteamGridDB API key showing up in a file, a log, an error message or the UI.
- A way to make Guild run a store tool (Legendary, gogdl, Nile or umu-launcher) or a Proton build that fails its hash check.
- A way to get an update installed that isn't signed with Guild's updater key.
- Data from a store, GOG GamesDB, IGDB or SteamGridDB that makes Guild run code or write files outside its own folders.

Bugs in Legendary, gogdl, Nile, umu-launcher or Proton themselves belong with those projects. If you're not sure where a problem belongs, report it here.
