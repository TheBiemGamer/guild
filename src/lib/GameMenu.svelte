<script lang="ts">
  import { onMount, tick, type Snippet } from 'svelte'
  import * as api from './api'
  import type { Collection, Listed } from './api'

  let { game, at = null, anchor, label, onclose, onchanged, onerror, extra }: {
    game: Listed
    /** Opens at the pointer (a card's right-click); otherwise it hangs below its parent, right-aligned. */
    at?: { x: number; y: number } | null
    /** The button that opened it: clicks on it toggle instead of closing from outside. */
    anchor?: HTMLElement
    label: string
    onclose: (refocus: boolean) => void
    /** After a change, so the library and the page reload. */
    onchanged: () => void
    onerror: (message: string) => void
    /** The page's own items, after a separator. */
    extra?: Snippet
  } = $props()

  let menu: HTMLDivElement
  let sub = $state<HTMLDivElement>()
  let trigger = $state<HTMLButtonElement>()
  let collections = $state<Collection[]>([])
  let subOpen = $state(false)
  /** The submenu opens to the left when there is no room on the right. */
  let subLeft = $state(false)
  let naming = $state(false)
  let name = $state('')
  let newButton = $state<HTMLButtonElement>()
  /** Where the menu sits when opened at the pointer, kept inside the window. */
  let pos = $state<{ x: number; y: number } | null>(null)

  onMount(() => {
    menu.querySelector<HTMLElement>('button')?.focus()
    if (at) {
      const r = menu.getBoundingClientRect()
      pos = { x: Math.max(8, Math.min(at.x, innerWidth - r.width - 8)), y: Math.max(8, Math.min(at.y, innerHeight - r.height - 8)) }
    }
    api.collections().then(
      (c) => (collections = c),
      (e) => onerror(api.message(e)),
    )
  })

  async function change(f: () => Promise<unknown>) {
    onclose(true)
    try {
      await f()
    } catch (e) {
      onerror(api.message(e))
    }
    onchanged()
  }

  async function openSub(focus: boolean) {
    subLeft = menu.getBoundingClientRect().right + 250 > innerWidth
    subOpen = true
    await tick()
    if (focus) sub?.querySelector<HTMLElement>('button')?.focus()
  }

  function closeSub() {
    subOpen = false
    naming = false
    trigger?.focus()
  }

  async function startNaming() {
    naming = true
    name = ''
    await tick()
    sub?.querySelector('input')?.focus()
  }

  async function create() {
    try {
      const id = await api.createCollection(name)
      await change(() => api.setInCollection(game.key, id, true))
    } catch (e) {
      // The name is empty or taken: the field stays for another try.
      onerror(api.message(e))
    }
  }

  async function nameKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault()
      create()
    } else if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      naming = false
      await tick()
      newButton?.focus()
    }
  }

  /** Arrows, Home and End move within the menu that has focus; Right opens the submenu, Left and Escape close it. */
  function onkeydown(e: KeyboardEvent) {
    const active = document.activeElement as HTMLElement
    if (e.key === 'Tab') return onclose(false)
    if (active.tagName === 'INPUT') return
    const inSub = !!sub?.contains(active)
    const list = inSub ? sub! : menu
    const items = Array.from(list.querySelectorAll<HTMLElement>('button')).filter((b) => b.closest('[role="menu"]') === list)
    const here = items.indexOf(active)
    const go = (i: number) => {
      e.preventDefault()
      items[(i + items.length) % items.length]?.focus()
    }
    if (e.key === 'ArrowDown') go(here + 1)
    else if (e.key === 'ArrowUp') go(here - 1)
    else if (e.key === 'Home') go(0)
    else if (e.key === 'End') go(-1)
    else if (e.key === 'ArrowRight' && active === trigger) {
      e.preventDefault()
      openSub(true)
    } else if ((e.key === 'ArrowLeft' && inSub) || (e.key === 'Escape' && inSub)) {
      e.preventDefault()
      e.stopPropagation()
      closeSub()
    } else if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      onclose(true)
    }
  }

  function onpointerdown(e: PointerEvent) {
    const target = e.target as Node
    if (e.button > 2 || menu.contains(target) || anchor?.contains(target)) return
    onclose(false)
  }

  /** Opened at the pointer, it would stay put while the grid scrolls the card away. */
  function onscroll(e: Event) {
    if (at && !menu.contains(e.target as Node)) onclose(false)
  }
