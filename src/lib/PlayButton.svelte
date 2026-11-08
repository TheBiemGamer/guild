<script lang="ts">
  import * as api from './api'
  import type { CopyView } from './bindings'
  import { sizeLabel } from './format'
  import { DropdownMenu } from 'bits-ui'
  import { t } from './i18n'
  import Check from './Check.svelte'
  import { keepOnBackForward } from './outside'
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
  let pair = $state<HTMLDivElement>()
  let menu = $state<HTMLElement | null>(null)

  $effect(() => {
    if (busy || running) open = false
  })

  /** The menu opens on the chosen launcher, whether by mouse or keyboard. */
  function focusChosen(e: Event) {
    const chosen = menu?.querySelector<HTMLElement>('[aria-checked="true"]')
    if (!chosen) return
    e.preventDefault()
    chosen.focus()
  }
</script>

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
  <DropdownMenu.Root bind:open>
    <div class="split">
      <div class="pair" bind:this={pair}>
        <DropdownMenu.Trigger>
          {#snippet child({ props })}
            <button {...props} class="switch" aria-label={t('play.chooseLauncher', { store: storeNames[copy.provider] })}>
              <StoreIcon provider={copy.provider} size={18} />
              <svg class="stroke" viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg>
            </button>
          {/snippet}
        </DropdownMenu.Trigger>
        <span class="sep"></span>
        <button class="go" onclick={go}>
          {@render goLabel()}
        </button>
      </div>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          bind:ref={menu}
          customAnchor={pair}
          align="end"
          sideOffset={8}
          aria-label={t('play.playWith')}
          onInteractOutside={keepOnBackForward}
          onOpenAutoFocus={focusChosen}
        >
          {#snippet child({ props, wrapperProps })}
            <div {...wrapperProps}>
              <div {...props} class="menu">
                <span class="label" role="presentation">{t('play.playWith')}</span>
                <DropdownMenu.RadioGroup value={copy.key} onValueChange={onchoose}>
                  {#each copies as c (c.key)}
                    <DropdownMenu.RadioItem value={c.key}>
                      {#snippet child({ props, checked })}
                        <button {...props}>
                          <StoreIcon provider={c.provider} size={18} />
                          <span class="txt"><span>{storeNames[c.provider]}</span><small>{sub(c)}</small></span>
                          {#if checked}<Check />{/if}
                        </button>
                      {/snippet}
                    </DropdownMenu.RadioItem>
                  {/each}
                </DropdownMenu.RadioGroup>
              </div>
            </div>
          {/snippet}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </div>
  </DropdownMenu.Root>
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
    /* bits-ui copies this onto its portal wrapper: above the Settings backdrop (20) and every popup (up to 40). */
    position: relative;
    z-index: 50;
    width: 290px;
    padding: 4px;
    display: flex;
    flex-direction: column;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: var(--shadow-modal);
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
