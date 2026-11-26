// The sidebar's pure view logic. `node --test scripts/` loads it as plain TypeScript: types only from Svelte-side modules.

import type { Collection, Counts, PlayStatus, Progress, QueueView } from './bindings.ts'
import { collectionFilter, type Filter, type ListFilter } from './filters.ts'

export type Pop = 'filters' | 'downloads' | 'sort' | 'update'
/** Below this window width the sidebar is a rail. */
export const RAIL_BELOW = 1100

/** The sidebar row that is selected: a list, or null for the Library. */
export type View = ListFilter | null

/** The filters `matches()` gets: the toolbar's, plus the view's list. */
export const withView = (filters: readonly Filter[], view: View): Filter[] => (view ? [...filters, view] : [...filters])

/** What `library(lists)` gets. */
export const viewLists = (view: View): ListFilter[] => (view ? [view] : [])

export function viewCount(view: View, counts: Counts, collections: readonly Collection[]): number {
  if (view === null) return counts.library
  if (view === 'favorites') return counts.favorites
  if (view === 'hidden') return counts.hidden
  if (view.startsWith('status-')) return counts[view.slice('status-'.length) as PlayStatus]
  return collections.find((c) => collectionFilter(c.id) === view)?.count ?? 0
}

/** The view itself, or the Library when its row is gone (an empty Favorites or Hidden, a deleted collection). */
export function fallbackView(view: View, counts: Counts, collections: readonly Collection[]): View {
  if (view === 'favorites') return counts.favorites > 0 ? view : null
  if (view === 'hidden') return counts.hidden > 0 ? view : null
  if (view === 'status-playing' || view === 'status-beaten' || view === 'status-dropped') return viewCount(view, counts, collections) > 0 ? view : null
  if (view === 'status-backlog') return view
  if (view !== null && !collections.some((c) => collectionFilter(c.id) === view)) return null
  return view
}

/** The active Epic job's progress, else the average of all jobs, else 0. */
export function downloadFraction(progress: Record<string, Progress>, queue: QueueView): number {
  const jobs = Object.values(progress)
  const active = queue.active ? progress[queue.active] : undefined
  return active ? active.fraction : jobs.length ? jobs.reduce((sum, p) => sum + p.fraction, 0) / jobs.length : 0
}
