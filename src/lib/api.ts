// Typed wrappers for the Rust commands in src-tauri/src/commands.rs.
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import type { GroupId, Progress } from './format'
import type { Collection, ListFilter } from './filters'

// Rust sends numbers and ids; `format.ts` turns them into labels.
export type { Collection, GroupId, Progress }

export type Store = 'steam' | 'epic'
export type Sort = 'title' | 'lastPlayed' | 'mostPlayed' | 'longestToBeat' | 'shortestToBeat'
export type Login = 'noLegendary' | 'loggedOut' | 'loggedIn' | 'unreachable'
export type Action = 'play' | 'playOutdated' | 'install' | 'update' | 'uninstall' | 'verify' | 'move'

/**
 * `stores`: every store this game is in, the one `key` belongs to first. `chosen`: the copy key Play and
 * Install use. `copyKeys`: every copy's key, which running state and install progress are reported under.
 * `playtimeMinutes`: summed over the copies, null when unknown. `ttbHours`: how long to beat in shown hours, null when unknown.
 * `favorite`, `hidden`, `collections` (ids): any copy is.
 */
export interface Card {
  key: string
  chosen: string
  copyKeys: string[]
  title: string
  stores: Store[]
  installed: boolean
  update: boolean
  playtimeMinutes: number | null
  ttbHours: number | null
  favorite: boolean
  hidden: boolean
  collections: number[]
}
/** What the game menu needs of a game (a card or the game page). */
export type Listed = Pick<Card, 'key' | 'title' | 'favorite' | 'hidden' | 'collections'>
/** A run of `count` cards; `groupLabel` names its `group`. */
export interface Section { group: GroupId; count: number }
export interface Library {
  /** Grouped by the sort, group after group. */
  games: Card[]
  /** The groups over `games`, in order; empty for the Title sort without favorites. */
  groups: Section[]
  /** Games in the library, hidden ones left out. */
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
  /** Size on disk of an installed copy. */
  sizeBytes: number | null
  /** Only set for an installed copy. */
  installDir: string | null
  /** An installed Epic copy that keeps its saves in Epic's cloud: the Saves tab. */
  cloudSaves: boolean
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
  /** Playtime tile minutes, 0 being Steam's "Never played"; null hides it (Epic doesn't report playtime). */
  playtimeMinutes: number | null
  /** Both copies contributed to the playtime. */
  playtimeBoth: boolean
  /** Unix seconds; null when never played. */
  lastPlayed: number | null
  /** The store played on last, only for a game with several copies. */
  lastPlayedStore: Store | null
  /** Store pages first, then info sites (only with a Steam app id). */
  links: LinkKind[]
  /** The Epic copies' DLC; a Steam copy's comes from `steamDlc`. */
  dlc: Dlc[]
  favorite: boolean
  hidden: boolean
  collections: number[]
}
export interface SettingsView {
  /** Whether Steam is installed on this PC. */
  steam: boolean
  steamLibrary: { account: string | null; attention: boolean }
  epic: Login | null
  /** The logged-in Epic account's display name, if Legendary's session has one. */
  epicAccount: string | null
  /** Null means Legendary's own default folder. */
  epicInstallDir: string | null
  igdbClientId: string | null
  /** The secret itself never reaches the UI. */
  igdbSecretSet: boolean
  showBanner: boolean
  ambient: boolean
  /** Epic games sync cloud saves around play unless the game's Saves tab says otherwise. */
  cloudSaves: boolean
  appVersion: string
  legendaryVersion: string | null
  cacheBytes: number
}
export type QueuedAction = 'install' | 'update' | 'verify'
/** The Epic download queue in order. `active` is the first job's key, also while `paused`. */
export interface QueueView { jobs: { key: string; action: QueuedAction; title: string }[]; paused: boolean; active: string | null }
export interface Poll { running: string | null; progress: Record<string, Progress>; changed: boolean; queue: QueueView }
export const emptyQueue: QueueView = { jobs: [], paused: false, active: null }
/** Where one of `keys` (a game's copy keys) is in the queue: running, paused at the head, or waiting; null when not queued. */
export interface QueueState { key: string; action: QueuedAction; state: 'active' | 'paused' | 'queued' }
export function queueState(queue: QueueView, keys: string[]): QueueState | null {
  const at = queue.jobs.findIndex((j) => keys.includes(j.key))
  if (at < 0) return null
  const { key, action } = queue.jobs[at]
  return { key, action, state: at > 0 ? 'queued' : queue.paused ? 'paused' : 'active' }
}

/**
 * Recently played groups by local calendar day, so it sends the time zone's offset east of UTC. `lists`: the Lists
 * filters that are on; Hidden brings hidden games back, and favorites go on top unless Favorites is the only one.
 */
export const library = (query: string, store: Store | null, installedOnly: boolean, sort: Sort, lists: ListFilter[]) =>
  invoke<Library>('library', { query, store, installedOnly, sort, lists, utcOffsetMin: -new Date().getTimezoneOffset() })
