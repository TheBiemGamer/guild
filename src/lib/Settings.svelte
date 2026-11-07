<script lang="ts">
  import { onMount, untrack } from 'svelte'
  import * as api from './api'
  import type { SettingsView } from './api'
  import { sizeLabel } from './format'
  import StoreIcon from './StoreIcon.svelte'
  import SteamLogin from './SteamLogin.svelte'
  import NotesPopover, { installedNotes } from './NotesPopover.svelte'
  import ChoiceMenu from './ChoiceMenu.svelte'
  import { around, t } from './i18n'

  let {
    view,
    section = null,
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
    /** When the last full refresh finished (ms), or null if none has this session. */
    refreshedAt: number | null
    refreshing: boolean
    loggingIn: boolean
    /** A newer release, from the app's last check. */
    update: api.UpdateInfo | null
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
  const sections = ['general', 'accounts', 'appearance', 'metadata', 'storage', 'about']
  let active = $state(sections[0])
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

  const disconnectSteam = () => change(() => api.steamDisconnect())

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

  function saveGogFolder() {
    if (gogFolder.trim() === (view.gogInstallDir ?? '')) return
    change(async () => {
      try {
        await api.updateSetting('gog_install_dir', gogFolder.trim() || null)
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
        await api.updateSetting('amazon_install_dir', amazonFolder.trim() || null)
      } catch (e) {
        amazonFolder = view.amazonInstallDir ?? ''
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

  async function saveGridKey() {
    if (!gridKey.trim()) return
    if (await change(() => api.setSteamgriddbKey(gridKey))) {
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
      await api.epicImport(importKey, path)
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
      await api.epicEglSync()
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
      gogImportNote = t('settings.imported', { title: await api.gogImport(path) })
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
    await change(() => api.gogLogIn())
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
      const games = await api.amazonImportable()
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
      await api.amazonImport(amazonImportKey, path)
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
    await change(() => api.amazonLogIn())
    amazonLoggingIn = false
  }

  const jobLabel = (kind: 'install' | 'update', job: { percent: number | null }) =>
    job.percent === null ? t(`settings.job.${kind}`) : t(`settings.job.${kind}Percent`, { percent: job.percent })

  const removeSecret = () => change(() => api.setIgdbSecret(null))
  const removeGridKey = () => change(() => api.setSteamgriddbKey(null))
  const logOut = () => change(() => api.epicLogOut())
  const gogLogOut = () => change(() => api.gogLogOut())
  const amazonLogOut = () => change(() => api.amazonLogOut())
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
      case 'noLegendary':
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
      case 'noGogdl':
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
      case 'noNile':
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

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="panel" role="dialog" aria-modal="true" aria-label={t('toolbar.settings')}>
    <nav aria-label={t('settings.sectionsLabel')}>
      <h2>{t('toolbar.settings')}</h2>
      {#each sections as name (name)}
        <button class="item" class:on={active === name} onclick={() => go(name)}>{t(`settings.section.${name}`)}</button>
      {/each}
    </nav>

    <div class="main" bind:this={main} {onscroll}>
      <button class="close" bind:this={closeButton} onclick={onclose} aria-label={t('settings.close')}>✕</button>

      {#if error}
        <p class="error" role="alert">{error}</p>
      {/if}

      <section id={id('general')}>
        <h4>{t('settings.section.general')}</h4>
        <div class="row">
          <div id="startup-label">{t('settings.startup')}</div>
          <button
            class="toggle"
            role="switch"
            aria-checked={view.startWithWindows}
            aria-labelledby="startup-label"
            onclick={() => toggle('start_with_windows', !view.startWithWindows)}
          ></button>
        </div>
        <div class="row">
          <div id="tray-label">{t('settings.tray')}</div>
          <button
            class="toggle"
            role="switch"
            aria-checked={view.closeToTray}
            aria-labelledby="tray-label"
            onclick={() => toggle('close_to_tray', !view.closeToTray)}
          ></button>
        </div>
        <div class="row">
          <div id="notify-label">{t('settings.notify')}</div>
          <button
            class="toggle"
            role="switch"
            aria-checked={view.notifyDownloads}
            aria-labelledby="notify-label"
            onclick={() => toggle('notify_downloads', !view.notifyDownloads)}
          ></button>
        </div>
        <div class="row">
          <div id="language-label">{t('settings.language')}</div>
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
              <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
            </button>
            {#if languageOpen}
              <ChoiceMenu
                bind:value={() => view.language, (v) => change(() => api.updateSetting('language', v))}
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
      </section>

      <section id={id('accounts')}>
        <div class="head">
          <h4>{t('settings.section.accounts')}</h4>
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
        <div class="acct">
          <div class="top">
            <div class="logo steam"><StoreIcon provider="steam" size={24} /></div>
            <div class="who">
              <b>Steam</b>
              {#if steam.problem}<span class="problem"><i class="dot"></i>{steam.problem}</span>
              {:else if steam.name}<span>{steam.name}</span>{/if}
            </div>
            {#if steam.fix}<button class="btn" onclick={() => (connecting = true)}>{steam.fix}</button>{/if}
            {#if steam.logout}<button class="btn danger" disabled={busy} onclick={disconnectSteam}>{t('settings.logOut')}</button>{/if}
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
                {t('settings.options')}
                <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
              </button>
            {/if}
            {#if view.epic === 'noLegendary'}
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
          {#if epicOptions && view.epic !== 'noLegendary'}
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
                    <button class="btn" disabled={managing !== null} onclick={runImport}>{managing === 'import' ? t('settings.importing') : t('settings.import')}</button>
                    <button class="btn" disabled={managing !== null} onclick={() => (importing = null)}>{t('common.cancel')}</button>
                  </div>
                </div>
              {/if}
              {#if importNote}<p class="note" title={importNote}>{importNote}</p>{/if}
              <div class="row">
                <div>{t('settings.eglSync')}<span>{t('settings.eglSyncHint')}</span></div>
                <button class="btn" disabled={managing !== null} onclick={eglSync}>{managing === 'egl' ? t('settings.syncing') : t('settings.sync')}</button>
              </div>
              {#if eglNote}<p class="note" title={eglNote}>{eglNote}</p>{/if}
              {#if legendary?.update}
                <div class="row">
                  <div>Legendary {legendary.version ?? ''}<span>{t('settings.toolAvailable', { version: legendary.pinned })}</span></div>
                  <button class="btn" disabled={legendaryJob !== null} aria-live="polite" onclick={installLegendary}>
                    {legendaryJob ? jobLabel('update', legendaryJob) : t('play.update')}
                  </button>
                </div>
                {#if legendaryError}<p class="note" title={legendaryError}>{legendaryError}</p>{/if}
              {/if}
            </div>
          {/if}
        </div>

        <div class="acct">
          <div class="top">
            <div class="logo gog"><StoreIcon provider="gog" size={24} /></div>
            <div class="who">
              <b>GOG</b>
              {#if gog.problem}<span class="problem"><i class="dot"></i>{gog.problem}</span>
              {:else if gog.name}<span>{gog.name}</span>{/if}
            </div>
            {#if view.gog !== 'noGogdl'}
              <button
                class="btn opt"
                class:open={gogOptions}
                aria-expanded={gogOptions}
                aria-controls="gog-options"
                onclick={() => (gogOptions = !gogOptions)}
              >
                {t('settings.options')}
                <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
              </button>
            {/if}
            {#if view.gog === 'noGogdl'}
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
          {#if gogOptions && view.gog !== 'noGogdl'}
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
              {#if gogImportNote}<p class="note" title={gogImportNote}>{gogImportNote}</p>{/if}
              {#if gogdl?.update}
                <div class="row">
                  <div>gogdl {gogdl.version ?? ''}<span>{t('settings.toolAvailable', { version: gogdl.pinned })}</span></div>
                  <button class="btn" disabled={gogdlJob !== null} aria-live="polite" onclick={installGogdl}>
                    {gogdlJob ? jobLabel('update', gogdlJob) : t('play.update')}
                  </button>
                </div>
                {#if gogdlError}<p class="note" title={gogdlError}>{gogdlError}</p>{/if}
              {/if}
            </div>
          {/if}
        </div>

        <div class="acct">
          <div class="top">
            <div class="logo amazon"><StoreIcon provider="amazon" size={24} /></div>
            <div class="who">
              <b>Amazon Games</b>
              {#if amazon.problem}<span class="problem"><i class="dot"></i>{amazon.problem}</span>
              {:else if amazon.name}<span>{amazon.name}</span>{/if}
            </div>
            {#if view.amazon !== 'noNile'}
              <button
                class="btn opt"
                class:open={amazonOptions}
                aria-expanded={amazonOptions}
                aria-controls="amazon-options"
                onclick={() => (amazonOptions = !amazonOptions)}
              >
                {t('settings.options')}
                <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
              </button>
            {/if}
            {#if view.amazon === 'noNile'}
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
          {#if amazonOptions && view.amazon !== 'noNile'}
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
                    <button class="btn" disabled={amazonImporting} onclick={runAmazonImport}>{amazonImporting ? t('settings.importing') : t('settings.import')}</button>
                    <button class="btn" disabled={amazonImporting} onclick={() => (amazonImport = null)}>{t('common.cancel')}</button>
                  </div>
                </div>
              {/if}
              {#if amazonImportNote}<p class="note" title={amazonImportNote}>{amazonImportNote}</p>{/if}
              {#if nile?.update}
                <div class="row">
                  <div>Nile {nile.version ?? ''}<span>{t('settings.toolAvailable', { version: nile.pinned })}</span></div>
                  <button class="btn" disabled={nileJob !== null} aria-live="polite" onclick={installNile}>
                    {nileJob ? jobLabel('update', nileJob) : t('play.update')}
                  </button>
                </div>
                {#if nileError}<p class="note" title={nileError}>{nileError}</p>{/if}
              {/if}
            </div>
          {/if}
        </div>
      </section>

      <section id={id('appearance')}>
        <h4>{t('settings.section.appearance')}</h4>
        <div class="row">
          <div id="ambient-label">{t('settings.ambient')}</div>
          <button
            class="toggle"
            role="switch"
            aria-checked={view.ambient}
            aria-labelledby="ambient-label"
            onclick={() => toggle('ambient', !view.ambient)}
          ></button>
        </div>
        <div class="row">
          <div id="banner-label">{t('settings.banner')}</div>
          <button
            class="toggle"
            role="switch"
            aria-checked={view.showBanner}
            aria-labelledby="banner-label"
            onclick={() => toggle('show_banner', !view.showBanner)}
          ></button>
        </div>
      </section>

      <section id={id('metadata')}>
        <h4>{t('settings.section.metadata')}</h4>
        <p class="hint">{igdbHint[0]}<button class="link" onclick={() => api.openTwitchConsole()}>{t('settings.igdbLink')}</button>{igdbHint[1]}</p>
        <div class="row stack">
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
        <div class="row stack">
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
              <span class="saved" id="igdb-secret">{t('settings.savedCheck')}</span>
              <button class="btn" onclick={() => (replacing = true)}>{t('settings.replace')}</button>
              <button class="btn danger" disabled={busy} onclick={removeSecret}>{t('menu.remove')}</button>
            {/if}
          </div>
        </div>
        <div class="row">
          <div>
            {t('settings.test')}
            {#if testPassed}<span class="ok">{t('settings.testPassed')}</span>{/if}
          </div>
          <button class="btn" disabled={busy || testing || !view.igdbClientId || !view.igdbSecretSet} onclick={testIgdb}>
            {testing ? t('settings.testing') : t('settings.testButton')}
          </button>
        </div>
        <p class="hint gap">{gridHint[0]}<button class="link" onclick={() => run(api.openSteamgriddb)}>{t('settings.gridLink')}</button>{gridHint[1]}</p>
        <div class="row stack">
          <label for="grid-key">{t('settings.gridKey')}</label>
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
              <span class="saved" id="grid-key">{t('settings.savedCheck')}</span>
              <button class="btn" onclick={() => (gridReplacing = true)}>{t('settings.replace')}</button>
              <button class="btn danger" disabled={busy} onclick={removeGridKey}>{t('menu.remove')}</button>
            {/if}
          </div>
        </div>
      </section>

      <section id={id('storage')}>
        <h4>{t('settings.section.storage')}</h4>
        <div class="row">
          <div>{t('settings.cache')}<span>{sizeLabel(view.cacheBytes)}</span></div>
          <button class="btn" disabled={busy || view.cacheBytes === 0} onclick={clearCache}>{t('settings.clearCache')}</button>
        </div>
      </section>

      <section id={id('about')}>
        <h4>{t('settings.section.about')}</h4>
        <div class="row">
          <div>{t('settings.version', { version: view.appVersion })}{#if versionNote}<span class="quiet">{versionNote}</span>{/if}</div>
          <div class="field">
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
        <div class="row">
          <div>Legendary (Epic Games)</div>
          <span class="value">{view.legendaryVersion ?? t('play.notInstalled')}</span>
        </div>
        <div class="row">
          <div>{t('settings.troubleshooting')}</div>
          <div class="field">
            <button class="btn" onclick={() => run(api.openLogFolder)}>{t('settings.openLog')}</button>
            <button class="btn" onclick={() => run(api.reportProblem)}>{t('settings.report')}</button>
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

  /* The Language menu hangs under its button, right-aligned (ChoiceMenu). */
  .choice {
    position: relative;
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

  .logo.gog {
    background: #86328a;
  }

  .logo.amazon {
    background: #232f3e;
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

  .row > div > span.quiet {
    font-size: 12px;
    color: var(--text-3);
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

  .hint.gap {
    margin-top: 16px;
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
