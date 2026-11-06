# One library across stores — design

Date: 2026-11-06. Branch: `feat/tauri-rewrite`, building on the ambient redesign (plan `docs/superpowers/plans/2026-11-05-ambient-redesign.md`).

## Why

The owner has games on both Steam and Epic, and today the launcher treats them as separate worlds:

- **Duplicates.** A game owned in both stores shows up twice.
- **Steam's list is incomplete.** Only installed Steam games appear, so most of the owner's 679 Steam games are invisible.
- **Epic pages are thinner than Steam pages.** Epic games get no "How long to beat" and no useful outside links, because nothing links an Epic game to IGDB or to a Steam app id.

This project makes the launcher feel like **one library**:

- every game the owner owns appears once;
- the game page tells them where they own it, lets them pick which store to play it through, and links to the store pages and the sites they use.

## Goals

1. **Full Steam library.** Show every Steam game the owner owns, installed or not, after a one-time QR login.
2. **Install from the launcher.** A not-installed Steam game can be installed from the launcher; Steam does the work.
3. **One entry per game.** A game owned on Steam and Epic is one card and one game page.
4. **Choose the launcher.** On the game page, the Play button has a launcher switcher to choose Steam or Epic for that game.
5. **Epic games are as rich as Steam games.** When their Steam twin is known, Epic games get How long to beat and the same exact links.
6. **Links on the game page.** A link to the store page of each store the game is owned in, plus PCGamingWiki, SteamDB and ProtonDB, each with an icon.

## Non-goals

- **Other stores.** No GOG, Xbox or other stores as libraries (GOG GamesDB is used only as a lookup service).
- **Other links.** No links beyond those six: no HowLongToBeat, Nexus Mods, Steam guides or IGDB chip. The owner removed these explicitly. The existing small "IGDB" credit in the How long to beat tile stays: IGDB attribution is required.
- **Merging by title alone.**
- **Cloud saves, achievements, friends or chat.**
- **Remembering a per-game launcher choice across visits.** See "Choosing the launcher" for the default rule.

## Anti-goals (a failure even if it technically works)

- **A wrong merge.** Two different games shown as one is worse than a duplicate card. Only merge on an exact id match.
- **A wrong "How long to beat".** Showing another game's times is worse than showing no tile.
- **Kicking the owner's Steam client off.** The test showed the desktop Steam client stays signed in. Any change to how we log on must keep it that way.
- **Leaking the Steam login.** The refresh token gives full account access. It must never be in SQLite, files, logs, the UI or error strings.
- **Losing the owner's settings or launch history** because the database file was bumped.
- **A slower library.** The existing benchmark targets must still hold: 0 slow frames at 4000 and 15000 px/s, idle CPU around today's 0.03%. Network work must never block startup or scrolling.

## Constraints

- **Platform.** Windows only. Rust (Tauri 2) backend, Svelte 5 frontend.
- **Shared conventions.** Follow the existing patterns and DESIGN.md tokens. The app must keep working offline from its cache.
- **Secrets.** Like the IGDB client secret, the Steam refresh token lives only in Windows Credential Manager (service `dev.noah.gamelauncher`, a new user name such as `steam-refresh-token`).
  - It is sent only to Steam: the CM logon and Steam's IAuthenticationService.
  - The launcher may decode the token's JWT payload in memory for its `sub` (SteamID) and `exp`. It must never print or store the token.
- **Dependencies.** The owner approved `steam-vent` for Steam, with the Rust-native preference stated explicitly. The owner also approved the Rust crate `qrcode` (2026-11-06), used with `default-features = false`. Any other new crate needs the owner's OK first. So does downloading icon files.
- **No attribution lines** in commits.
- **Database.**
  - Today's database comment says: the next schema change must copy `settings` and `launches` from the old file, or add real migrations, instead of just bumping `library-v3.sqlite`. This project will change the schema, so it must do one of those.
  - Launch history is keyed by game key (`steam-<appid>`, `epic-<app_name>`). That history must keep counting for merged games.

## What the test (spike) proved, 2026-11-06

The spike is throwaway code in the session scratchpad. Re-implement it; don't copy it.

