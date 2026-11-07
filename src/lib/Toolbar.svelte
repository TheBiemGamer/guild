<script lang="ts">
  import StoreIcon from './StoreIcon.svelte'
  import FilterPopover from './FilterPopover.svelte'
  import DownloadsPopover from './DownloadsPopover.svelte'
  import { filterChips, removeChip, type Filter } from './filters'
  import type { Card, Group, Progress, QueueView, Sort, Store } from './api'

  let {
    query = $bindable(),
    store = $bindable(),
    installedOnly = $bindable(),
    filters = $bindable(),
    sort = $bindable(),
    group = $bindable(),
    popoverOpen = $bindable(),
    total,
    attention,
    progress,
    queue,
    games,
    modal,
    oncancel,
    onpause,
    onresume,
    onmove,
    onsettings,
  }: {
    query: string
    store: Store | null
    installedOnly: boolean
    /** The Filters popover's extra filters. */
    filters: Filter[]
    sort: Sort
    group: Group
    /** A popover is open; App ignores mouse Back/Forward meanwhile, as for Settings. */
    popoverOpen: boolean
    total: number
    /** Something in settings needs a look (Epic logged out, Legendary missing…). */
    attention: boolean
    /** `poll`'s map, by copy key: the Downloads chip shows while it has anything, or the queue does. */
    progress: Record<string, Progress>
    /** The Epic download queue, active, waiting or paused. */
    queue: QueueView
    games: Card[]
    /** A modal (Settings) is open: Ctrl K leaves focus where it is. */
    modal: boolean
    oncancel: (key: string) => void
    onpause: () => void
    onresume: () => void
    onmove: (key: string, up: boolean) => void
    onsettings: () => void
  } = $props()

  const stores: Store[] = ['steam', 'epic']
  const chips = $derived(filterChips(filters))

  let open = $state<'filters' | 'downloads' | null>(null)
  let search = $state<HTMLInputElement>()
  let filtersChip = $state<HTMLButtonElement>()
  let downloadsChip = $state<HTMLButtonElement>()
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

  function close(refocus: boolean) {
    const chip = open === 'filters' ? filtersChip : downloadsChip
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
      placeholder={total ? `Search ${total} games` : 'Search games'}
      aria-keyshortcuts="Control+K"
      bind:this={search}
      bind:value={query}
    />
    {#if !query}<kbd aria-hidden="true">Ctrl K</kbd>{/if}
  </label>

  <div class="chips" role="group" aria-label="Filter">
    <button class="chip" aria-pressed={store === null} onclick={() => (store = null)}>All</button>
    {#each stores as s (s)}
      <button
        class="chip icon"
        aria-pressed={store === s}
        title={s === 'steam' ? 'Steam' : 'Epic Games'}
        onclick={() => (store = store === s ? null : s)}
      >
        <StoreIcon provider={s} size={18} />
      </button>
    {/each}
    <button class="chip" aria-pressed={installedOnly} onclick={() => (installedOnly = !installedOnly)}>
      Installed
    </button>
    {#each chips as chip (chip.filter)}
      <button class="chip on" aria-label="Remove filter: {chip.label}" onclick={() => (filters = removeChip(filters, chip))}>
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
        Filters
      </button>
      {#if open === 'filters'}
        <FilterPopover bind:filters anchor={filtersChip} onclose={close} />
      {/if}
    </div>
  </div>

  <span class="spacer"></span>

  <label class="sort">
    <span class="prefix" aria-hidden="true">Group:</span>
    <select bind:value={group} aria-label="Group by">
      <option value="none">None</option>
      <option value="store">Store</option>
      <option value="playtime">Your playtime</option>
      <option value="timeToBeat">How long to beat</option>
    </select>
    <svg class="line" viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path d="m7 10 5 5 5-5" /></svg>
  </label>

  <label class="sort">
    <select bind:value={sort} aria-label="Sort by">
      <option value="lastPlayed">Recently played</option>
      <option value="mostPlayed">Most played</option>
      <option value="longestToBeat">Longest to beat</option>
      <option value="shortestToBeat">Shortest to beat</option>
      <option value="title">Title</option>
    </select>
    <svg class="line" viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path d="m7 10 5 5 5-5" /></svg>
  </label>

  {#if downloading}
    <div class="dd">
      <button
        class="chip icon"
        bind:this={downloadsChip}
        aria-label="Downloads, {Math.round(fraction * 100)}%"
        title="Downloads"
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

  <button class="chip icon settings" onclick={onsettings} aria-label={attention ? 'Settings (needs attention)' : 'Settings'} title="Settings">
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

  .spacer {
    flex: 1;
  }

  .sort {
    position: relative;
    height: 36px;
    display: flex;
    align-items: center;
    border-radius: 10px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
    color: var(--text-2);
  }

  .sort:hover {
    background: var(--glass-hover);
  }

  .sort select {
    appearance: none;
    height: 100%;
    padding: 0 30px 0 12px;
    border: 0;
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }

  /* "Group:" before the chosen grouping, as one chip. */
  .sort .prefix {
    padding-left: 12px;
    color: var(--text-2);
    pointer-events: none;
  }

  .prefix + select {
    padding-left: 4px;
  }

  .sort select:focus-visible {
    outline: none;
  }

  .sort:focus-within {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }

  .sort option {
    background: var(--surface);
  }

  .sort .line {
    position: absolute;
    right: 10px;
    pointer-events: none;
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
