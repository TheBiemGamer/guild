// node --test scripts/
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  groupLabel,
  lastPlayedLabel,
  playtimeLabel,
  progressLong,
  progressShort,
  sizeLabel,
  syncedLabel,
  ttbLabel,
  type Progress,
} from '../src/lib/format.ts'

const GiB = 2 ** 30
const p = (over: Partial<Progress>): Progress => ({
  fraction: 0.5,
  action: 'install',
  doneBytes: 0,
  totalBytes: null,
  secsLeft: null,
  doneFiles: null,
  totalFiles: null,
  ...over,
})

test('short progress line rounds the time left and leaves it out while unknown', () => {
  assert.equal(progressShort(p({ doneBytes: 3 * 2 ** 29 })), '50% · 1.5 GB')
  assert.equal(progressShort(p({ secsLeft: 0 })), '50% · 0.0 GB · 0 s left')
  assert.equal(progressShort(p({ secsLeft: 13 })), '50% · 0.0 GB · 13 s left')
  assert.equal(progressShort(p({ secsLeft: 60 })), '50% · 0.0 GB · 1 min left')
  assert.equal(progressShort(p({ secsLeft: 61 })), '50% · 0.0 GB · 2 min left')
  assert.equal(progressShort(p({ secsLeft: 4800 })), '50% · 0.0 GB · 1 h 20 min left')
  assert.equal(progressShort(p({ fraction: 0.1234, doneBytes: 2 * GiB, secsLeft: 65 })), '12% · 2.0 GB · 2 min left')
  // Steam's total isn't in the short line.
  assert.equal(progressShort(p({ action: 'download', fraction: 0.25, doneBytes: GiB / 2, totalBytes: 2 * GiB, secsLeft: 6 })), '25% · 0.5 GB · 6 s left')
})

test('short line names repair and verify only', () => {
  assert.equal(progressShort(p({ action: 'update', fraction: 0 })), '0% · 0.0 GB')
  assert.equal(progressShort(p({ action: 'repair', doneBytes: 2 * GiB, secsLeft: 120 })), 'Repairing 50% · 2.0 GB · 2 min left')
  const verify = p({ action: 'verify', fraction: 0.035, doneBytes: null, doneFiles: 12, totalFiles: 345 })
  assert.equal(progressShort(verify), 'Verifying 4% · 12 of 345 files')
  assert.equal(progressLong(verify), 'Verifying 4% · 12 of 345 files')
})

test('long progress line: verb, bytes of the total when known, time', () => {
  const steam = p({ action: 'download', doneBytes: 0.7 * GiB, totalBytes: 1.9 * GiB, secsLeft: 61 })
  assert.equal(progressLong(steam), 'Downloading 50% · 0.7 of 1.9 GB · 2 min left')
  assert.equal(progressLong(p({ doneBytes: 0.7 * GiB, secsLeft: 13 })), 'Installing 50% · 0.7 GB downloaded · 13 s left')
  assert.equal(progressLong(p({ action: 'update', doneBytes: 0.7 * GiB })), 'Updating 50% · 0.7 GB downloaded')
  assert.equal(progressLong(p({ action: 'repair', fraction: 1, doneBytes: null })), 'Repairing 100% · 0.0 GB downloaded')
})

test('sizes: kilobytes rounded up, whole megabytes, one decimal of a gigabyte', () => {
  assert.equal(sizeLabel(0), '0 KB')
  assert.equal(sizeLabel(100), '1 KB')
  assert.equal(sizeLabel(500 * 1024), '500 KB')
  assert.equal(sizeLabel(2 ** 20 - 1), '1024 KB')
  assert.equal(sizeLabel(2 ** 20), '1 MB')
  assert.equal(sizeLabel(812 * 2 ** 20 + 5), '812 MB')
  assert.equal(sizeLabel(GiB), '1.0 GB')
  assert.equal(sizeLabel((27_300 * GiB) / 1000), '27.3 GB')
})

test('playtime tile', () => {
  assert.equal(playtimeLabel(0), 'Never played')
  assert.equal(playtimeLabel(1), '1 min')
  assert.equal(playtimeLabel(45), '45 min')
  assert.equal(playtimeLabel(60), '1 h 0 min')
  assert.equal(playtimeLabel(903), '15 h 3 min')
})

test('last played counts whole days since, months of 30 and years of 365', () => {
  const now = 1_000 * 86_400
  const daysAgo = (d: number) => lastPlayedLabel(now - d * 86_400, now)
  assert.equal(daysAgo(0), 'Today')
  assert.equal(lastPlayedLabel(now - 86_399, now), 'Today')
  assert.equal(daysAgo(1), 'Yesterday')
  assert.equal(daysAgo(2), '2 days ago')
  assert.equal(daysAgo(29), '29 days ago')
  assert.equal(daysAgo(30), '1 month ago')
  assert.equal(daysAgo(65), '2 months ago')
  assert.equal(daysAgo(365), '1 year ago')
  assert.equal(daysAgo(800), '2 years ago')
  // A clock that went backwards: today.
  assert.equal(lastPlayedLabel(now + 60, now), 'Today')
})

test('saves sync time', () => {
  assert.equal(syncedLabel(null, 0), 'Not synced yet')
  assert.equal(syncedLabel(1_000, 1_005), 'Synced just now')
  assert.equal(syncedLabel(1_000, 970), 'Synced just now')
  assert.equal(syncedLabel(1_000, 1_300), 'Synced 5 min ago')
  assert.equal(syncedLabel(0, 3 * 3_600 + 59), 'Synced 3 h ago')
  assert.equal(syncedLabel(0, 86_400), 'Synced 1 day ago')
  assert.equal(syncedLabel(0, 40 * 86_400), 'Synced 40 days ago')
})

test('how long to beat: minutes under an hour, else the nearest hour', () => {
  assert.equal(ttbLabel(60), '1 min')
  assert.equal(ttbLabel(2_700), '45 min')
  assert.equal(ttbLabel(3_600), '1 h')
  assert.equal(ttbLabel(5_400), '2 h')
  assert.equal(ttbLabel(115_200), '32 h')
})

test('group names', () => {
  assert.equal(groupLabel('thisWeek'), 'This week')
  assert.equal(groupLabel('played100h'), '100–500 h')
  assert.equal(groupLabel('beat0h'), 'Under 5 h')
  assert.equal(groupLabel('neverPlayed'), 'Never played')
})
