<script lang="ts">
  import { onMount, tick } from 'svelte'
  import * as api from './lib/api'
  import type { Action, Card, Featured, Library, Progress, SettingsView, Sort, Store } from './lib/api'
  import Ambient from './lib/Ambient.svelte'
  import Toolbar from './lib/Toolbar.svelte'
  import Continue from './lib/Continue.svelte'
  import Grid from './lib/Grid.svelte'
  import Detail from './lib/Detail.svelte'
  import Settings from './lib/Settings.svelte'
  import InstallPopup from './lib/InstallPopup.svelte'

  let query = $state('')
  let store = $state<Store | null>(null)
  let sort = $state<Sort>('title')
  let installedOnly = $state(false)

  let lib = $state<Library>({ games: [], total: 0, status: '', epic: null, epicAttention: false, epicNotice: false })
  let featured = $state<Featured | null>(null)
  let version = $state(0)
  let running = $state<string | null>(null)
  let progress = $state<Record<string, Progress>>({})
  let open = $state<string | null>(null)
  /** The open game's art for the tint; undefined until it's known, so the library's tint stays meanwhile. */
  let tint = $state<string | null | undefined>(undefined)
  let error = $state<string | null>(null)
  let loggingIn = $state(false)
  let settings = $state<SettingsView | null>(null)
  let settingsOpen = $state(false)
  let refreshing = $state(false)
  /** When the last full refresh finished (ms since epoch). */
  let refreshedAt = $state<number | null>(null)
  /** The saved library has been read; until then the window stays quiet instead of saying "loading". */
  let started = $state(false)
  let benchSpeed = $state<number | null>(null)
  /** The benchmark waits for both stores, so it scrolls the whole library. */
  let ready = $state(false)

  async function load() {
    lib = await api.library(query, store, installedOnly, sort)
    version++
  }

  /** Library and the last-played game together, so the banner and the tint arrive with the grid. */
  async function reload() {
    const [, f] = await Promise.all([load(), api.featured()])
    featured = f
  }

  $effect(() => {
    void [query, store, sort, installedOnly]
    load()
  })

  /** The Epic game whose install is being confirmed, and its cover's rectangle. */
  let confirming = $state<{ game: Card; anchor: DOMRect } | null>(null)

  /** Steam installs in its own dialog; Epic asks here first, with size and folder. */
  function install(key: string, anchor: DOMRect) {
    const game = lib.games.find((g) => g.key === key)
    if (game?.provider === 'epic') confirming = { game, anchor }
    else act('install', key)
  }

  async function act(action: Action, key: string) {
    try {
      await api.act(action, key)
    } catch (e) {
      error = api.message(e)
    }
    await reload()
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
    await reload()
  }

  async function loadSettings() {
    try {
      settings = await api.settingsView()
    } catch (e) {
      error = `Settings: ${api.message(e)}`
    }
  }

  async function openSettings() {
    // Read fresh each time: the Epic state and cache size change while the panel is closed.
    await loadSettings()
    settingsOpen = settings !== null
  }

  /** After a setting changed: the panel, the tint and the banner follow, and a logout changes the library. */
  async function settingsChanged() {
    await Promise.all([loadSettings(), reload()])
  }

  async function refreshNow() {
    refreshing = true
    try {
      await api.refresh(true)
      refreshedAt = Date.now()
    } finally {
      refreshing = false
    }
    await settingsChanged()
  }

  function dismissNotice() {
    lib.epicNotice = false
    api.dismissEpicNotice()
  }

  onMount(() => {
    // Saved library first, then a quiet rescan: Steam is quick, Legendary can take a while.
    ;(async () => {
      benchSpeed = await api.benchSpeed()
      await api.loadCache().catch((e) => console.error('library cache', e))
      await reload()
      started = true
      loadSettings()
      await api.refresh(true)
      refreshedAt = Date.now()
      await reload()
      ready = true
    })()

    const poller = setInterval(async () => {
      const p = await api.poll()
      running = p.running
      progress = p.progress
      if (p.changed) await reload()
    }, 2000)
    // Steam installs and uninstalls finish in Steam's own dialogs; pick them up.
    const steam = setInterval(() => api.refresh(false).then(reload), 60_000)
    return () => {
      clearInterval(poller)
      clearInterval(steam)
    }
  })

  /** What opened the game page (a card or the banner), so Back can return focus there. */
  let opener: HTMLElement | null = null

  function openGame(key: string) {
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null
    open = key
  }

  async function closeGame() {
    open = null
    tint = undefined
    const back = opener
    opener = null
    // The library is visible again only after the update; a recycled-away card is gone (isConnected).
    await tick()
    if (back?.isConnected) back.focus({ preventScroll: true })
  }

  const ambientHero = $derived(open !== null && tint !== undefined ? tint : (featured?.hero ?? null))
  const runningTitle = $derived(running ? lib.games.find((g) => g.key === running)?.title : undefined)
  const showContinue = $derived(featured !== null && query === '' && (settings?.showBanner ?? true))
  const BANNER_H = 180
  const LABEL_H = 18
  const headerHeight = $derived((showContinue ? BANNER_H + 24 : 0) + LABEL_H)
  /** The Epic filter with no Epic games at all: say why and offer the fix. */
  const epicEmpty = $derived(store === 'epic' && query === '' && !installedOnly && lib.games.length === 0)
