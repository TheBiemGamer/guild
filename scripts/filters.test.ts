// node --test scripts/
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { collectionFilter, filterChips, filterGroups, isList, matches, removeChip, storeSegments, toggleFilter, type Filter } from '../src/lib/filters.ts'

test('chips follow the popover order, one per filter that is on', () => {
  assert.deepEqual(filterChips([]), [])
  assert.deepEqual(filterChips(['bothStores', 'notInstalled']), [
    { filter: 'notInstalled', label: 'Not installed' },
    { filter: 'bothStores', label: 'On more than one store' },
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

const game = {
  installed: true,
  update: false,
  stores: ['steam'],
  copyKeys: ['steam-1'],
  playtimeMinutes: 60,
  ttbHours: 10,
  favorite: false,
  hidden: false,
  collections: [] as number[],
}

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
  assert.deepEqual(filterChips(['ttb100hPlus', 'neverPlayed', 'bothStores']).map((c) => c.label), ['Never played', '100 h+', 'On more than one store'])
})

const collections = [
  { id: 3, name: 'Backlog', count: 2 },
  { id: 7, name: 'Co-op', count: 1 },
]

test('Lists come after Status: Favorites, each collection, then Hidden', () => {
  const lists = filterGroups(collections)[1]
  assert.deepEqual(lists, { label: 'Lists', filters: ['favorites', 'collection-3', 'collection-7', 'hidden'] })
  assert.deepEqual(filterGroups([])[1].filters, ['favorites', 'hidden'])
  assert.deepEqual(filterGroups([]).map((g) => g.label), ['Status', 'Lists', 'Your playtime', 'How long to beat', 'Has'])
})

test('list chips are named after the list and come after Status', () => {
  const on: Filter[] = ['bothStores', 'hidden', collectionFilter(7), 'notInstalled', 'favorites']
  assert.deepEqual(filterChips(on, collections).map((c) => c.label), ['Not installed', 'Favorites', 'Co-op', 'Hidden', 'On more than one store'])
  const left = removeChip(on, filterChips(on, collections)[2])
  assert.deepEqual(left, ['bothStores', 'hidden', 'notInstalled', 'favorites'])
  assert.ok(isList('collection-7') && isList('hidden') && !isList('update'))
})

test('lists widen each other and narrow the other groups', () => {
  const fav = { ...game, favorite: true }
  const inCoop = { ...game, collections: [7] }
  const hidden = { ...game, hidden: true, installed: false }
  assert.ok(matches(fav, ['favorites'], {}))
  assert.ok(!matches(inCoop, ['favorites'], {}))
  assert.ok(matches(inCoop, ['favorites', collectionFilter(7)], {}))
  assert.ok(!matches(inCoop, [collectionFilter(3)], {}))
  assert.ok(matches(hidden, ['hidden'], {}))
  assert.ok(!matches(game, ['hidden'], {}))
  assert.ok(matches(hidden, ['hidden', 'notInstalled'], {}))
  assert.ok(!matches(fav, ['favorites', 'notInstalled'], {}))
})

test('more than one store means two or more, of any stores', () => {
  assert.ok(!matches(game, ['bothStores'], {}))
  assert.ok(matches({ ...game, stores: ['gog', 'amazon'] }, ['bothStores'], {}))
  assert.ok(matches({ ...game, stores: ['steam', 'epic', 'gog'] }, ['bothStores'], {}))
})

test('the Has group shows only with more than one store in use', () => {
  const ids = (multi: boolean) => filterGroups([], multi).flatMap((g) => g.filters)
  assert.ok(ids(true).includes('bothStores'))
  assert.ok(!ids(false).includes('bothStores'))
  assert.equal(filterGroups([], false).length, filterGroups([]).length - 1)
})

test('store segments: All plus the stores in use, none for a single store', () => {
  assert.deepEqual(storeSegments([]), [])
  assert.deepEqual(storeSegments(['steam']), [])
  assert.deepEqual(storeSegments(['steam', 'epic']), [null, 'steam', 'epic'])
  assert.deepEqual(storeSegments(['steam', 'epic', 'gog', 'amazon']), [null, 'steam', 'epic', 'gog', 'amazon'])
})
