// Typed wrappers for the Rust commands in src-tauri/src/commands.rs.
import { convertFileSrc, invoke } from '@tauri-apps/api/core'

export type Store = 'steam' | 'epic'
export type Sort = 'title' | 'lastPlayed' | 'playtime'
export type Login = 'noLegendary' | 'loggedOut' | 'loggedIn' | 'unreachable'
export type Action = 'play' | 'install' | 'uninstall'

export interface Card { key: string; title: string; provider: Store; installed: boolean }
export interface Library {
  games: Card[]
  total: number
  status: string
  epic: Login | null
  /** Epic's state is known and it isn't logged in: the settings button carries a dot. */
  epicAttention: boolean
  /** Logged out since the last run: one dismissible banner. */
  epicNotice: boolean
}
/** The game played last; `hero` is ready for an `<img>` or CSS `url()`. */
export interface Featured { key: string; title: string; hero: string | null }
export interface Detail {
  key: string
  title: string
  provider: Store
  installed: boolean
  /** Playtime tile text ("15 h 3 min", Steam's "Never played"); null hides it (Epic doesn't report playtime). */
  playtimeTile: string | null
  /** Last played tile text ("12 days ago"); null when never played. */
  lastPlayedTile: string | null
  /** Size on disk of an installed game, like "27.3 GB". */
  size: string | null
  /** Only set for an installed game. */
  installDir: string | null
}
export interface SettingsView {
  steam: { name: string | null; path: string } | null
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
export const refresh = (epic: boolean) => invoke<void>('refresh', { epic })
export const game = (key: string) => invoke<Detail | null>('game', { key })
export const act = (action: Action, key: string) => invoke<void>('act', { action, key })
/** Epic download size like "27.3 GB" (asks Legendary online, cached); null when unknown. */
export const downloadSize = (key: string) => invoke<string | null>('download_size', { key })
/** Store details for the game page; screenshots are ready-to-use URLs. Null when the store has none. */
export type GameInfo = {
  description: string
  genres: string[]
  developer: string | null
  publisher: string | null
  released: string | null
  screenshots: string[]
}
export const gameInfo = async (key: string) => {
  const info = await invoke<GameInfo | null>('game_info', { key })
  return info && { ...info, screenshots: info.screenshots.flatMap((p) => url(p) ?? []) }
}
/** How long to beat from IGDB, as labels like "32 h". Null: no credentials, an Epic game, or IGDB doesn't know it. */
export type TimeToBeat = { normally: string | null; hastily: string | null; completely: string | null }
export const timeToBeat = (key: string) => invoke<TimeToBeat | null>('time_to_beat', { key })
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
export const openSteam = () => invoke<void>('open_steam')
export const openTwitchConsole = () => invoke<void>('open_twitch_console')
export const openFolder = (key: string) => invoke<void>('open_folder', { key })
export const openStorePage = (key: string) => invoke<void>('open_store_page', { key })
/** The "Data from IGDB" credit. */
export const openIgdb = () => invoke<void>('open_igdb')
export const clearCache = async () => {
  await invoke<void>('clear_cache')
  // Covers shown from now on are fetched and shrunk again.
  covers.clear()
}
export const benchSpeed = () => invoke<number | null>('bench_speed')
export const benchDone = (frameMs: number[], games: number) => invoke<void>('bench_done', { frameMs, games })

/** Local files come back as paths; the webview loads them through the asset protocol. */
const url = (path: string | null) => (path ? convertFileSrc(path) : null)

// Remounted cards (virtual scrolling) reuse the answer instead of asking again.
// "No art" isn't kept, so a later rescan that finds art can still show it.
const covers = new Map<string, Promise<string | null>>()
export const cover = (key: string) => {
  let found = covers.get(key)
  if (!found) {
    found = invoke<string | null>('cover', { key }).then((path) => {
      if (!path) covers.delete(key)
      return url(path)
    })
    covers.set(key, found)
  }
  return found
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
