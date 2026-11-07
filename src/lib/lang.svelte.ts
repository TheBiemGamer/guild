// Makes `t()` reactive: every lookup reads `version`, so a language switch re-renders whatever used it.
import { setup, use, type Words } from './i18n.ts'

/** Every language file; one more `src/lib/i18n/<lang>.json` is one more language. */
const files = import.meta.glob<Words>('./i18n/*.json', { eager: true, import: 'default' })

let version = $state(0)
let current = 'en'
setup(() => version, import.meta.env.DEV)

/** Shows the UI in `lang` ("en", "nl"), the language Rust resolved from the Language setting. */
export function setLanguage(lang: string) {
  if (lang === current) return
  current = lang
  use(lang, files[`./i18n/${lang}.json`] ?? files['./i18n/en.json'])
  document.documentElement.lang = lang
  version++
}
