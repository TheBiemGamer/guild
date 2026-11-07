// The library's extra filters (the Filters popover) and their toolbar chips. No imports, so
// `node --test scripts/` can load it as plain TypeScript.

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

/** The popover's groups, in order. Filters in one group widen each other (or); groups narrow (and). */
export const filterGroups: { label: string; filters: Filter[] }[] = [
  { label: 'Status', filters: ['notInstalled', 'update', 'downloading'] },
  { label: 'Your playtime', filters: ['neverPlayed', 'under10h', 'played10to50h', 'played50hPlus'] },
  { label: 'How long to beat', filters: ['ttbUnder5h', 'ttb5to15h', 'ttb15to40h', 'ttb40to100h', 'ttb100hPlus'] },
  { label: 'Has', filters: ['bothStores'] },
]

export const filterNames: Record<Filter, string> = {
  notInstalled: 'Not installed',
  update: 'Update available',
  downloading: 'Downloading',
  neverPlayed: 'Never played',
  under10h: 'Under 10 h',
  played10to50h: '10–50 h',
  played50hPlus: '50 h+',
  ttbUnder5h: 'Under 5 h',
  ttb5to15h: '5–15 h',
  ttb15to40h: '15–40 h',
  ttb40to100h: '40–100 h',
  ttb100hPlus: '100 h+',
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
  /** Unknown (null) counts as never played. */
  playtimeMinutes: number | null
  /** How long to beat in the hours the card shows; unknown (null) matches no range. */
  ttbHours: number | null
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
  }
}

/** Whether the game passes the filters that are on; `progress` is `poll`'s map, by copy key. */
export function matches(game: Filterable, on: readonly Filter[], progress: Record<string, unknown>): boolean {
  return filterGroups.every((g) => {
    const active = g.filters.filter((f) => on.includes(f))
    return active.length === 0 || active.some((f) => test(f, game, progress))
  })
}
