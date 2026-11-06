<script lang="ts">
  import { onMount, untrack } from 'svelte'
  import * as api from './api'
  import type { SettingsView } from './api'
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

  const sections = ['Accounts', 'Library', 'Downloads', 'Appearance', 'Extra metadata', 'Storage', 'About']
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

  const removeSecret = () => change(() => api.setIgdbSecret(null))
  const logOut = () => change(() => api.epicLogOut())
  const clearCache = () => change(() => api.clearCache())

  function ago(at: number | null, nowMs: number) {
    if (at === null) return 'Not refreshed since the app started.'
    const mins = Math.floor((nowMs - at) / 60_000)
    if (mins < 1) return 'Last refreshed just now.'
    if (mins < 60) return `Last refreshed ${mins} minute${mins === 1 ? '' : 's'} ago.`
    return `Last refreshed ${Math.floor(mins / 60)} hour${mins < 120 ? '' : 's'} ago.`
  }

  /** What the Steam row says; built like the Epic row. */
  const steam = $derived.by(() => {
    const account = view.steamLibrary.account
    if (!account) return { tone: 'warn', text: 'Not logged in', detail: '', fix: 'Log in', logout: false }
    if (view.steamLibrary.attention)
      return { tone: 'warn', text: `Logged in as ${account}`, detail: 'Steam needs you to log in again', fix: 'Log in', logout: true }
    return { tone: 'ok', text: `Logged in as ${account}`, detail: '', fix: null, logout: true }
  })

  /** What the Epic row says, and which fix it offers. */
  const epic = $derived.by(() => {
    const games = `${view.epicGames} game${view.epicGames === 1 ? '' : 's'}`
    switch (view.epic) {
      case 'loggedIn':
        return { tone: 'ok', text: 'Logged in', detail: games, fix: 'Reconnect', logout: true }
      case 'loggedOut':
        return { tone: 'warn', text: 'Not logged in', detail: view.epicGames ? `${games} saved` : '', fix: 'Log in', logout: false }
      case 'unreachable':
        return { tone: 'warn', text: "Can't reach Epic", detail: `${games} saved`, fix: 'Reconnect', logout: false }
      case 'noLegendary':
        return { tone: 'warn', text: "Legendary isn't installed", detail: 'Install it to list Epic games', fix: null, logout: false }
      default:
        return { tone: 'idle', text: 'Checking…', detail: '', fix: null, logout: false }
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
        <h3>Accounts</h3>
        <div class="acct">
          <div class="logo steam"><StoreIcon provider="steam" size={24} /></div>
          <div class="who">
            <b>Steam</b>
            <span>
              <span class={steam.tone}>● {steam.text}</span>{#if steam.detail}{' · '}{steam.detail}{/if}
            </span>
            <span class="line">
              {#if view.steam}
                Found{#if view.steam.name}{' · '}{view.steam.name}{/if}{' · '}{view.steam.path}
              {:else}
                Not found on this PC · Install Steam to play its games
              {/if}
            </span>
          </div>
          {#if view.steam}<button class="btn" onclick={() => api.openSteam()}>Open Steam</button>{/if}
          {#if steam.fix}<button class="btn" onclick={() => (connecting = true)}>{steam.fix}</button>{/if}
          {#if steam.logout}<button class="btn danger" disabled={busy} onclick={disconnectSteam}>Log out</button>{/if}
        </div>

        <div class="acct">
          <div class="logo epic"><StoreIcon provider="epic" size={24} /></div>
          <div class="who">
            <b>Epic Games</b>
            <span>
              <span class={epic.tone}>● {epic.text}</span>{#if epic.detail}{' · '}{epic.detail}{/if}
            </span>
          </div>
          {#if epic.fix}
            <button class="btn" disabled={loggingIn} onclick={onlogin}>{loggingIn ? 'Logging in…' : epic.fix}</button>
          {/if}
          {#if epic.logout}
            <button class="btn danger" disabled={busy} onclick={logOut}>Log out</button>
          {/if}
        </div>
      </section>

      <section id={id('Library')}>
        <h4>Library</h4>
        <div class="row">
          <div>Refresh library now<span>Rescans Steam and Epic. {ago(refreshedAt, now)}</span></div>
          <button class="btn" disabled={refreshing} onclick={onrefresh}>{refreshing ? 'Refreshing…' : 'Refresh'}</button>
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

      <section id={id('Downloads')}>
        <h4>Downloads</h4>
        <div class="row stack">
          <label for="epic-folder">
            Epic install folder
            <span>Where new Epic games install. Empty uses Legendary's default (~/Games).</span>
          </label>
          <div class="field">
            <input
              id="epic-folder"
              type="text"
              placeholder="Legendary's default"
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
      </section>

      <section id={id('Appearance')}>
        <h4>Appearance</h4>
        <div class="row">
          <div id="ambient-label">
            Ambient color from your last-played game<span>Off uses a neutral dark gradient.</span>
          </div>
          <button
            class="toggle"
            role="switch"
            aria-checked={view.ambient}
            aria-labelledby="ambient-label"
            onclick={() => toggle('ambient', !view.ambient)}
          ></button>
        </div>
      </section>

      <section id={id('Extra metadata')}>
        <h4>Extra metadata</h4>
        <p class="hint">
          IGDB fills how long to beat and extra details for Steam and Epic games. It needs a free Twitch application:
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
            <span class:ok={testPassed}>
              {testPassed ? 'OK: Twitch and IGDB accepted them.' : 'Checks that Twitch accepts them.'}
            </span>
          </div>
          <button class="btn" disabled={busy || testing || !view.igdbClientId || !view.igdbSecretSet} onclick={testIgdb}>
            {testing ? 'Testing…' : 'Test'}
          </button>
        </div>
      </section>

      <section id={id('Storage')}>
        <h4>Storage</h4>
        <div class="row">
          <div>Cached covers and art<span>{view.cacheSize}. They download again when needed.</span></div>
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

  h3 {
    margin: 4px 0 16px;
    font-size: 20px;
  }

  h4 {
    margin: 26px 0 10px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-2);
  }

  .error {
    margin: 0 0 12px;
    padding: 8px 12px;
    border-radius: 10px;
    border: 1px solid rgb(239 68 68 / 0.35);
    color: #fecaca;
  }

  .acct {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 10px;
    padding: 14px 16px;
    border-radius: 14px;
    background: rgb(255 255 255 / 0.05);
    border: 1px solid var(--glass-line);
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
  }

  .who span {
    color: var(--text-2);
    overflow-wrap: anywhere;
  }

  .ok {
    color: #4ade80;
  }

  .warn {
    color: var(--warn);
  }

  .btn {
    flex: none;
    padding: 8px 14px;
    border-radius: 10px;
    background: rgb(255 255 255 / 0.1);
    border: 1px solid rgb(255 255 255 / 0.14);
  }

  .who .line {
    display: block;
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

  .row > div > span,
  .row > label > span {
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

  input {
    flex: 1;
    min-width: 0;
    height: 36px;
    padding: 0 12px;
    border-radius: 10px;
    border: 1px solid var(--glass-line);
    background: var(--glass);
    color: var(--text);
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
