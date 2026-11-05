// Typed wrappers for the Rust commands in src-tauri/src/commands.rs.
import { convertFileSrc, invoke } from '@tauri-apps/api/core'

export type Store = 'steam' | 'epic'
export type Sort = 'title' | 'lastPlayed' | 'playtime'
export type Login = 'noLegendary' | 'loggedOut' | 'loggedIn' | 'unreachable'
export type Action = 'play' | 'install' | 'uninstall'

export interface Card { key: string; title: string; installed: boolean }
export interface Library { games: Card[]; total: number; status: string; epic: Login | null }
export interface Detail {
  key: string
  title: string
  provider: Store
  installed: boolean
  playtime: string
  lastPlayed: string
}
export interface Progress { fraction: number; text: string }
export interface Poll { running: string | null; progress: Record<string, Progress>; changed: boolean }

export const library = (query: string, store: Store | null, installedOnly: boolean, sort: Sort) =>
  invoke<Library>('library', { query, store, installedOnly, sort })
export const loadCache = () => invoke<void>('load_cache')
export const refresh = (epic: boolean) => invoke<void>('refresh', { epic })
export const game = (key: string) => invoke<Detail | null>('game', { key })
export const act = (action: Action, key: string) => invoke<void>('act', { action, key })
export const cancelInstall = (key: string) => invoke<void>('cancel_install', { key })
export const poll = () => invoke<Poll>('poll')
export const epicLogIn = () => invoke<void>('epic_log_in')
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

export const detailArt = async (key: string) => {
  const art = await invoke<{ hero: string | null; logo: string | null }>('detail_art', { key })
  return { hero: url(art.hero), logo: url(art.logo) }
}

/** Command errors arrive as plain strings. */
export const message = (e: unknown) => (typeof e === 'string' ? e : String(e))
