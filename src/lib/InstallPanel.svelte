<script lang="ts">
  import { DropdownMenu, RadioGroup } from 'bits-ui'
  import { onMount } from 'svelte'
  import * as api from './api'
  import { commands, type Build, type InstallChoice, type InstallFolder, type InstallPlan, type Store } from './bindings'
  import ChoiceMenu from './ChoiceMenu.svelte'
  import Check from './Check.svelte'
  import Tip from './Tip.svelte'
  import { keepOnBackForward } from './outside'
  import { sizeLabel } from './format'
  import { t } from './i18n'

  let {
    game,
    anchor = null,
    opener = null,
    onconfirm,
    oncancel,
  }: {
    /** The Epic, GOG or Amazon copy's key and the game's title. */
    game: { key: string; title: string }
    /** The cover's rectangle in the library: the panel floats beside it. Null: inline, under the game page's Install button. */
    anchor?: DOMRect | null
    /** Inline: what opened the panel (the Install pill), whose clicks are left to it. */
    opener?: HTMLElement | null
    onconfirm: (choice: InstallChoice) => void
    oncancel: () => void
  } = $props()

  const WIDTH = 380
  const GAP = 8
  /** Up to this many DLC get a row each; more fold into one "All DLC" row. */
  const DLC_ROWS = 4
  const store = $derived(game.key.slice(0, game.key.indexOf('-')) as Store)

  let el: HTMLDivElement
  let installButton: HTMLButtonElement
  let changeButton = $state<HTMLButtonElement>()
  let height = $state(0)
  /** undefined until the first answer; kept while a new language or build is asked. */
  let plan = $state<InstallPlan | null | undefined>(undefined)
  let language = $state<string | null>(null)
  let build = $state<Build | null>(null)
  let folders = $state<InstallFolder[]>([])
  let folder = $state<InstallFolder | null>(null)
  let chosen = $state<string[]>([])
  let menuOpen = $state(false)
  let languagesOpen = $state(false)
  let dlcOpen = $state(false)

  const x = $derived(anchor && (anchor.right + GAP + WIDTH <= window.innerWidth - GAP ? anchor.right + GAP : Math.max(GAP, anchor.left - GAP - WIDTH)))
  const y = $derived(anchor && Math.max(GAP, Math.min(anchor.top, window.innerHeight - height - GAP)))

  const picked = $derived(plan?.dlc.filter((d) => chosen.includes(d.id)) ?? [])
  /** Known bytes of the game plus the chosen DLC; null when the game's own is unknown. */
  const total = (own: number | null | undefined, field: 'downloadBytes' | 'diskBytes') =>
    own == null ? null : picked.reduce((sum, d) => sum + (d[field] ?? 0), own)
  const download = $derived(total(plan?.downloadBytes, 'downloadBytes'))
  const disk = $derived(total(plan?.diskBytes ?? plan?.downloadBytes, 'diskBytes'))
  const sizes = $derived(
    [download !== null && t('install.download', { size: sizeLabel(download) }), disk !== null && t('install.onDisk', { size: sizeLabel(disk) })]
      .filter(Boolean)
      .join(' · '),
  )
  const free = $derived(folder?.freeBytes ?? null)
  const room = $derived(free === null || disk === null || disk <= free)
  const sep = $derived(folder?.path.includes('\\') ? '\\' : '/')
  const path = $derived(folder && (plan?.folderName ? `${folder.path.replace(/[\\/]+$/, '')}${sep}${plan.folderName}` : folder.path))
  const languageNames = new Intl.DisplayNames([document.documentElement.lang || 'en'], { type: 'language' })
  const languageName = (code: string) => languageNames.of(code) ?? code
  const allChosen = $derived(!!plan && plan.dlc.length > 0 && chosen.length === plan.dlc.length)

  function ask() {
    const now = { language, build }
    api.installPlan(game.key, language, build).then(
      (p) => {
        if (now.language !== language || now.build !== build) return
        // The DLC start all chosen, as an install took them before.
        if (plan === undefined) chosen = p?.dlc.map((d) => d.id) ?? []
        plan = p
        language = p?.language ?? null
        build = p?.build ?? null
      },
      () => plan === undefined && (plan = null),
    )
  }

  onMount(() => {
    const before = document.activeElement
    installButton.focus()
    ask()
    commands.installFolders(store).then((f) => {
      folders = f
      folder ??= f[0] ?? null
    })
    if (!anchor) return () => before instanceof HTMLElement && before.isConnected && before.focus()
    // The grid scrolls under a fixed popup, so a scroll anywhere closes it.
    const onscroll = (e: Event) => !el.contains(e.target as Node) && oncancel()
    window.addEventListener('scroll', onscroll, true)
    return () => {
      window.removeEventListener('scroll', onscroll, true)
      if (before instanceof HTMLElement && before.isConnected) before.focus()
    }
  })

  // Captured, so an open menu (portalled outside the panel) is seen before it closes itself; it takes the Escape or click.
  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && !menuOpen && !languagesOpen) oncancel()
  }

  // A click outside closes it. Clicks on the cover or the Install pill are left to them (they toggle it).
  function onpointerdown(e: PointerEvent) {
    const target = e.target as Node
    if (menuOpen || languagesOpen || el.contains(target) || opener?.contains(target)) return
    const inside = !!anchor && e.clientX >= anchor.left && e.clientX <= anchor.right && e.clientY >= anchor.top && e.clientY <= anchor.bottom
    if (!inside) oncancel()
  }

  async function another() {
    const picked = await api.pickFolder()
    if (picked) folder = await commands.folderSpace(picked)
    changeButton?.focus()
  }

  function toggle(id: string, on: boolean) {
    chosen = on ? [...chosen, id] : chosen.filter((c) => c !== id)
  }

  function install() {
    if (!folder || !room) return
    // Before the plan arrives nothing could be unticked: all DLC, as before.
    onconfirm({ folder: folder.path, dlc: plan ? chosen : null, language, build })
  }

  const percent = (bytes: number) => (folder?.totalBytes ? Math.min(100, (bytes / folder.totalBytes) * 100) : 0)
