<script lang="ts" module>
  export type Action = {
    label: string
    run: () => void
    /** Why it can't run now; the item stays listed, disabled, with this as its title. */
    disabledWhy?: string
    danger?: boolean
    /** A line above it, setting it apart from the items before. */
    apart?: boolean
  }
</script>

<script lang="ts">
  import { popover } from './popover'

  let {
    actions,
    label,
    anchor,
    onclose,
  }: {
    actions: Action[]
    /** The menu's accessible name. */
    label: string
    /** The button that opened it: clicks on it toggle instead of closing from outside. */
    anchor: HTMLElement | undefined
    onclose: (refocus: boolean) => void
  } = $props()

  let menu: HTMLDivElement

  function pick(a: Action) {
    onclose(true)
    a.run()
  }

  /** Arrows, Home and End move between the items that can run. */
  function onkeydown(e: KeyboardEvent) {
    const items = Array.from(menu.querySelectorAll<HTMLElement>('[role="menuitem"]:not(:disabled)'))
    const at = items.indexOf(document.activeElement as HTMLElement)
    const next =
      e.key === 'ArrowDown'
        ? (at + 1) % items.length
        : e.key === 'ArrowUp'
          ? (at <= 0 ? items.length : at) - 1
          : e.key === 'Home'
            ? 0
            : e.key === 'End'
              ? items.length - 1
              : null
    if (next === null) return
    e.preventDefault()
    items[next].focus()
  }
</script>

<div class="pop" role="menu" aria-label={label} tabindex="-1" bind:this={menu} {onkeydown} use:popover={{ anchor, close: onclose }}>
  {#each actions as a (a.label)}
    {#if a.apart}<div class="sep" role="separator"></div>{/if}
    <button class="o" class:danger={a.danger} role="menuitem" disabled={!!a.disabledWhy} title={a.disabledWhy} onclick={() => pick(a)}>{a.label}</button>
  {/each}
</div>

<style>
  .pop {
    position: absolute;
    z-index: 6;
    top: calc(100% + 6px);
    right: 0;
    min-width: 180px;
    padding: 6px;
    display: grid;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: var(--shadow-modal);
  }

  .pop:focus-visible {
    outline: none;
  }

  .o {
    width: 100%;
    padding: 8px 10px;
    border-radius: 8px;
    background: transparent;
    color: var(--text-2);
    text-align: left;
    white-space: nowrap;
  }

  .o:hover:not(:disabled),
  .o:focus-visible {
    background: var(--glass-hover);
    color: var(--text);
  }

  .o:focus-visible {
    outline-offset: -2px;
  }

  .o.danger,
  .o.danger:hover:not(:disabled) {
    color: var(--danger-text);
  }

  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--glass-line);
  }
</style>
