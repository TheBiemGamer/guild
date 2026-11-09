// The Rust commands and their types come from `bindings.ts`, generated from `src-tauri/src` (see DEVELOPMENT.md).
// This adds only what needs logic: remembered answers, cover batching, asset URLs, pickers and message words.
import { Channel, convertFileSrc } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import {
  commands,
  type Action,
  type Card,
  type Conflict,
  type Cover,
  type CoverPick,
  type CustomGame,
  type Featured,
  type GridCover,
  type Keep,
  type ProtonKind,
  type QueuedAction,
  type QueueView,
  type Sort,
  type Store,
  type ToolView,
} from './bindings'
import type { ListFilter } from './filters'
import { render } from './messages'
import { t } from './i18n'

/** What the game menu needs of a game (a card or the game page). */
export type Listed = Pick<Card, 'key' | 'title' | 'favorite' | 'hidden' | 'collections'>
export const emptyQueue: QueueView = { jobs: [], paused: false, active: null }
/** Where one of `keys` (a game's copy keys) is in the queue: running, paused at the head, or waiting; null when not queued. */
export interface QueueState {
  key: string
  action: QueuedAction
  state: 'active' | 'paused' | 'queued'
}
export function queueState(queue: QueueView, keys: string[]): QueueState | null {
  const at = queue.jobs.findIndex((j) => keys.includes(j.key))
  if (at < 0) return null
  const { key, action } = queue.jobs[at]
  return { key, action, state: at > 0 ? 'queued' : queue.paused ? 'paused' : 'active' }
}

/** `lists`: the Lists filters that are on; Hidden brings hidden games back, and favorites go on top unless Favorites is the only one. */
export const library = (query: string, store: Store | null, installedOnly: boolean, sort: Sort, lists: ListFilter[]) =>
  commands.library(query, store, installedOnly, sort, lists)
/**
 * `withDlc`: an Epic or GOG install takes the owned DLC too (Rust's default). `dlc`: act on that DLC of the copy `key`.
 * `keep`: the answer to the cloud save conflict an earlier Play returned. `dest`: the folder an Epic, GOG or Amazon Move goes to.
 */
export interface ActOptions {
  withDlc?: boolean
  dlc?: string
  keep?: Keep
  dest?: string
}
/** Resolves to a conflict when an Epic or GOG Play asks first instead of launching; null otherwise. */
export const act = (action: Action, key: string, o: ActOptions = {}): Promise<Conflict | null> =>
  commands.act(action, key, o.withDlc ?? null, o.dlc ?? null, o.keep ?? null, o.dest ?? null)
/**
 * The Steam copy's owned DLC (Steam logs on for it: connected account only, cached in Rust). Asked once per
 * game and kept, like the other game page answers; an empty list (none, or a failure) is null, so it's asked again.
 */
export const steamDlc = (key: string) => remembered('steamDlc', key, () => commands.steamDlc(key).then((d) => (d.length ? d : null)))
/** Download size in bytes (Epic asks Legendary online, GOG asks gogdl, Steam logs on; cached); null when unknown. */
export const downloadSize = (key: string) => remembered('size', key, () => commands.downloadSize(key))
/** Store details for the game page; `screenshots` are remote URLs for `screenshots`. Null when the store has none. */
export const gameInfo = (key: string) => remembered('info', key, () => commands.gameInfo(key))
/** `gameInfo`'s screenshots as ready-to-use URLs, downloaded on first use; failed ones are left out. */
export const screenshots = async (key: string, urls: string[]) => (await commands.screenshots(key, urls)).flatMap((p) => url(p) ?? [])
/** How long to beat from IGDB in seconds, null where unknown. Null: no credentials, an Epic game, or IGDB doesn't know it. */
export const timeToBeat = (key: string) => remembered('ttb', key, () => commands.timeToBeat(key))

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
}
/** Downloads and checks a store tool; `onPercent` hears each new whole percent once the size is known. */
export function installTool(name: ToolView['name'], onPercent: (percent: number) => void) {
  const channel = new Channel<number>()
  channel.onmessage = onPercent
  return commands.installTool(name, channel)
}