**What the owner's run showed:**
- `steam-vent` from Codeberg `main` (0.6.0, unreleased) builds on Windows with this machine's toolchain (rustc 1.97, CMake and NASM present).
- **The QR steps go over HTTPS, so no fork of steam-vent is needed:**
  - `POST https://api.steampowered.com/IAuthenticationService/BeginAuthSessionViaQR/v1/` with the form field `input_protobuf_encoded` = base64 of the protobuf request. The response is the raw protobuf, and the `x-eresult` header is `1` on success.
  - The request uses `website_id "Client"` and `device_details { device_friendly_name, platform_type = SteamClient }`.
  - The response has `challenge_url` (an `https://s.team/…` link, drawn as the QR), `client_id`, `request_id` and `interval` (5 s).
  - The client then calls `PollAuthSessionStatus { client_id, request_id }` the same way every `interval`.
  - Each poll handles `new_client_id`, `new_challenge_url` (redraw the QR) and `had_remote_interaction` ("scanned, waiting for approval").
  - It stops when `refresh_token` is non-empty; `account_name` comes back as well.
- **Logon and library:**
  - `steam_vent::Connection::login_with_refresh_token(&ServerList::discover().await?, &RefreshToken::new(token)?)` logged on.
  - `IPlayerService.GetOwnedGames { steamid, include_appinfo, include_played_free_games }` returned 679 games with appid, name and `playtime_forever`. No Web API key was needed.
- **Client and token:**
  - The owner's desktop Steam client stayed signed in. steam-vent sends `obfuscated_private_ip = 0`, which differs from the real client's logon id.
  - The token was valid for about 211 days (read from the JWT `exp`).

**Not verified:**
- **Token renewal.** It would use `Authentication.GenerateAccessTokenForApp` with `renewal_type Allow`; SteamKit2 does this.
- **Free-to-play games.** A game the owner never played may be missing from `GetOwnedGames`.
- **Rate limits.**
- **steam-vent stability.** The owner accepts that the protocol is unofficial and can break. When Steam changes it, the launcher must fail quietly: keep the cached list, put a dot on Settings and ask to reconnect, show no error banners.

Notes on SteamKit2 (LGPL): it was used only as a reference for the protocol. Reimplement from the protocol; don't copy its code. steam-vent is MIT.

## The parts

### 1. Connecting Steam (Settings)

**Where it lives (my call):**
- Settings already has a Steam row showing the local install: "● Connected · <persona> · <Steam path>".
- The QR connection goes **inside that row**, as a second line: "Library: Connect", or "Library: <account> · Disconnect" once connected.
- The local-install state is renamed **"Found"**, so "Connected" means only the QR connection.
- The attention dot for a token that stopped working belongs to this row.
- If the QR account differs from the local client's persona, show both. Nothing else changes: owned games come from the QR account, installed state from the local install.

**Connecting:**
- The row gets a **Connect** button.
- Connect opens a dialog with the QR code and a short instruction: open the Steam app → Steam Guard → scan the QR code.
- The QR code redraws when Steam rotates the challenge.
- After scanning, the dialog shows "Approve the login in the Steam app".
- On success, the dialog closes and the section shows the account name with a **Disconnect** button.
- Closing the dialog cancels polling.

**Disconnecting:**
- Disconnect deletes the token from Credential Manager.
- It also drops the not-installed Steam games from the library, back to installed-only. Installed games and launch history stay.

**Without a connection**, the Steam library works exactly as today (installed games only), with no nagging.

**The token stops working** (expired, revoked or rejected):
- The launcher keeps the last cached list and puts a quiet attention dot on Settings, like the Epic notice.
- The fix is to reconnect.
- If renewal works, renew before expiry. Whether it does is for the builder to find out.

**The device name** shown in the owner's Steam account is "Game Launcher". The spike used "Game Launcher (test)"; the owner can remove that device.

### 2. The full Steam library

**When to fetch:**
- When connected, fetch the owned list with `GetOwnedGames` in the background after the window is up, then at most every 6 hours.
- Cache it in SQLite so startup and offline use need no network.
- Installed-state detection stays as today (appmanifests). `GetOwnedGames` adds the not-installed games and their playtime.

**For each not-installed Steam game:**
- **Title:** from `GetOwnedGames`.
- **Art:** from Steam's CDN by appid, the same URLs the launcher already uses as a fallback, downloaded lazily into the art cache. Filling art for 600+ games must not slow scrolling. Load it as cards scroll into view or in a throttled background queue; the builder chooses.
- **Install:** opens `steam://install/<appid>`. Steam shows its own dialog.
- **Progress:** the existing appmanifest progress reading takes over once Steam starts the download.

