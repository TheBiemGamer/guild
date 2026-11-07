<script lang="ts">
  import { onMount, untrack } from 'svelte'
  import * as api from './api'
  import type { SettingsView } from './api'
  import { sizeLabel } from './format'
  import StoreIcon from './StoreIcon.svelte'
  import SteamLogin from './SteamLogin.svelte'

  let { view, refreshedAt, refreshing, loggingIn, onclose, onchange, onrefresh, onlogin }: {
    view: SettingsView
    /** When the last full refresh finished (ms), or null if none has this session. */
    refreshedAt: number | null
    refreshing: boolean
    loggingIn: boolean
    onclose: () => void
    /** A setting changed: the app re-reads settings and the library. */
    onchange: () => Promise<void>
    onrefresh: () => void
    onlogin: () => void
  } = $props()

  const sections = ['Accounts', 'Appearance', 'Extra metadata', 'Storage', 'About']
  let active = $state(sections[0])
  let main: HTMLElement
  let closeButton: HTMLButtonElement

  let error = $state<string | null>(null)
  // Drafts start from the saved values; after that they are the user's text.
  let folder = $state(untrack(() => view.epicInstallDir ?? ''))
  let clientId = $state(untrack(() => view.igdbClientId ?? ''))
  let secret = $state('')
  let replacing = $state(false)
  let busy = $state(false)
  let connecting = $state(false)
  /** The Epic card's Options panel; folded each time Settings opens. */
  let epicOptions = $state(false)
  /** Re-rendered every 30 s so "2 minutes ago" stays honest. */
  let now = $state(Date.now())

  const id = (name: string) => `settings-${name.replace(' ', '-').toLowerCase()}`
  const showSecretField = $derived(!view.igdbSecretSet || replacing)

  /** Runs a change, shows its error, then refreshes the app's copy. Returns whether it worked. */
  async function change(work: () => Promise<unknown>) {
    error = null
    busy = true
    let ok = true
    try {
      await work()
    } catch (e) {
      error = api.message(e)
      ok = false
    }
    busy = false
    await onchange()
    return ok
  }

  const disconnectSteam = () => change(() => api.steamDisconnect())

  async function steamConnected() {
    connecting = false
    await onchange()
  }

  const toggle = (name: string, on: boolean) => change(() => api.updateSetting(name, String(on)))

  function saveFolder() {
    if (folder.trim() === (view.epicInstallDir ?? '')) return
    change(async () => {
      try {
        await api.updateSetting('epic_install_dir', folder.trim() || null)
      } catch (e) {
        folder = view.epicInstallDir ?? ''
        throw e
      }
    })
  }

  function saveClientId() {
    if (clientId.trim() === (view.igdbClientId ?? '')) return
    change(() => api.updateSetting('igdb_client_id', clientId.trim() || null))
  }

  async function saveSecret() {
    if (!secret.trim()) return
    if (await change(() => api.setIgdbSecret(secret))) {
      secret = ''
      replacing = false
    }
  }

  /** Runs an action that changes nothing here, showing its error. */
  async function run(work: () => Promise<unknown>) {
    error = null
    try {
      await work()
    } catch (e) {
      error = api.message(e)
    }
  }

  let testing = $state(false)
  let testPassed = $state(false)

  async function testIgdb() {
    error = null
    testPassed = false
    testing = true
    try {
      await api.testIgdb()
      testPassed = true
    } catch (e) {
      error = api.message(e)
    }
    testing = false
  }

  /** Import and EGL sync: which one runs, the picked folder and game, and each row's last result. */
  let managing = $state<'import' | 'egl' | null>(null)
  let importing = $state<{ path: string; games: { key: string; title: string }[] } | null>(null)
  let importKey = $state('')
  let importNote = $state('')
  let eglNote = $state('')

  async function pickImport() {
    importNote = ''
    try {
      const path = await api.pickFolder()
      if (!path) return
      const games = await api.epicImportable()
      if (games.length === 0) {
        importNote = 'Every owned Epic game is already installed.'
        return
      }
      importing = { path, games }
      importKey = games[0].key
    } catch (e) {
      importNote = api.message(e)
    }
  }

  async function runImport() {
    const { path, games } = importing!
    managing = 'import'
    try {
      await api.epicImport(importKey, path)
      importNote = `Imported ${games.find((g) => g.key === importKey)?.title ?? 'the game'}.`
      importing = null
    } catch (e) {
      importNote = api.message(e)
    }
    managing = null
    await onchange()
  }

  async function eglSync() {
    eglNote = ''
    managing = 'egl'
    try {
      await api.epicEglSync()
      eglNote = 'Synced with the Epic Games Launcher.'
    } catch (e) {
      eglNote = api.message(e)
    }
    managing = null
    await onchange()
  }

  const removeSecret = () => change(() => api.setIgdbSecret(null))
  const logOut = () => change(() => api.epicLogOut())
  const clearCache = () => change(() => api.clearCache())

  function ago(at: number | null, nowMs: number) {
    if (at === null) return 'Not refreshed yet'
    const mins = Math.floor((nowMs - at) / 60_000)
    if (mins < 1) return 'Refreshed just now'
    if (mins < 60) return `Refreshed ${mins} min ago`
    return `Refreshed ${Math.floor(mins / 60)} h ago`
  }

  /** What the Steam card says: the account name, or a problem in its place. */
  const steam = $derived.by(() => {
    const account = view.steamLibrary.account
    if (!view.steam) return { name: null, problem: "Steam isn't installed", fix: null, logout: !!account }
    if (!account || view.steamLibrary.attention) return { name: null, problem: 'Not logged in', fix: 'Log in', logout: false }
    return { name: account, problem: null, fix: null, logout: true }
  })

  /** What the Epic card says, and which fix it offers. */
  const epic = $derived.by(() => {
    switch (view.epic) {
      case 'loggedIn':
        return { name: view.epicAccount ?? 'Logged in', problem: null, fix: null, logout: true }
      case 'loggedOut':
        return { name: null, problem: 'Not logged in', fix: 'Log in', logout: false }
      case 'unreachable':
        return { name: null, problem: "Can't reach Epic", fix: 'Reconnect', logout: false }
      case 'noLegendary':
        return { name: null, problem: "Legendary isn't installed", fix: null, logout: false }
      default:
        return { name: 'Checking…', problem: null, fix: null, logout: false }
    }
  })

  function go(name: string) {
    document.getElementById(id(name))?.scrollIntoView({ block: 'start' })
    active = name
  }

  function onscroll() {
    // The last section whose top has passed the top edge of the scroller.
    const top = main.getBoundingClientRect().top + 24
    for (const name of sections) {
      const el = document.getElementById(id(name))
      if (el && el.getBoundingClientRect().top <= top) active = name
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose()
  }

  onMount(() => {
    closeButton.focus()
    const tick = setInterval(() => (now = Date.now()), 30_000)
    return () => clearInterval(tick)
  })
</script>

<svelte:window {onkeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="panel" role="dialog" aria-modal="true" aria-label="Settings">
    <nav aria-label="Settings sections">
      <h2>Settings</h2>
      {#each sections as name (name)}
        <button class="item" class:on={active === name} onclick={() => go(name)}>{name}</button>
      {/each}
    </nav>

    <div class="main" bind:this={main} {onscroll}>
      <button class="close" bind:this={closeButton} onclick={onclose} aria-label="Close settings">✕</button>

      {#if error}
        <p class="error" role="alert">{error}</p>
      {/if}

      <section id={id('Accounts')}>
        <div class="head">
          <h4>Accounts</h4>
          {#if refreshing}<span class="ago still">Refreshing…</span>{/if}
          <span class="ago" class:moving={refreshing}>{ago(refreshedAt, now)}</span>
          <button
            class="refresh"
            class:spinning={refreshing}
            disabled={refreshing}
            onclick={onrefresh}
            aria-label="Refresh library"
            title="Refresh library"
          >
            <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7" /></svg>
          </button>
        </div>
        <div class="acct">
          <div class="top">
            <div class="logo steam"><StoreIcon provider="steam" size={24} /></div>
            <div class="who">
              <b>Steam</b>
              {#if steam.problem}<span class="problem"><i class="dot"></i>{steam.problem}</span>
              {:else if steam.name}<span>{steam.name}</span>{/if}
            </div>
            {#if steam.fix}<button class="btn" onclick={() => (connecting = true)}>{steam.fix}</button>{/if}
            {#if steam.logout}<button class="btn danger" disabled={busy} onclick={disconnectSteam}>Log out</button>{/if}
          </div>
        </div>

        <div class="acct">
          <div class="top">
            <div class="logo epic"><StoreIcon provider="epic" size={24} /></div>
            <div class="who">
              <b>Epic Games</b>
              {#if epic.problem}<span class="problem"><i class="dot"></i>{epic.problem}</span>
              {:else if epic.name}<span>{epic.name}</span>{/if}
            </div>
            {#if view.epic !== 'noLegendary'}
              <button
                class="btn opt"
                class:open={epicOptions}
                aria-expanded={epicOptions}
                aria-controls="epic-options"
                onclick={() => (epicOptions = !epicOptions)}
              >
                Options
                <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
              </button>
            {/if}
            {#if epic.fix}
              <button class="btn" disabled={loggingIn} onclick={onlogin}>{loggingIn ? 'Logging in…' : epic.fix}</button>
            {/if}
            {#if epic.logout}
              <button class="btn danger" disabled={busy} onclick={logOut}>Log out</button>
            {/if}
          </div>
          {#if epicOptions && view.epic !== 'noLegendary'}
            <div class="opts" id="epic-options">
              <div class="row">
                <label for="epic-folder">Install folder</label>
                <div class="field">
                  <input
                    id="epic-folder"
                    type="text"
                    placeholder="Default (~/Games)"
                    spellcheck="false"
                    bind:value={folder}
                    onblur={saveFolder}
                    onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
                  />
                  {#if view.epicInstallDir}
                    <button
                      class="btn"
                      onclick={() => {
                        folder = ''
                        saveFolder()
                      }}
                    >
                      Reset
                    </button>
                  {/if}
                </div>
              </div>
              <div class="row">
                <div id="saves-label">Sync cloud saves</div>
                <button
                  class="toggle"
                  role="switch"
                  aria-checked={view.cloudSaves}
                  aria-labelledby="saves-label"
                  onclick={() => toggle('cloud_saves', !view.cloudSaves)}
                ></button>
              </div>
              <div class="row">
                <div>Import an existing install</div>
                <button class="btn" disabled={managing !== null || importing !== null} onclick={pickImport}>Import…</button>
              </div>
              {#if importing}
                <div class="row stack">
                  <label for="import-game">Which game is in {importing.path}?</label>
                  <div class="field">
                    <select id="import-game" bind:value={importKey}>
                      {#each importing.games as g (g.key)}<option value={g.key}>{g.title}</option>{/each}
                    </select>
                    <button class="btn" disabled={managing !== null} onclick={runImport}>{managing === 'import' ? 'Importing…' : 'Import'}</button>
                    <button class="btn" disabled={managing !== null} onclick={() => (importing = null)}>Cancel</button>
                  </div>
                </div>
              {/if}
              {#if importNote}<p class="note" title={importNote}>{importNote}</p>{/if}
              <div class="row">
                <div>Sync with Epic Games Launcher<span>Adds the games it installed.</span></div>
                <button class="btn" disabled={managing !== null} onclick={eglSync}>{managing === 'egl' ? 'Syncing…' : 'Sync'}</button>
              </div>
              {#if eglNote}<p class="note" title={eglNote}>{eglNote}</p>{/if}
            </div>
          {/if}
        </div>
      </section>

      <section id={id('Appearance')}>
        <h4>Appearance</h4>
        <div class="row">
          <div id="ambient-label">
            Ambient color from your last-played game
          </div>
          <button
            class="toggle"
            role="switch"
            aria-checked={view.ambient}
            aria-labelledby="ambient-label"
            onclick={() => toggle('ambient', !view.ambient)}
          ></button>
        </div>
        <div class="row">
          <div id="banner-label">Show the "Continue playing" banner</div>
          <button
            class="toggle"
            role="switch"
            aria-checked={view.showBanner}
            aria-labelledby="banner-label"
            onclick={() => toggle('show_banner', !view.showBanner)}
          ></button>
        </div>
      </section>

      <section id={id('Extra metadata')}>
        <h4>Extra metadata</h4>
        <p class="hint">
          IGDB fills how long to beat and extra details. It needs a free Twitch application:
          <button class="link" onclick={() => api.openTwitchConsole()}>open the Twitch developer console</button>.
        </p>
        <div class="row stack">
          <label for="igdb-id">Client ID</label>
          <div class="field">
            <input
              id="igdb-id"
              type="text"
              spellcheck="false"
              autocomplete="off"
              bind:value={clientId}
              onblur={saveClientId}
              onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
            />
          </div>
        </div>
        <div class="row stack">
          <label for="igdb-secret">Client Secret</label>
          <div class="field">
            {#if showSecretField}
              <input
                id="igdb-secret"
                type="password"
                autocomplete="off"
                placeholder="Paste the secret"
                bind:value={secret}
                onkeydown={(e) => e.key === 'Enter' && saveSecret()}
              />
              <button class="btn" disabled={busy || !secret.trim()} onclick={saveSecret}>Save</button>
              {#if replacing}<button class="btn" onclick={() => ((replacing = false), (secret = ''))}>Cancel</button>{/if}
            {:else}
              <span class="saved" id="igdb-secret">Saved ✓</span>
              <button class="btn" onclick={() => (replacing = true)}>Replace</button>
              <button class="btn danger" disabled={busy} onclick={removeSecret}>Remove</button>
            {/if}
          </div>
        </div>
        <div class="row">
          <div>
            Test the credentials
            {#if testPassed}<span class="ok">Twitch and IGDB accepted them.</span>{/if}
          </div>
          <button class="btn" disabled={busy || testing || !view.igdbClientId || !view.igdbSecretSet} onclick={testIgdb}>
            {testing ? 'Testing…' : 'Test'}
          </button>
        </div>
      </section>

      <section id={id('Storage')}>
        <h4>Storage</h4>
        <div class="row">
          <div>Cached covers and art<span>{sizeLabel(view.cacheBytes)}</span></div>
          <button class="btn" disabled={busy || view.cacheBytes === 0} onclick={clearCache}>Clear cache</button>
        </div>
      </section>

      <section id={id('About')}>
        <h4>About</h4>
        <div class="row"><div>App version</div><span class="value">{view.appVersion}</span></div>
        <div class="row">
          <div>Legendary (Epic Games)</div>
          <span class="value">{view.legendaryVersion ?? 'Not installed'}</span>
        </div>
        <div class="row">
          <div>Troubleshooting</div>
          <div class="field">
            <button class="btn" onclick={() => run(api.openLogFolder)}>Open log folder</button>
            <button class="btn" onclick={() => run(api.reportProblem)}>Report a problem</button>
          </div>
        </div>
      </section>
    </div>
  </div>
</div>

{#if connecting}
  <SteamLogin onclose={() => (connecting = false)} onconnected={steamConnected} />
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    display: grid;
    place-items: center;
    background: rgb(8 8 12 / 0.55);
  }

  .panel {
    width: min(760px, calc(100% - 48px));
    height: calc(100% - 48px);
    display: grid;
    grid-template-columns: 190px 1fr;
    overflow: hidden;
    border-radius: 18px;
    background: rgb(22 22 32 / 0.82);
    backdrop-filter: blur(24px);
    border: 1px solid rgb(255 255 255 / 0.1);
    box-shadow: 0 30px 80px rgb(0 0 0 / 0.6);
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 18px 12px;
    border-right: 1px solid var(--glass-line);
  }

  h2 {
    margin: 0;
    padding: 0 10px 12px;
    font-size: 16px;
    font-weight: 700;
  }

  .item {
    padding: 9px 12px;
    border-radius: 10px;
    background: transparent;
    color: rgb(255 255 255 / 0.78);
    text-align: left;
  }

  .item:hover:not(:disabled) {
    background: var(--glass-hover);
  }

  .item.on {
    background: rgb(255 255 255 / 0.1);
    color: #fff;
  }

  .main {
    position: relative;
    overflow-y: auto;
    padding: 20px 26px 40px;
  }

  .close {
    position: sticky;
    top: 0;
    float: right;
    z-index: 1;
    width: 32px;
    height: 32px;
    padding: 0;
    border-radius: 10px;
    background: rgb(255 255 255 / 0.08);
  }

  h4 {
    margin: 26px 0 10px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-2);
  }

  .head {
    display: flex;
    align-items: center;
    margin: 4px 0 10px;
  }

  .head h4 {
    flex: 1;
    margin: 0;
  }

  .ago {
    margin-right: 8px;
    color: var(--text-3);
    font-size: 12px;
  }

  .ago.still {
    display: none;
  }

  /* No spin under reduced motion (app.css stops it), so the text says it instead. */
  @media (prefers-reduced-motion: reduce) {
    .ago.still {
      display: inline;
    }

    .ago.moving {
      display: none;
    }
  }

  .refresh {
    flex: none;
    width: 30px;
    height: 30px;
    padding: 0;
    display: grid;
    place-items: center;
    border-radius: 999px;
    background: var(--glass-on);
    border: 1px solid var(--glass-on-line);
  }

  .refresh.spinning svg {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .error {
    margin: 0 0 12px;
    padding: 8px 12px;
    border-radius: 10px;
    border: 1px solid rgb(239 68 68 / 0.35);
    color: #fecaca;
  }

  .acct {
    margin-bottom: 10px;
    border-radius: 12px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
    overflow: hidden;
  }

  .top {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 14px;
  }

  .btn.opt {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    border-color: var(--glass-line);
    color: var(--text-2);
  }

  .btn.opt:hover:not(:disabled) {
    background: var(--glass-hover);
  }

  .btn.opt svg {
    stroke-width: 2.4;
    transition: transform 150ms ease-out;
  }

  .btn.opt.open svg {
    transform: rotate(180deg);
  }

  /* Indented to line up with the store name (14px padding + 40px logo + 12px). */
  .opts {
    padding: 4px 14px 8px 66px;
    border-top: 1px solid var(--glass-line);
  }

  .opts .row {
    padding: 8px 0;
    border-bottom-color: var(--glass-line);
  }

  .opts > :last-child {
    border-bottom: 0;
  }

  .opts .row:not(.stack) > .field {
    flex: 0 1 260px;
  }

  .logo {
    flex: none;
    width: 40px;
    height: 40px;
    display: grid;
    place-items: center;
    border-radius: 12px;
    color: #fff;
  }

  .logo.steam {
    background: #1b2838;
  }

  .logo.epic {
    background: #2a2a2a;
  }

  .who {
    flex: 1;
    min-width: 0;
  }

  .who b {
    display: block;
    font-size: 14px;
    font-weight: 600;
  }

  .who span {
    display: block;
    color: var(--text-3);
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  .who .problem {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-2);
  }

  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--warn);
  }

  .btn {
    flex: none;
    padding: 8px 14px;
    border-radius: 10px;
    background: rgb(255 255 255 / 0.1);
    border: 1px solid rgb(255 255 255 / 0.14);
  }

  .link {
    padding: 0;
    background: none;
    border: 0;
    color: var(--text);
    text-decoration: underline;
    cursor: pointer;
  }

  .btn.danger {
    color: var(--danger-text);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 0;
    border-bottom: 1px solid rgb(255 255 255 / 0.07);
  }

  .row.stack {
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
  }

  .row > div > span {
    display: block;
    margin-top: 2px;
    color: var(--text-2);
  }

  .value {
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }

  .field {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  input,
  select {
    flex: 1;
    min-width: 0;
    height: 36px;
    padding: 0 12px;
    border-radius: 10px;
    border: 1px solid var(--glass-line);
    background: var(--glass);
    color: var(--text);
  }

  .note {
    margin: 6px 0 0;
    color: var(--text-2);
    font-size: 13px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  input:focus-visible {
    outline-offset: 0;
  }

  input::placeholder {
    color: var(--text-3);
  }

  .saved {
    flex: 1;
    color: #4ade80;
  }

  span.ok {
    color: #4ade80;
  }

  .hint {
    margin: 0;
    color: var(--text-2);
    font-size: 13px;
  }

  .link {
    padding: 0;
    background: transparent;
    color: var(--accent-2);
    text-decoration: underline;
  }

  .link:hover:not(:disabled) {
    background: transparent;
    color: #fff;
  }

  .toggle {
    position: relative;
    flex: none;
    width: 40px;
    height: 22px;
    padding: 0;
    border-radius: 999px;
    background: rgb(255 255 255 / 0.18);
    transition: background 120ms;
  }

  .toggle::after {
    content: '';
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    transition: transform 120ms;
  }

  .toggle:hover:not(:disabled) {
    background: rgb(255 255 255 / 0.26);
  }

  .toggle[aria-checked='true'],
  .toggle[aria-checked='true']:hover:not(:disabled) {
    background: var(--accent);
  }

  .toggle[aria-checked='true']::after {
    transform: translateX(18px);
  }

  .toggle:active:not(:disabled) {
    transform: none;
  }

  @media (max-width: 640px) {
    .panel {
      grid-template-columns: 1fr;
      grid-template-rows: auto 1fr;
    }

    nav {
      flex-direction: row;
      flex-wrap: wrap;
      border-right: 0;
      border-bottom: 1px solid var(--glass-line);
    }

    h2 {
      width: 100%;
      padding-bottom: 4px;
    }
  }
</style>
