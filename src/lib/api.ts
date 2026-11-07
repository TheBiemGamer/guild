// Typed wrappers for the Rust commands in src-tauri/src/commands.rs.
import { convertFileSrc, invoke } from '@tauri-apps/api/core'

export type Store = 'steam' | 'epic'
export type Sort = 'title' | 'lastPlayed' | 'playtime'
export type Login = 'noLegendary' | 'loggedOut' | 'loggedIn' | 'unreachable'
export type Action = 'play' | 'playOutdated' | 'install' | 'update' | 'uninstall'

/**
 * `stores`: every store this game is in, the one `key` belongs to first. `chosen`: the copy key Play and
 * Install use. `copyKeys`: every copy's key, which running state and install progress are reported under.
 */
export interface Card { key: string; chosen: string; copyKeys: string[]; title: string; stores: Store[]; installed: boolean; update: boolean }
export interface Library {
  games: Card[]
  total: number
  status: string
  epic: Login | null
  /** Epic's state is known and it isn't logged in: the settings button carries a dot. */
  epicAttention: boolean
  /** Steam rejected the saved login: the settings button carries a dot too. */
  steamAttention: boolean
  /** Logged out since the last run: one dismissible banner. */
  epicNotice: boolean
}
/** The game played last; `hero` is ready for an `<img>` or CSS `url()`. `chosen` and `copyKeys` as on `Card`. */
export interface Featured { key: string; chosen: string; copyKeys: string[]; title: string; hero: string | null }
/** One store's copy of a game. */
export interface CopyView {
  key: string
  provider: Store
  installed: boolean
  /** An installed Epic copy has a newer version. */
  updateAvailable: boolean
  /** Size on disk of an installed copy, like "27.3 GB". */
  size: string | null
  /** Only set for an installed copy. */
  installDir: string | null
}
export type LinkKind = 'steamStore' | 'epicStore' | 'pcGamingWiki' | 'steamDb' | 'protonDb'
export interface Detail {
  /** The entry key. */
  key: string
  title: string
  stores: Store[]
  /** Any copy. */
  installed: boolean
  copies: CopyView[]
  /** The copy key Play and Install use. */
  chosen: string
  /** Playtime tile text ("15 h 3 min", Steam's "Never played"); null hides it (Epic doesn't report playtime). */
  playtimeTile: string | null
  /** "Steam + Epic" when both copies contributed. */
  playtimeNote: string | null
  /** Last played tile text ("12 days ago"); null when never played. */
  lastPlayedTile: string | null
  /** "on Steam" / "on Epic Games", only for a game with several copies. */
  lastPlayedStore: string | null
  /** Store pages first, then info sites (only with a Steam app id). */
  links: LinkKind[]
  /** The Epic copies' DLC; a Steam copy's comes from `steamDlc`. */
  dlc: Dlc[]
}
export interface SettingsView {
  steam: { name: string | null; path: string } | null
  steamLibrary: { account: string | null; attention: boolean }
  epic: Login | null
  epicGames: number
  /** Null means Legendary's own default folder. */
  epicInstallDir: string | null
  igdbClientId: string | null
  /** The secret itself never reaches the UI. */
  igdbSecretSet: boolean
  showBanner: boolean
  ambient: boolean
  appVersion: string
  legendaryVersion: string | null
  cacheBytes: number
  /** `cacheBytes` as shown, like "812 MB". */
  cacheSize: string
}
export interface Progress { fraction: number; text: string }
export interface Poll { running: string | null; progress: Record<string, Progress>; changed: boolean }

export const library = (query: string, store: Store | null, installedOnly: boolean, sort: Sort) =>
  invoke<Library>('library', { query, store, installedOnly, sort })