</script>

<svelte:window onkeydowncapture={onkeydown} onpointerdowncapture={onpointerdown} />

{#snippet dlcRow(d: InstallPlan['dlc'][number])}
  <label class="row">
    <input type="checkbox" checked={chosen.includes(d.id)} onchange={(e) => toggle(d.id, e.currentTarget.checked)} />
    <span class="grow">{d.title}</span>
    {#if (d.downloadBytes ?? d.diskBytes) !== null}<span class="num">{sizeLabel((d.downloadBytes ?? d.diskBytes)!)}</span>{/if}
  </label>
{/snippet}

<div
  class="panel"
  class:popup={anchor}
  bind:this={el}
  bind:clientHeight={height}
  role="dialog"
  aria-label={t('install.label', { title: game.title })}
  style={anchor ? `left: ${x}px; top: ${y}px; width: ${WIDTH}px` : ''}
>
  {#if anchor}<h4>{t('install.question', { title: game.title })}</h4>{/if}
  <p class="sub">{plan === undefined ? t('install.checking') : sizes}</p>

  <div class="label">{t('install.folder')}</div>
  <div class="group">
    <div class="row">
      <div class="grow">
        <Tip text={path ?? ''} truncated>
          {#snippet children({ props })}<span {...props} class="path">{path ?? ''}</span>{/snippet}
        </Tip>
        {#if folder?.totalBytes && free !== null}
          {@const used = folder.totalBytes - free}
          <div class="bar" aria-hidden="true">
            <i class="used" style="width: {percent(used)}%"></i>
            {#if disk !== null}<i class="game" class:over={!room} style="width: {percent(disk)}%"></i>{/if}
          </div>
          {#if !room}
            <small class="warn">{t('install.noRoom', { need: sizeLabel(disk!), free: sizeLabel(free) })}</small>
          {:else if disk !== null}
            <small>{t('install.freeAfter', { size: sizeLabel(free - disk) })}</small>
          {:else}
            <small>{t('install.free', { size: sizeLabel(free) })}</small>
          {/if}
        {/if}
      </div>
      <DropdownMenu.Root bind:open={menuOpen}>
        <DropdownMenu.Trigger>
          {#snippet child({ props })}<button {...props} class="glass" bind:this={changeButton}>{t('install.change')}</button>{/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content align="end" sideOffset={8} preventScroll={false} aria-label={t('install.folders')} onInteractOutside={keepOnBackForward}>
            {#snippet child({ props, wrapperProps })}
              <div {...wrapperProps}>
                <div {...props} class="menu">
                  <DropdownMenu.RadioGroup value={folder?.path ?? ''} onValueChange={(path) => (folder = folders.find((f) => f.path === path) ?? folder)}>
                    {#each folders as f (f.path)}
                      <DropdownMenu.RadioItem value={f.path}>
                        {#snippet child({ props, checked })}
                          <button {...props} class="item">
                            <span class="path">{f.path}</span>
                            {#if f.freeBytes !== null}<small>{t('install.free', { size: sizeLabel(f.freeBytes) })}</small>{/if}
                            {#if checked}<Check />{/if}
                          </button>
                        {/snippet}
                      </DropdownMenu.RadioItem>
                    {/each}
                  </DropdownMenu.RadioGroup>
                  <DropdownMenu.Separator class="sep" />
                  <DropdownMenu.Item onSelect={another}>
                    {#snippet child({ props })}<button {...props} class="item">{t('install.anotherFolder')}</button>{/snippet}
                  </DropdownMenu.Item>
                </div>
              </div>
            {/snippet}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </div>
  </div>

  {#if plan?.dlc.length}
    <div class="label">{t('install.dlc')}</div>
    <div class="group">
      {#if plan.dlc.length <= DLC_ROWS}
        {#each plan.dlc as d (d.id)}{@render dlcRow(d)}{/each}
      {:else}
        {@const all = plan.dlc}
        {@const allSize = all.every((d) => (d.downloadBytes ?? d.diskBytes) !== null) ? all.reduce((s, d) => s + (d.downloadBytes ?? d.diskBytes)!, 0) : null}
        <div class="row">
          <label class="grow check">
            <input
              type="checkbox"
              checked={allChosen}
              indeterminate={chosen.length > 0 && !allChosen}
              onchange={(e) => (chosen = e.currentTarget.checked ? all.map((d) => d.id) : [])}
            />
            {t('install.allDlc', { n: all.length })}
          </label>
          {#if allSize !== null}<span class="num">{sizeLabel(allSize)}</span>{/if}
          <button class="fold" aria-expanded={dlcOpen} aria-label={t('install.showDlc')} onclick={() => (dlcOpen = !dlcOpen)}>
            <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg>
          </button>
        </div>
        {#if dlcOpen}
          <div class="list">
            {#each all as d (d.id)}{@render dlcRow(d)}{/each}
          </div>
        {/if}
      {/if}
    </div>
  {/if}

  {#if plan?.languages.length}
    <div class="label">{t('install.language')}</div>
    <div class="group">
      <ChoiceMenu
        bind:value={
          () => language ?? '',
          (v) => {
            language = v
            ask()
          }
        }
        options={plan.languages.map((l) => ({ value: l, label: languageName(l) }))}
        label={t('install.language')}
        bind:open={languagesOpen}
      >
        {#snippet trigger({ props })}
          <button {...props} class="row choice">
            <span class="grow">{language ? languageName(language) : ''}</span>
            <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg>
          </button>
        {/snippet}
      </ChoiceMenu>
    </div>
  {/if}

  {#if plan?.builds.length}
    {@const builds = plan.builds}
    <div class="label" id="build-label">{t('install.build')}</div>
    <RadioGroup.Root
      orientation="horizontal"
      aria-labelledby="build-label"
      bind:value={
        () => build ?? '',
        (v) => {
          build = v as Build
          ask()
        }
      }
    >
      {#snippet child({ props })}
        <div {...props} class="seg">
          {#each builds as b (b)}
            <RadioGroup.Item value={b}>
              {#snippet child({ props })}<button {...props}>{b === 'linux' ? 'Linux' : 'Windows'}</button>{/snippet}
            </RadioGroup.Item>
          {/each}
        </div>
      {/snippet}
    </RadioGroup.Root>
  {/if}

  <div class="buttons">
    <button class="pill" bind:this={installButton} disabled={!folder || !room} onclick={install}>{t('common.install')}</button>
    <button class="glass round" onclick={oncancel}>{t('common.cancel')}</button>
  </div>
</div>

<style>
  .panel {
    box-sizing: border-box;
    width: 380px;
    max-width: 100%;
    padding: 20px;
    border-radius: var(--radius-modal);
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    text-align: left;
    box-shadow: var(--shadow-modal);
  }

  .popup {
    position: fixed;
    z-index: 30;
    /* A small window with every DLC unfolded: the popup scrolls rather than leave the window. */
    max-height: calc(100vh - 16px);
    overflow-y: auto;
  }

  h4 {
    margin: 0 0 4px;
    font-size: 16px;
    font-weight: 700;
    overflow-wrap: anywhere;
  }

  .sub {
    min-height: 1.4em;
    margin: 0;
    font-size: 13px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }

  .label {
    margin: 14px 0 6px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-3);
  }

  .group {
    border-radius: 12px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    font-size: 14px;
    color: var(--text-2);
  }

  .row + .row,
  .list {
    border-top: 1px solid var(--glass-line);
  }

  /* 14 DLC would push the panel past the window. */
  .list {
    max-height: 200px;
    overflow-y: auto;
  }

  .list .row {
    padding-left: 38px;
  }

  label.row,
  .check {
    cursor: pointer;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .path {
    display: block;
    overflow: hidden;
    font-family: ui-monospace, 'Cascadia Mono', monospace;
    font-size: 13px;
    color: var(--text);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  small {
    font-size: 12px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }

  small.warn {
    color: var(--warn);
  }

  .num {
    font-size: 13px;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  input[type='checkbox'] {
    flex: none;
    width: 16px;
    height: 16px;
    margin: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .bar {
    display: flex;
    height: 6px;
    margin: 6px 0 4px;
    overflow: hidden;
    border-radius: 3px;
    background: var(--glass-hover);
  }

  .bar i {
    height: 100%;
  }

  .used {
    background: rgb(255 255 255 / 0.28);
  }

  .game {
    background: var(--accent);
  }

  .game.over {
    background: var(--warn);
  }

  .menu {
    /* bits-ui copies this onto its portal wrapper: above the panel (30). */
    position: relative;
    z-index: 50;
    width: 300px;
    padding: 4px;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: var(--shadow-modal);
  }

  .menu:focus-visible {
    outline: none;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 10px;
    background: none;
    color: var(--text);
    font-size: 14px;
    text-align: left;
  }

  .item .path {
    flex: 1;
  }

  .item:hover:not(:disabled) {
    background: var(--glass-hover);
  }

  .menu :global(.sep) {
    margin: 4px 6px;
    border: 0;
    border-top: 1px solid var(--glass-line);
  }

  .fold,
  .choice {
    background: none;
    color: var(--text-3);
  }

  .fold {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
  }

  .fold[aria-expanded='true'] svg {
    transform: rotate(180deg);
  }

  .choice {
    width: 100%;
    border-radius: 12px;
    color: var(--text-2);
    text-align: left;
  }

  svg path {
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
  }

  .seg {
    display: inline-flex;
    gap: 2px;
    height: 36px;
    padding: 2px;
    box-sizing: border-box;
    border-radius: 10px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
  }

  .seg button {
    padding: 0 14px;
    background: none;
    color: var(--text-2);
  }

  .seg button[aria-checked='true'] {
    background: var(--glass-on);
    color: var(--text);
  }

  .buttons {
    display: flex;
    gap: 8px;
    margin-top: 16px;
  }

  .round {
    border-radius: 999px;
  }
</style>
