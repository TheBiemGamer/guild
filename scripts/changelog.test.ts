// node --test scripts/
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { parseNotes, section } from '../src/lib/changelog.ts'

const log = [
  '# Changelog',
  '',
  '## 0.2.0 (unreleased)',
  '### Fixed',
  '- Covers stay while scrolling',
  '',
  '## v0.1.0, 2026-11-07',
  '### New',
  '- One library',
  '',
].join('\r\n')

test('section returns one version, CRLF or not, with or without a v', () => {
  assert.equal(section(log, '0.2.0'), '### Fixed\n- Covers stay while scrolling')
  assert.equal(section(log, 'v0.1.0'), '### New\n- One library')
  assert.equal(section(log.replaceAll('\r\n', '\n'), '0.1.0'), '### New\n- One library')
})

test('section is null for a missing or empty version', () => {
  assert.equal(section(log, '0.3.0'), null)
  assert.equal(section(log, '0.1'), null)
  assert.equal(section('## 1.0.0\n\n## 0.9.0\n- x', '1.0.0'), null)
  assert.equal(section('', '1.0.0'), null)
})

test('parseNotes groups bullets under their heading', () => {
  assert.deepEqual(parseNotes('- before any heading\n### New\n- a\ntext\n-  b \n### Empty\n### Fixed\r\n- c'), [
    { title: 'New', items: ['a', 'b'] },
    { title: 'Fixed', items: ['c'] },
  ])
  assert.deepEqual(parseNotes(''), [])
})
