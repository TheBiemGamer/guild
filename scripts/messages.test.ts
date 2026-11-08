// node --test scripts/
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { render } from '../src/lib/messages.ts'
import en from '../src/lib/i18n/en.json' with { type: 'json' }

const words: Record<string, string> = en

const rustDir = join(import.meta.dirname, '..', 'src-tauri', 'src')
const rust = readdirSync(rustDir, { recursive: true, encoding: 'utf8' })
  .filter((f) => f.endsWith('.rs'))
  .map((f) => readFileSync(join(rustDir, f), 'utf8'))
  .join('\n')

test('every code Rust sends has English words', () => {
  const codes = new Set([...rust.matchAll(/msg!\(\s*(\w+)/g), ...rust.matchAll(/Msg::new\("(\w+)"\)/g)].map((m) => m[1]))
  assert.ok(codes.size > 100, `found only ${codes.size} codes`)
  const missing = [...codes].filter((c) => !(`msg.${c}` in words))
  assert.deepEqual(missing, [])
})

/** The rows of msg.rs's native table `name`, as [code, text]. */
function nativeRows(name: string) {
  const table = rust.slice(rust.indexOf(`const ${name}: &[(&str, &str)]`))
  return [...table.slice(0, table.indexOf('];')).matchAll(/\("(\w+)", "([^"]*)"\)/g)].map((m) => [m[1], m[2]])
}

test('the native English table matches en.json', () => {
  const rows = nativeRows('EN')
  assert.ok(rows.length > 5)
  for (const [code, text] of rows) assert.equal(words[`msg.${code}`], text, code)
  // `native` answers "" for a code it doesn't know.
  const known = new Set(rows.map((r) => r[0]))
  const unknown = [...rust.matchAll(/native\("(\w+)"\)/g)].map((m) => m[1]).filter((c) => !known.has(c))
  assert.deepEqual(unknown, [])
})

const nlFile = join(import.meta.dirname, '..', 'src', 'lib', 'i18n', 'nl.json')
test('the native Dutch table matches nl.json', { skip: !existsSync(nlFile) }, () => {
  const nl: Record<string, string> = JSON.parse(readFileSync(nlFile, 'utf8'))
  const rows = nativeRows('NL')
  assert.deepEqual(
    rows.map((r) => r[0]),
    nativeRows('EN').map((r) => r[0]),
    'every native code has Dutch text',
  )
  for (const [code, text] of rows) assert.equal(nl[`msg.${code}`], text, code)
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
