<script lang="ts">
  import { tick } from 'svelte'
  import * as api from './api'
  import type { CopyView } from './bindings'
  import { sizeLabel } from './format'
  import { t } from './i18n'
  import StoreIcon, { storeNames } from './StoreIcon.svelte'

  let {
    copies,
    chosen,
    running,
    busy,
    onchoose,
    onplay,
    oninstall,
    onupdate,
  }: {
    copies: CopyView[]
    /** The copy key Play and Install act on. */
    chosen: string
    running: boolean
    /** The busy pill's text while an action runs. */
    busy: string | null
    onchoose: (key: string) => void
    onplay: () => void
    oninstall: () => void
    onupdate: () => void
  } = $props()

  const copy = $derived(copies.find((c) => c.key === chosen) ?? copies[0])
  const split = $derived(copies.length > 1)

  // Download sizes of not-installed store copies. A key not in here is still being asked.
  let sizes = $state<Record<string, number | null>>({})
  const asked = new Set<string>()
  $effect(() => {
    for (const c of copies) {
      if (c.provider === 'custom' || c.installed || asked.has(c.key)) continue
      asked.add(c.key)
      const key = c.key
      api.downloadSize(key).then(
        (b) => (sizes[key] = b),
        () => (sizes[key] = null),
      )
    }
  })

  const downloadText = (key: string) =>
    key in sizes ? (sizes[key] !== null ? t('install.download', { size: sizeLabel(sizes[key]) }) : '') : t('install.checking')

  function sub(c: CopyView): string {
    if (c.installed) return c.sizeBytes !== null ? t('play.installedSize', { size: sizeLabel(c.sizeBytes) }) : t('play.installed')
    const size = sizes[c.key] ?? null
    return size !== null ? t('play.notInstalledSize', { size: sizeLabel(size) }) : t('play.notInstalled')
  }

  const go = () => (copy.installed ? (copy.updateAvailable ? onupdate() : onplay()) : oninstall())

  let open = $state(false)
  let wrap = $state<HTMLDivElement>()
  let switcher = $state<HTMLButtonElement>()
  let menu = $state<HTMLDivElement>()

  async function openMenu() {
    open = true
    await tick()
    const items = menu?.querySelectorAll('button')
    ;(menu?.querySelector<HTMLButtonElement>('[aria-checked="true"]') ?? items?.[0])?.focus()
  }

  $effect(() => {
    if (busy || running) open = false
  })

  function close(refocus: boolean) {
    open = false
    if (refocus) switcher?.focus()
  }

  function menuKeydown(e: KeyboardEvent) {
    if (!menu) return
    const items = Array.from(menu.querySelectorAll('button'))
    const at = items.indexOf(document.activeElement as HTMLButtonElement)
    const go = (i: number) => {
      e.preventDefault()
      items[(i + items.length) % items.length]?.focus()
    }
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      close(true)
    } else if (e.key === 'ArrowDown') go(at + 1)
    else if (e.key === 'ArrowUp') go(at - 1)
    else if (e.key === 'Home') go(0)
    else if (e.key === 'End') go(-1)
    else if (e.key === 'Tab') close(false)
  }

  function onpointerdown(e: PointerEvent) {
    if (open && !wrap?.contains(e.target as Node)) close(false)
  }

  function press(f: () => void) {
    open = false
    f()
  }

  function choose(key: string) {
    close(true)
    onchoose(key)
  }
</script>

<svelte:window {onpointerdown} />

{#if !copy.installed && !busy && !running && copy.provider !== 'custom' && downloadText(copy.key)}
  <span class="download">{downloadText(copy.key)}</span>
{/if}

{#if busy}
  <button class="pill big" aria-disabled="true">{busy}</button>
{:else if running}
  <button class="pill big" disabled>{t('play.playing')}</button>
{:else if !split}
  <button class="pill big" onclick={go}>
    {@render goLabel()}
  </button>
{:else}
  <div class="split" bind:this={wrap}>
    <div class="pair">
      <button
        class="switch"
        bind:this={switcher}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={t('play.chooseLauncher', { store: storeNames[copy.provider] })}
        onclick={() => (open ? close(true) : openMenu())}
      >
        <StoreIcon provider={copy.provider} size={18} />
        <svg class="stroke" viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg>
      </button>
      <span class="sep"></span>
      <button class="go" onclick={() => press(go)}>
        {@render goLabel()}
      </button>
    </div>
    {#if open}
      <div class="menu" role="menu" aria-label={t('play.playWith')} tabindex="-1" bind:this={menu} onkeydown={menuKeydown}>
        <span class="label" role="presentation">{t('play.playWith')}</span>
        {#each copies as c (c.key)}
          <button role="menuitemradio" aria-checked={c.key === copy.key} tabindex="-1" onclick={() => choose(c.key)}>
            <StoreIcon provider={c.provider} size={18} />
            <span class="txt"><span>{storeNames[c.provider]}</span><small>{sub(c)}</small></span>
            <svg class="check stroke" viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path d="M5 12l5 5 9-10" /></svg>
          </button>
        {/each}
      </div>
    {/if}
  </div>
{/if}

{#snippet goLabel()}
  {#if copy.installed && copy.updateAvailable}
    <svg class="stroke" viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path d="M12 4v11M7 10l5 5 5-5M5 20h14" /></svg>
    {t('play.update')}
  {:else if copy.installed}
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path d="M7 4.5v15l12.5-7.5z" /></svg>
    {t('play.play')}
  {:else}
    <svg class="stroke" viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path d="M12 4v11M7 10l5 5 5-5M5 20h14" /></svg>
    {t('common.install')}
  {/if}
{/snippet}

<style>
  .pill.big {
    height: 44px;
    padding: 0 32px;
  }

  .download {
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }

  .split {
    position: relative;
  }

  .pair {
    display: flex;
    align-items: stretch;
    height: 44px;
    border-radius: 999px;
    overflow: hidden;
    background: #fff;
    color: var(--bg);
  }

  .pair button {
    display: flex;
    align-items: center;
    gap: 8px;
    border-radius: 0;
    background: #fff;
    color: var(--bg);
    font-weight: 700;
  }

  .pair button:hover:not(:disabled) {
    background: #e4e6ee;
  }

  .pair button:focus-visible {
    outline-offset: -2px;
  }

  .switch {
    gap: 6px;
    padding: 0 12px 0 16px;
  }

  .go {
    padding: 0 28px 0 18px;
  }

  .sep {
    width: 1px;
    margin: 10px 0;
    background: rgb(14 14 20 / 0.18);
  }

  .menu {
    position: absolute;
    z-index: 5;
    top: calc(100% + 8px);
    right: 0;
    width: 290px;
    padding: 4px;
    display: flex;
    flex-direction: column;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: 0 30px 80px rgb(0 0 0 / 0.6);
  }

  .label {
    padding: 8px 12px 4px;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-2);
  }

  .menu button {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    text-align: left;
    background: transparent;
  }

  .menu button:hover:not(:disabled),
  .menu button:focus-visible {
    background: var(--glass-hover);
  }

  .menu button:focus-visible {
    outline-offset: -2px;
  }

  .menu button:active:not(:disabled) {
    transform: none;
  }

  .txt {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  small {
    font-size: 12px;
    color: var(--text-2);
  }

  .check {
    visibility: hidden;
  }

  [aria-checked='true'] .check {
    visibility: visible;
  }

  svg {
    flex: none;
    fill: currentColor;
  }

  svg.stroke {
    fill: none;
    stroke: currentColor;
    stroke-width: 2.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