export const loadCache = () => invoke<void>('load_cache')
/** `owned` forces a fresh owned-games fetch; without it the 6 h rule applies. Resolves to whether the library changed. */
export const refresh = (epic: boolean, owned: boolean) => invoke<boolean>('refresh', { epic, owned })
export const game = (key: string) => invoke<Detail | null>('game', { key })
/** `withDlc`: an Epic install takes the owned DLC too (Rust's default). `dlc`: act on that DLC of the Epic copy `key`. */
export interface ActOptions { withDlc?: boolean; dlc?: string }
export const act = (action: Action, key: string, options: ActOptions = {}) => invoke<void>('act', { action, key, ...options })
/** A DLC under its game. `copy` is the Epic copy's key for Install and Uninstall; null for Steam, which installs DLC itself. */
export interface Dlc { id: string; title: string; installed: boolean; copy: string | null }
/**
 * The Steam copy's owned DLC (Steam logs on for it: connected account only, cached in Rust). Asked once per
 * game and kept, like the other game page answers.
 */
export const steamDlc = (key: string) =>
  remembered('steamDlc', key, () => invoke<{ id: string; title: string }[]>('steam_dlc', { key }))
/** Epic download size like "27.3 GB" (asks Legendary online, cached); null when unknown. */
export const downloadSize = (key: string) => remembered('size', key, () => invoke<string | null>('download_size', { key }))
/** Store details for the game page; `screenshots` are remote URLs for `screenshots`. Null when the store has none. */
export type GameInfo = {
  description: string
  genres: string[]
  developer: string | null
  publisher: string | null
  released: string | null
  screenshots: string[]
}
export const gameInfo = (key: string) => remembered('info', key, () => invoke<GameInfo | null>('game_info', { key }))
/** `gameInfo`'s screenshots as ready-to-use URLs, downloaded on first use; failed ones are left out. */
export const screenshots = async (key: string, urls: string[]) =>
  (await invoke<string[]>('screenshots', { key, urls })).flatMap((p) => url(p) ?? [])
/** How long to beat from IGDB, as labels like "32 h". Null: no credentials, an Epic game, or IGDB doesn't know it. */
export type TimeToBeat = { normally: string | null; hastily: string | null; completely: string | null }
export const timeToBeat = (key: string) => remembered('ttb', key, () => invoke<TimeToBeat | null>('time_to_beat', { key }))

/** Game page answers by game key, then by command; the 20 keys used last are kept. */
const answers = new Map<string, Map<string, Promise<unknown>>>()
const ANSWERS_KEPT = 20

/** `ask`'s answer, shared with earlier askers. Nulls and errors aren't kept, so they are asked again. */
function remembered<T>(command: string, key: string, ask: () => Promise<T | null>): Promise<T | null> {
  const byCommand = answers.get(key) ?? new Map<string, Promise<unknown>>()
  // Newest last, so the first key is the one used longest ago.
  answers.delete(key)
  answers.set(key, byCommand)
  if (answers.size > ANSWERS_KEPT) answers.delete(answers.keys().next().value!)
  let found = byCommand.get(command) as Promise<T | null> | undefined
  if (!found) {
    const forget = () => byCommand.delete(command)
    found = ask().then(
      (answer) => {
        if (answer === null) forget()
        return answer
      },
      (e) => {
        forget()
        throw e
      },
    )
    byCommand.set(command, found)
  }
  return found
}

/** Asks for what the game page shows, so opening it usually finds the answers ready. */
export function prefetch(card: Card) {
  const quiet = () => {}
  gameInfo(card.key).catch(quiet)
  timeToBeat(card.key).catch(quiet)
  // Epic sizes only: a Steam size logs on to Steam, which a passing pointer must not do.
  if (!card.installed && card.chosen.startsWith('epic-')) downloadSize(card.chosen).catch(quiet)
}
/** Settings "Test": resolves when Twitch and IGDB accept the saved credentials, else rejects with the reason. */
export const testIgdb = () => invoke<void>('test_igdb')
export const cancelInstall = (key: string) => invoke<void>('cancel_install', { key })
export const poll = () => invoke<Poll>('poll')
export const epicLogIn = () => invoke<void>('epic_log_in')
export const epicLogOut = () => invoke<void>('epic_log_out')
export const dismissEpicNotice = () => invoke<void>('dismiss_epic_notice')
export const settingsView = () => invoke<SettingsView>('settings_view')
/** `null` resets a setting to its default. Booleans travel as "true" / "false". */
export const updateSetting = (name: string, value: string | null) => invoke<void>('update_setting', { name, value })
export const setIgdbSecret = (value: string | null) => invoke<void>('set_igdb_secret', { value })
export type ConnectStep =
  | { step: 'waiting' }
  | { step: 'rotated'; svg: string }
  | { step: 'scanned' }
  | { step: 'connected'; account: string }
  | { step: 'failed'; message: string }
