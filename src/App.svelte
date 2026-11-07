<script lang="ts" module>
  /** Both stores and the owned Steam list are refreshed at least this often while the app is open. */
  export const FULL_REFRESH_EVERY_MS = 6 * 60 * 60 * 1000
  /** Coming back to the window refreshes when the last full refresh is at least this old. */
  export const REFRESH_ON_RETURN_AFTER_MS = 60 * 60 * 1000

  /** Whether at least `minGapMs` passed since `lastMs`; never refreshed (`null`) is due. */
  export function dueForRefresh(lastMs: number | null, nowMs: number, minGapMs: number): boolean {
    return lastMs === null || nowMs - lastMs >= minGapMs
  }

  /** The mouse's Back (3) and Forward (4) buttons: the next open page and the page Forward would reopen, or null for no step. */
  export function navStep(
    button: number,
    open: string | null,
    last: string | null,
  ): { open: string | null; last: string | null } | null {
    if (button === 3 && open !== null) return { open: null, last: open }
    if (button === 4 && open === null && last !== null) return { open: last, last }
    return null
  }
</script>

<script lang="ts">
  import { onMount, tick } from 'svelte'
  import * as api from './lib/api'
  import type { Action, Featured, Library, Progress, SettingsView, Sort, Store } from './lib/api'
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

  let lib = $state<Library>({ games: [], total: 0, status: '', epic: null, epicAttention: false, steamAttention: false, epicNotice: false })
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

  /** The Epic copy whose install is being confirmed, the rectangle the popup opens beside, and what installs it. */
  let confirming = $state<{ game: { key: string; title: string }; anchor: DOMRect; go: () => void } | null>(null)

  /** Steam installs in its own dialog; an Epic copy asks here first, with size and folder. */
  function confirmInstall(key: string, title: string, anchor: DOMRect, go: () => void) {
    if (key.startsWith('epic-')) confirming = { game: { key, title }, anchor, go }
    else go()
  }

  /** `key` is the card's chosen copy. */
  function install(key: string, anchor: DOMRect) {
    const title = lib.games.find((g) => g.chosen === key)?.title ?? ''
    confirmInstall(key, title, anchor, () => act('install', key))
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
      await api.refresh(true, true)
      refreshedAt = Date.now()
    } finally {
      refreshing = false
    }
    await settingsChanged()
  }

  /** The quiet full refresh: no status text, failures only show as the Settings dot. A running refresh wins. */
  async function backgroundRefresh() {
    if (refreshing) return
    refreshing = true
    try {
      await api.refresh(true, true)
      refreshedAt = Date.now()
      await reload()
    } catch (e) {
      console.error('background refresh', e)
    } finally {
      refreshing = false
    }
  }

  function returned() {
    if (document.visibilityState === 'visible' && dueForRefresh(refreshedAt, Date.now(), REFRESH_ON_RETURN_AFTER_MS)) {
      backgroundRefresh()
    }
  }

  function dismissNotice() {
    lib.epicNotice = false
    api.dismissEpicNotice()
  }

  /** Waits, at most 300 ms, for the covers of the cards on screen at start, so the window opens on a filled grid. */
  async function firstCovers() {
    // The grid's card pitch (180 px card + 16 px gap, rows 286 px); measured from the window, since the
    // hidden window may not lay out yet. A few extra covers are harmless.
    const cols = Math.max(1, Math.floor((window.innerWidth - 16) / 196))
    const rows = Math.ceil(window.innerHeight / 286)
    const covers = lib.games.slice(0, cols * rows).map((g, i) => api.cover(g.key, i))
    await Promise.race([Promise.allSettled(covers), new Promise((done) => setTimeout(done, 300))])
  }

  onMount(() => {
    // Saved library first, then a quiet rescan: Steam is quick, Legendary can take a while.
    ;(async () => {
      benchSpeed = await api.benchSpeed()
      await api.loadCache().catch((e) => console.error('library cache', e))
      await reload()
      started = true
      await firstCovers()
      api.ready()
      loadSettings()
      // Holds the flag so a focus event during startup doesn't start a second full refresh.
      refreshing = true
      try {
        await api.refresh(true, false)
        refreshedAt = Date.now()
      } finally {
        refreshing = false
      }
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
    const steam = setInterval(async () => {
      if (await api.refresh(false, false)) await reload()
    }, 60_000)
    const full = setInterval(backgroundRefresh, FULL_REFRESH_EVERY_MS)
    document.addEventListener('visibilitychange', returned)
    window.addEventListener('focus', returned)
    return () => {
      clearInterval(poller)
      clearInterval(steam)
      clearInterval(full)
      document.removeEventListener('visibilitychange', returned)
      window.removeEventListener('focus', returned)
    }
  })

  /** What opened the game page (a card or the banner), so Back can return focus there. */
  let opener: HTMLElement | null = null

  function openGame(key: string) {
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null
    open = key
  }

  /** The last page closed this session, for the Forward button. */
  let lastClosed: string | null = null

  async function closeGame() {
    if (open) lastClosed = open
    open = null
    tint = undefined
    const back = opener
    opener = null
    // The library is visible again only after the update; a recycled-away card is gone (isConnected).
    await tick()
    if (back?.isConnected) back.focus({ preventScroll: true })
  }

  function mouseNav(e: MouseEvent) {
    if (e.button !== 3 && e.button !== 4) return
    e.preventDefault()
    // mouseup and auxclick both arrive for one press; only mouseup acts.
    if (e.type !== 'mouseup') return
    const step = navStep(e.button, open, lastClosed)
    if (!step) return
    if (step.open === null) closeGame()
    else if (lib.games.some((g) => g.copyKeys.includes(step.open!) || g.chosen === step.open)) openGame(step.open)
  }

  const ambientHero = $derived(open !== null && tint !== undefined ? tint : (featured?.hero ?? null))
  const runningTitle = $derived(running ? lib.games.find((g) => g.copyKeys.includes(running!))?.title : undefined)
  const showContinue = $derived(featured !== null && query === '' && (settings?.showBanner ?? true))
  const BANNER_H = 180
  const LABEL_H = 18
  const headerHeight = $derived((showContinue ? BANNER_H + 24 : 0) + LABEL_H)
  /** The Epic filter with no Epic games at all: say why and offer the fix. */
  const epicEmpty = $derived(store === 'epic' && query === '' && !installedOnly && lib.games.length === 0)
</script>

<svelte:window onmouseup={mouseNav} onauxclick={mouseNav} />

<Ambient hero={ambientHero} enabled={settings?.ambient ?? true} />

<main>
  <Toolbar
    bind:query
    bind:store
    bind:installedOnly
    bind:sort
    total={lib.total}
    attention={lib.epicAttention || lib.steamAttention}
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
      folder={settings?.epicInstallDir}
      oncancel={() => (confirming = null)}
      onconfirm={() => {
        const { go } = confirming!
        confirming = null
        go()
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
                running={running !== null && featured.copyKeys.includes(running)}
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
        runningKey={running}
        progressByKey={progress}
        {version}
        onback={closeGame}
        onerror={(m) => (error = m)}
        onchanged={reload}
        ontint={(url) => (tint = url)}
        onconfirminstall={confirmInstall}
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
