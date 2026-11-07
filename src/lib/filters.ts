// The library's extra filters (the Filters popover) and their toolbar chips. No imports, so
// `node --test scripts/` can load it as plain TypeScript.

export type Filter = 'notInstalled' | 'update' | 'downloading' | 'bothStores'

/** The popover's groups, in order. Filters in one group widen each other (or); groups narrow (and). */
export const filterGroups: { label: string; filters: Filter[] }[] = [
  { label: 'Status', filters: ['notInstalled', 'update', 'downloading'] },
  { label: 'Has', filters: ['bothStores'] },
]

export const filterNames: Record<Filter, string> = {
  notInstalled: 'Not installed',
  update: 'Update available',
  downloading: 'Downloading',
  bothStores: 'On both stores',
}

/** An active filter as the toolbar shows it; removing it clears exactly `filter`. */
export interface Chip {
  filter: Filter
  label: string
}

/** The toolbar chips for the filters that are on, in popover order. */
export function filterChips(on: readonly Filter[]): Chip[] {
  return filterGroups.flatMap((g) => g.filters.filter((f) => on.includes(f))).map((filter) => ({ filter, label: filterNames[filter] }))
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
}

function test(filter: Filter, game: Filterable, progress: Record<string, unknown>): boolean {
  switch (filter) {
    case 'notInstalled':
      return !game.installed
    case 'update':
      return game.update
    case 'downloading':
      return game.copyKeys.some((k) => k in progress)
    case 'bothStores':
      return game.stores.length > 1
  }
}

/** Whether the game passes the filters that are on; `progress` is `poll`'s map, by copy key. */
export function matches(game: Filterable, on: readonly Filter[], progress: Record<string, unknown>): boolean {
  return filterGroups.every((g) => {
    const active = g.filters.filter((f) => on.includes(f))
    return active.length === 0 || active.some((f) => test(f, game, progress))
  })
}
