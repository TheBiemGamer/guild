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

const game = { installed: true, update: false, stores: ['steam'], copyKeys: ['steam-1'], playtimeMinutes: 60, ttbHours: 10 }

test('filters in a group widen, groups narrow', () => {
  assert.ok(matches(game, [], {}))
  assert.ok(!matches(game, ['notInstalled'], {}))
  assert.ok(matches(game, ['notInstalled', 'downloading'], { 'steam-1': {} }))
  assert.ok(!matches(game, ['downloading', 'bothStores'], { 'steam-1': {} }))
  assert.ok(matches({ ...game, stores: ['steam', 'epic'], copyKeys: ['steam-1', 'epic-a'] }, ['downloading', 'bothStores'], { 'epic-a': {} }))
  assert.ok(matches({ ...game, update: true }, ['update'], {}))
})

const played = (playtimeMinutes: number | null, ttbHours: number | null = null) => ({ ...game, playtimeMinutes, ttbHours })

test('playtime ranges: never played includes unknown, edges go up', () => {
  assert.ok(matches(played(null), ['neverPlayed'], {}))
  assert.ok(matches(played(0), ['neverPlayed'], {}))
  assert.ok(!matches(played(1), ['neverPlayed'], {}))
  assert.ok(matches(played(1), ['under10h'], {}))
  assert.ok(matches(played(599), ['under10h'], {}))
  assert.ok(!matches(played(600), ['under10h'], {}))
  assert.ok(matches(played(600), ['played10to50h'], {}))
  assert.ok(matches(played(2999), ['played10to50h'], {}))
  assert.ok(matches(played(3000), ['played50hPlus'], {}))
  assert.ok(!matches(played(0), ['under10h', 'played10to50h', 'played50hPlus'], {}))
})

test('time to beat ranges on the shown hours; unknown matches none', () => {
  const ranges: [Filter, number, number][] = [
    ['ttbUnder5h', 1, 4],
    ['ttb5to15h', 5, 14],
    ['ttb15to40h', 15, 39],
    ['ttb40to100h', 40, 99],
    ['ttb100hPlus', 100, 1000],
  ]
  for (const [f, lo, hi] of ranges) {
    assert.ok(matches(played(0, lo), [f], {}), `${f} ${lo}`)
    assert.ok(matches(played(0, hi), [f], {}), `${f} ${hi}`)
    assert.ok(!matches(played(0, lo - 1), [f], {}) || lo === 1, `${f} below`)
    assert.ok(!matches(played(0, hi + 1), [f], {}) || hi === 1000, `${f} above`)
    assert.ok(!matches(played(0, null), [f], {}))
  }
})

test('the new groups widen inside and narrow across', () => {
  assert.ok(matches(played(30, 3), ['neverPlayed', 'under10h'], {}))
  assert.ok(!matches(played(30, 3), ['under10h', 'ttb5to15h'], {}))
  assert.ok(matches(played(30, 3), ['under10h', 'ttb5to15h', 'ttbUnder5h'], {}))
  assert.deepEqual(filterChips(['ttb100hPlus', 'neverPlayed', 'bothStores']).map((c) => c.label), ['Never played', '100 h+', 'On both stores'])
})
