// node --test scripts/
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { en, render } from '../src/lib/messages.ts'

const rustDir = join(import.meta.dirname, '..', 'src-tauri', 'src')
const rust = readdirSync(rustDir, { recursive: true, encoding: 'utf8' })
  .filter((f) => f.endsWith('.rs'))
  .map((f) => readFileSync(join(rustDir, f), 'utf8'))
  .join('\n')

test('every code Rust sends has English words', () => {
  const codes = new Set([...rust.matchAll(/msg!\(\s*(\w+)/g), ...rust.matchAll(/Msg::new\("(\w+)"\)/g)].map((m) => m[1]))
  assert.ok(codes.size > 100, `found only ${codes.size} codes`)
  const missing = [...codes].filter((c) => !(c in en))
  assert.deepEqual(missing, [])
})

test('the native table matches messages.ts', () => {
  const table = rust.slice(rust.indexOf('const EN: &[(&str, &str)]'))
  const rows = [...table.slice(0, table.indexOf('];')).matchAll(/\("(\w+)", "([^"]*)"\)/g)]
  assert.ok(rows.length > 5)
  for (const [, code, text] of rows) assert.equal(en[code], text, code)
  // `native` answers "" for a code it doesn't know.
  const known = new Set(rows.map((r) => r[1]))
  const unknown = [...rust.matchAll(/native\("(\w+)"\)/g)].map((m) => m[1]).filter((c) => !known.has(c))
  assert.deepEqual(unknown, [])
})

test('renders params, nested messages and plain text', () => {
  const reason = { code: 'closeGameFirst', params: {} }
  assert.equal(render({ code: 'storeError', params: { store: 'GOG', reason } }), 'GOG: Close the game first.')
  assert.equal(render({ code: 'system', params: { detail: 'Access is denied. {path}' } }), 'Access is denied. {path}')
  // A sync error stored before message codes.
  assert.equal(render({ code: 'savesDidntSync', params: { reason: 'Login failed!' } }), "Cloud saves didn't sync: Login failed!")
  assert.equal(render('plain text'), 'plain text')
  assert.equal(render({ code: 'noGamesFound' }), 'No games found.')
})
