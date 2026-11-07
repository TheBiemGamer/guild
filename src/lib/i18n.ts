// The UI's words. Every string the owner reads comes from a language file in `src/lib/i18n/` through `t()`;
// a key missing from the current language falls back to English. No Svelte here, so `node --test scripts/`
// loads it as plain TypeScript; `lang.svelte.ts` makes a language switch re-render.

import en from './i18n/en.json' with { type: 'json' }

/** `{name}` placeholders; `n` also picks the plural form (`key_one`, `key_other`, …). */
export type Params = Record<string, string | number>
export type Words = Record<string, string>

export const english: Words = en
let words: Words = en
let plural = new Intl.PluralRules('en')
let decimal = new Intl.NumberFormat('en', { minimumFractionDigits: 1, maximumFractionDigits: 1, useGrouping: false })
/** Read on every lookup: the Svelte side makes it read reactive state, so templates re-run on a switch. */
let track: () => unknown = () => {}
let warn = false
const warned = new Set<string>()

/** Hooks the reactive read in, and whether to log missing keys (dev builds). */
export function setup(read: () => unknown, dev: boolean) {
  track = read
  warn = dev
}

/** Switches to `lang` ("en", "nl") with its words; missing ones stay English. */
export function use(lang: string, table: Words) {
  words = table
  plural = new Intl.PluralRules(lang)
  decimal = new Intl.NumberFormat(lang, { minimumFractionDigits: 1, maximumFractionDigits: 1, useGrouping: false })
}

function template(key: string, n: unknown): string {
  track()
  if (typeof n === 'number') {
    const form = `${key}_${plural.select(n)}`
    const other = `${key}_other`
    if (form in words || form in en) key = form
    else if (other in words || other in en) key = other
  }
  const text = words[key]
  if (text !== undefined) return text
  if (warn && !warned.has(key)) {
    warned.add(key)
    console.warn(`i18n: no words for ${key}`)
  }
  return english[key] ?? key
}

/** Fills `{name}` from `params`; unknown placeholders stay as written. */
export function fill(text: string, params: Params): string {
  return text.replace(/\{(\w+)\}/g, (all, name: string) => (name in params ? String(params[name]) : all))
}

/** The words for `key`, filled in. */
export function t(key: string, params?: Params): string {
  const text = template(key, params?.n)
  return params ? fill(text, params) : text
}

/** `key`'s words split at `{name}`, for markup around the value: "Updated to {version}" → ["Updated to ", ""]. */
export function around(key: string, name: string): [string, string] {
  const text = template(key, undefined)
  const at = text.indexOf(`{${name}}`)
  return at < 0 ? [text, ''] : [text.slice(0, at), text.slice(at + name.length + 2)]
}

/** One decimal in the current language: "1.5" in English, "1,5" in Dutch. */
export function oneDecimal(x: number): string {
  track()
  return decimal.format(x)
}
