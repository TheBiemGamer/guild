<script lang="ts">
  import StoreIcon, { storeNames } from './StoreIcon.svelte'
  import FilterPopover from './FilterPopover.svelte'
  import DownloadsPopover from './DownloadsPopover.svelte'
  import UpdatePopover from './UpdatePopover.svelte'
  import ChoiceMenu from './ChoiceMenu.svelte'
  import { filterChips, removeChip, storeSegments, type Collection, type Filter } from './filters'
  import { t } from './i18n'
  import { onMount } from 'svelte'
  import { onOpenDownloads, type Card, type Progress, type QueueView, type Sort, type Store, type UpdateInfo } from './api'

  let {
    query = $bindable(),
    store = $bindable(),
    installedOnly = $bindable(),
    filters = $bindable(),
    collections,
    sort = $bindable(),
    popoverOpen = $bindable(),
    attention,
    progress,
    queue,
    games,
    storesInUse,
    modal,
    oncancel,
    onpause,
    onresume,
    onmove,
    onsettings,
    onadd,
    oncollections,
    update,
    appVersion,
    running,
    installing,
    installError,
    oninstall,
  }: {
    query: string
    store: Store | null
    installedOnly: boolean
    /** The Filters popover's extra filters. */
    filters: Filter[]
    /** For the Lists filters' names. */
    collections: Collection[]
    sort: Sort
    /** A popover is open; App ignores mouse Back/Forward meanwhile, as for Settings. */
    popoverOpen: boolean
    /** Something in settings needs a look (Epic logged out, Legendary missing…). */
    attention: boolean
    /** `poll`'s map, by copy key: the Downloads chip shows while it has anything, or the queue does. */
    progress: Record<string, Progress>
    /** The Epic download queue, active, waiting or paused. */
    queue: QueueView
    games: Card[]
    /** The stores the whole library uses (`Library::stores`), not just what the search shows. */
    storesInUse: Store[]
    /** A modal (Settings, Add a game) is open: Ctrl K leaves focus where it is. */
    modal: boolean
    oncancel: (key: string) => void
    onpause: () => void
    onresume: () => void
    onmove: (key: string, up: boolean) => void
    onsettings: () => void
    /** Opens the Add a game dialog. */
    onadd: () => void
    /** A collection was renamed or deleted in the Filters popover. */
    oncollections: () => void
    /** A newer release: the Update chip shows while it's set. */
    update: UpdateInfo | null
    appVersion: string
    /** A game is running; the update waits for it. */
    running: boolean
    installing: boolean
    installError: string | null
    oninstall: () => void
  } = $props()

  /** The store segments, in order; null is All. Empty (no control) with fewer than two stores. */
  const stores = $derived(storeSegments(storesInUse))
  const storeIndex = $derived(stores.indexOf(store))
  let segments = $state<HTMLButtonElement[]>([])
  const chips = $derived(filterChips(filters, collections))

  /** Arrow keys move to the next or previous store and pick it, wrapping around (radio group). */
  function onSegmentKey(e: KeyboardEvent) {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key]
    if (!step) return
    e.preventDefault()
    const i = (storeIndex + step + stores.length) % stores.length
    store = stores[i]
    segments[i]?.focus()
  }

  const sorts = $derived(
    (['lastPlayed', 'mostPlayed', 'longestToBeat', 'shortestToBeat', 'title'] as const).map((value) => ({ value, label: t(`sort.${value}`) })),
  )
  const sortName = $derived(sorts.find((s) => s.value === sort)?.label)

  let open = $state<'filters' | 'downloads' | 'sort' | 'update' | null>(null)
  let search = $state<HTMLInputElement>()
  let filtersChip = $state<HTMLButtonElement>()
  let downloadsChip = $state<HTMLButtonElement>()
  let sortChip = $state<HTMLButtonElement>()
  let updateChip = $state<HTMLButtonElement>()
  $effect(() => {
    popoverOpen = open !== null
  })

  const jobs = $derived(Object.values(progress))
  const downloading = $derived(jobs.length > 0 || queue.jobs.length > 0)
  // The active Epic job's progress, else the average.
  const activeProgress = $derived(queue.active ? progress[queue.active] : undefined)
  const fraction = $derived(
    activeProgress ? activeProgress.fraction : jobs.length ? jobs.reduce((sum, p) => sum + p.fraction, 0) / jobs.length : 0,
  )
  // The last download ended while its popover was open.
  $effect(() => {
    if (!downloading && open === 'downloads') open = null
  })
  // The tray's Downloads item: the window is already shown; open the popover only if there is something in it.
  onMount(() => {
    const unlisten = onOpenDownloads(() => {
      if (downloading && !modal) open = 'downloads'
    })
    return () => void unlisten.then((f) => f())
  })

  function close(refocus: boolean) {
    const chip = { filters: filtersChip, downloads: downloadsChip, sort: sortChip, update: updateChip }[open ?? 'filters']
    open = null
    if (refocus) chip?.focus()
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.ctrlKey && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'k' && !modal) {
      e.preventDefault()
      open = null
      search?.focus()
      search?.select()
    }
  }
