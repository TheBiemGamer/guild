<script lang="ts" module>
  import changelogRaw from '../../CHANGELOG.md?raw'
  import { parseNotes, section } from './changelog'

  /** The installed version's CHANGELOG.md section, or null. */
  export const installedNotes = (version: string) => section(changelogRaw, version)
</script>

<script lang="ts">
  import type { Snippet } from 'svelte'
  import { popover } from './popover'
  import { t } from './i18n'

  let {
    version,
    sub,
    notes,
    anchor,
    onclose,
    children,
  }: {
    version: string
    /** After the version, in `text-3`. */
    sub?: string
    /** A CHANGELOG.md section. */
    notes: string
    anchor: HTMLElement | undefined
    onclose: (refocus: boolean) => void
    /** The footer. */
    children?: Snippet
  } = $props()

  const WIDTH = 380
  const GAP = 8
  const EDGE = 16
  const groups = $derived(parseNotes(notes))
  /** The content's own height; the popover scrolls when it's taller than the room. */
  let height = $state(0)
  // Fixed to the viewport, so the Settings panel never clips it: right-aligned under the anchor, or above
  // it when the content doesn't fit below and there's more room above.
  const rect = $derived(anchor?.getBoundingClientRect())
  const left = $derived(rect ? Math.max(EDGE, Math.min(rect.right - WIDTH, window.innerWidth - WIDTH - EDGE)) : EDGE)
  const place = $derived.by(() => {
    if (!rect) return { top: EDGE, max: window.innerHeight - 2 * EDGE }
    const below = window.innerHeight - EDGE - rect.bottom - GAP
    const above = rect.top - GAP - EDGE
    if (height <= below || below >= above) return { top: rect.bottom + GAP, max: below }
    return { top: rect.top - GAP - Math.min(height, above), max: above }
  })
</script>

<div
  class="pop"
  role="dialog"
  aria-label={t('notes.label', { version })}
  tabindex="-1"
  style="left: {left}px; top: {place.top}px; max-height: {place.max}px"
  use:popover={{ anchor, close: onclose }}
>
  <div class="body" bind:offsetHeight={height}>
    <div class="ver">
      <b>{version}</b>{#if sub}<span>{sub}</span>{/if}
    </div>
    {#each groups as g (g.title)}
      <h3>{g.title}</h3>
      <ul>
        {#each g.items as item, i (i)}<li>{item}</li>{/each}
      </ul>
    {/each}
    {@render children?.()}
  </div>
</div>

<style>
  .pop {
    position: fixed;
    z-index: 30;
    width: 380px;
    overflow-y: auto;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: var(--shadow-modal);
    color: var(--text);
    text-align: left;
  }

  .pop:focus-visible {
    outline: none;
  }

  .body {
    padding: 14px;
  }

  .ver {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .ver b {
    font-size: 16px;
    font-weight: 600;
  }

  .ver span {
    font-size: 12px;
    color: var(--text-3);
  }

  h3 {
    margin: 12px 0 4px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-3);
  }

  ul {
    margin: 0;
    padding-left: 18px;
    color: var(--text-2);
    line-height: 1.55;
  }

  li {
    margin: 2px 0;
  }
</style>