**Playtime:** prefer `playtime_forever` from `GetOwnedGames` when connected. Otherwise use localconfig as today.

**Not games:** filter out obvious non-games if `GetOwnedGames` returns them (tools, soundtracks). How is the builder's call.

### 3. Knowing that an Epic game and a Steam game are the same

**Main path: GOG GamesDB.**
- `GET https://gamesdb.gog.com/platforms/epic/external_releases/<Legendary app_name>` returns a GamesDB game whose `releases` list includes `{platform_id: "steam", external_id: "<appid>"}` when one exists.
- Verified for Last Day of June: `Pumpkin` → Steam `635320`.
- The endpoint is unofficial and keyless; Heroic uses it.

**Caching:**
- Cache the result per Epic game for 30 days, including "no Steam twin" (a null marker, as the info cache already does), so each game is asked about rarely.
- Ask in the background and throttled, never on startup's critical path.

**The merge rule:** an Epic game merges with a Steam game **only** when GamesDB's Steam appid equals the appid of a Steam game the owner owns. That can be an installed one, or one from `GetOwnedGames` when connected. There are no title matches for merging.

**Only one-to-one matches merge (my call).** An entry has at most one copy per store. Merge only when:
- GamesDB lists exactly one Steam release for the Epic game, and
- no other Epic game in the library maps to that same appid.

Any ambiguity means no merge: the games stay separate cards. An example is a base game and a "Definitive Edition" both pointing at one appid. The Steam twin can still be used for that game's links and How long to beat when it is unique for that Epic game.

**Without a Steam connection**, merges can still happen with installed Steam games.

### 4. One entry, several copies

A library entry can have one or more **copies**: a store plus that store's id. A merged entry has a Steam copy and an Epic copy.

**On the card:**
- The cover art comes from the Steam copy when there is one; its art is usually best.
- The card shows the logos of all stores it is owned in.

**Store filters:** a merged entry appears under the Steam filter and the Epic filter.

**Installed:** the entry counts as installed if any copy is installed.

**Playtime:** the sum of the copies.
- Epic playtime is usually unknown. An unknown copy adds nothing.
- The tile notes "Steam + Epic" when both contribute.

**Last played:**
- The latest across copies, from store data and the launcher's own launch history (keys of either copy).
- The tile adds "· on Steam" or "· on Epic".

**Featured game and "continue playing":** treat the entry as one game.

**The key used to address an entry** is left to the builder, with two requirements:
- existing `steam-…` and `epic-…` keys keep finding their game (launch history, info cache, art cache);
- a merge appearing or disappearing doesn't lose history.

### 5. Choosing the launcher (Play button)

Built exactly as the approved mockup, `docs/superpowers/specs/2026-11-06-one-library-mockup.html`.

**The button:**
- The white Play pill becomes a split button. Its left segment shows the chosen store's logo and a chevron, with the accessible name "Choose launcher, now Steam". After a thin divider comes Play.
- The left segment opens a "Play with" menu: one item per copy, with the store logo, the name, and "Installed · <size>" or "Not installed · <size> download". The chosen one is checked.
  - A not-installed **Steam** copy shows just "Not installed", with no size (my call). Getting it would need PICS depot lookups over the CM connection, and Steam's own install dialog shows the size anyway.
- Choosing a store switches the main action: **Play** if that copy is installed, **Install** if not. Install then runs that store's existing install flow, including the Epic install popup with folder and size.
- A game with only one copy shows today's plain Play or Install button, with no switcher.

**The default choice, by the owner's rule:**
- the copy that is installed;
- if both or neither are installed, the copy played last.

The choice is not stored. It is recomputed each time the page opens (my call).

**Keyboard and menu behaviour** match the existing ⋯ menu:
- arrow keys, Home and End move through the items;
- Escape closes the menu, and focus returns to the switcher;
- Tab closes the menu;
- clicking outside closes it.

**The ⋯ menu:**
- "Open install folder" and "Uninstall" act on the chosen copy.
- "Open … store page" moves into the Links row, so the menu drops it.

