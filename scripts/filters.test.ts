// node --test scripts/
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { filterChips, matches, removeChip, toggleFilter, type Filter } from '../src/lib/filters.ts'

test('chips follow the popover order, one per filter that is on', () => {
  assert.deepEqual(filterChips([]), [])
  assert.deepEqual(filterChips(['bothStores', 'notInstalled']), [
    { filter: 'notInstalled', label: 'Not installed' },
    { filter: 'bothStores', label: 'On both stores' },
  ])
})

test('removing a chip clears exactly that filter', () => {
  const on: Filter[] = ['notInstalled', 'update', 'bothStores']
  for (const chip of filterChips(on)) {
    const left = removeChip(on, chip)
    assert.deepEqual(left, on.filter((f) => f !== chip.filter))
    assert.deepEqual(filterChips(left).map((c) => c.filter), filterChips(on).map((c) => c.filter).filter((f) => f !== chip.filter))
  }
})

test('toggle turns a filter on and off', () => {
  assert.deepEqual(toggleFilter([], 'update'), ['update'])
  assert.deepEqual(toggleFilter(['update', 'downloading'], 'update'), ['downloading'])
})

const game = { installed: true, update: false, stores: ['steam'], copyKeys: ['steam-1'] }

test('filters in a group widen, groups narrow', () => {
  assert.ok(matches(game, [], {}))
  assert.ok(!matches(game, ['notInstalled'], {}))
  assert.ok(matches(game, ['notInstalled', 'downloading'], { 'steam-1': {} }))
  assert.ok(!matches(game, ['downloading', 'bothStores'], { 'steam-1': {} }))
  assert.ok(matches({ ...game, stores: ['steam', 'epic'], copyKeys: ['steam-1', 'epic-a'] }, ['downloading', 'bothStores'], { 'epic-a': {} }))
  assert.ok(matches({ ...game, update: true }, ['update'], {}))
})
