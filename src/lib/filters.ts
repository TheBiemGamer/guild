// The library's extra filters (the Filters popover) and their toolbar chips. `node --test scripts/` loads it
// as plain TypeScript.

import { t } from './i18n.ts'

export type Filter =
  | 'notInstalled'
  | 'update'
  | 'downloading'
  | 'neverPlayed'
  | 'under10h'
  | 'played10to50h'
  | 'played50hPlus'
  | 'ttbUnder5h'
  | 'ttb5to15h'
  | 'ttb15to40h'
  | 'ttb40to100h'
  | 'ttb100hPlus'
  | 'bothStores'
  | ListFilter

/** The Lists group: favorites, hidden games and the owner's collections (by id). */
export type ListFilter = 'favorites' | 'hidden' | `collection-${number}`
export const collectionFilter = (id: number): ListFilter => `collection-${id}`
export const isList = (f: Filter): f is ListFilter => f === 'favorites' || f === 'hidden' || f.startsWith('collection-')

/** A collection as Rust's `collections` sends it; `count` is how many games are in it. */
export interface Collection {
  id: number
  name: string
  count: number
}

type Group = { label: string; filters: Filter[] }
/** `id` names the group's words: `filters.group.<id>`. */
type Ids = { id: string; filters: Filter[] }
const status: Ids = { id: 'status', filters: ['notInstalled', 'update', 'downloading'] }
const rest: Ids[] = [
  { id: 'playtime', filters: ['neverPlayed', 'under10h', 'played10to50h', 'played50hPlus'] },
  { id: 'ttb', filters: ['ttbUnder5h', 'ttb5to15h', 'ttb15to40h', 'ttb40to100h', 'ttb100hPlus'] },
  { id: 'has', filters: ['bothStores'] },
]

/** The popover's groups, in order, the Lists group made from `collections`. Filters in one group widen each other (or); groups narrow (and).
 * Has (more than one store) is left out when the library uses fewer than two stores (`multiStore` false). */
export function filterGroups(collections: readonly Collection[], multiStore = true): Group[] {
  const lists: Ids = { id: 'lists', filters: ['favorites', ...collections.map((c) => collectionFilter(c.id)), 'hidden'] }
  return [status, lists, ...rest]
    .filter((g) => multiStore || g.id !== 'has')
    .map((g) => ({ label: t(`filters.group.${g.id}`), filters: g.filters }))
}

/** The toolbar's store segments for the stores the library uses, null being All; none when there is only one store. */
export function storeSegments<S extends string>(inUse: readonly S[]): (S | null)[] {
  return inUse.length > 1 ? [null, ...inUse] : []
}

/** A collection is named after itself; one that no longer exists has no name. */
export function filterName(filter: Filter, collections: readonly Collection[]): string {
  if (!filter.startsWith('collection-')) return t(`filters.${filter}`)
  return collections.find((c) => collectionFilter(c.id) === filter)?.name ?? ''
}

/** An active filter as the toolbar shows it; removing it clears exactly `filter`. */
export interface Chip {
  filter: Filter
  label: string
}

/** The toolbar chips for the filters that are on, in popover order. */
export function filterChips(on: readonly Filter[], collections: readonly Collection[] = []): Chip[] {
  return filterGroups(collections)
    .flatMap((g) => g.filters.filter((f) => on.includes(f)))
    .map((filter) => ({ filter, label: filterName(filter, collections) }))
}

/** The filters left after the chip's ✕. */
export function removeChip(on: readonly Filter[], chip: Chip): Filter[] {
  return on.filter((f) => f !== chip.filter)
}

export function toggleFilter(on: readonly Filter[], filter: Filter): Filter[] {
  return on.includes(filter) ? on.filter((f) => f !== filter) : [...on, filter]
}

/** What a filter looks at on a library card. */
export interface Filterable {
  installed: boolean
  update: boolean
  stores: readonly string[]
  copyKeys: readonly string[]
  /** Unknown (null) counts as never played. */
  playtimeMinutes: number | null
  /** How long to beat in the hours the card shows; unknown (null) matches no range. */
  ttbHours: number | null
  favorite: boolean
  hidden: boolean
  /** Ids of the collections it is in. */
  collections: readonly number[]
}

/** Whether `value` is known and in [lo, hi). */
const within = (value: number | null, lo: number, hi = Infinity) => value !== null && value >= lo && value < hi

function test(filter: Filter, game: Filterable, progress: Record<string, unknown>): boolean {
  switch (filter) {
    case 'notInstalled':
      return !game.installed
    case 'update':
      return game.update
    case 'downloading':
      return game.copyKeys.some((k) => k in progress)
    case 'neverPlayed':
      return !game.playtimeMinutes
    case 'under10h':
      return within(game.playtimeMinutes, 1, 600)
    case 'played10to50h':
      return within(game.playtimeMinutes, 600, 3000)
    case 'played50hPlus':
      return within(game.playtimeMinutes, 3000)
    case 'ttbUnder5h':
      return within(game.ttbHours, 0, 5)
    case 'ttb5to15h':
      return within(game.ttbHours, 5, 15)
    case 'ttb15to40h':
      return within(game.ttbHours, 15, 40)
    case 'ttb40to100h':
      return within(game.ttbHours, 40, 100)
    case 'ttb100hPlus':
      return within(game.ttbHours, 100)
    case 'bothStores':
      return game.stores.length > 1
    case 'favorites':
      return game.favorite
    case 'hidden':
      return game.hidden
    default:
      return game.collections.includes(Number(filter.slice('collection-'.length)))
  }
}

/** Whether the game passes the filters that are on; `progress` is `poll`'s map, by copy key. */
export function matches(game: Filterable, on: readonly Filter[], progress: Record<string, unknown>): boolean {
  return [status.filters, on.filter(isList), ...rest.map((g) => g.filters)].every((filters) => {
    const active = filters.filter((f) => on.includes(f))
    return active.length === 0 || active.some((f) => test(f, game, progress))
  })
}