</script>

<svelte:window onpointerdowncapture={onpointerdown} onscrollcapture={onscroll} />

<div
  class="menu"
  class:at={!!at}
  style={at ? `left: ${(pos ?? at).x}px; top: ${(pos ?? at).y}px` : undefined}
  role="menu"
  aria-label={label}
  tabindex="-1"
  bind:this={menu}
  {onkeydown}
>
  <button role="menuitemcheckbox" aria-checked={game.favorite} tabindex="-1" onclick={() => change(() => api.setFavorite(game.key, !game.favorite))}>
    {#if game.favorite}<span class="check" aria-hidden="true">✓</span>{/if}
    Favorite
  </button>
  <div class="subwrap">
    <button
      role="menuitem"
      aria-haspopup="menu"
      aria-expanded={subOpen}
      tabindex="-1"
      bind:this={trigger}
      onclick={() => (subOpen ? closeSub() : openSub(true))}
    >
      Add to collection<span class="more" aria-hidden="true">›</span>
    </button>
    {#if subOpen}
      <div class="menu sub" class:left={subLeft} role="menu" aria-label="Add to collection" tabindex="-1" bind:this={sub}>
        {#each collections as c (c.id)}
          {@const on = game.collections.includes(c.id)}
          <button role="menuitemcheckbox" aria-checked={on} tabindex="-1" onclick={() => change(() => api.setInCollection(game.key, c.id, !on))}>
            {#if on}<span class="check" aria-hidden="true">✓</span>{/if}
            {c.name}
          </button>
        {/each}
        {#if collections.length}<div class="sep" role="separator"></div>{/if}
        {#if naming}
          <input aria-label="Collection name" placeholder="Name" bind:value={name} onkeydown={nameKeydown} />
        {:else}
          <button role="menuitem" tabindex="-1" bind:this={newButton} onclick={startNaming}>New collection…</button>
        {/if}
      </div>
    {/if}
  </div>
  <button role="menuitem" tabindex="-1" onclick={() => change(() => api.setHidden(game.key, !game.hidden))}>
    {game.hidden ? 'Show in library' : 'Hide from library'}
  </button>
  {#if extra}
    <div class="sep" role="separator"></div>
    {@render extra()}
  {/if}
</div>

<style>
  .menu {
    position: absolute;
    z-index: 5;
    top: calc(100% + 8px);
    right: 0;
    min-width: 240px;
    padding: 6px;
    display: grid;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: 0 30px 80px rgb(0 0 0 / 0.6);
    color: var(--text);
    text-align: left;
  }

  .menu.at {
    position: fixed;
    z-index: 20;
    right: auto;
  }

  .menu:focus-visible {
    outline: none;
  }

  /* Every row leaves room for the check, so the names line up. Buttons from `extra` follow the same rules. */
  .menu :global(button) {
    position: relative;
    display: flex;
    align-items: center;
    width: 100%;
    padding: 8px 10px 8px 36px;
    border-radius: 8px;
    text-align: left;
    white-space: nowrap;
    background: transparent;
  }

  .menu :global(button:hover:not(:disabled)),
  .menu :global(button:focus-visible),
  .menu :global(button[aria-expanded='true']) {
    background: var(--glass-hover);
  }

  .menu :global(button:focus-visible) {
    outline-offset: -2px;
  }

  .menu :global(button:active:not(:disabled)) {
    transform: none;
  }

  .check {
    position: absolute;
    left: 10px;
    width: 16px;
    text-align: center;
    color: var(--accent-2);
  }

  .more {
    margin-left: auto;
    padding-left: 16px;
    color: var(--text-3);
  }

  .subwrap {
    position: relative;
  }

  .sub {
    top: -6px;
    left: calc(100% + 10px);
    right: auto;
    max-height: 320px;
    overflow-y: auto;
  }

  .sub.left {
    left: auto;
    right: calc(100% + 10px);
  }

  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--glass-line);
  }

  input {
    height: 34px;
    margin: 2px 0;
    padding: 0 10px;
    border-radius: 8px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
    color: var(--text);
    font: inherit;
  }

  input::placeholder {
    color: var(--text-3);
  }
</style>
