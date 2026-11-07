// Prints a version's CHANGELOG.md section: `node scripts/release-notes.ts 0.2.0`. Exits 1 if it has none.
import { readFileSync } from 'node:fs'
import { section } from '../src/lib/changelog.ts'

const version = process.argv[2] ?? ''
const notes = section(readFileSync(new URL('../CHANGELOG.md', import.meta.url), 'utf8'), version)
if (notes === null) {
  console.error(`CHANGELOG.md has no section for ${version || '(no version given)'}`)
  process.exit(1)
}
console.log(notes)
