<script lang="ts" generics="T extends string">
  import { popover } from './popover'

  let { value = $bindable(), options, label, anchor, onclose }: {
    value: T
    options: { value: T; label: string }[]
    /** The menu's accessible name, like "Sort by". */
    label: string
    /** The chip that opened it: clicks on it toggle instead of closing from outside. */
    anchor: HTMLElement | undefined
    onclose: (refocus: boolean) => void
  } = $props()

  let menu: HTMLDivElement

  /** Runs after `popover`, which focuses the first item: a menu opens on the chosen one. */
  function focusChosen(node: HTMLElement) {
    node.querySelector<HTMLElement>('[aria-checked="true"]')?.focus()
  }

  function pick(v: T) {
    value = v
    onclose(true)
  }

  /** Arrows, Home and End move between the options. */
  function onkeydown(e: KeyboardEvent) {
    const items = Array.from(menu.querySelectorAll<HTMLElement>('[role="menuitemradio"]'))
    const at = items.indexOf(document.activeElement as HTMLElement)
    const next =
      e.key === 'ArrowDown' ? (at + 1) % items.length
      : e.key === 'ArrowUp' ? (at <= 0 ? items.length - 1 : at - 1)
      : e.key === 'Home' ? 0
      : e.key === 'End' ? items.length - 1
      : null
    if (next === null) return
    e.preventDefault()
    items[next].focus()
  }
</script>

<div class="pop" role="menu" aria-label={label} tabindex="-1" bind:this={menu} {onkeydown} use:popover={{ anchor, close: onclose }} use:focusChosen>
  {#each options as o (o.value)}
    <button class="o" role="menuitemradio" aria-checked={o.value === value} onclick={() => pick(o.value)}>
      {o.label}
    </button>
  {/each}
</div>

<style>
  .pop {
    position: absolute;
    z-index: 6;
    top: 44px;
    right: 0;
    min-width: 230px;
    padding: 6px;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: 0 30px 80px rgb(0 0 0 / 0.6);
  }

  .pop:focus-visible {
    outline: none;
  }

  .o {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    padding: 8px 10px;
    border-radius: 8px;
    background: transparent;
    color: var(--text-2);
    text-align: left;
    white-space: nowrap;
  }

  .o:hover:not(:disabled) {
    background: var(--glass-hover);
    color: var(--text);
  }

  .o[aria-checked='true'] {
    color: var(--text);
  }

  .o[aria-checked='true']::after {
    content: '✓';
    color: var(--text-2);
  }
</style>