/** Linux: downloads umu-launcher, reporting each new whole percent. */
export function installUmu(onPercent: (percent: number) => void) {
  const channel = new Channel<number>()
  channel.onmessage = onPercent
  return commands.installUmu(channel)
}

/** Linux: downloads one Proton release, reporting each new whole percent. */
export function installProton(kind: ProtonKind, tag: string, onPercent: (percent: number) => void) {
  const channel = new Channel<number>()
  channel.onmessage = onPercent
  return commands.installProton(kind, tag, channel)
}
/** The system folder picker; null when it was closed without picking. */
export async function pickFolder(): Promise<string | null> {
  const path = await open({ directory: true })
  return typeof path === 'string' ? path : null
}
/** An .exe picked from `folder` (it opens there); null when cancelled. */
export async function pickExe(folder: string): Promise<string | null> {
  const path = await open({ defaultPath: folder, filters: [{ name: t('dialog.programs'), extensions: ['exe'] }] })
  return typeof path === 'string' ? path : null
}
/** The tray menu's Downloads item. Resolves to the unlisten function. */
export const onOpenDownloads = (f: () => void) => listen('open-downloads', f)

/**
 * Adds a game (`key` null) or saves the one with `key`; `cover` null keeps its cover. Resolves to the game's key.
 * `args` are split on spaces; double quotes keep words together. No shell.
 */
export async function saveCustomGame(key: string | null, game: CustomGame, cover: CoverPick | null) {
  const saved = await commands.saveCustomGame(key, game.name, game.exe, game.args, cover)
  // A new cover: the card asks again.
  if (cover) covers.delete(saved)
  return saved
}
/** Up to four SteamGridDB covers for `name`, `thumb` ready for an `<img>`; null when no API key is saved. A failed search is an empty list. */
export async function gridSearch(name: string): Promise<GridCover[] | null> {
  const found = await commands.gridSearch(name)
  return found && found.map((c) => ({ url: c.url, thumb: convertFileSrc(c.thumb) }))
}
/** A program (.exe) to add; null when cancelled. */
export async function pickProgram(): Promise<string | null> {
  const path = await open({ filters: [{ name: t('dialog.programs'), extensions: ['exe'] }] })
  return typeof path === 'string' ? path : null
}
/** A PNG or JPEG cover; null when cancelled. */
export async function pickImage(): Promise<string | null> {
  const path = await open({ filters: [{ name: t('dialog.images'), extensions: ['png', 'jpg', 'jpeg'] }] })
  return typeof path === 'string' ? path : null
}
export const clearCache = async () => {
  await commands.clearCache()
  // Covers shown from now on are fetched and shrunk again.
  covers.clear()
}

/** Local files come back as paths; the webview loads them through the asset protocol. */
const url = (path: string | null) => (path ? convertFileSrc(path) : null)

// Remounted cards (virtual scrolling) reuse the answer instead of asking again.
// "No art" isn't kept, so a later rescan that finds art can still show it.
const covers = new Map<string, Promise<Cover | null>>()
/** Asked in this task, sent lowest `order` first; Rust then works through them in that order. */
let batch: { key: string; order: number; send: (cover: Promise<Cover | null>) => void }[] = []

/**
 * `src` is ready for an `<img>`; `color` (`#rrggbb`) is the image's average, to paint while it loads.
 * `order` is the card's place in the grid, so covers load top to bottom, left to right; -1 goes first.
 */
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
      commands.cover(key).then(
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

/** The game played last; `hero` is ready for an `<img>` or CSS `url()`. */
export const featured = async (): Promise<Featured | null> => {
  const f = await commands.featured()
  return f && { ...f, hero: url(f.hero) }
}

export const detailArt = async (key: string) => {
  const art = await commands.detailArt(key)
  return { hero: url(art.hero) }
}

/** A command error or other `Msg` from Rust in words (`messages.ts`, `i18n/en.json`). */
export const message = render
