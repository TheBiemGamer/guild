<script lang="ts">
  import { onMount, untrack } from 'svelte'
  import * as api from './api'
  import { commands, type SettingsView, type UpdateInfo } from './bindings'
  import { sizeLabel } from './format'
  import StoreIcon from './StoreIcon.svelte'
  import SteamLogin from './SteamLogin.svelte'
  import NotesPopover, { installedNotes } from './NotesPopover.svelte'
  import ChoiceMenu from './ChoiceMenu.svelte'
  import { around, t } from './i18n'

  let {
    view,
    section = null,
    attention,
    refreshedAt,
    refreshing,
    loggingIn,
    update,
    updateChecked,
    installing,
    installError,
    oncheck,
    oninstall,
    onclose,
    onchange,
    onrefresh,
    onlogin,
  }: {
    view: SettingsView
    /** The section to open at; null is the top. */
    section?: string | null
    /** A store needs the owner: the Settings button's dot, repeated on Accounts. */
    attention: boolean
    /** When the last full refresh finished (ms), or null if none has this session. */
    refreshedAt: number | null
    refreshing: boolean
    loggingIn: boolean
    /** A newer release, from the app's last check. */
    update: UpdateInfo | null
    /** A check answered this session, so "Up to date" is known. */
    updateChecked: boolean
    installing: boolean
    installError: string | null
    oncheck: () => Promise<void>
    oninstall: () => void
    onclose: () => void
    /** A setting changed: the app re-reads settings and the library. */
    onchange: () => Promise<void>
    onrefresh: () => void
    onlogin: () => void
  } = $props()

  /** Section ids; `settings.section.<id>` names them. */
  const sections = ['accounts', 'general', 'services', 'about'] as const
  /** 24px stroke icons, one path each: the section list and the row tiles. */
  const icons = {
    accounts: 'M8 8a4 4 0 1 0 8 0a4 4 0 1 0-8 0M4 20c1.5-4 4.5-6 8-6s6.5 2 8 6',
    general: 'M4 7h10M18 7h2M4 17h4M12 17h8M14 7a2 2 0 1 0 4 0a2 2 0 1 0-4 0M8 17a2 2 0 1 0 4 0a2 2 0 1 0-4 0',
    services: 'M9 7V3M15 7V3M7 7h10v4a5 5 0 0 1-10 0zM12 16v5',
    about: 'M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0M12 11v6M12 7.5h.01',
    power: 'M12 3v8M6.3 6.3a8 8 0 1 0 11.4 0',
    tray: 'M4 14h4l1 3h6l1-3h4M4 14l2-9h12l2 9v5H4z',
    bell: 'M6 16v-5a6 6 0 0 1 12 0v5l2 2H4zM10 20a2 2 0 0 0 4 0',
    palette:
      'M12 3a9 9 0 0 0 0 18c1.5 0 2-1 2-2s-1-2 0-3 2-1 4-1a3 3 0 0 0 3-3c0-5-4-9-9-9zM7 10a1 1 0 1 0 2 0a1 1 0 1 0-2 0M11 7a1 1 0 1 0 2 0a1 1 0 1 0-2 0M15 10a1 1 0 1 0 2 0a1 1 0 1 0-2 0',
    banner: 'M5 5h14a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2zM6 19h12',
    globe: 'M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0M3 12h18M12 3c3 3 3 15 0 18M12 3c-3 3-3 15 0 18',
    disk: 'M5 5h14a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2zM7 15h.01M11 15h6',
    file: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8zM14 3v5h5M9 13h6M9 17h6',
  }
  type Icon = keyof typeof icons
  let active = $state<string>(sections[0])
  let main: HTMLElement
  let closeButton: HTMLButtonElement

  let error = $state<string | null>(null)
  // Drafts start from the saved values; after that they are the user's text.
  let folder = $state(untrack(() => view.epicInstallDir ?? ''))
  let gogFolder = $state(untrack(() => view.gogInstallDir ?? ''))
  let amazonFolder = $state(untrack(() => view.amazonInstallDir ?? ''))
  let clientId = $state(untrack(() => view.igdbClientId ?? ''))
  let secret = $state('')
  let replacing = $state(false)
  let gridKey = $state('')
  let gridReplacing = $state(false)
  /** The services' fields, folded like the Options panels; SteamGridDB's open when Add a game's link opened Settings here. */
  let igdbOpen = $state(false)
  let gridOpen = $state(untrack(() => section === 'services'))
  let busy = $state(false)
  let connecting = $state(false)
  /** The Epic, GOG and Amazon cards' Options panels; folded each time Settings opens. */
  let epicOptions = $state(false)
  let gogOptions = $state(false)
  let gogLoggingIn = $state(false)
  let amazonOptions = $state(false)
  let amazonLoggingIn = $state(false)
  let checking = $state(false)
  let checkError = $state<string | null>(null)
  const notes = $derived(installedNotes(view.appVersion))
  let notesOpen = $state(false)
  let notesButton = $state<HTMLButtonElement>()
  const versionNote = $derived(
    checkError ?? installError ?? (update ? t('settings.available', { version: update.version }) : updateChecked ? t('settings.upToDate') : null),
  )

  async function checkNow() {
    checking = true
    checkError = null
    try {
      await oncheck()
    } catch (e) {
      checkError = api.message(e)
    }
    checking = false
  }

  /** Legendary's install or update while it runs, with its percent once known. */
  let legendaryJob = $state<{ percent: number | null } | null>(null)
  let legendaryError = $state('')
  const legendary = $derived(view.tools.find((t) => t.name === 'legendary'))
  /** gogdl's install or update while it runs, like Legendary's. */
  let gogdlJob = $state<{ percent: number | null } | null>(null)
  let gogdlError = $state('')
  const gogdl = $derived(view.tools.find((t) => t.name === 'gogdl'))
  /** Nile's install or update while it runs, like Legendary's. */
  let nileJob = $state<{ percent: number | null } | null>(null)
  let nileError = $state('')
  const nile = $derived(view.tools.find((t) => t.name === 'nile'))
  /** Re-rendered every 30 s so "2 minutes ago" stays honest. */
  let now = $state(Date.now())

  const id = (name: string) => `settings-${name}`
  const igdbSet = $derived(!!view.igdbClientId && view.igdbSecretSet)
  const showSecretField = $derived(!view.igdbSecretSet || replacing)
  const showGridField = $derived(!view.steamgriddbKeySet || gridReplacing)

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

  const disconnectSteam = () => change(() => commands.steamDisconnect())

  /** The Language setting: "system" follows Windows' display language. Language names stay in their own language. */
  const languages = $derived([
    { value: 'system' as const, label: t('settings.languageSystem') },
    { value: 'en' as const, label: 'English' },
    { value: 'nl' as const, label: 'Nederlands' },
  ])
  const languageName = $derived(languages.find((l) => l.value === view.language)?.label ?? '')
  let languageOpen = $state(false)
  let languageButton = $state<HTMLButtonElement>()
  /** The hints' words around their links. */
  const igdbHint = $derived(around('settings.igdbHint', 'link'))
  const gridHint = $derived(around('settings.gridHint', 'link'))

  async function steamConnected() {
    connecting = false
    await onchange()
  }

  const toggle = (name: string, on: boolean) => change(() => commands.updateSetting(name, String(on)))

  function saveFolder() {
    if (folder.trim() === (view.epicInstallDir ?? '')) return
    change(async () => {
      try {
        await commands.updateSetting('epic_install_dir', folder.trim() || null)
      } catch (e) {
        folder = view.epicInstallDir ?? ''
        throw e
      }
    })
  }

  function saveGogFolder() {
    if (gogFolder.trim() === (view.gogInstallDir ?? '')) return
    change(async () => {
      try {
        await commands.updateSetting('gog_install_dir', gogFolder.trim() || null)
      } catch (e) {
        gogFolder = view.gogInstallDir ?? ''
        throw e
      }
    })
  }

  function saveAmazonFolder() {
    if (amazonFolder.trim() === (view.amazonInstallDir ?? '')) return
    change(async () => {
      try {
        await commands.updateSetting('amazon_install_dir', amazonFolder.trim() || null)
      } catch (e) {
        amazonFolder = view.amazonInstallDir ?? ''
        throw e
      }
    })
  }

  function saveClientId() {
    if (clientId.trim() === (view.igdbClientId ?? '')) return
    change(() => commands.updateSetting('igdb_client_id', clientId.trim() || null))
  }

  async function saveSecret() {
    if (!secret.trim()) return
    if (await change(() => commands.setIgdbSecret(secret))) {
      secret = ''
      replacing = false
    }
  }

  async function saveGridKey() {
    if (!gridKey.trim()) return
    if (await change(() => commands.setSteamgriddbKey(gridKey))) {
      gridKey = ''
      gridReplacing = false
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
      await commands.testIgdb()
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
      const games = await commands.epicImportable()
      if (games.length === 0) {
        importNote = t('settings.epicAllInstalled')
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
      await commands.epicImport(importKey, path)
      importNote = t('settings.imported', { title: games.find((g) => g.key === importKey)?.title ?? t('settings.theGame') })
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
      await commands.epicEglSync()
      eglNote = t('settings.eglSynced')
    } catch (e) {
      eglNote = api.message(e)
    }
    managing = null
    await onchange()
  }

  async function installLegendary() {
    legendaryError = ''
    legendaryJob = { percent: null }
    try {
      await api.installTool('legendary', (percent) => (legendaryJob = { percent }))
    } catch (e) {
      legendaryError = api.message(e)
    }
    legendaryJob = null
    await onchange()
  }

  /** GOG's Import: gogdl reads which game the picked folder holds. */
  let gogImporting = $state(false)
  let gogImportNote = $state('')

  async function gogImport() {
    gogImportNote = ''
    try {
      const path = await api.pickFolder()
      if (!path) return
      gogImporting = true
      gogImportNote = t('settings.imported', { title: await commands.gogImport(path) })
    } catch (e) {
      gogImportNote = api.message(e)
    }
    gogImporting = false
    await onchange()
  }

  async function installGogdl() {
    gogdlError = ''
    gogdlJob = { percent: null }
    try {
      await api.installTool('gogdl', (percent) => (gogdlJob = { percent }))
    } catch (e) {
      gogdlError = api.message(e)
    }
    gogdlJob = null
    await onchange()
  }

  async function gogLogIn() {
    gogLoggingIn = true
    await change(() => commands.gogLogIn())
    gogLoggingIn = false
  }

  /** Amazon's Import, like Epic's: the picked folder, then which game it holds. */
  let amazonImport = $state<{ path: string; games: { key: string; title: string }[] } | null>(null)
  let amazonImportKey = $state('')
  let amazonImporting = $state(false)
  let amazonImportNote = $state('')

  async function pickAmazonImport() {
    amazonImportNote = ''
    try {
      const path = await api.pickFolder()
      if (!path) return
      const games = await commands.amazonImportable()
      if (games.length === 0) {
        amazonImportNote = t('settings.amazonAllInstalled')
        return
      }
      amazonImport = { path, games }
      amazonImportKey = games[0].key
    } catch (e) {
      amazonImportNote = api.message(e)
    }
  }

  async function runAmazonImport() {
    const { path, games } = amazonImport!
    amazonImporting = true
    try {
      await commands.amazonImport(amazonImportKey, path)
      amazonImportNote = t('settings.imported', { title: games.find((g) => g.key === amazonImportKey)?.title ?? t('settings.theGame') })
      amazonImport = null
    } catch (e) {
      amazonImportNote = api.message(e)
    }
    amazonImporting = false
    await onchange()
  }

  async function installNile() {
    nileError = ''
    nileJob = { percent: null }
    try {
      await api.installTool('nile', (percent) => (nileJob = { percent }))
    } catch (e) {
      nileError = api.message(e)
    }
    nileJob = null
    await onchange()
  }

  async function amazonLogIn() {
    amazonLoggingIn = true
    await change(() => commands.amazonLogIn())
    amazonLoggingIn = false
  }

  const jobLabel = (kind: 'install' | 'update', job: { percent: number | null }) =>
    job.percent === null ? t(`settings.job.${kind}`) : t(`settings.job.${kind}Percent`, { percent: job.percent })

  const removeSecret = () => change(() => commands.setIgdbSecret(null))
  const removeGridKey = () => change(() => commands.setSteamgriddbKey(null))
  const logOut = () => change(() => commands.epicLogOut())
  const gogLogOut = () => change(() => commands.gogLogOut())
  const amazonLogOut = () => change(() => commands.amazonLogOut())
  const clearCache = () => change(() => api.clearCache())

  function ago(at: number | null, nowMs: number) {
    if (at === null) return t('settings.refreshed.never')
    const mins = Math.floor((nowMs - at) / 60_000)
    if (mins < 1) return t('settings.refreshed.justNow')
    if (mins < 60) return t('settings.refreshed.minutes', { n: mins })
    return t('settings.refreshed.hours', { n: Math.floor(mins / 60) })
  }

  /** What the Steam card says: the account name, or a problem in its place. */
  const steam = $derived.by(() => {
    const account = view.steamLibrary.account
    if (!view.steam) return { name: null, problem: t('settings.steamMissing'), fix: null, logout: !!account }
    if (!account || view.steamLibrary.attention) return { name: null, problem: t('settings.notLoggedIn'), fix: t('common.logIn'), logout: false }
    return { name: account, problem: null, fix: null, logout: true }
  })

  /** What the Epic card says, and which fix it offers. */
  const epic = $derived.by(() => {
    switch (view.epic) {
      case 'loggedIn':
        return { name: view.epicAccount ?? t('settings.loggedIn'), problem: null, fix: null, logout: true }
      case 'loggedOut':
        return { name: null, problem: t('settings.notLoggedIn'), fix: t('common.logIn'), logout: false }
      case 'unreachable':
        return { name: null, problem: t('settings.epicUnreachable'), fix: t('settings.reconnect'), logout: false }
      case 'noTool':
        return legendaryError
          ? { name: null, problem: legendaryError, fix: null, logout: false }
          : { name: t('settings.needLegendary'), problem: null, fix: null, logout: false }
      default:
        return { name: t('steam.checking'), problem: null, fix: null, logout: false }
    }
  })

  /** What the GOG card says, and which fix it offers. */
  const gog = $derived.by(() => {
    switch (view.gog) {
      case 'loggedIn':
        return { name: view.gogAccount ?? t('settings.loggedIn'), problem: null, fix: null, logout: true }
      case 'loggedOut':
        return { name: null, problem: t('settings.notLoggedIn'), fix: t('common.logIn'), logout: false }
      case 'unreachable':
        return { name: null, problem: t('settings.gogUnreachable'), fix: t('settings.reconnect'), logout: false }
      case 'noTool':
        return gogdlError
          ? { name: null, problem: gogdlError, fix: null, logout: false }
          : { name: t('settings.needGogdl'), problem: null, fix: null, logout: false }
      default:
        return { name: t('steam.checking'), problem: null, fix: null, logout: false }
    }
  })

  /** What the Amazon card says, and which fix it offers. */
  const amazon = $derived.by(() => {
    switch (view.amazon) {
      case 'loggedIn':
        return { name: view.amazonAccount ?? t('settings.loggedIn'), problem: null, fix: null, logout: true }
      case 'loggedOut':
        return { name: null, problem: t('settings.notLoggedIn'), fix: t('common.logIn'), logout: false }
      case 'unreachable':
        return { name: null, problem: t('settings.amazonUnreachable'), fix: t('settings.reconnect'), logout: false }
      case 'noTool':
        return nileError
          ? { name: null, problem: nileError, fix: null, logout: false }
          : { name: t('settings.needNile'), problem: null, fix: null, logout: false }
      default:
        return { name: t('steam.checking'), problem: null, fix: null, logout: false }
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
    // A short last section never reaches the top edge; at the bottom it's the one being read.
    if (main.scrollTop + main.clientHeight >= main.scrollHeight - 1) active = sections[sections.length - 1]
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose()
  }

  onMount(() => {
    closeButton.focus()
    if (section) go(section)
    const tick = setInterval(() => (now = Date.now()), 30_000)
    return () => clearInterval(tick)
  })
</script>

<svelte:window {onkeydown} />

{#snippet icon(name: Icon)}
  <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path d={icons[name]} /></svg>
{/snippet}

{#snippet tile(name: Icon)}
  <span class="tile">{@render icon(name)}</span>
{/snippet}

{#snippet switchRow(name: Icon, setting: string, on: boolean, label: string, hint?: string)}
  <div class="r">
    {@render tile(name)}
    <div class="what">
      <span id="{setting}-label">{label}</span>{#if hint}<small>{hint}</small>{/if}
    </div>
    <button class="toggle" role="switch" aria-checked={on} aria-labelledby="{setting}-label" onclick={() => toggle(setting, !on)}></button>
  </div>
{/snippet}

{#snippet chevron()}
  <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
{/snippet}

{#snippet fold(set: boolean, open: boolean, controls: string, onclick: () => void)}
  <button class="btn opt" class:setup={!set} class:open aria-expanded={open} aria-controls={controls} {onclick}>
    {set ? t('settings.edit') : t('settings.setUp')}
    {@render chevron()}
  </button>
{/snippet}

{#snippet saved(id: string)}
  <span class="secret" {id}><span class="dots">••••••••</span><span class="ok">{t('settings.saved')}</span></span>
{/snippet}

{#snippet who(store: string, card: { name: string | null; problem: string | null })}
  <div class="what">
    {store}
    {#if card.problem}<small class="problem">{card.problem}</small>
    {:else if card.name}<small>{card.name}</small>{/if}
  </div>
{/snippet}

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="panel" role="dialog" aria-modal="true" aria-label={t('toolbar.settings')}>
    <nav aria-label={t('settings.sectionsLabel')}>
      <h2>{t('toolbar.settings')}</h2>
      {#each sections as name (name)}
        <button class="item" class:on={active === name} aria-current={active === name ? 'true' : undefined} onclick={() => go(name)}>
          {@render icon(name)}
          {t(`settings.section.${name}`)}
          {#if name === 'accounts' && attention}<i class="dot" aria-hidden="true"></i><span class="visually-hidden">{t('settings.needsAttention')}</span>{/if}
        </button>
      {/each}
    </nav>

    <div class="main" bind:this={main} {onscroll}>
      <button class="close" bind:this={closeButton} onclick={onclose} aria-label={t('settings.close')}>✕</button>

      {#if error}
        <p class="error" role="alert">{error}</p>
      {/if}

      <section id={id('accounts')}>
        <div class="head">
          <h3>{t('settings.section.accounts')}</h3>
          {#if refreshing}<span class="ago still">{t('settings.refreshing')}</span>{/if}
          <span class="ago" class:moving={refreshing}>{ago(refreshedAt, now)}</span>
          <button
            class="refresh"
            class:spinning={refreshing}
            disabled={refreshing}
            onclick={onrefresh}
            aria-label={t('settings.refreshLibrary')}
            title={t('settings.refreshLibrary')}
          >
            <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7" /></svg>
          </button>
        </div>
        <div class="group">
          <div class="r">
            <div class="logo steam"><StoreIcon provider="steam" size={17} /></div>
            {@render who('Steam', steam)}
            {#if steam.fix}<button class="btn" onclick={() => (connecting = true)}>{steam.fix}</button>{/if}
            {#if steam.logout}<button class="btn danger" disabled={busy} onclick={disconnectSteam}>{t('settings.logOut')}</button>{/if}
          </div>

          <div class="r">
            <div class="logo epic"><StoreIcon provider="epic" size={17} /></div>
            {@render who('Epic Games', epic)}
            {#if view.epic !== 'noTool'}
              <button
                class="btn opt"
                class:open={epicOptions}
                aria-expanded={epicOptions}
                aria-controls="epic-options"
                onclick={() => (epicOptions = !epicOptions)}
              >
                {t('settings.options')}
                {@render chevron()}
              </button>
            {/if}
            {#if view.epic === 'noTool'}
              <button class="btn" disabled={legendaryJob !== null} aria-live="polite" onclick={installLegendary}>
                {legendaryJob ? jobLabel('install', legendaryJob) : t('settings.installTool', { tool: 'Legendary' })}
              </button>
            {/if}
            {#if epic.fix}
              <button class="btn" disabled={loggingIn} onclick={onlogin}>{loggingIn ? t('common.loggingIn') : epic.fix}</button>
            {/if}
            {#if epic.logout}
              <button class="btn danger" disabled={busy} onclick={logOut}>{t('settings.logOut')}</button>
            {/if}
          </div>
          {#if epicOptions && view.epic !== 'noTool'}
            <div class="opts" id="epic-options">
              <div class="row">
                <label for="epic-folder">{t('settings.installFolder')}</label>
                <div class="field">
                  <input
                    id="epic-folder"
                    type="text"
                    placeholder={t('settings.defaultFolder')}
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
                      {t('common.reset')}
                    </button>
                  {/if}
                </div>
              </div>
              <div class="row">
                <div id="saves-label">{t('settings.syncSaves')}</div>
                <button
                  class="toggle"
                  role="switch"
                  aria-checked={view.cloudSaves}
                  aria-labelledby="saves-label"
                  onclick={() => toggle('cloud_saves', !view.cloudSaves)}
                ></button>
              </div>
              <div class="row">
                <div>{t('settings.importInstall')}</div>
                <button class="btn" disabled={managing !== null || importing !== null} onclick={pickImport}>{t('settings.importButton')}</button>
              </div>
              {#if importing}
                <div class="row stack">
                  <label for="import-game">{t('settings.whichGame', { path: importing.path })}</label>
                  <div class="field">
                    <select id="import-game" bind:value={importKey}>
                      {#each importing.games as g (g.key)}<option value={g.key}>{g.title}</option>{/each}
                    </select>
                    <button class="btn" disabled={managing !== null} onclick={runImport}
                      >{managing === 'import' ? t('settings.importing') : t('settings.import')}</button
                    >
                    <button class="btn" disabled={managing !== null} onclick={() => (importing = null)}>{t('common.cancel')}</button>
                  </div>
                </div>
              {/if}
              {#if importNote}<p class="note" role="status" title={importNote}>{importNote}</p>{/if}
              <div class="row">
                <div>{t('settings.eglSync')}<span>{t('settings.eglSyncHint')}</span></div>
                <button class="btn" disabled={managing !== null} onclick={eglSync}>{managing === 'egl' ? t('settings.syncing') : t('settings.sync')}</button>
              </div>
              {#if eglNote}<p class="note" role="status" title={eglNote}>{eglNote}</p>{/if}
              {#if legendary?.update}
                <div class="row">
                  <div>Legendary {legendary.version ?? ''}<span>{t('settings.toolAvailable', { version: legendary.pinned })}</span></div>
                  <button class="btn" disabled={legendaryJob !== null} aria-live="polite" onclick={installLegendary}>
                    {legendaryJob ? jobLabel('update', legendaryJob) : t('play.update')}
                  </button>
                </div>
                {#if legendaryError}<p class="note" role="status" title={legendaryError}>{legendaryError}</p>{/if}
              {/if}
            </div>
          {/if}

          <div class="r">
            <div class="logo gog"><StoreIcon provider="gog" size={17} /></div>
            {@render who('GOG', gog)}
            {#if view.gog !== 'noTool'}
              <button class="btn opt" class:open={gogOptions} aria-expanded={gogOptions} aria-controls="gog-options" onclick={() => (gogOptions = !gogOptions)}>
                {t('settings.options')}
                {@render chevron()}
              </button>
            {/if}
            {#if view.gog === 'noTool'}
              <button class="btn" disabled={gogdlJob !== null} aria-live="polite" onclick={installGogdl}>
                {gogdlJob ? jobLabel('install', gogdlJob) : t('settings.installTool', { tool: 'gogdl' })}
              </button>
            {/if}
            {#if gog.fix}
              <button class="btn" disabled={gogLoggingIn} onclick={gogLogIn}>{gogLoggingIn ? t('common.loggingIn') : gog.fix}</button>
            {/if}
            {#if gog.logout}
              <button class="btn danger" disabled={busy} onclick={gogLogOut}>{t('settings.logOut')}</button>
            {/if}
          </div>
          {#if gogOptions && view.gog !== 'noTool'}
            <div class="opts" id="gog-options">
              <div class="row">
                <label for="gog-folder">{t('settings.installFolder')}</label>
                <div class="field">
                  <input
                    id="gog-folder"
                    type="text"
                    placeholder={t('settings.defaultFolder')}
                    spellcheck="false"
                    bind:value={gogFolder}
                    onblur={saveGogFolder}
                    onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
                  />
                  {#if view.gogInstallDir}
                    <button
                      class="btn"
                      onclick={() => {
                        gogFolder = ''
                        saveGogFolder()
                      }}
                    >
                      {t('common.reset')}
                    </button>
                  {/if}
                </div>
              </div>
              <div class="row">
                <div id="gog-saves-label">{t('settings.syncSaves')}</div>
                <button
                  class="toggle"
                  role="switch"
                  aria-checked={view.gogCloudSaves}
                  aria-labelledby="gog-saves-label"
                  onclick={() => toggle('gog_cloud_saves', !view.gogCloudSaves)}
                ></button>
              </div>
              <div class="row">
                <div>{t('settings.importInstall')}</div>
                <button class="btn" disabled={gogImporting} onclick={gogImport}>{gogImporting ? t('settings.importing') : t('settings.importButton')}</button>
              </div>
              {#if gogImportNote}<p class="note" role="status" title={gogImportNote}>{gogImportNote}</p>{/if}
              {#if gogdl?.update}
                <div class="row">
                  <div>gogdl {gogdl.version ?? ''}<span>{t('settings.toolAvailable', { version: gogdl.pinned })}</span></div>
                  <button class="btn" disabled={gogdlJob !== null} aria-live="polite" onclick={installGogdl}>
                    {gogdlJob ? jobLabel('update', gogdlJob) : t('play.update')}
                  </button>
                </div>
                {#if gogdlError}<p class="note" role="status" title={gogdlError}>{gogdlError}</p>{/if}
              {/if}
            </div>
          {/if}

          <div class="r">
            <div class="logo amazon"><StoreIcon provider="amazon" size={17} /></div>
            {@render who('Amazon Games', amazon)}
            {#if view.amazon !== 'noTool'}
              <button
                class="btn opt"
                class:open={amazonOptions}
                aria-expanded={amazonOptions}
                aria-controls="amazon-options"
                onclick={() => (amazonOptions = !amazonOptions)}
              >
                {t('settings.options')}
                {@render chevron()}
              </button>
            {/if}
            {#if view.amazon === 'noTool'}
              <button class="btn" disabled={nileJob !== null} aria-live="polite" onclick={installNile}>
                {nileJob ? jobLabel('install', nileJob) : t('settings.installTool', { tool: 'Nile' })}
              </button>
            {/if}
            {#if amazon.fix}
              <button class="btn" disabled={amazonLoggingIn} onclick={amazonLogIn}>{amazonLoggingIn ? t('common.loggingIn') : amazon.fix}</button>
            {/if}
            {#if amazon.logout}
              <button class="btn danger" disabled={busy} onclick={amazonLogOut}>{t('settings.logOut')}</button>
            {/if}
          </div>
          {#if amazonOptions && view.amazon !== 'noTool'}
            <div class="opts" id="amazon-options">
              <div class="row">
                <label for="amazon-folder">{t('settings.installFolder')}</label>
                <div class="field">
                  <input
                    id="amazon-folder"
                    type="text"
                    placeholder={t('settings.defaultFolder')}
                    spellcheck="false"
                    bind:value={amazonFolder}
                    onblur={saveAmazonFolder}
                    onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
                  />
                  {#if view.amazonInstallDir}
                    <button
                      class="btn"
                      onclick={() => {
                        amazonFolder = ''
                        saveAmazonFolder()
                      }}
                    >
                      {t('common.reset')}
                    </button>
                  {/if}
                </div>
              </div>
              <div class="row">
                <div>{t('settings.importInstall')}</div>
                <button class="btn" disabled={amazonImporting || amazonImport !== null} onclick={pickAmazonImport}>{t('settings.importButton')}</button>
              </div>
              {#if amazonImport}
                <div class="row stack">
                  <label for="amazon-import-game">{t('settings.whichGame', { path: amazonImport.path })}</label>
                  <div class="field">
                    <select id="amazon-import-game" bind:value={amazonImportKey}>
                      {#each amazonImport.games as g (g.key)}<option value={g.key}>{g.title}</option>{/each}
                    </select>
                    <button class="btn" disabled={amazonImporting} onclick={runAmazonImport}
                      >{amazonImporting ? t('settings.importing') : t('settings.import')}</button
                    >
                    <button class="btn" disabled={amazonImporting} onclick={() => (amazonImport = null)}>{t('common.cancel')}</button>
                  </div>
                </div>
              {/if}
              {#if amazonImportNote}<p class="note" role="status" title={amazonImportNote}>{amazonImportNote}</p>{/if}
              {#if nile?.update}
                <div class="row">
                  <div>Nile {nile.version ?? ''}<span>{t('settings.toolAvailable', { version: nile.pinned })}</span></div>
                  <button class="btn" disabled={nileJob !== null} aria-live="polite" onclick={installNile}>
                    {nileJob ? jobLabel('update', nileJob) : t('play.update')}
                  </button>
                </div>
                {#if nileError}<p class="note" role="status" title={nileError}>{nileError}</p>{/if}
              {/if}
            </div>
          {/if}
        </div>
      </section>

      <section id={id('general')}>
        <h3>{t('settings.section.general')}</h3>
        <h4>{t('settings.group.startup')}</h4>
        <div class="group">
          {@render switchRow('power', 'start_with_windows', view.startWithWindows, t('settings.startup'))}
          {@render switchRow('tray', 'close_to_tray', view.closeToTray, t('settings.tray'), t('settings.trayHint'))}
          {@render switchRow('bell', 'notify_downloads', view.notifyDownloads, t('settings.notify'))}
        </div>
        <h4>{t('settings.group.look')}</h4>
        <div class="group">
          {@render switchRow('palette', 'ambient', view.ambient, t('settings.ambient'))}
          {@render switchRow('banner', 'show_banner', view.showBanner, t('settings.banner'))}
          <div class="r">
            {@render tile('globe')}
            <div class="what">{t('settings.language')}</div>
            <div class="choice">
              <button
                class="btn opt"
                class:open={languageOpen}
                bind:this={languageButton}
                aria-haspopup="menu"
                aria-expanded={languageOpen}
                aria-label={t('settings.languageNow', { name: languageName })}
                onclick={() => (languageOpen = !languageOpen)}
              >
                {languageName}
                {@render chevron()}
              </button>
              {#if languageOpen}
                <ChoiceMenu
                  bind:value={() => view.language, (v) => change(() => commands.updateSetting('language', v))}
                  options={languages}
                  label={t('settings.language')}
                  anchor={languageButton}
                  onclose={(refocus) => {
                    languageOpen = false
                    if (refocus) languageButton?.focus()
                  }}
                />
              {/if}
            </div>
          </div>
        </div>
      </section>

      <section id={id('services')}>
        <h3>{t('settings.section.services')}</h3>
        <div class="group">
          <div class="r">
            <span class="tile mark">IG</span>
            <div class="what">
              IGDB
              <small>
                {t('settings.igdbWhat')} ·
                {#if igdbSet}<span class="ok">{t('settings.connected')}</span>
                {:else}{igdbHint[0]}<button class="link" onclick={() => commands.openTwitchConsole()}>{t('settings.igdbLink')}</button>{igdbHint[1]}{/if}
              </small>
            </div>
            {@render fold(igdbSet, igdbOpen, 'igdb-fields', () => (igdbOpen = !igdbOpen))}
          </div>
          {#if igdbOpen}
            <div class="opts creds" id="igdb-fields">
              <div class="row">
                <label for="igdb-id">{t('settings.clientId')}</label>
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
              <div class="row">
                <label for="igdb-secret">{t('settings.clientSecret')}</label>
                <div class="field">
                  {#if showSecretField}
                    <input
                      id="igdb-secret"
                      type="password"
                      autocomplete="off"
                      placeholder={t('settings.pasteSecret')}
                      bind:value={secret}
                      onkeydown={(e) => e.key === 'Enter' && saveSecret()}
                    />
                    <button class="btn" disabled={busy || !secret.trim()} onclick={saveSecret}>{t('common.save')}</button>
                    {#if replacing}<button class="btn" onclick={() => ((replacing = false), (secret = ''))}>{t('common.cancel')}</button>{/if}
                  {:else}
                    {@render saved('igdb-secret')}
                    <button class="btn" onclick={() => (replacing = true)}>{t('settings.replace')}</button>
                    <button class="btn danger" disabled={busy} onclick={removeSecret}>{t('menu.remove')}</button>
                  {/if}
                </div>
              </div>
              <div class="row">
                <span class="lab"></span>
                <div class="field">
                  <button class="btn" disabled={busy || testing || !view.igdbClientId || !view.igdbSecretSet} onclick={testIgdb}>
                    {testing ? t('settings.testing') : t('settings.testButton')}
                  </button>
                  {#if testPassed}<small class="ok">{t('settings.testPassed')}</small>{/if}
                </div>
              </div>
            </div>
          {/if}
          <div class="r">
            <span class="tile mark">SG</span>
            <div class="what">
              SteamGridDB
              <small>
                {t('settings.gridWhat')} ·
                {#if view.steamgriddbKeySet}<span class="ok">{t('settings.connected')}</span>
                {:else}{gridHint[0]}<button class="link" onclick={() => run(commands.openSteamgriddb)}>{t('settings.gridLink')}</button>{gridHint[1]}{/if}
              </small>
            </div>
            {@render fold(view.steamgriddbKeySet, gridOpen, 'grid-fields', () => (gridOpen = !gridOpen))}
          </div>
          {#if gridOpen}
            <div class="opts creds" id="grid-fields">
              <div class="row">
                <label for="grid-key">{t('settings.apiKey')}</label>
                <div class="field">
                  {#if showGridField}
                    <input
                      id="grid-key"
                      type="password"
                      autocomplete="off"
                      placeholder={t('settings.pasteKey')}
                      bind:value={gridKey}
                      onkeydown={(e) => e.key === 'Enter' && saveGridKey()}
                    />
                    <button class="btn" disabled={busy || !gridKey.trim()} onclick={saveGridKey}>{t('common.save')}</button>
                    {#if gridReplacing}<button class="btn" onclick={() => ((gridReplacing = false), (gridKey = ''))}>{t('common.cancel')}</button>{/if}
                  {:else}
                    {@render saved('grid-key')}
                    <button class="btn" onclick={() => (gridReplacing = true)}>{t('settings.replace')}</button>
                    <button class="btn danger" disabled={busy} onclick={removeGridKey}>{t('menu.remove')}</button>
                  {/if}
                </div>
              </div>
            </div>
          {/if}
        </div>
      </section>

      <section id={id('about')}>
        <h3>{t('settings.section.about')}</h3>
        <h4>{t('settings.group.updates')}</h4>
        <div class="group">
          <div class="r">
            {@render tile('about')}
            <div class="what">
              {t('settings.version', { version: view.appVersion })}{#if versionNote}<small>{versionNote}</small>{/if}
            </div>
            {#if notes}
              <button class="btn" bind:this={notesButton} aria-haspopup="dialog" aria-expanded={notesOpen} onclick={() => (notesOpen = !notesOpen)}>
                {t('app.whatsNew')}
              </button>
            {/if}
            {#if update}
              <button class="btn" aria-disabled={installing} onclick={() => installing || oninstall()}>
                {installing ? t('update.downloading') : t('update.restart')}
              </button>
            {:else}
              <button class="btn" disabled={checking} onclick={checkNow}>{checking ? t('steam.checking') : t('settings.checkUpdates')}</button>
            {/if}
          </div>
        </div>
        <h4>{t('settings.group.storage')}</h4>
        <div class="group">
          <div class="r">
            {@render tile('disk')}
            <div class="what">{t('settings.cache')}<small>{sizeLabel(view.cacheBytes)}</small></div>
            <button class="btn" disabled={busy || view.cacheBytes === 0} onclick={clearCache}>{t('settings.clearCache')}</button>
          </div>
        </div>
        <h4>{t('settings.group.troubleshooting')}</h4>
        <div class="group">
          <div class="r">
            {@render tile('file')}
            <div class="what">{t('settings.logFile')}</div>
            <button class="btn" onclick={() => run(commands.openLogFolder)}>{t('settings.openLog')}</button>
            <button class="btn" onclick={() => run(commands.reportProblem)}>{t('settings.report')}</button>
          </div>
        </div>
      </section>
    </div>
  </div>
</div>

{#if notesOpen && notes}
  <!-- Outside the panel: its backdrop blur would trap a fixed popover inside it. -->
  <NotesPopover
    version={view.appVersion}
    {notes}
    anchor={notesButton}
    onclose={(refocus) => {
      notesOpen = false
      if (refocus) notesButton?.focus()
    }}
  />
{/if}

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
    border-radius: var(--radius-modal);
    background: rgb(22 22 32 / 0.82);
    backdrop-filter: blur(24px);
    border: 1px solid rgb(255 255 255 / 0.1);
    box-shadow: var(--shadow-modal);
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 16px 10px;
    border-right: 1px solid var(--glass-line);
  }

  h2 {
    margin: 0;
    padding: 0 10px 12px;
    font-size: 16px;
    font-weight: 700;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 8px;
    background: transparent;
    color: var(--text-2);
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

  section + section {
    margin-top: 32px;
  }

  /* A section opened from the list keeps the top padding above its title. */
  section {
    scroll-margin-top: 20px;
  }

  h3 {
    margin: 0 0 14px;
    font-size: 20px;
    font-weight: 700;
  }

  h4 {
    margin: 18px 0 8px;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-3);
  }

  h3 + h4 {
    margin-top: 0;
  }

  .head {
    display: flex;
    align-items: center;
    margin-bottom: 14px;
  }

  .head h3 {
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
    color: var(--error-text);
  }

  /* One rounded group per heading; its rows (and an open Options panel) are split by thin lines. No overflow
     clip: the Language menu hangs out of its group. */
  .group {
    border-radius: 12px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
  }

  .group > * + * {
    border-top: 1px solid var(--glass-line);
  }

  .r {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    align-items: center;
    gap: 8px 12px;
    padding: 11px 14px;
  }

  .tile,
  .logo {
    flex: none;
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border-radius: 8px;
  }

  .tile {
    background: rgb(255 255 255 / 0.06);
    color: var(--text-2);
  }

  .item svg,
  .tile svg {
    stroke-width: 1.8;
  }

  /* Long account names break anywhere, but a label keeps room for a word: at the narrowest, buttons wrap
     to a second line, right-aligned. */
  .what {
    flex: 1 1 6em;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .what small {
    display: block;
    margin-top: 1px;
    color: var(--text-3);
    font-size: 12px;
  }

  .what small.problem {
    color: var(--warn);
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

  /* "Set up" on a service that isn't: the standard button, not the quiet one. */
  .btn.opt.setup {
    background: rgb(255 255 255 / 0.1);
    border-color: rgb(255 255 255 / 0.14);
    color: var(--text);
  }

  .tile.mark {
    font-size: 11px;
    font-weight: 700;
  }

  /* The Language menu hangs under its button, right-aligned (ChoiceMenu). */
  .choice {
    position: relative;
  }

  /* Indented to line up with the store name (14px padding + 30px logo + 12px). */
  .opts {
    padding: 4px 14px 8px 56px;
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

  /* A service's fields: the label at a fixed width on the left, its control filling the rest. */
  .creds .row {
    justify-content: flex-start;
    flex-wrap: wrap;
    gap: 8px 12px;
  }

  .creds .row > label,
  .creds .lab {
    flex: none;
    width: 100px;
  }

  /* Wraps under the label only when narrower than this; the secret's dots give way first. */
  .opts.creds .row > .field {
    flex: 1 1 280px;
    min-width: 0;
  }

  /* Beats `.row > div > span`, which stacks a row's subtitle. */
  .creds .field > .secret {
    flex: 1;
    min-width: 0;
    height: 36px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    border-radius: 10px;
    border: 1px solid var(--glass-line);
    background: var(--glass);
    color: var(--text-3);
  }

  .secret .dots {
    min-width: 0;
    overflow: hidden;
  }

  .secret .ok {
    margin-left: auto;
    font-size: 12px;
  }

  .logo {
    color: #fff;
  }

  .logo.steam {
    background: #1b2838;
  }

  .logo.epic {
    background: #2a2a2a;
  }

  .logo.gog {
    background: #86328a;
  }

  .logo.amazon {
    background: #232f3e;
  }

  /* Accounts in the section list: a store needs the owner. */
  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    margin-left: auto;
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

  .btn[aria-disabled='true'] {
    cursor: default;
    opacity: 0.6;
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

  .ok {
    color: var(--success);
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
