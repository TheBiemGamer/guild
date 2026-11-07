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
  import type { Action, Card, Collection, Featured, Library, Progress, QueueView, Section, SettingsView, Sort, Store } from './lib/api'
  import Ambient from './lib/Ambient.svelte'
  import Toolbar from './lib/Toolbar.svelte'
  import Continue from './lib/Continue.svelte'
  import Grid from './lib/Grid.svelte'
  import Detail from './lib/Detail.svelte'
  import Settings from './lib/Settings.svelte'
  import InstallPopup from './lib/InstallPopup.svelte'
  import GameMenu from './lib/GameMenu.svelte'
  import AddGame from './lib/AddGame.svelte'
  import NotesPopover, { installedNotes } from './lib/NotesPopover.svelte'
  import { collectionFilter, isList, matches, type Filter } from './lib/filters'

  let query = $state('')
  let store = $state<Store | null>(null)
  let sort = $state<Sort>('title')
  let installedOnly = $state(false)
  /** The Filters popover's extra filters, applied here to the loaded list. */
  let filters = $state<Filter[]>([])
  let popoverOpen = $state(false)
  let collections = $state<Collection[]>([])

  let lib = $state<Library>({ games: [], groups: [], total: 0, status: '', epic: null, epicAttention: false, steamAttention: false, epicNotice: false, gog: null, gogAttention: false })
  let featured = $state<Featured | null>(null)
  let version = $state(0)
  let running = $state<string | null>(null)
  let progress = $state<Record<string, Progress>>({})
  let queue = $state<QueueView>(api.emptyQueue)
  let open = $state<string | null>(null)
  /** The open game's art for the tint; undefined until it's known, so the library's tint stays meanwhile. */
  let tint = $state<string | null | undefined>(undefined)
  let error = $state<string | null>(null)
  let loggingIn = $state(false)
  let settings = $state<SettingsView | null>(null)
  let settingsOpen = $state(false)
  /** The Settings section to open at; null is the top. */
  let settingsSection = $state<string | null>(null)
  /** The Add a game dialog: 'new', or the added game being edited. */
  let adding = $state<'new' | ({ key: string } & api.CustomGame) | null>(null)
  let refreshing = $state(false)
  /** When the last full refresh finished (ms since epoch). */
  let refreshedAt = $state<number | null>(null)
  /** The saved library has been read; until then the window stays quiet instead of saying "loading". */
  let started = $state(false)
  let benchSpeed = $state<number | null>(null)
  /** The benchmark waits for both stores, so it scrolls the whole library. */
  let ready = $state(false)
  /** A newer release, from the last check. */
  let update = $state<api.UpdateInfo | null>(null)
  /** A check answered this session. */
  let updateChecked = $state(false)
  let installing = $state(false)
  let installError = $state<string | null>(null)
  /** The version this start updated to, for the notice; null once dismissed. */
  let updatedTo = $state<string | null>(null)
  let updatedNotesOpen = $state(false)
  let updatedNotesButton = $state<HTMLButtonElement>()

  async function checkUpdate() {
    update = await api.checkUpdate()
    updateChecked = true
  }

  /** The schedule's check: failures are logged in Rust and only show when Settings checks. */
  function quietCheck() {
    if (import.meta.env.DEV || benchSpeed !== null) return
    checkUpdate().catch(() => {})
  }

  /** On success the installer closes the app and starts the new version. */
  async function installUpdate() {
    installing = true
    installError = null
    try {
      await api.installUpdate()
    } catch (e) {
      installError = api.message(e)
    } finally {
      installing = false
    }
  }

  async function load() {
    // Rust needs the Lists filters: Hidden brings hidden games back, and they decide whether favorites go on top.
    lib = await api.library(query, store, installedOnly, sort, filters.filter(isList))
    version++
  }

  /** Library, the last-played game and the collections together, so the banner and the tint arrive with the grid. */
  async function reload() {
    const [, f, c] = await Promise.all([load(), api.featured(), api.collections().catch(() => collections)])
    featured = f
    collections = c
    // A collection that went away takes its filter along.
    const gone = (f: Filter) => f.startsWith('collection-') && !c.some((x) => collectionFilter(x.id) === f)
    if (filters.some(gone)) filters = filters.filter((f) => !gone(f))
  }

  async function setFavorite(key: string, on: boolean) {
    await api.setFavorite(key, on).catch((e) => (error = api.message(e)))
    await reload()
  }

  /** The game menu a card's right-click opened, at the pointer; focus goes back to what had it. */
  let cardMenu = $state<{ game: Card; x: number; y: number; back: HTMLElement | null } | null>(null)
  function openCardMenu(game: Card, x: number, y: number) {
    cardMenu = { game, x, y, back: document.activeElement instanceof HTMLElement ? document.activeElement : null }
  }
  function closeCardMenu(refocus: boolean) {
    const back = cardMenu?.back
    cardMenu = null
    if (refocus && back?.isConnected) back.focus({ preventScroll: true })
  }

  $effect(() => {
    void [query, store, sort, installedOnly, filters]
    load()
  })

  /** The sort as saved; null until the saved one is restored at start. */
  let savedSort: Sort | null = null
  $effect(() => {
    const now = sort
    if (savedSort === null) return
    if (now !== savedSort) api.updateSetting('library_sort', now).catch((e) => console.error('saving the library sort', e))
    savedSort = now
  })

  /** The Epic or GOG copy whose install is being confirmed, the rectangle the popup opens beside, and what installs it. */
  let confirming = $state<{ game: { key: string; title: string }; anchor: DOMRect; go: (withDlc: boolean) => void } | null>(null)

  /** Steam installs in its own dialog; an Epic or GOG copy asks here first, with size and folder. */
  function confirmInstall(key: string, title: string, anchor: DOMRect, go: (withDlc: boolean) => void) {
    if (key.startsWith('epic-') || key.startsWith('gog-')) confirming = { game: { key, title }, anchor, go }
    else go(true)
  }

  /** `key` is the card's chosen copy. */
  function install(key: string, anchor: DOMRect) {
    const title = lib.games.find((g) => g.chosen === key)?.title ?? ''
    confirmInstall(key, title, anchor, (withDlc) => act('install', key, { withDlc }))
  }

  /** A Play from a card that stopped at a cloud save conflict; the game page asks, in its hero. */
  let saveConflict = $state<{ key: string; conflict: api.Conflict } | null>(null)

  async function act(action: Action, key: string, options: api.ActOptions = {}) {
    try {
      const conflict = await api.act(action, key, options)
      if (conflict) {
        saveConflict = { key, conflict }
        openGame(lib.games.find((g) => g.copyKeys.includes(key))?.key ?? key)
      }
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

  async function openSettings(section: string | null = null) {
    // Read fresh each time: the Epic state and cache size change while the panel is closed.
    await loadSettings()
    settingsSection = section
    settingsOpen = settings !== null
  }

  /** What had focus before the Add a game dialog opened. */
  let addOpener: HTMLElement | null = null

  /** Adds a game, or edits the added game `key`. */
  async function openAddGame(key?: string) {
    addOpener = document.activeElement instanceof HTMLElement ? document.activeElement : null
    if (!key) {
      adding = 'new'
      return
    }
    try {
      const found = await api.customGame(key)
      if (found) adding = { key, ...found }
    } catch (e) {
      error = api.message(e)
    }
  }

  function closeAddGame(refocus: boolean) {
    adding = null
    if (refocus && addOpener?.isConnected) addOpener.focus({ preventScroll: true })
    addOpener = null
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
      const saved = await api.loadCache().catch((e) => console.error('library cache', e))
      if (saved) {
        sort = saved.sort
        updatedTo = saved.updatedTo
      }
      savedSort = sort
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
      quietCheck()
    })()

    const poller = setInterval(async () => {
      const p = await api.poll()
      running = p.running
      progress = p.progress
      queue = p.queue
      if (p.changed) await reload()
    }, 2000)
    // Steam installs and uninstalls finish in Steam's own dialogs; pick them up.
    const steam = setInterval(async () => {
      if (await api.refresh(false, false)) await reload()
    }, 60_000)
    const full = setInterval(backgroundRefresh, FULL_REFRESH_EVERY_MS)
    const updates = setInterval(quietCheck, FULL_REFRESH_EVERY_MS)
    document.addEventListener('visibilitychange', returned)
    window.addEventListener('focus', returned)
    return () => {
      clearInterval(poller)
      clearInterval(steam)
      clearInterval(full)
      clearInterval(updates)
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
    saveConflict = null
    tint = undefined
    const back = opener
    opener = null
    // The library is visible again only after the update; a recycled-away card is gone (isConnected).
    await tick()
    if (back?.isConnected) back.focus({ preventScroll: true })
  }

  async function mouseNav(e: MouseEvent) {
    if (e.button !== 3 && e.button !== 4) return
    e.preventDefault()
    // mouseup and auxclick both arrive for one press; only mouseup acts. Settings (and the Steam log in
    // inside it) is modal: the page behind it stays put and focus stays in the dialog.
    if (e.type !== 'mouseup' || settingsOpen || popoverOpen || adding) return
    const step = navStep(e.button, open, lastClosed)
    if (!step) return
    if (step.open === null) {
      closeGame()
      return
    }
    // Reopens even when a filter hides the game now; a game gone from the library is forgotten.
    const key = step.open
    const still = await api.game(key).catch(() => null)
    if (!still) lastClosed = null
    else if (open === null && !settingsOpen) openGame(key)
  }

  // Worked out when the list, the filters or (with "Downloading" on) the progress change; never per frame.
  // Filtering keeps each group's cards together and drops groups left empty.
  const view = $derived.by((): { games: Library['games']; sections: Section[] | null } => {
    const sections = lib.groups.length ? lib.groups : null
    if (filters.length === 0) return { games: lib.games, sections }
    const p = filters.includes('downloading') ? progress : {}
    const keep = (g: Library['games'][number]) => matches(g, filters, p)
    if (!sections) return { games: lib.games.filter(keep), sections }
    const games: Library['games'] = []
    const kept: Section[] = []
    let at = 0
    for (const s of sections) {
      const before = games.length
      for (const g of lib.games.slice(at, at + s.count)) if (keep(g)) games.push(g)
      at += s.count
      if (games.length > before) kept.push({ group: s.group, count: games.length - before })
    }
    // No favorite left: under Title the rest is one list again.
    return { games, sections: kept.length === 1 && kept[0].group === 'allGames' ? null : kept }
  })
  const shown = $derived(view.games)
  /** The hours every card's badge shows while sorting by them: how long to beat or playtime. */
  const stat = $derived(sort === 'longestToBeat' || sort === 'shortestToBeat' ? 'ttb' : sort === 'mostPlayed' ? 'playtime' : null)

  /** Queue changes answer with the queue as it is now, so it shows before the next poll. Resolves to whether it worked. */
  async function changeQueue(f: () => Promise<QueueView>) {
    try {
      queue = await f()
      return true
    } catch (e) {
      error = api.message(e)
      return false
    }
  }
  const cancelDownload = (key: string) => changeQueue(() => api.cancelInstall(key))
  const resumeDownloads = () => changeQueue(api.resumeDownloads)

  const ambientHero = $derived(open !== null && tint !== undefined ? tint : (featured?.hero ?? null))
  const showContinue = $derived(featured !== null && query === '' && (settings?.showBanner ?? true))
  const BANNER_H = 180
  const LABEL_H = 18
  // Grouped, each group has its own label row in the grid instead of the one "Library" label.
  const headerHeight = $derived((showContinue ? BANNER_H + 24 : 0) + (view.sections ? 0 : LABEL_H))
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
    bind:filters
    {collections}
    oncollections={reload}
    bind:sort
    bind:popoverOpen
    attention={lib.epicAttention || lib.steamAttention || lib.gogAttention}
    {progress}
    {queue}
    games={lib.games}
    modal={settingsOpen || adding !== null}
    oncancel={cancelDownload}
    onpause={() => changeQueue(api.pauseDownloads)}
    onresume={resumeDownloads}
    onmove={(key, up) => changeQueue(() => api.moveDownload(key, up))}
    onsettings={() => openSettings()}
    onadd={() => openAddGame()}
    {update}
    appVersion={settings?.appVersion ?? ''}
    running={running !== null}
    {installing}
    {installError}
    oninstall={installUpdate}
  />

  {#if updatedTo}
    {@const notes = installedNotes(updatedTo)}
    <div class="notice">
      <span>Updated to <b>{updatedTo}</b></span>
      {#if notes}
        <button class="glass" bind:this={updatedNotesButton} onclick={() => (updatedNotesOpen = !updatedNotesOpen)}>What's new</button>
        {#if updatedNotesOpen}
          <NotesPopover
            version={updatedTo}
            {notes}
            anchor={updatedNotesButton}
            onclose={(refocus) => {
              updatedNotesOpen = false
              if (refocus) updatedNotesButton?.focus()
            }}
          />
        {/if}
      {/if}
      <button class="close" onclick={() => (updatedTo = null)} aria-label="Dismiss">✕</button>
    </div>
  {/if}
  {#if lib.epicNotice}
    <div class="notice">
      <span>You were logged out of Epic Games. Log in again in Settings.</span>
      <button class="glass" onclick={() => openSettings()}>Settings</button>
      <button class="close" onclick={dismissNotice} aria-label="Dismiss">✕</button>
    </div>
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
      folder={confirming.game.key.startsWith('gog-') ? settings?.gogInstallDir : settings?.epicInstallDir}
      oncancel={() => (confirming = null)}
      onconfirm={(withDlc) => {
        const { go } = confirming!
        confirming = null
        go(withDlc)
      }}
    />
    {/key}
  {/if}

  <div class="content">
    <!-- Hidden, not unmounted, under the game page: the grid keeps its scroll position. -->
    <div class="library" class:away={open !== null}>
    {#if shown.length > 0}
      <Grid
        games={shown}
        sections={view.sections}
        {stat}
        resetKey={`${query}|${store}|${sort}|${installedOnly}|${filters.join()}`}
        {running}
        {progress}
        {queue}
        onopen={openGame}
        onplay={(key) => act('play', key)}
        oninstall={install}
        oncancel={cancelDownload}
        onresume={resumeDownloads}
        onfavorite={setFavorite}
        onmenu={openCardMenu}
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
          {#if !view.sections}
            <h2 class="label">
              {query || store || installedOnly || filters.length ? 'Results' : 'Library'}
              <span>{shown.length}</span>
            </h2>
          {/if}
        {/snippet}
      </Grid>
    {:else if epicEmpty && lib.epic === 'noLegendary'}
      <div class="empty">
        <p>Legendary isn't installed.</p>
        <small>Install it, then refresh.</small>
      </div>
    {:else if epicEmpty && lib.epic === 'loggedOut'}
      <div class="empty">
        <p>Your Epic games show after you log in.</p>
        <button class="pill" disabled={loggingIn} onclick={logIn}>{loggingIn ? 'Logging in…' : 'Log in to Epic'}</button>
      </div>
    {:else if epicEmpty && lib.epic === 'unreachable'}
      <div class="empty">
        <p>Can't reach Epic.</p>
        <small>Your Epic games load once it's reachable.</small>
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
        {queue}
        oncancel={cancelDownload}
        onresume={resumeDownloads}
        {version}
        onback={closeGame}
        onerror={(m) => (error = m)}
        onchanged={reload}
        ontint={(url) => (tint = url)}
        onconfirminstall={confirmInstall}
        onedit={openAddGame}
        conflict={saveConflict}
      />
    {/if}
  </div>
</main>

{#if cardMenu}
  {#key cardMenu}
    <GameMenu
      game={cardMenu.game}
      at={cardMenu}
      label={cardMenu.game.title}
      onclose={closeCardMenu}
      onchanged={reload}
      onerror={(m) => (error = m)}
      onedit={openAddGame}
    />
  {/key}
{/if}

{#if adding}
  {#key adding}
    <AddGame
      edit={adding === 'new' ? null : adding}
      onclose={closeAddGame}
      onsaved={reload}
      onsettings={() => {
        closeAddGame(false)
        openSettings('Extra metadata')
      }}
    />
  {/key}
{/if}

{#if settingsOpen && settings}
  <Settings
    view={settings}
    section={settingsSection}
    {refreshedAt}
    {refreshing}
    {loggingIn}
    {update}
    {updateChecked}
    {installing}
    {installError}
    oncheck={checkUpdate}
    oninstall={installUpdate}
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

  .notice b {
    font-weight: 600;
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
