// node --test scripts/
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { around, english, t, use } from '../src/lib/i18n.ts'

const src = join(import.meta.dirname, '..', 'src')
const sources = readdirSync(src, { recursive: true, encoding: 'utf8' })
  .filter((f) => /\.(svelte|ts)$/.test(f))
  .map((f) => readFileSync(join(src, f), 'utf8'))
  .join('\n')
const keys = new Set(Object.keys(english))
const known = (key: string) => keys.has(key) || keys.has(`${key}_other`)

test('every key the UI asks for has English words', () => {
  const asked = [...sources.matchAll(/\b(?:t|around)\(\s*'([\w.]+)'/g)].map((m) => m[1])
  assert.ok(asked.length > 300, `found only ${asked.length} keys`)
  assert.deepEqual(
    [...new Set(asked)].filter((k) => !known(k)),
    [],
  )
  // Keys made from a value, like t(`group.${id}`): at least one key fits the pattern.
  const made = [...sources.matchAll(/\bt\(\s*`([\w.]*)\$\{[^}]+\}([\w.]*)`/g)]
  assert.ok(made.length > 5)
  const unmatched = made
    .map((m) => `${m[1]}…${m[2]}`)
    .filter((p) => {
      const [before, after] = p.split('…')
      return ![...keys].some((k) => k.startsWith(before) && k.endsWith(after) && k.length > before.length + after.length)
    })
  assert.deepEqual(unmatched, [])
})

test('plurals follow n and placeholders fill in', () => {
  assert.equal(t('lastPlayed.days', { n: 1 }), '1 day ago')
  assert.equal(t('lastPlayed.days', { n: 12 }), '12 days ago')
  // No plural forms: the key itself, n filled like any param.
  assert.equal(t('time.minutes', { n: 1 }), '1 min')
  assert.equal(t('install.question', { title: 'Hades' }), 'Install Hades?')
  // A param that isn't given stays as written; a value with braces isn't filled again.
  assert.equal(t('install.question'), 'Install {title}?')
  assert.equal(t('install.question', { title: '{title}' }), 'Install {title}?')
  assert.deepEqual(around('app.updatedTo', 'version'), ['Updated to ', ''])
  assert.equal(t('no.such.key'), 'no.such.key')
})

test('a language falls back to English for words it lacks', () => {
  try {
    use('nl', { 'install.question': '{title} installeren?', 'lastPlayed.days_other': '{n} dagen geleden' })
    assert.equal(t('install.question', { title: 'Hades' }), 'Hades installeren?')
    assert.equal(t('lastPlayed.days', { n: 3 }), '3 dagen geleden')
    // Missing in Dutch: the English form for that number.
    assert.equal(t('lastPlayed.days', { n: 1 }), '1 day ago')
    assert.equal(t('common.cancel'), 'Cancel')
  } finally {
    use('en', english)
  }
})

const placeholders = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort()

test('nl.json has exactly the English keys, with the same placeholders', { skip: !existsSync(join(src, 'lib', 'i18n', 'nl.json')) }, () => {
  const nl: Record<string, string> = JSON.parse(readFileSync(join(src, 'lib', 'i18n', 'nl.json'), 'utf8'))
  assert.deepEqual(
    [...keys].filter((k) => !(k in nl)),
    [],
    'missing in nl.json',
  )
  assert.deepEqual(
    Object.keys(nl).filter((k) => !keys.has(k)),
    [],
    'not in en.json',
  )
  // A singular may spell its number out ("een dag geleden").
  const fills = (key: string, text: string) => placeholders(text).filter((p) => !(key.endsWith('_one') && p === 'n'))
  for (const [key, text] of Object.entries(nl)) assert.deepEqual(fills(key, text), fills(key, english[key]), key)
})
