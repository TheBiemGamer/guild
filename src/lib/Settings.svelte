<script lang="ts">
  import { Dialog, RadioGroup, Switch, Tabs } from 'bits-ui'
  import { onMount, tick, untrack } from 'svelte'
  import * as api from './api'
  import { commands, type PrefixView, type ProtonKind, type ReleaseView, type SettingsView, type UpdateInfo } from './bindings'
  import { protonName, sizeLabel } from './format'
  import StoreIcon from './StoreIcon.svelte'
  import Tip from './Tip.svelte'
  import SteamLogin from './SteamLogin.svelte'
  import NotesPopover, { installedNotes } from './NotesPopover.svelte'
  import ChoiceMenu from './ChoiceMenu.svelte'
  import GamePicker from './GamePicker.svelte'
  import ActionMenu from './ActionMenu.svelte'
  import { around, t } from './i18n'
  import { keepOnBackForward } from './outside'
  import igdbSvg from './icons/igdb.svg?raw'
  // Multicolour, so an img element; `no-inline` keeps it out of data: URLs, which the CSP blocks.
  import gridFile from './icons/steamgriddb.svg?no-inline'
  import guildFile from '../../src-tauri/icons/source/guild.svg?no-inline'

  let {
    view = $bindable(),
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
    /** The section to open at; null is Accounts, and `gamedata` is Accounts scrolled to IGDB and SteamGridDB. */
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

  // Simple Icons ship one path per logo, like StoreIcon reads them.
  const igdbPath = /\sd="([^"]+)"/.exec(igdbSvg)?.[1] ?? ''
  /** Section ids; `settings.section.<id>` names them. */
  const sections = $derived(view.runner ? (['accounts', 'general', 'wine', 'about'] as const) : (['accounts', 'general', 'about'] as const))
  /** 24px stroke icons, one path each: the section list and the row tiles. */
  const icons = {
    accounts: 'M8 8a4 4 0 1 0 8 0a4 4 0 1 0-8 0M4 20c1.5-4 4.5-6 8-6s6.5 2 8 6',
    general: 'M4 7h10M18 7h2M4 17h4M12 17h8M14 7a2 2 0 1 0 4 0a2 2 0 1 0-4 0M8 17a2 2 0 1 0 4 0a2 2 0 1 0-4 0',
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
    wine: 'M8 3h8l-.5 6a3.5 3.5 0 0 1-7 0zM12 12.5V20M8.5 21h7',
    window: 'M4 5h16v14H4zM4 9h16M7 7h.01M10 7h.01',
    layers: 'M12 3l9 5-9 5-9-5zM3 13l9 5 9-5',
  }
  type Icon = keyof typeof icons
  let active = $state<string>(untrack(() => (section === 'gamedata' ? null : section) ?? 'accounts'))
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
  /** The game data fields, folded like the Options panels; SteamGridDB's open when Add a game's link opened Settings here. */
  let igdbOpen = $state(false)
  let gridOpen = $state(untrack(() => section === 'gamedata'))
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
  /** Linux, the Wine section: umu-launcher's download while it runs, like Legendary's. */
  let umuJob = $state<{ percent: number | null } | null>(null)
  let wineError = $state('')
  /** The Install another build panel, the Proton source picked in it, and what asking GitHub for its releases gave. */
  let installOpen = $state(false)
  let releasesKind = $state<ProtonKind>('geProton')
  let releases = $state<ReleaseView[] | null>(null)
  let releasesState = $state<'loading' | 'failed' | 'done'>('done')
  /** One Proton build installs at a time: the release, with its percent once known. */
  let protonJob = $state<{ tag: string; percent: number | null } | null>(null)
  let removingProton = $state<string | null>(null)
  let confirmProton = $state<string | null>(null)
  let prefixes = $state<PrefixView[]>([])
  /** Sizes by prefix copy key and by Proton build name; they arrive after the lists. */
  let prefixSizes = $state<Record<string, number>>({})
  let protonSizes = $state<Record<string, number>>({})
  const prefixTotal = $derived(Object.values(prefixSizes).reduce((a, b) => a + b, 0))
  let confirmPrefix = $state<string | null>(null)
  let confirmPrefixCancel = $state<HTMLButtonElement>()
  /** The prefix whose ⋯ menu is open. */
  let prefixMenu = $state<string | null>(null)
  /** The build games run with when they choose none: the one set as default, else the newest found. */
  const defaultProton = $derived(
    view.runner ? (view.runner.protons.find((p) => p.name === view.runner!.proton)?.name ?? view.runner.protons[0]?.name ?? null) : null,
  )
  /** The Proton for Windows games menu: Newest (naming the build it is now), then each build; '' is Newest. */
  let protonOpen = $state(false)
  const protonChoices = $derived(
    view.runner?.protons.length
      ? [
          { value: '', label: t('settings.protonNewest', { name: protonName(view.runner.protons[0].name) }) },
          ...view.runner.protons.map((p) => ({ value: p.name, label: protonName(p.name) })),
        ]
      : [],
  )
  /** A default that is no longer found counts as Newest, as it does when a game starts. */
  const protonChoice = $derived(view.runner?.protons.some((p) => p.name === view.runner!.proton) ? view.runner!.proton! : '')

  async function installUmu() {
    wineError = ''
    umuJob = { percent: null }
    try {
      await api.installUmu((percent) => (umuJob = { percent }))
    } catch (e) {
      wineError = api.message(e)
    }
    umuJob = null
    await onchange()
  }

  async function loadReleases(kind: ProtonKind) {
    releasesState = 'loading'
    const found = await commands.protonReleases(kind).catch(() => null)
    if (releasesKind !== kind) return
    releases = found
    releasesState = found ? 'done' : 'failed'
  }

  function pickReleases(kind: ProtonKind) {
    releasesKind = kind
    releases = null
    loadReleases(kind)
  }

  async function installProton(kind: ProtonKind, tag: string) {
    wineError = ''
    protonJob = { tag, percent: null }
    try {
      await api.installProton(kind, tag, (percent) => (protonJob = { tag, percent }))
    } catch (e) {
      wineError = api.message(e)
    }
    protonJob = null
    await onchange()
    if (releasesKind === kind) loadReleases(kind)
    loadPrefixes()
  }

  async function removeProton(name: string) {
    confirmProton = null
    removingProton = name
    await change(() => commands.removeProton(name))
    removingProton = null
    loadPrefixes()
  }

  /** The prefixes, then every prefix's and Proton build's size, which take longer. */
  async function loadPrefixes() {
    prefixes = await commands.winePrefixes()
    const sizes = await commands.wineSizes()
    prefixSizes = Object.fromEntries(sizes.prefixes.map((s) => [s.key, s.bytes]))
    protonSizes = Object.fromEntries(sizes.protons.map((s) => [s.key, s.bytes]))
  }

  async function askRemovePrefix(key: string) {
    confirmPrefix = key
    await tick()
    confirmPrefixCancel?.focus()
  }

  async function removePrefix(key: string) {
    confirmPrefix = null
    await change(() => commands.removePrefix(key))
    await loadPrefixes()
  }

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
  /** The hints' words around their links. */
  const igdbHint = $derived(around('settings.igdbHint', 'link'))
  const gridHint = $derived(around('settings.gridHint', 'link'))

  async function steamConnected() {
    connecting = false
    await onchange()
  }

  type SwitchField = { [K in keyof SettingsView]: SettingsView[K] extends boolean ? K : never }[keyof SettingsView]

  /** A switch flips at once and flips back if the save fails. None of them change the library, so nothing reloads. */
  async function toggle(name: string, field: SwitchField, on: boolean) {
    error = null
    view[field] = on
    try {
      await commands.updateSetting(name, String(on))
    } catch (e) {
      view[field] = !on
      error = api.message(e)
    }
  }

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

  /** Each section starts at its top, also when the one before was scrolled down. */
  function show(name: string) {
    active = name
    main.scrollTop = 0
  }

  let open = $state(true)

  /** bits-ui asks before `bind:this` has run, so the close button gets focus after this update. */
  function opened(e: Event) {
    e.preventDefault()
    tick().then(() => {
      closeButton?.focus()
      if (section === 'gamedata') document.getElementById(id('gamedata'))?.scrollIntoView({ block: 'start' })
    })
  }

  onMount(() => {
    if (view.runner) loadPrefixes()
    const clock = setInterval(() => (now = Date.now()), 30_000)
    return () => clearInterval(clock)
  })