/** Starts a Steam QR login; resolves to the code as an SVG. */
export const steamConnectStart = () => invoke<string>('steam_connect_start')
export const steamConnectPoll = () => invoke<ConnectStep>('steam_connect_poll')
export const steamConnectCancel = () => invoke<void>('steam_connect_cancel')
export type LoginStep =
  | { step: 'approve'; email: boolean }
  | { step: 'code'; kind: 'email' | 'app' }
  | { step: 'connected'; account: string }
  | { step: 'failed'; message: string }
/** Steam login with account name and password; resolves to the first step. */
export const steamPasswordLogin = (account: string, password: string) =>
  invoke<LoginStep>('steam_password_login', { account, password })
/** Sends the Steam Guard code; resolves to what happened next. */
export const steamPasswordCode = (code: string) => invoke<LoginStep>('steam_password_code', { code })
/** Waits for the login to finish after an Approve. */
export const steamPasswordNext = () => invoke<LoginStep>('steam_password_next')
export const steamLoginCancel = () => invoke<void>('steam_login_cancel')
export const steamDisconnect = () => invoke<void>('steam_disconnect')
export const openSteam = () => invoke<void>('open_steam')
export const openTwitchConsole = () => invoke<void>('open_twitch_console')
export const openFolder = (key: string) => invoke<void>('open_folder', { key })
export const openLink = (key: string, kind: LinkKind) => invoke<void>('open_link', { key, kind })
/** The "Data from IGDB" credit. */
export const openIgdb = () => invoke<void>('open_igdb')
export const clearCache = async () => {
  await invoke<void>('clear_cache')
  // Covers shown from now on are fetched and shrunk again.
  covers.clear()
}
/** Shows the window (it starts hidden); only the first call counts. */
export const ready = () => invoke<void>('ready')
export const benchSpeed = () => invoke<number | null>('bench_speed')
export const benchDone = (frameMs: number[], games: number) => invoke<void>('bench_done', { frameMs, games })

/** Local files come back as paths; the webview loads them through the asset protocol. */
const url = (path: string | null) => (path ? convertFileSrc(path) : null)

/** `src` is ready for an `<img>`; `color` (`#rrggbb`) is the image's average, to paint while it loads. */
export interface Cover { src: string; color: string | null }

// Remounted cards (virtual scrolling) reuse the answer instead of asking again.
// "No art" isn't kept, so a later rescan that finds art can still show it.
const covers = new Map<string, Promise<Cover | null>>()
/** Asked in this task, sent lowest `order` first; Rust then works through them in that order. */
let batch: { key: string; order: number; send: (cover: Promise<Cover | null>) => void }[] = []

/** `order` is the card's place in the grid, so covers load top to bottom, left to right; -1 goes first. */
export const cover = (key: string, order = -1) => {
  let found = covers.get(key)
  if (!found) {
    found = new Promise((send) => {
      if (batch.length === 0) queueMicrotask(sendBatch)
      batch.push({ key, order, send })
    })
    covers.set(key, found)
  }
  return found
}

function sendBatch() {
  const now = batch.sort((a, b) => a.order - b.order)
  batch = []
  for (const { key, send } of now) {
    send(
      invoke<Cover | null>('cover', { key }).then(
        (c) => {
          if (!c) covers.delete(key)
          return c && { src: convertFileSrc(c.src), color: c.color }
        },
        (e) => {
          // Forgotten, so the next ask tries again; the card shows its title meanwhile.
          console.error('cover', key, e)
          covers.delete(key)
          return null
        },
      ),
    )
  }
}

export const featured = async (): Promise<Featured | null> => {
  const f = await invoke<Featured | null>('featured')
  return f && { ...f, hero: url(f.hero) }
}

export const detailArt = async (key: string) => {
  const art = await invoke<{ hero: string | null }>('detail_art', { key })
  return { hero: url(art.hero) }
}

/** Command errors arrive as plain strings. */
export const message = (e: unknown) => (typeof e === 'string' ? e : String(e))
