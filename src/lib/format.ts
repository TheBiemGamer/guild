// Every label made from the numbers Rust sends: progress, sizes, times and group names. No imports,
// so `node --test scripts/` can load it as plain TypeScript.

export type ProgressAction = 'download' | 'install' | 'update' | 'repair' | 'verify'
/** A job in progress. Steam's are all `download`, with `totalBytes`; Epic's bytes have no total; verify counts files. */
export interface Progress {
  /** 0 to 1. */
  fraction: number
  action: ProgressAction
  doneBytes: number | null
  totalBytes: number | null
  /** Null until known. */
  secsLeft: number | null
  doneFiles: number | null
  totalFiles: number | null
}

/** Group ids from Rust's `view::group`, in its order per sort, and `favorites` and `allGames` from `view::favorites_first`. */
export type GroupId =
  | 'favorites'
  | 'allGames'
  | 'today'
  | 'thisWeek'
  | 'thisMonth'
  | 'earlier'
  | 'neverPlayed'
  | 'played500h'
  | 'played100h'
  | 'played50h'
  | 'played20h'
  | 'played10h'
  | 'played0h'
  | 'beat0h'
  | 'beat5h'
  | 'beat15h'
  | 'beat40h'
  | 'beat100h'
  | 'unknown'

const GROUPS: Record<GroupId, string> = {
  favorites: 'Favorites',
  allGames: 'All games',
  today: 'Today',
  thisWeek: 'This week',
  thisMonth: 'This month',
  earlier: 'Earlier',
  neverPlayed: 'Never played',
  played500h: '500 h+',
  played100h: '100–500 h',
  played50h: '50–100 h',
  played20h: '20–50 h',
  played10h: '10–20 h',
  played0h: '0–10 h',
  beat0h: 'Under 5 h',
  beat5h: '5–15 h',
  beat15h: '15–40 h',
  beat40h: '40–100 h',
  beat100h: '100 h+',
  unknown: 'Unknown',
}
export const groupLabel = (id: GroupId) => GROUPS[id]

const KB = 1024
const MB = 1024 * KB
const GB = 1024 * MB
const gb = (bytes: number) => (bytes / GB).toFixed(1)
const percent = (p: Progress) => `${Math.round(p.fraction * 100)}%`
/** " · 13 s left", " · 2 min left" (rounded up), " · 1 h 20 min left"; empty while unknown. */
function left(secs: number | null): string {
  if (secs === null) return ''
  const time = secs < 60 ? `${secs} s` : secs < 3600 ? `${Math.ceil(secs / 60)} min` : `${Math.floor(secs / 3600)} h ${Math.floor((secs % 3600) / 60)} min`
  return ` · ${time} left`
}
const verifying = (p: Progress) => `Verifying ${percent(p)} · ${p.doneFiles} of ${p.totalFiles} files`

/** The compact line (Downloads popover): "50% · 0.7 GB · 13 s left", "Repairing 50% · …", "Verifying 40% · 120 of 300 files". */
export function progressShort(p: Progress): string {
  if (p.action === 'verify') return verifying(p)
  return `${p.action === 'repair' ? 'Repairing ' : ''}${percent(p)} · ${gb(p.doneBytes ?? 0)} GB${left(p.secsLeft)}`
}

const VERBS: Record<Exclude<ProgressAction, 'verify'>, string> = {
  download: 'Downloading',
  install: 'Installing',
  update: 'Updating',
  repair: 'Repairing',
}
/** The game page's line: "Downloading 50% · 0.7 of 1.9 GB · 1 min left" (Steam), "Installing 50% · 0.7 GB downloaded · 13 s left". */
export function progressLong(p: Progress): string {
  if (p.action === 'verify') return verifying(p)
  const done = gb(p.doneBytes ?? 0)
  const bytes = p.totalBytes === null ? `${done} GB downloaded` : `${done} of ${gb(p.totalBytes)} GB`
  return `${VERBS[p.action]} ${percent(p)} · ${bytes}${left(p.secsLeft)}`
}

/** "27.3 GB", whole megabytes under 1 GB, whole kilobytes (rounded up, so only empty is "0 KB") under 1 MB. */
export function sizeLabel(bytes: number): string {
  if (bytes < MB) return `${Math.ceil(bytes / KB)} KB`
  if (bytes < GB) return `${Math.floor(bytes / MB)} MB`
  return `${gb(bytes)} GB`
}

/** Playtime tile: "Never played", "45 min", "15 h 3 min". */
export function playtimeLabel(minutes: number): string {
  if (minutes === 0) return 'Never played'
  return minutes < 60 ? `${minutes} min` : `${Math.floor(minutes / 60)} h ${minutes % 60} min`
}

const ago = (n: number, unit: string) => `${n} ${unit}${n === 1 ? '' : 's'} ago`
/** Last played tile, by whole 24 h periods since `at` (months 30 days, years 365): "Today", "Yesterday", "12 days ago". */
export function lastPlayedLabel(at: number, now = Date.now() / 1000): string {
  const days = Math.floor(Math.max(0, now - at) / 86_400)
  if (days === 0) return 'Today'
  if (days === 1) return 'Yesterday'
  if (days < 30) return ago(days, 'day')
  if (days < 365) return ago(Math.floor(days / 30), 'month')
  return ago(Math.floor(days / 365), 'year')
}

/** Saves tab: "Synced just now", "Synced 5 min ago", "Synced 3 h ago", "Synced 2 days ago", or "Not synced yet". */
export function syncedLabel(at: number | null, now = Date.now() / 1000): string {
  if (at === null) return 'Not synced yet'
  const s = Math.max(0, Math.floor(now - at))
  if (s < 60) return 'Synced just now'
  if (s < 3_600) return `Synced ${Math.floor(s / 60)} min ago`
  if (s < 86_400) return `Synced ${Math.floor(s / 3_600)} h ago`
  return `Synced ${ago(Math.floor(s / 86_400), 'day')}`
}

/** How long to beat: "32 h" (nearest hour), "45 min" under an hour. */
export function ttbLabel(secs: number): string {
  return secs < 3600 ? `${Math.floor(secs / 60)} min` : `${Math.floor((secs + 1800) / 3600)} h`
}
