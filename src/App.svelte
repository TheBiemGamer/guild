<script lang="ts">
  import { onMount } from 'svelte'
  import * as api from './lib/api'
  import type { Library, Progress, Sort, Store } from './lib/api'
  import Grid from './lib/Grid.svelte'
  import Detail from './lib/Detail.svelte'

  let query = $state('')
  let store = $state<Store | null>(null)
  let sort = $state<Sort>('title')
  let installedOnly = $state(false)

  let lib = $state<Library>({ games: [], total: 0, status: 'Loading library…', epic: null })
  let version = $state(0)
  let running = $state<string | null>(null)
  let progress = $state<Record<string, Progress>>({})
  let open = $state<string | null>(null)
  let error = $state<string | null>(null)
  let epicLoading = $state(false)
  let loggingIn = $state(false)
  let benchSpeed = $state<number | null>(null)
  /** The benchmark waits for both stores, so it scrolls the whole library. */
  let ready = $state(false)

  async function load() {
    lib = await api.library(query, store, installedOnly, sort)
    version++
  }

  $effect(() => {
    void [query, store, sort, installedOnly]
    load()
  })

  async function refreshEpic() {
    epicLoading = true
    await api.refresh(true)
    epicLoading = false
    await load()
  }

  async function logIn() {
    loggingIn = true
    try {
      await api.epicLogIn()
      error = null
    } catch (e) {
      error = `Epic login: ${api.message(e)}`
    }
    loggingIn = false
    await load()
  }

  onMount(() => {
    // Cached library first, then the stores: Steam is quick, Legendary can take a while.
    ;(async () => {
      benchSpeed = await api.benchSpeed()
      await api.loadCache().catch((e) => console.error('library cache', e))
      await load()
      await refreshEpic()
      ready = true
    })()

    const tick = setInterval(async () => {
      const p = await api.poll()
      running = p.running
      progress = p.progress
      if (p.changed) await load()
    }, 2000)
    // Steam installs and uninstalls finish in Steam's own dialogs; pick them up.
    const steam = setInterval(() => api.refresh(false).then(load), 60_000)
    return () => {
      clearInterval(tick)
      clearInterval(steam)
    }
  })

  const runningTitle = $derived(running ? lib.games.find((g) => g.key === running)?.title : undefined)
</script>

<main>
  <nav class="toolbar">
    <input class="search" type="search" placeholder="Search {lib.total} games…" bind:value={query} />
    <select bind:value={store} aria-label="Store">
      <option value={null}>All stores</option>
      <option value="steam">Steam</option>
      <option value="epic">Epic Games</option>
    </select>
    <select bind:value={sort} aria-label="Sort by">
      <option value="title">Title</option>
      <option value="lastPlayed">Last played</option>
      <option value="playtime">Playtime</option>
    </select>
    <label class="check"><input type="checkbox" bind:checked={installedOnly} /> Installed only</label>
    <span class="epic">
      {#if loggingIn}
        <button class="primary" disabled>Logging in…</button>
      {:else if epicLoading}
        <span class="loading">Loading Epic games…</span>
      {:else if lib.epic === 'loggedOut'}
        <button class="primary" onclick={logIn}>Log in to Epic</button>
      {:else if lib.epic === 'noLegendary'}
        <span class="muted">Install Legendary to add Epic games</span>
      {:else if lib.epic === 'unreachable'}
        <span class="muted">Can't reach Epic, showing saved games</span>
      {/if}
    </span>
  </nav>

  {#if running}
    <div class="banner playing">▶ Playing {runningTitle ?? 'a game'}</div>
  {/if}
  {#if error}
    <div class="banner error">
      <span>{error}</span>
      <button onclick={() => (error = null)} aria-label="Dismiss">✕</button>
    </div>
  {/if}

  <div class="content">
    {#if lib.games.length > 0}
      <Grid
        games={lib.games}
        resetKey={`${query}|${store}|${sort}|${installedOnly}`}
        {running}
        {progress}
        onopen={(key) => (open = key)}
        benchSpeed={ready ? benchSpeed : null}
      />
    {:else}
      <p class="empty">{lib.total > 0 ? 'No games match.' : lib.status}</p>
    {/if}
    {#if open}
      <Detail
        gameKey={open}
        running={running === open}
        progress={progress[open]}
        {version}
        onback={() => (open = null)}
        onerror={(m) => (error = m)}
        onchanged={load}
      />
    {/if}
  </div>
</main>

<style>
  main {
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--line);
    background: var(--surface);
  }

  .search {
    flex: 1;
    max-width: 520px;
    padding: 9px 14px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg);
  }

  select {
    padding: 8px 10px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg);
  }

  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
    user-select: none;
  }

  .epic {
    margin-left: auto;
  }

  .muted {
    color: var(--muted);
  }

  /* Static on purpose: an endless animation kept the page repainting while Epic loads. */
  .loading {
    color: var(--accent-2);
  }

  .banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 16px;
  }

  .banner button {
    padding: 2px 8px;
    background: transparent;
  }

  .playing {
    background: rgb(34 197 94 / 0.14);
    color: #86efac;
  }

  .error {
    background: rgb(239 68 68 / 0.14);
    color: #fca5a5;
  }

  .content {
    position: relative;
    flex: 1;
    min-height: 0;
  }

  .empty {
    display: grid;
    place-items: center;
    height: 100%;
    margin: 0;
    color: var(--muted);
    white-space: pre-line;
  }
</style>