/** `key`: a game's key; every copy of it changes. */
export const setFavorite = (key: string, on: boolean) => invoke<void>('set_favorite', { key, on })
export const setHidden = (key: string, on: boolean) => invoke<void>('set_hidden', { key, on })
/** By name. */
export const collections = () => invoke<Collection[]>('collections')
/** Fails when the name is empty or taken (ignoring case); resolves to the new id. */
export const createCollection = (name: string) => invoke<number>('create_collection', { name })
export const renameCollection = (id: number, name: string) => invoke<void>('rename_collection', { id, name })
/** The games stay in the library. */
export const deleteCollection = (id: number) => invoke<void>('delete_collection', { id })
export const setInCollection = (key: string, id: number, on: boolean) => invoke<void>('set_in_collection', { key, id, on })
/** Resolves to the saved sort. */
export const loadCache = () => invoke<{ sort: Sort }>('load_cache')
/** `owned` forces a fresh owned-games fetch; without it the 6 h rule applies. Resolves to whether the library changed. */
export const refresh = (epic: boolean, owned: boolean) => invoke<boolean>('refresh', { epic, owned })
export const game = (key: string) => invoke<Detail | null>('game', { key })
/**
 * `withDlc`: an Epic install takes the owned DLC too (Rust's default). `dlc`: act on that DLC of the Epic copy `key`.
 * `keep`: the answer to the cloud save conflict an earlier Play returned. `dest`: the folder an Epic Move goes to.
 */
export interface ActOptions { withDlc?: boolean; dlc?: string; keep?: 'cloud' | 'local'; dest?: string }
/** Which save is newer when an Epic Play stopped to ask before launching. */
export type Conflict = 'localNewer' | 'cloudNewer'
/** Resolves to a conflict when an Epic Play asks first instead of launching; null otherwise. */
export const act = (action: Action, key: string, options: ActOptions = {}) =>
  invoke<Conflict | null>('act', { action, key, ...options })
/**
 * An Epic copy's Saves tab. `choice`: its own On/Off, null follows Settings. `syncedAt`: unix seconds of the last
 * sync that worked, null when never or when `error` says what went wrong with the last one.
 */
export interface SavesView { choice: boolean | null; syncedAt: number | null; error: string | null }
export const saves = (key: string) => invoke<SavesView | null>('saves', { key })
export const setSaves = (key: string, choice: boolean | null) => invoke<void>('set_saves', { key, choice })
/** The newer save wins; the outcome shows in `saves`. */
export const syncSavesNow = (key: string) => invoke<void>('sync_saves_now', { key })
/** An installed Epic copy's Launch options tab. `exe`: relative to the install folder; null runs the default. */
export interface LaunchOptions { args: string; offline: boolean; skipVersionCheck: boolean; exe: string | null }
export const launchOptions = (key: string) => invoke<LaunchOptions | null>('launch_options', { key })
/** `exe` may be absolute; resolves to what was stored (`exe` made relative). Fails for an exe outside the install folder. */
export const setLaunchOptions = (key: string, options: LaunchOptions) => invoke<LaunchOptions>('set_launch_options', { key, options })
/** A Steam copy's properties in Steam, where its launch options live. */
export const openSteamProperties = (key: string) => invoke<void>('open_steam_properties', { key })
/** A DLC under its game. `copy` is the Epic copy's key for Install and Uninstall; null for Steam, which installs DLC itself. */
export interface Dlc { id: string; title: string; installed: boolean; copy: string | null }
/**
 * The Steam copy's owned DLC (Steam logs on for it: connected account only, cached in Rust). Asked once per
 * game and kept, like the other game page answers; an empty list (none, or a failure) is null, so it's asked again.
 */
export const steamDlc = (key: string) =>
  remembered('steamDlc', key, () =>
    invoke<{ id: string; title: string }[]>('steam_dlc', { key }).then((d) => (d.length ? d : null)),
  )
/** Download size in bytes (Epic asks Legendary online, Steam logs on; cached); null when unknown. */
export const downloadSize = (key: string) => remembered('size', key, () => invoke<number | null>('download_size', { key }))
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
/** How long to beat from IGDB in seconds, null where unknown. Null: no credentials, an Epic game, or IGDB doesn't know it. */
export type TimeToBeat = { normally: number | null; hastily: number | null; completely: number | null }
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
// These resolve to the queue as it is now, so the UI needn't wait for the next poll.
/** Stops a game's active download, or takes a waiting or paused one out of the queue. */
export const cancelInstall = (key: string) => invoke<QueueView>('cancel_install', { key })
/** Pause stops the active Legendary job and keeps it first; Resume starts it again. */
export const pauseDownloads = () => invoke<QueueView>('pause_downloads')
export const resumeDownloads = () => invoke<QueueView>('resume_downloads')
/** One place up or down; the active job stays first. */
export const moveDownload = (key: string, up: boolean) => invoke<QueueView>('move_download', { key, up })
export const poll = () => invoke<Poll>('poll')
export const epicLogIn = () => invoke<void>('epic_log_in')
export const epicLogOut = () => invoke<void>('epic_log_out')
/** Owned Epic games that aren't installed, by title, for "Import an existing install". */
export const epicImportable = () => invoke<{ key: string; title: string }[]>('epic_importable')
export const epicImport = (key: string, path: string) => invoke<void>('epic_import', { key, path })
export const epicEglSync = () => invoke<void>('epic_egl_sync')
/** The system folder picker; null when it was closed without picking. */
export async function pickFolder(): Promise<string | null> {
  const path = await open({ directory: true })
  return typeof path === 'string' ? path : null
}
/** An .exe picked from `folder` (it opens there); null when cancelled. */
export async function pickExe(folder: string): Promise<string | null> {
  const path = await open({ defaultPath: folder, filters: [{ name: 'Programs', extensions: ['exe'] }] })
  return typeof path === 'string' ? path : null
}
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
export const openTwitchConsole = () => invoke<void>('open_twitch_console')
export const openFolder = (key: string) => invoke<void>('open_folder', { key })
export const openLink = (key: string, kind: LinkKind) => invoke<void>('open_link', { key, kind })
/** The "Data from IGDB" credit. */
export const openIgdb = () => invoke<void>('open_igdb')
export const openLogFolder = () => invoke<void>('open_log_folder')
/** A new GitHub bug report with the app and Windows versions filled in. */
export const reportProblem = () => invoke<void>('report_problem')
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