**Meta line:** under the title, the meta line shows the logos of all owning stores and "Installed on Steam" (or "Installed on Epic Games", or "Not installed").

### 6. How long to beat for Epic games

- **When the Steam twin is known** (from part 3, even when the owner doesn't own it on Steam): use the existing Steam path, IGDB `external_games` with source 1 and uid = appid.
- **Otherwise, exact title within IGDB's Epic list.** "Otherwise" covers two cases: no twin is known, or the twin is known but IGDB has no row for that appid. The lookups chain (my call). Query `external_games` where `external_game_source = 26 & name = "<title>"`.
  - Accept only if every returned row has the same `game` id; then use that game's time to beat.
  - Zero rows or more than one game id means no tile.
  - The owner checked that Last Day of June returns two Epic rows, both game 36543.
- Cache it like today's ttb cache.
- `can_match(Epic)` stops returning false.
- **Confirmed by the owner's live queries:** source ids Steam = 1 and Epic = 26. IGDB's Epic `uid` values do not match anything Legendary has (checked against app_name, catalog id, namespace and the other metadata ids), so there is no id path for Epic.

### 7. Links in Details

Two new rows at the bottom of the existing **Details** panel. The page layout stays exactly as today: hero, then three tiles, then About on the left and Details on the right, with no new panel.

**Store pages:** one chip per copy, with the store logo.
- **Steam:** `https://store.steampowered.com/app/<appid>`.
- **Epic:** the Epic store URL from IGDB, whose `external_games` rows for source 26 carry a `url` such as `https://store.epicgames.com/en-US/p/last-day-of-june`. When there is none, use today's search URL (`store.epicgames.com/browse?q=<title>`).

**Info:** shown only when a Steam appid is known (owned, or the Steam twin from part 3). With no appid, the row is hidden; there are no search fallbacks.
- PCGamingWiki: `https://www.pcgamingwiki.com/api/appid.php?appid=<appid>`.
- SteamDB: `https://steamdb.info/app/<appid>/`.
- ProtonDB: `https://www.protondb.com/app/<appid>`.

**Icons:** every chip has an icon.
- Steam and Epic use the existing Simple Icons logos.
- PCGamingWiki, SteamDB and ProtonDB use their real logos, drawn as monochrome `currentColor` paths the same way `StoreIcon.svelte` draws the store icons.
  - The owner approved the download (2026-11-06). The files are already in the repo: `src/lib/icons/pcgamingwiki.svg`, `steamdb.svg` and `protondb.svg`, from Simple Icons 16.36.0.
  - Each file is a single path with no scripts or styles.
  - The drawn glyphs in the mockup were placeholders.

**Opening links:** links open in the default browser through a Rust command, as `open_store_page` does today. Allow only these URL shapes; the frontend sends a kind, not a URL.

## The approved look

- **The mockup:** `docs/superpowers/specs/2026-11-06-one-library-mockup.html` is the approved look for the game page. It covers the split Play button and menu, the two logos in the meta line, the tile suffixes, and the Store pages and Info rows in Details with icons.
- **Where it differs from the app,** the app and DESIGN.md win on tokens and spacing. The mockup wins on what is shown and where.
- **Placeholders:** values in [brackets] in the mockup are placeholders.

## Left to the builder

- **Data model:** how copies and merges are stored and keyed, within part 4's requirements.
- **Schema migration:** copy `settings` and `launches`, or add migrations.
- **Network pacing:** throttling and backoff for GamesDB, IGDB and Steam calls, and how art for not-installed games is filled.
- **Code layout:** where the Steam session lives (it should not hold a CM connection open while idle; connect, fetch, disconnect is fine), and how `commands.rs` grows. It is already about 1000 lines, so splitting it now is reasonable.
- **QR rendering:** draw it in Rust with `qrcode` (for example as SVG for the UI). The exact form is the builder's choice.
- **Steam-vent version:** a git dependency on Codeberg `main` pinned to a commit, or a release once 0.6 ships. Pin it either way.
- **Tests:** follow the existing fixture style for:
  - parsers (GamesDB response, `GetOwnedGames`, the IGDB Epic title match);
  - merge rules (merge on equal appid, no merge on title, default copy choice, summed playtime).
- **Live checks:** steps that need the owner, such as scanning a QR code or watching the Steam client, go in a VERIFY-LATER list. At most 10 app launches per task.