</script>

<Ambient hero={ambientHero} enabled={settings?.ambient ?? true} />

<main>
  <Toolbar
    bind:query
    bind:store
    bind:installedOnly
    bind:sort
    total={lib.total}
    attention={lib.epicAttention}
    onsettings={openSettings}
  />

  {#if lib.epicNotice}
    <div class="notice">
      <span>You were logged out of Epic Games. Log in again in Settings.</span>
      <button class="glass" onclick={openSettings}>Settings</button>
      <button class="close" onclick={dismissNotice} aria-label="Dismiss">✕</button>
    </div>
  {/if}
  {#if running}
    <div class="notice playing"><span class="live"></span>Playing {runningTitle ?? 'a game'}</div>
  {/if}
  {#if error}
    <div class="notice error">
      <span>{error}</span>
      <button class="close" onclick={() => (error = null)} aria-label="Dismiss">✕</button>
    </div>
  {/if}

  {#if confirming}
    <!-- A fresh popup per game, so a late size never lands on another game. -->
    {#key confirming.game.key}
    <InstallPopup
      game={confirming.game}
      anchor={confirming.anchor}
      oncancel={() => (confirming = null)}
      onconfirm={() => {
        const { key } = confirming!.game
        confirming = null
        act('install', key)
      }}
    />
    {/key}
  {/if}

  <div class="content">
    <!-- Hidden, not unmounted, under the game page: the grid keeps its scroll position. -->
    <div class="library" class:away={open !== null}>
    {#if lib.games.length > 0}
      <Grid
        games={lib.games}
        resetKey={`${query}|${store}|${sort}|${installedOnly}`}
        {running}
        {progress}
        onopen={openGame}
        onplay={(key) => act('play', key)}
        oninstall={install}
        benchSpeed={ready ? benchSpeed : null}
        {headerHeight}
      >
        {#snippet header()}
          {#if showContinue && featured}
            <div style="height: {BANNER_H}px; margin-bottom: 24px">
              <Continue
                {featured}
                running={running === featured.key}
                onplay={(key) => act('play', key)}
                onopen={openGame}
              />
            </div>
          {/if}
          <h2 class="label">
            {query || store || installedOnly ? 'Results' : 'Library'}
            <span>{lib.games.length}</span>
          </h2>
        {/snippet}
      </Grid>
    {:else if epicEmpty && lib.epic === 'noLegendary'}
      <div class="empty">
        <p>Legendary isn't installed, so Epic games can't be listed.</p>
        <small>Install Legendary, then refresh to add your Epic library.</small>
      </div>
    {:else if epicEmpty && lib.epic === 'loggedOut'}
      <div class="empty">
        <p>Log in to Epic to see your Epic games</p>
        <button class="pill" disabled={loggingIn} onclick={logIn}>{loggingIn ? 'Logging in…' : 'Log in to Epic'}</button>
      </div>
    {:else if epicEmpty && lib.epic === 'unreachable'}
      <div class="empty">
        <p>Can't reach Epic, showing saved games</p>
        <small>No Epic games are saved yet. They load once Epic is reachable.</small>
      </div>
    {:else if started}
      <div class="empty">
        <p>{lib.total > 0 ? 'No games match.' : lib.status}</p>
      </div>
    {/if}
    </div>
    {#if open}
      <Detail
        gameKey={open}
        running={running === open}
        progress={progress[open]}
        {version}
        onback={closeGame}
        onerror={(m) => (error = m)}
        onchanged={reload}
        ontint={(url) => (tint = url)}
      />
    {/if}
  </div>
</main>

{#if settingsOpen && settings}
  <Settings
    view={settings}
    {refreshedAt}
    {refreshing}
    {loggingIn}
    onclose={() => (settingsOpen = false)}
    onchange={settingsChanged}
    onrefresh={refreshNow}
    onlogin={async () => {
      await logIn()
      await loadSettings()
    }}
  />
{/if}

<style>
  main {
    position: relative;
    z-index: 1;
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  .notice {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 0 24px 12px;
    padding: 8px 8px 8px 16px;
    min-height: 44px;
    border-radius: 12px;
    background: var(--glass-strong);
    border: 1px solid var(--glass-line);
    color: var(--text);
  }

  .notice > span:first-child {
    flex: 1;
  }

  .notice.playing {
    gap: 10px;
    color: var(--text-2);
  }

  .live {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--play);
  }

  .notice.error {
    border-color: rgb(239 68 68 / 0.35);
    color: #fecaca;
  }

  .close {
    width: 28px;
    height: 28px;
    padding: 0;
    background: transparent;
    color: var(--text-2);
  }

  .content {
    position: relative;
    flex: 1;
    min-height: 0;
  }

  .library {
    display: contents;
  }

  .library.away {
    visibility: hidden;
  }

  .label {
    display: flex;
    align-items: baseline;
    gap: 8px;
    height: 18px;
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    line-height: 18px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-2);
  }

  .label span {
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    height: 100%;
    padding: 24px;
    text-align: center;
    color: var(--text-2);
  }

  .empty p {
    margin: 0;
    font-size: 16px;
    white-space: pre-line;
  }

  .empty small {
    color: var(--text-3);
  }
</style>
