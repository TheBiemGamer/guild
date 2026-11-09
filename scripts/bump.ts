// Sets the app version in package.json, src-tauri/Cargo.toml, src-tauri/Cargo.lock and the AUR PKGBUILD: `npm run bump 0.2.0`.
import { execSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'

const version = process.argv[2] ?? ''
if (!/^\d+\.\d+\.\d+$/.test(version)) {
  console.error('Usage: npm run bump <x.y.z>')
  process.exit(1)
}

const root = new URL('..', import.meta.url)
execSync(`npm version ${version} --no-git-tag-version`, { cwd: root, stdio: 'inherit' })

function rewrite(file: string, pattern: RegExp, to: string) {
  const url = new URL(file, root)
  const text = readFileSync(url, 'utf8')
  if (!pattern.test(text)) throw new Error(`${file}: no version line found`)
  writeFileSync(url, text.replace(pattern, to))
}

rewrite('src-tauri/Cargo.toml', /^version = ".*"$/m, `version = "${version}"`)
rewrite('src-tauri/Cargo.lock', /^name = "launcher"\r?\nversion = ".*"$/m, `name = "launcher"\nversion = "${version}"`)
rewrite('packaging/aur/PKGBUILD', /^pkgver=.*$/m, `pkgver=${version}`)
// A new version starts its package release count over.
rewrite('packaging/aur/PKGBUILD', /^pkgrel=.*$/m, 'pkgrel=1')
console.log(`Version is now ${version}.`)
