<script lang="ts">
  import StoreIcon from './StoreIcon.svelte'
  import type { Sort, Store } from './api'

  let {
    query = $bindable(),
    store = $bindable(),
    installedOnly = $bindable(),
    sort = $bindable(),
    total,
    attention,
    onsettings,
  }: {
    query: string
    store: Store | null
    installedOnly: boolean
    sort: Sort
    total: number
    /** Something in settings needs a look (Epic logged out, Legendary missing…). */
    attention: boolean
    onsettings: () => void
  } = $props()

  const stores: Store[] = ['steam', 'epic']
</script>

<nav class="bar">
  <label class="search">
    <svg class="line" viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <circle cx="11" cy="11" r="6.5" />
      <path d="m16 16 4 4" />
    </svg>
    <input type="search" placeholder={total ? `Search ${total} games` : 'Search games'} bind:value={query} />
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
  </div>

  <span class="spacer"></span>

  <label class="sort">
    <span>Sort</span>
    <select bind:value={sort} aria-label="Sort by">
      <option value="title">Title</option>
      <option value="lastPlayed">Last played</option>
      <option value="playtime">Playtime</option>
    </select>
    <svg class="line" viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path d="m7 10 5 5 5-5" /></svg>
  </label>

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

  .chips {
    display: flex;
    gap: 6px;
  }

  .chip {
    position: relative;
    height: 36px;
    min-width: 36px;
    padding: 0 12px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
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

  .chip[aria-pressed='true'] {
    background: var(--glass-on);
    border-color: var(--glass-on-line);
    color: var(--text);
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

  .sort span {
    padding-left: 12px;
    color: var(--text-3);
    pointer-events: none;
  }

  .sort select {
    appearance: none;
    height: 100%;
    padding: 0 30px 0 6px;
    border: 0;
    background: transparent;
    color: var(--text);
    cursor: pointer;
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
