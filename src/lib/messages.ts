// The message codes Rust sends instead of sentences (`src-tauri/src/msg.rs`) in words. The words live in the
// language files under `msg.<code>` (`src/lib/i18n/en.json`). Loads in `node --test scripts/` as plain TypeScript.

import { english, fill, t, type Params } from './i18n.ts'

/** `{ code, params }` from Rust. A param that is itself a `Msg` is rendered first. */
export interface Msg {
  code: string
  params: Record<string, string | Msg>
}

/** A `Msg` in words. Plain text (an error from Tauri itself, or text stored before message codes) shows as is. */
export function render(m: unknown): string {
  if (typeof m === 'string') return m
  if (typeof m !== 'object' || m === null || typeof (m as Msg).code !== 'string') return String(m)
  const { code, params = {} } = m as Msg
  const key = `msg.${code}`
  if (!(key in english)) return code
  const words: Params = {}
  for (const [name, value] of Object.entries(params)) words[name] = render(value)
  return fill(t(key), words)
}
