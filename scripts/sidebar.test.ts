// node --test scripts/
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { downloadFraction, fallbackView, viewCount, viewLists, withView } from '../src/lib/sidebar.ts'
import type { Filter } from '../src/lib/filters.ts'
import type { Progress, QueueView } from '../src/lib/bindings.ts'

const counts = { library: 9, favorites: 2, hidden: 1 }
const collections = [{ id: 3, name: 'Backlog', count: 4, covers: [] }]

test('the view joins the filters matches() gets', () => {
  assert.deepEqual(withView(['notInstalled'], 'collection-3'), ['notInstalled', 'collection-3'])
  const f: Filter[] = ['update']
  assert.deepEqual(withView(f, null), f)
})

test('the library is asked for the view as its only list', () => {
  assert.deepEqual(viewLists(null), [])
  assert.deepEqual(viewLists('hidden'), ['hidden'])
})

test('a view counts its own games', () => {
  assert.equal(viewCount(null, counts, collections), 9)
  assert.equal(viewCount('favorites', counts, []), 2)
  assert.equal(viewCount('hidden', counts, []), 1)
  assert.equal(viewCount('collection-3', counts, collections), 4)
})

test('a view whose row is gone falls back to the library', () => {
  assert.equal(fallbackView('favorites', { ...counts, favorites: 0 }, []), null)
  assert.equal(fallbackView('favorites', counts, []), 'favorites')
  assert.equal(fallbackView('hidden', { ...counts, hidden: 0 }, []), null)
  assert.equal(fallbackView('collection-3', counts, []), null)
  assert.equal(fallbackView('collection-3', counts, collections), 'collection-3')
  assert.equal(fallbackView(null, counts, []), null)
})

const job = (fraction: number) => ({ fraction }) as Progress

test('the download fraction is the active job, else the average, else 0', () => {
  const queue = (active: string | null) => ({ jobs: [], paused: false, active }) as QueueView
  assert.equal(downloadFraction({ a: job(0.2), b: job(0.9) }, queue('b')), 0.9)
  assert.ok(Math.abs(downloadFraction({ a: job(0.2), b: job(0.6) }, queue(null)) - 0.4) < 1e-9)
  assert.equal(downloadFraction({}, queue(null)), 0)
  assert.equal(downloadFraction({ a: job(0.5) }, queue('gone')), 0.5)
})