</script>

{#snippet icon(name: Icon)}
  <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path d={icons[name]} /></svg>
{/snippet}

{#snippet tile(name: Icon)}
  <span class="tile">{@render icon(name)}</span>
{/snippet}

{#snippet toggleButton(on: boolean, labelId: string, onchange: (on: boolean) => void)}
  <Switch.Root checked={on} onCheckedChange={onchange} aria-labelledby={labelId}>
    {#snippet child({ props })}
      <button {...props} class="toggle"></button>
    {/snippet}
  </Switch.Root>
{/snippet}

{#snippet switchRow(name: Icon, setting: string, field: SwitchField, label: string, hint?: string)}
  <div class="r">
    {@render tile(name)}
    <div class="what">
      <span id="{setting}-label">{label}</span>{#if hint}<small>{hint}</small>{/if}
    </div>
    {@render toggleButton(view[field], `${setting}-label`, (v) => toggle(setting, field, v))}
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

<Dialog.Root bind:open onOpenChange={(o) => o || onclose()}>
  <Dialog.Portal>
    <Dialog.Overlay>
      {#snippet child({ props })}<div {...props} class="backdrop"></div>{/snippet}
    </Dialog.Overlay>
    <Dialog.Content onInteractOutside={keepOnBackForward} onOpenAutoFocus={opened}>
      {#snippet child({ props })}
        <div {...props} class="panel" aria-label={t('toolbar.settings')}>
          <Tabs.Root value={active} onValueChange={show} orientation="vertical">
            {#snippet child({ props })}
              <div {...props} class="tabs">
                <nav>
                  <h2>{t('toolbar.settings')}</h2>
                  <Tabs.List aria-label={t('settings.sectionsLabel')}>
                    {#snippet child({ props })}
                      <div {...props} class="list">
                        {#each sections as name (name)}
                          <Tabs.Trigger value={name}>
                            {#snippet child({ props })}
                              <button {...props} class="item">
                                {@render icon(name)}
                                {t(`settings.section.${name}`)}
                                {#if name === 'accounts' && attention}<i class="dot" aria-hidden="true"></i><span class="visually-hidden"
                                    >{t('settings.needsAttention')}</span
                                  >{/if}
                              </button>
                            {/snippet}
                          </Tabs.Trigger>
                        {/each}
                      </div>
                    {/snippet}
                  </Tabs.List>
                </nav>

                <div class="main" bind:this={main}>
                  <button class="close" bind:this={closeButton} onclick={onclose} aria-label={t('settings.close')}>✕</button>

                  {#if error}
                    <p class="error" role="alert">{error}</p>
                  {/if}

                  <Tabs.Content value={active} tabindex={-1}>
                    {#if active === 'accounts'}
                      <section>
                        <div class="head">
                          <h3>{t('settings.section.accounts')}</h3>
                          {#if refreshing}<span class="ago still">{t('settings.refreshing')}</span>{/if}
                          <span class="ago" class:moving={refreshing}>{ago(refreshedAt, now)}</span>
                          <Tip text={t('settings.refreshLibrary')}>
                            {#snippet children({ props })}
                              <span {...props} class="tip-anchor">
                                <button
                                  class="refresh"
                                  class:spinning={refreshing}
                                  disabled={refreshing}
                                  onclick={onrefresh}
                                  aria-label={t('settings.refreshLibrary')}
                                >
                                  <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7" /></svg>
                                </button>
                              </span>
                            {/snippet}
                          </Tip>
                        </div>
                        <h4>{t('settings.group.stores')}</h4>
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
                                    placeholder={view.defaultInstallDir ?? ''}
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
                                {@render toggleButton(view.cloudSaves, 'saves-label', (v) => toggle('cloud_saves', 'cloudSaves', v))}
                              </div>
                              <div class="row">
                                <div>{t('settings.importInstall')}</div>
                                <button class="btn" disabled={managing !== null || importing !== null} onclick={pickImport}>{t('settings.importButton')}</button
                                >
                              </div>
                              {#if importing}
                                <div class="row stack">
                                  <label for="import-game">{t('settings.whichGame', { path: importing.path })}</label>
                                  <div class="field">
                                    <GamePicker id="import-game" games={importing.games} bind:value={importKey} />
                                    <button class="btn" disabled={managing !== null || importKey === ''} onclick={runImport}
                                      >{managing === 'import' ? t('settings.importing') : t('settings.import')}</button
                                    >
                                    <button class="btn" disabled={managing !== null} onclick={() => (importing = null)}>{t('common.cancel')}</button>
                                  </div>
                                </div>
                              {/if}
                              {#if importNote}<Tip text={importNote} truncated
                                  >{#snippet children({ props })}<p {...props} class="note" role="status">{importNote}</p>{/snippet}</Tip
                                >{/if}
                              <div class="row">
                                <div>{t('settings.eglSync')}<span>{t('settings.eglSyncHint')}</span></div>
                                <button class="btn" disabled={managing !== null} onclick={eglSync}
                                  >{managing === 'egl' ? t('settings.syncing') : t('settings.sync')}</button
                                >
                              </div>
                              {#if eglNote}<Tip text={eglNote} truncated
                                  >{#snippet children({ props })}<p {...props} class="note" role="status">{eglNote}</p>{/snippet}</Tip
                                >{/if}
                              {#if legendary?.update}
                                <div class="row">
                                  <div>Legendary {legendary.version ?? ''}<span>{t('settings.toolAvailable', { version: legendary.pinned })}</span></div>
                                  <button class="btn" disabled={legendaryJob !== null} aria-live="polite" onclick={installLegendary}>
                                    {legendaryJob ? jobLabel('update', legendaryJob) : t('play.update')}
                                  </button>
                                </div>
                                {#if legendaryError}<Tip text={legendaryError} truncated
                                    >{#snippet children({ props })}<p {...props} class="note" role="status">{legendaryError}</p>{/snippet}</Tip
                                  >{/if}
                              {/if}
                            </div>
                          {/if}

                          <div class="r">
                            <div class="logo gog"><StoreIcon provider="gog" size={17} /></div>
                            {@render who('GOG', gog)}
                            {#if view.gog !== 'noTool'}
                              <button
                                class="btn opt"
                                class:open={gogOptions}
                                aria-expanded={gogOptions}
                                aria-controls="gog-options"
                                onclick={() => (gogOptions = !gogOptions)}
                              >
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
                                    placeholder={view.defaultInstallDir ?? ''}
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
                                {@render toggleButton(view.gogCloudSaves, 'gog-saves-label', (v) => toggle('gog_cloud_saves', 'gogCloudSaves', v))}
                              </div>
                              <div class="row">
                                <div>{t('settings.importInstall')}</div>
                                <button class="btn" disabled={gogImporting} onclick={gogImport}
                                  >{gogImporting ? t('settings.importing') : t('settings.importButton')}</button
                                >
                              </div>
                              {#if gogImportNote}<Tip text={gogImportNote} truncated
                                  >{#snippet children({ props })}<p {...props} class="note" role="status">{gogImportNote}</p>{/snippet}</Tip
                                >{/if}
                              {#if gogdl?.update}
                                <div class="row">
                                  <div>gogdl {gogdl.version ?? ''}<span>{t('settings.toolAvailable', { version: gogdl.pinned })}</span></div>
                                  <button class="btn" disabled={gogdlJob !== null} aria-live="polite" onclick={installGogdl}>
                                    {gogdlJob ? jobLabel('update', gogdlJob) : t('play.update')}
                                  </button>
                                </div>
                                {#if gogdlError}<Tip text={gogdlError} truncated
                                    >{#snippet children({ props })}<p {...props} class="note" role="status">{gogdlError}</p>{/snippet}</Tip
                                  >{/if}
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
                              <button class="btn" disabled={amazonLoggingIn} onclick={amazonLogIn}
                                >{amazonLoggingIn ? t('common.loggingIn') : amazon.fix}</button
                              >
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
                                    placeholder={view.defaultInstallDir ?? ''}
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
                                <button class="btn" disabled={amazonImporting || amazonImport !== null} onclick={pickAmazonImport}
                                  >{t('settings.importButton')}</button
                                >
                              </div>
                              {#if amazonImport}
                                <div class="row stack">
                                  <label for="amazon-import-game">{t('settings.whichGame', { path: amazonImport.path })}</label>
                                  <div class="field">
                                    <GamePicker id="amazon-import-game" games={amazonImport.games} bind:value={amazonImportKey} />
                                    <button class="btn" disabled={amazonImporting || amazonImportKey === ''} onclick={runAmazonImport}
                                      >{amazonImporting ? t('settings.importing') : t('settings.import')}</button
                                    >
                                    <button class="btn" disabled={amazonImporting} onclick={() => (amazonImport = null)}>{t('common.cancel')}</button>
                                  </div>
                                </div>
                              {/if}
                              {#if amazonImportNote}<Tip text={amazonImportNote} truncated
                                  >{#snippet children({ props })}<p {...props} class="note" role="status">{amazonImportNote}</p>{/snippet}</Tip
                                >{/if}
                              {#if nile?.update}
                                <div class="row">
                                  <div>Nile {nile.version ?? ''}<span>{t('settings.toolAvailable', { version: nile.pinned })}</span></div>
                                  <button class="btn" disabled={nileJob !== null} aria-live="polite" onclick={installNile}>
                                    {nileJob ? jobLabel('update', nileJob) : t('play.update')}
                                  </button>
                                </div>
                                {#if nileError}<Tip text={nileError} truncated
                                    >{#snippet children({ props })}<p {...props} class="note" role="status">{nileError}</p>{/snippet}</Tip
                                  >{/if}
                              {/if}
                            </div>
                          {/if}
                        </div>
                        <h4 id={id('gamedata')}>{t('settings.group.gameData')}</h4>
                        <div class="group">
                          <div class="r">
                            <div class="logo igdb">
                              <svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true"><path d={igdbPath} fill="currentColor" /></svg>
                            </div>
                            <div class="what">
                              IGDB
                              <small>
                                {t('settings.igdbWhat')} ·
                                {#if igdbSet}<span class="ok">{t('settings.connected')}</span>
                                {:else}{igdbHint[0]}<button class="link" onclick={() => commands.openTwitchConsole()}>{t('settings.igdbLink')}</button
                                  >{igdbHint[1]}{/if}
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
                            <div class="logo grid"><img src={gridFile} width="22" height="22" alt="" /></div>
                            <div class="what">
                              SteamGridDB
                              <small>
                                {t('settings.gridWhat')} ·
                                {#if view.steamgriddbKeySet}<span class="ok">{t('settings.connected')}</span>
                                {:else}{gridHint[0]}<button class="link" onclick={() => run(commands.openSteamgriddb)}>{t('settings.gridLink')}</button
                                  >{gridHint[1]}{/if}
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
                                    {#if gridReplacing}<button class="btn" onclick={() => ((gridReplacing = false), (gridKey = ''))}
                                        >{t('common.cancel')}</button
                                      >{/if}
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
                    {/if}

                    {#if active === 'general'}
                      <section>
                        <h3>{t('settings.section.general')}</h3>
                        <h4>{t('settings.group.startup')}</h4>
                        <div class="group">
                          {@render switchRow(
                            'power',
                            'start_with_windows',
                            'startWithWindows',
                            view.linux ? t('settings.startupLinux') : t('settings.startup'),
                          )}
                          {@render switchRow('tray', 'close_to_tray', 'closeToTray', t('settings.tray'), t('settings.trayHint'))}
                          {@render switchRow('bell', 'notify_downloads', 'notifyDownloads', t('settings.notify'))}
                        </div>
                        <h4>{t('settings.group.look')}</h4>
                        <div class="group">
                          {@render switchRow('palette', 'ambient', 'ambient', t('settings.ambient'))}
                          {@render switchRow('banner', 'show_banner', 'showBanner', t('settings.banner'))}
                          <div class="r">
                            {@render tile('globe')}
                            <div class="what">{t('settings.language')}</div>
                            <div class="choice">
                              <ChoiceMenu
                                bind:value={() => view.language, (v) => change(() => commands.updateSetting('language', v))}
                                options={languages}
                                label={t('settings.language')}
                                bind:open={languageOpen}
                              >
                                {#snippet trigger({ props })}
                                  <button {...props} class="btn opt" class:open={languageOpen} aria-label={t('settings.languageNow', { name: languageName })}>
                                    {languageName}
                                    {@render chevron()}
                                  </button>
                                {/snippet}
                              </ChoiceMenu>
                            </div>
                          </div>
                        </div>
                      </section>
                    {/if}

                    {#if active === 'wine' && view.runner}
                      {@const runner = view.runner}
                      <section>
                        <h3>{t('settings.section.wine')}</h3>
                        <div class="group">
                          {#if !runner.umu}
                            <div class="r">
                              {@render tile('window')}
                              <div class="what">umu-launcher<small class="problem">{t('settings.umuMissing')}</small></div>
                              <button class="btn" disabled={umuJob !== null} aria-live="polite" onclick={installUmu}>
                                {umuJob ? jobLabel('install', umuJob) : t('settings.installTool', { tool: 'umu-launcher' })}
                              </button>
                            </div>
                          {/if}
                          <div class="r">
                            {@render tile('layers')}
                            <div class="what">
                              {t('settings.protonFor')}
                              {#if runner.protons.length}<small>{t('settings.protonForHint')}</small>
                              {:else}<small class="problem">{t('settings.protonMissing')}</small>{/if}
                            </div>
                            {#if runner.protons.length}
                              {@const chosen = protonChoices.find((c) => c.value === protonChoice)?.label ?? ''}
                              <div class="choice">
                                <ChoiceMenu
                                  bind:value={() => protonChoice, (v) => change(() => commands.updateSetting('proton', v || null))}
                                  options={protonChoices}
                                  label={t('settings.protonFor')}
                                  bind:open={protonOpen}
                                >
                                  {#snippet trigger({ props })}
                                    <button {...props} class="btn opt" class:open={protonOpen} aria-label={t('settings.protonNow', { name: chosen })}>
                                      {chosen}
                                      {@render chevron()}
                                    </button>
                                  {/snippet}
                                </ChoiceMenu>
                              </div>
                            {/if}
                          </div>
                        </div>

                        <h4>{t('settings.group.protonBuilds')}</h4>
                        <div class="group even">
                          {#each runner.protons as p (p.name)}
                            <div class="r">
                              <div class="what">
                                {protonName(p.name)}
                                {#if p.name === defaultProton}<span class="tag in-use">{t('settings.inUse')}</span>{/if}
                                {#if !p.removable}<span class="tag">Steam</span>{/if}
                                {#if confirmProton === p.name}<small class="problem">{t('settings.removeProtonAsk')}</small>{/if}
                              </div>
                              {#if protonSizes[p.name] !== undefined}<span class="num">{sizeLabel(protonSizes[p.name])}</span>{/if}
                              {#if confirmProton === p.name}
                                <button class="btn danger" onclick={() => removeProton(p.name)}>{t('menu.remove')}</button>
                                <button class="btn" onclick={() => (confirmProton = null)}>{t('common.cancel')}</button>
                              {:else if p.removable}
                                <button class="btn danger" disabled={removingProton !== null} onclick={() => (confirmProton = p.name)}>
                                  {removingProton === p.name ? t('settings.removing') : t('menu.remove')}
                                </button>
                              {/if}
                            </div>
                          {/each}
                          <div class="r">
                            <div class="what">{t('settings.installAnother')}</div>
                            <button
                              class="btn opt"
                              class:open={installOpen}
                              aria-expanded={installOpen}
                              aria-controls="proton-releases"
                              onclick={() => {
                                installOpen = !installOpen
                                if (installOpen && !releases) loadReleases(releasesKind)
                              }}
                            >
                              {installOpen ? t('settings.hide') : t('settings.show')}
                              {@render chevron()}
                            </button>
                          </div>
                          {#if installOpen}
                            <div class="opts releases" id="proton-releases">
                              <RadioGroup.Root
                                orientation="horizontal"
                                aria-label={t('settings.installAnother')}
                                bind:value={() => releasesKind, (v) => pickReleases(v as ProtonKind)}
                              >
                                {#snippet child({ props })}
                                  <div {...props} class="seg">
                                    {#each [['geProton', 'GE-Proton'], ['protonCachyos', 'Proton-CachyOS']] as const as [kind, name] (kind)}
                                      <RadioGroup.Item value={kind}>
                                        {#snippet child({ props })}<button {...props}>{name}</button>{/snippet}
                                      </RadioGroup.Item>
                                    {/each}
                                  </div>
                                {/snippet}
                              </RadioGroup.Root>
                              {#if releasesState === 'loading'}
                                <p class="note" role="status">{t('settings.releasesLoading')}</p>
                              {:else if releasesState === 'failed'}
                                <p class="note" role="status">{t('settings.releasesFailed')}</p>
                              {:else if releases?.length === 0}
                                <p class="note" role="status">{t('settings.releasesNone')}</p>
                              {:else}
                                {@const kind = releasesKind}
                                {#each releases ?? [] as r (r.tag)}
                                  <div class="row">
                                    <div>{protonName(r.name)}</div>
                                    {#if r.installed}
                                      <small class="ok">{t('play.installed')}</small>
                                    {:else}
                                      <button class="btn" disabled={protonJob !== null} aria-live="polite" onclick={() => installProton(kind, r.tag)}>
                                        {protonJob?.tag === r.tag ? jobLabel('install', protonJob) : t('common.install')}
                                      </button>
                                    {/if}
                                  </div>
                                {/each}
                              {/if}
                            </div>
                          {/if}
                          {#if wineError}<Tip text={wineError} truncated
                              >{#snippet children({ props })}<p {...props} class="note" role="status">{wineError}</p>{/snippet}</Tip
                            >{/if}
                        </div>

                        <h4 class="split">
                          {t('settings.group.prefixes')}
                          {#if prefixes.length}<span>{[prefixes.length, ...(Object.keys(prefixSizes).length ? [sizeLabel(prefixTotal)] : [])].join(' · ')}</span
                            >{/if}
                        </h4>
                        <div class="group even">
                          {#each prefixes as p (p.key)}
                            {@const name = t('settings.prefixActions', { name: p.title ?? p.key })}
                            <div class="r">
                              <div class="what">
                                {p.title ?? p.key}
                                {#if confirmPrefix === p.key}<small class="problem">{t('settings.removePrefixAsk')}</small>
                                {:else if p.title === null}<small>{t('settings.prefixGameGone')}</small>{/if}
                              </div>
                              {#if prefixSizes[p.key] !== undefined}<span class="num">{sizeLabel(prefixSizes[p.key])}</span>{/if}
                              {#if confirmPrefix === p.key}
                                <button class="btn danger" disabled={busy} onclick={() => removePrefix(p.key)}>{t('menu.remove')}</button>
                                <button
                                  class="btn"
                                  bind:this={confirmPrefixCancel}
                                  onclick={async () => {
                                    confirmPrefix = null
                                    await tick()
                                    document.getElementById(`prefix-more-${p.key}`)?.focus()
                                  }}>{t('common.cancel')}</button
                                >
                              {:else}
                                <div class="choice">
                                  <ActionMenu
                                    label={name}
                                    bind:open={() => prefixMenu === p.key, (o) => (prefixMenu = o ? p.key : null)}
                                    actions={[
                                      { label: t('settings.openFolder'), run: () => run(() => commands.openPrefix(p.key)) },
                                      { label: 'winecfg', run: () => run(() => commands.runInPrefix(p.key, 'winecfg')) },
                                      { label: 'winetricks', run: () => run(() => commands.runInPrefix(p.key, 'winetricks')), disabled: !runner.winetricks },
                                      { label: t('settings.removeEllipsis'), run: () => askRemovePrefix(p.key), danger: true, apart: true, movesFocus: true },
                                    ]}
                                  >
                                    {#snippet trigger({ props })}
                                      <button {...props} class="btn more" id="prefix-more-{p.key}" aria-label={name}>⋯</button>
                                    {/snippet}
                                  </ActionMenu>
                                </div>
                              {/if}
                            </div>
                          {:else}
                            <div class="r">
                              <div class="what"><small>{t('settings.noPrefixes')}</small></div>
                            </div>
                          {/each}
                          {#if !runner.winetricks && prefixes.length > 0}<p class="note">{t('settings.noWinetricks')}</p>{/if}
                        </div>
                      </section>
                    {/if}

                    {#if active === 'about'}
                      <section>
                        <h3>{t('settings.section.about')}</h3>
                        <div class="ident">
                          <img src={guildFile} width="64" height="64" alt="" />
                          <div>
                            <div class="name">Guild<span>{view.appVersion}</span></div>
                            <p class="acronym">{t('settings.about.acronym')}</p>
                            <p>{t('settings.about.what')}</p>
                            <p class="links">
                              <button class="link" onclick={() => run(commands.openSource)}>{t('settings.about.source')}</button> · GPL-3.0 ·
                              <button class="link" onclick={() => run(commands.openCredits)}>{t('settings.about.credits')}</button>
                            </p>
                          </div>
                        </div>
                        <h4>{t('settings.group.updates')}</h4>
                        <div class="group">
                          <div class="r">
                            {@render tile('about')}
                            <div class="what">
                              <!-- The header shows the version, so this row says only what the last check found. -->
                              {versionNote ?? t('settings.version', { version: view.appVersion })}
                            </div>
                            {#if notes}
                              <button
                                class="btn"
                                bind:this={notesButton}
                                aria-haspopup="dialog"
                                aria-expanded={notesOpen}
                                onclick={() => (notesOpen = !notesOpen)}
                              >
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
                    {/if}
                  </Tabs.Content>
                </div>
              </div>
            {/snippet}
          </Tabs.Root>
        </div>
      {/snippet}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

{#if notes}
  <NotesPopover version={view.appVersion} {notes} bind:open={notesOpen} anchor={notesButton} />
{/if}

{#if connecting}
  <SteamLogin onclose={() => (connecting = false)} onconnected={steamConnected} />
{/if}

<style>
  /* Menus and popovers opened in here sit at 30 and up, the Steam log in at 40. */
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    background: rgb(8 8 12 / 0.55);
  }

  .panel {
    position: fixed;
    inset: 0;
    z-index: 20;
    margin: auto;
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
    padding: 16px 10px;
    border-right: 1px solid var(--glass-line);
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
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

  .item[data-state='active'] {
    background: rgb(255 255 255 / 0.1);
    color: #fff;
  }

  /* Tabs.Root wraps the list and the content but stays out of the panel's grid. */
  .tabs {
    display: contents;
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

  /* Game data opened from Add a game keeps the top padding above its heading. */
  h4 {
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

  /* A heading with a summary on the right: the prefix count and their total size. */
  h4.split {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }

  h4.split span {
    text-transform: none;
    letter-spacing: 0;
    font-weight: 400;
    font-variant-numeric: tabular-nums;
  }

  /* Where a Proton build comes from, and the one games use when they choose none. */
  .tag {
    display: inline-block;
    margin-left: 6px;
    padding: 0 7px;
    border-radius: 999px;
    border: 1px solid var(--glass-line);
    color: var(--text-3);
    font-size: 11px;
    line-height: 18px;
    vertical-align: 1px;
  }

  .tag.in-use {
    color: var(--accent-2);
    border-color: color-mix(in srgb, var(--accent) 50%, transparent);
  }

  .num {
    color: var(--text-2);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  /* Rows with and without a button are the same height, so the list reads evenly. */
  .even > .r {
    min-height: 60px;
  }

  .group > .note {
    margin: 0;
    padding: 10px 14px;
  }

  .btn.more {
    padding: 8px 12px;
    line-height: 1;
  }

  .opts.releases {
    padding-left: 14px;
  }

  .seg {
    display: inline-flex;
    gap: 2px;
    height: 36px;
    margin: 4px 0;
    padding: 2px;
    box-sizing: border-box;
    border-radius: 10px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
  }

  .seg button {
    padding: 0 14px;
    border-radius: 8px;
    background: none;
    color: var(--text-2);
  }

  .seg button[aria-checked='true'] {
    background: var(--glass-on);
    color: var(--text);
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

  .ident {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 4px;
  }

  .ident img {
    flex: none;
  }

  .ident p {
    margin: 2px 0 0;
    color: var(--text-2);
  }

  .ident .name {
    font-size: 26px;
    font-weight: 700;
    line-height: 1.15;
  }

  .ident .name span {
    margin-left: 6px;
    font-size: 14px;
    font-weight: 400;
    color: var(--text-3);
  }

  .ident .acronym {
    margin: 0;
    font-size: 13px;
    font-style: italic;
    color: var(--text-3);
  }

  .ident .links {
    color: var(--text-3);
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

  .logo.igdb {
    background: #9147ff;
  }

  .logo.grid {
    background: rgb(255 255 255 / 0.06);
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
      border-right: 0;
      border-bottom: 1px solid var(--glass-line);
    }

    .list {
      flex-direction: row;
      flex-wrap: wrap;
    }

    h2 {
      padding-bottom: 4px;
    }
  }
</style>