</script>

<svelte:window {onkeydown} />

<nav class="bar">
  <label class="search">
    <svg class="line" viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <circle cx="11" cy="11" r="6.5" />
      <path d="m16 16 4 4" />
    </svg>
    <input
      type="search"
      placeholder={t('toolbar.search')}
      aria-keyshortcuts="Control+K"
      bind:this={search}
      bind:value={query}
    />
    {#if !query}<kbd aria-hidden="true">{t('toolbar.shortcut')}</kbd>{/if}
  </label>

  <div class="chips" role="group" aria-label={t('toolbar.filter')}>
    {#if stores.length}
      <div class="seg" role="radiogroup" aria-label={t('toolbar.store')} style="--at: {storeIndex}">
        <span class="thumb" aria-hidden="true"></span>
        {#each stores as s, i (s ?? 'all')}
          {@const name = s === null ? undefined : storeNames[s]}
          <button
            role="radio"
            aria-checked={store === s}
            aria-label={name}
            title={name}
            tabindex={store === s ? 0 : -1}
            bind:this={segments[i]}
            onclick={() => (store = s)}
            onkeydown={onSegmentKey}
          >
            {#if s === null}{t('toolbar.all')}{:else}<StoreIcon provider={s} size={18} />{/if}
          </button>
        {/each}
      </div>
    {/if}
    <button class="chip" aria-pressed={installedOnly} onclick={() => (installedOnly = !installedOnly)}>
      {t('play.installed')}
    </button>
    {#each chips as chip (chip.filter)}
      <button class="chip on" aria-label={t('toolbar.removeFilter', { name: chip.label })} onclick={() => (filters = removeChip(filters, chip))}>
        {chip.label}<i aria-hidden="true">✕</i>
      </button>
    {/each}
    <div class="dd">
      <button
        class="chip"
        bind:this={filtersChip}
        aria-haspopup="dialog"
        aria-expanded={open === 'filters'}
        onclick={() => (open = open === 'filters' ? null : 'filters')}
      >
        <svg class="line" viewBox="0 0 24 24" width="15" height="15" aria-hidden="true">
          <path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12" />
          <circle cx="16" cy="6" r="2" /><circle cx="10" cy="12" r="2" /><circle cx="18" cy="18" r="2" />
        </svg>
        {t('filters.title')}
      </button>
      {#if open === 'filters'}
        <FilterPopover bind:filters {collections} multiStore={storesInUse.length > 1} anchor={filtersChip} onclose={close} onchanged={oncollections} />
      {/if}
    </div>
  </div>

  <span class="spacer"></span>

  <div class="dd">
    <button
      class="chip"
      bind:this={sortChip}
      aria-haspopup="menu"
      aria-expanded={open === 'sort'}
      aria-label={t('toolbar.sort', { sort: sortName ?? '' })}
      onclick={() => (open = open === 'sort' ? null : 'sort')}
    >
      {sortName}
      <svg class="line" viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path d="m7 10 5 5 5-5" /></svg>
    </button>
    {#if open === 'sort'}
      <ChoiceMenu bind:value={sort} options={sorts} label={t('toolbar.sortBy')} anchor={sortChip} onclose={close} />
    {/if}
  </div>

  {#if update}
    <div class="dd">
      <button
        class="chip"
        bind:this={updateChip}
        aria-haspopup="dialog"
        aria-expanded={open === 'update'}
        onclick={() => (open = open === 'update' ? null : 'update')}
      >
        <svg class="line up" viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path d="M12 19V5M6 11l6-6 6 6" /></svg>
        {t('play.update')}
      </button>
      {#if open === 'update'}
        <UpdatePopover
          {update}
          current={appVersion}
          {running}
          {installing}
          error={installError}
          anchor={updateChip}
          onclose={close}
          {oninstall}
        />
      {/if}
    </div>
  {/if}

  {#if downloading}
    <div class="dd">
      <button
        class="chip icon"
        bind:this={downloadsChip}
        aria-label={t('toolbar.downloadsAt', { percent: `${Math.round(fraction * 100)}%` })}
        title={t('downloads.title')}
        aria-haspopup="dialog"
        aria-expanded={open === 'downloads'}
        onclick={() => (open = open === 'downloads' ? null : 'downloads')}
      >
        <span class="ring" style="--done: {fraction * 100}%"></span>
        <svg class="line" viewBox="0 0 24 24" width="17" height="17" aria-hidden="true"><path d="M12 4v11M7 10l5 5 5-5M5 20h14" /></svg>
      </button>
      {#if open === 'downloads'}
        <DownloadsPopover {progress} {queue} {games} anchor={downloadsChip} onclose={close} {oncancel} {onpause} {onresume} {onmove} />
      {/if}
    </div>
  {/if}

  <button class="chip icon" onclick={onadd} aria-label={t('toolbar.addGame')} title={t('toolbar.addGame')}>
    <svg class="line" viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
  </button>

  <button class="chip icon settings" onclick={onsettings} aria-label={attention ? t('toolbar.settingsAttention') : t('toolbar.settings')} title={t('toolbar.settings')}>
    <svg class="line" viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
      <circle cx="12" cy="12" r="3" />
      <path
        d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
      />
    </svg>
    {#if attention}<span class="dot"></span>{/if}
  </button>
</nav>

<style>
  .bar {
    position: relative;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 16px 24px 12px;
  }

  .line {
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .search {
    flex: 0 1 380px;
    min-width: 160px;
    height: 36px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    border-radius: 10px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
    color: var(--text-2);
    transition: border-color 120ms, background 120ms;
  }

  .search:focus-within {
    border-color: var(--glass-on-line);
    background: var(--glass-strong);
  }

  .search input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--text);
  }

  .search input::placeholder {
    color: var(--text-3);
  }

  kbd {
    flex: none;
    padding: 1px 6px;
    border-radius: 5px;
    border: 1px solid var(--glass-on-line);
    font: inherit;
    font-size: 11px;
    color: var(--text-2);
  }

  .search:focus-within kbd {
    display: none;
  }

  .chips {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .dd {
    position: relative;
  }

  /* Store picker: one chip-sized segmented control; the selected segment's pill slides under it. */
  .seg {
    position: relative;
    display: flex;
    height: 36px;
    padding: 2px;
    border-radius: 10px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
  }

  .seg button {
    position: relative;
    width: 36px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--text-2);
  }

  .seg button:hover,
  .seg button[aria-checked='true'] {
    color: var(--text);
  }

  .seg button:hover:not([aria-checked='true']) {
    background: var(--glass-hover);
  }

  .thumb {
    position: absolute;
    top: 2px;
    bottom: 2px;
    left: 2px;
    width: 36px;
    border-radius: 8px;
    background: var(--glass-on);
    border: 1px solid var(--glass-on-line);
    transform: translateX(calc(var(--at) * 36px));
    transition: transform 150ms ease-out;
  }

  .chip {
    position: relative;
    height: 36px;
    min-width: 36px;
    padding: 0 12px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    white-space: nowrap;
    border-radius: 10px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
    color: var(--text-2);
  }

  .chip.icon {
    padding: 0;
  }

  .chip:hover:not(:disabled) {
    background: var(--glass-hover);
    color: var(--text);
  }

  .chip[aria-pressed='true'],
  .chip.on {
    background: var(--glass-on);
    border-color: var(--glass-on-line);
    color: var(--text);
  }

  .chip.on {
    gap: 8px;
  }

  .chip.on i {
    font-style: normal;
    font-size: 11px;
    color: var(--text-3);
  }

  /* Progress as a ring inside the chip's edge; it changes with each poll, never per frame. */
  .ring {
    position: absolute;
    inset: 3px;
    padding: 2px;
    border-radius: 8px;
    background: conic-gradient(var(--accent-2) 0 var(--done), transparent var(--done));
    mask:
      linear-gradient(#000 0 0) content-box exclude,
      linear-gradient(#000 0 0);
    pointer-events: none;
  }

  .up {
    color: var(--accent-2);
  }

  .spacer {
    flex: 1;
  }

  .dot {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--warn);
    box-shadow: 0 0 0 2px var(--bg);
  }
</style>
