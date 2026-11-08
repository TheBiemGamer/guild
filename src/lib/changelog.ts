// CHANGELOG.md sections, for release notes and the update notes. No imports, so `node --test scripts/`
// and the release scripts can load it as plain TypeScript.

/** The body of `version`'s `## x.y.z` section (a leading v on either side, and a comma or anything after a space, is ignored), or null. */
export function section(changelog: string, version: string): string | null {
  const want = version.replace(/^v/, '')
  const lines = changelog.split(/\r?\n/)
  const start = lines.findIndex((l) => /^##\s+v?([^\s,]+)/.exec(l)?.[1] === want)
  if (start < 0) return null
  const rest = lines.slice(start + 1)
  const end = rest.findIndex((l) => l.startsWith('## '))
  const body = (end < 0 ? rest : rest.slice(0, end)).join('\n').trim()
  return body || null
}

/** `### Title` opens a group and `- item` adds to it; other lines and empty groups are dropped. */
export function parseNotes(body: string): { title: string; items: string[] }[] {
  const groups: { title: string; items: string[] }[] = []
  for (const line of body.split(/\r?\n/)) {
    const heading = /^###\s+(.+)/.exec(line)
    const item = /^-\s+(.+)/.exec(line)
    if (heading) groups.push({ title: heading[1].trim(), items: [] })
    else if (item && groups.length) groups[groups.length - 1].items.push(item[1].trim())
  }
  return groups.filter((g) => g.items.length)
}
