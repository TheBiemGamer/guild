// Every label made from the numbers Rust sends: progress, sizes, times and group names, plus the name a
// picked program suggests. The words come from the language files; `node --test scripts/` loads it as plain TypeScript.

import { oneDecimal, t } from './i18n.ts'
import type { Progress } from './bindings.ts'

export type { Progress }

/** `id` is a group id from Rust's `view::group`, in its order per sort, and `favorites` and `allGames` from `view::favorites_first`. */
export const groupLabel = (id: string) => t(`group.${id}`)

const KB = 1024
const MB = 1024 * KB
const GB = 1024 * MB
const gb = (bytes: number) => oneDecimal(bytes / GB)
const percent = (p: Progress) => `${Math.round(p.fraction * 100)}%`
/** "45 min", "1 h 20 min". */
const hoursMinutes = (minutes: number) =>
  minutes < 60 ? t('time.minutes', { n: minutes }) : t('time.hoursMinutes', { h: Math.floor(minutes / 60), m: minutes % 60 })
/** " · 13 s left", " · 2 min left" (rounded up), " · 1 h 20 min left"; empty while unknown. */
function left(secs: number | null): string {
  if (secs === null) return ''
  const time = secs < 60 ? t('time.seconds', { n: secs }) : secs < 3600 ? t('time.minutes', { n: Math.ceil(secs / 60) }) : hoursMinutes(Math.floor(secs / 60))
  return ` · ${t('progress.left', { time })}`
}
const verifying = (p: Progress) => t('progress.verify', { percent: percent(p), done: p.doneFiles ?? 0, total: p.totalFiles ?? 0 })

/** The compact line (Downloads popover): "50% · 0.7 GB · 13 s left", "Repairing 50% · …", "Verifying 40% · 120 of 300 files". */
export function progressShort(p: Progress): string {
  if (p.action === 'verify') return verifying(p)
  const head = p.action === 'repair' ? t('progress.repair', { percent: percent(p) }) : percent(p)
  return `${head} · ${t('size.gb', { size: gb(p.doneBytes ?? 0) })}${left(p.secsLeft)}`
}

/** The game page's line: "Downloading 50% · 0.7 of 1.9 GB · 1 min left" (Steam), "Installing 50% · 0.7 GB downloaded · 13 s left". */
export function progressLong(p: Progress): string {
  if (p.action === 'verify') return verifying(p)
  const done = gb(p.doneBytes ?? 0)
  const bytes = p.totalBytes === null ? t('progress.downloaded', { done }) : t('progress.of', { done, total: gb(p.totalBytes) })
  return `${t(`progress.${p.action}`, { percent: percent(p) })} · ${bytes}${left(p.secsLeft)}`
}

/** "27.3 GB", whole megabytes under 1 GB, whole kilobytes (rounded up, so only empty is "0 KB") under 1 MB. */
export function sizeLabel(bytes: number): string {
  if (bytes < MB) return t('size.kb', { size: Math.ceil(bytes / KB) })
  if (bytes < GB) return t('size.mb', { size: Math.floor(bytes / MB) })
  return t('size.gb', { size: gb(bytes) })
}

/** Playtime tile: "Never played", "45 min", "15 h 3 min". */
export function playtimeLabel(minutes: number): string {
  return minutes === 0 ? t('playtime.never') : hoursMinutes(minutes)
}

/** Last played tile, by whole 24 h periods since `at` (months 30 days, years 365): "Today", "Yesterday", "12 days ago". */
export function lastPlayedLabel(at: number, now = Date.now() / 1000): string {
  const days = Math.floor(Math.max(0, now - at) / 86_400)
  if (days === 0) return t('lastPlayed.today')
  if (days === 1) return t('lastPlayed.yesterday')
  if (days < 30) return t('lastPlayed.days', { n: days })
  if (days < 365) return t('lastPlayed.months', { n: Math.floor(days / 30) })
  return t('lastPlayed.years', { n: Math.floor(days / 365) })
}

/** Saves tab: "Synced just now", "Synced 5 min ago", "Synced 3 h ago", "Synced 2 days ago", or "Not synced yet". */
export function syncedLabel(at: number | null, now = Date.now() / 1000): string {
  if (at === null) return t('synced.never')
  const s = Math.max(0, Math.floor(now - at))
  if (s < 60) return t('synced.justNow')
  if (s < 3_600) return t('synced.minutes', { n: Math.floor(s / 60) })
  if (s < 86_400) return t('synced.hours', { n: Math.floor(s / 3_600) })
  return t('synced.days', { n: Math.floor(s / 86_400) })
}

/** How long to beat: "32 h" (nearest hour), "45 min" under an hour. */
export function ttbLabel(secs: number): string {
  return secs < 3600 ? t('time.minutes', { n: Math.floor(secs / 60) }) : t('time.hours', { n: Math.floor((secs + 1800) / 3600) })
}

/** The name the Add a game dialog suggests for a program: its file name without the extension. */
export function nameFromExe(path: string): string {
  const file = path.split(/[\\/]/).pop() ?? ''
  return file.replace(/\.[^.]*$/, '')
}
