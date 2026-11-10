<script lang="ts" module>
  import { t } from './i18n'

  /** The year in a store date: Steam "28 Nov, 2016", Epic "2019-06-26". */
  export function releaseYear(date: string): string | null {
    return /\b(\d{4})\b/.exec(date)?.[1] ?? null
  }

  // The stores' English month names, for reading their dates. Shown through the language files: toLocaleDateString's
  // short months vary by ICU version ("Sept").
  const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

  /** Month number 1–12 from a name like "Nov" or "November"; null when it isn't one. */
  function monthNumber(name: string): number | null {
    const at = MONTHS.indexOf(name.charAt(0).toUpperCase() + name.slice(1, 3).toLowerCase())
    return at < 0 ? null : at + 1
  }

  const dayMonthYear = (day: number, month: number, year: string) => t('date.dayMonthYear', { day, month: t(`date.month${month}`), year })

  /**
   * A store date as "28 Nov 2016": Epic "2019-06-26", Steam "28 Nov, 2016" or "Nov 28, 2016".
   * Anything not recognised ("Coming soon", "Q1 2025") is shown as given.
   */
  export function releaseDate(date: string): string {
    const iso = /^(\d{4})-(\d{2})-(\d{2})/.exec(date)
    if (iso) {
      const month = Number(iso[2])
      const day = Number(iso[3])
      if (month >= 1 && month <= 12 && day >= 1 && day <= 31) return dayMonthYear(day, month, iso[1])
      return date
    }
    const dmy = /^(\d{1,2}) ([A-Za-z]+)\.?,? (\d{4})$/.exec(date.trim())
    const mdy = /^([A-Za-z]+)\.? (\d{1,2}),? (\d{4})$/.exec(date.trim())
    const parts: string[] = dmy ? [dmy[1], dmy[2], dmy[3]] : mdy ? [mdy[2], mdy[1], mdy[3]] : []
    const [day, name, year] = parts
    const month = name ? monthNumber(name) : null
    return day && year && month ? dayMonthYear(Number(day), month, year) : date
  }
</script>

<script lang="ts">
  import { DropdownMenu, Tabs } from 'bits-ui'
  import { onMount, tick } from 'svelte'
  import * as api from './api'
  import type { ActOptions } from './api'
  import {
    commands,
    type Action,
    type Conflict,
    type Detail,
    type Dlc,
    type GameInfo,
    type LaunchOptions,
    type LinkKind,
    type Progress,
    type QueueView,
    type SavesView,
    type TimeToBeat,
  } from './bindings'
  import PlayButton from './PlayButton.svelte'
  import InstallPanel from './InstallPanel.svelte'
  import GameMenu from './GameMenu.svelte'
  import LinkIcon, { linkNames } from './LinkIcon.svelte'
  import ChoiceMenu from './ChoiceMenu.svelte'
  import StoreIcon, { storeNames } from './StoreIcon.svelte'
  import Tip from './Tip.svelte'
  import { lastPlayedLabel, playtimeLabel, progressLong, sizeLabel, syncedLabel, ttbLabel } from './format'

  let {
    gameKey,
    runningKey,
    settingUp = false,
    progressByKey,
    queue,
    oncancel,
    onresume,
    version,
    onback,
    onerror,
    onchanged,
    ontint,
    onedit,
    conflict = null,
  }: {
    gameKey: string
    /** Copy keys, as `poll` reports them. */
    runningKey: string | null
    /** The running game's first launch on Linux still sets up (`poll`'s `settingUp`). */
    settingUp?: boolean
    progressByKey: Record<string, Progress>
    /** The Epic, GOG and Amazon download queue; this game's job in it decides the busy pill. */
    queue: QueueView
    /** Stops or removes a queued job, by copy key; resolves to whether it worked. */
    oncancel: (key: string) => Promise<boolean>
    onresume: () => void
    /** Bumped whenever the library reloads, so installed state and stats stay current. */
    version: number
    onback: () => void
    onerror: (message: string) => void
    onchanged: () => void
    /** This game's art for the window tint; null when it has none. */
    ontint: (url: string | null) => void
    /** Opens the Edit dialog for a game the owner added. */
    onedit: (key: string) => void
    /** A Play from a card stopped at this cloud save conflict (`key`: the copy played); the hero asks. */
    conflict?: { key: string; conflict: Conflict } | null
  } = $props()

  // undefined: still loading. null: the answer is "none".
  let game = $state<Detail | null | undefined>(undefined)
  let art = $state<{ hero: string | null }>({ hero: null })
  let heroLoaded = $state(false)
  let info = $state<GameInfo | null | undefined>(undefined)
  /** Local screenshot URLs; undefined while downloading, when their slots stay reserved. */
  let shots = $state<string[] | undefined>(undefined)
  let ttb = $state<TimeToBeat | null | undefined>(undefined)
  let steamDlc = $state<{ id: string; title: string }[]>([])
  /** The Epic or GOG DLC whose uninstall is being confirmed, as `copy|id`. */
  let confirmingDlc = $state<string | null>(null)
  let dlcCancel = $state<HTMLButtonElement>()
  let busy = $state<string | null>(null)
  let confirming = $state(false)
  /** The Epic, GOG or Amazon copy whose install panel is open. */
  let installPanel = $state<string | null>(null)
  let installOpener = $state<HTMLElement | null>(null)
  let menuOpen = $state(false)
  let moreButton = $state<HTMLButtonElement>()
  let backButton = $state<HTMLButtonElement>()
  let confirmCancel = $state<HTMLButtonElement>()
  let actionsEl = $state<HTMLDivElement>()
  type Tab = 'overview' | 'dlc' | 'saves' | 'launch' | 'installation'
  let tab = $state<Tab>('overview')
  /** The chosen copy's Saves tab; null without cloud saves. */
  let saves = $state<SavesView | null>(null)
  let syncing = $state(false)
  /** The chosen installed Epic copy's launch options; null for others. */
  let launch = $state<LaunchOptions | null>(null)
  let argsText = $state('')
  let argsSaved = $state(false)
  /** A Play that stopped at a cloud save conflict, asked in the hero. */
  let asking = $state<{ conflict: Conflict; action: Action; key: string } | null>(null)
  let askCancel = $state<HTMLButtonElement>()

  // The page takes focus on open, so keyboard users start at "← Library".
  onMount(() => backButton?.focus({ preventScroll: true }))

  /** After the actions swap buttons, focus would fall to <body>; put it on what replaced them. */
  async function keepFocus(target: () => HTMLElement | null | undefined) {
    await tick()
    const active = document.activeElement
    if (!active || active === document.body) target()?.focus()
  }

  // Each answer lands only if this is still the game asked about.
  $effect(() => {
    const key = gameKey
    void version
    let alive = true
    commands.game(key).then(
      (g) => alive && (game = g),
      (e) => alive && onerror(api.message(e)),
    )
    return () => {
      alive = false
    }
  })

  $effect(() => {
    const key = gameKey
    let alive = true
    art = { hero: null }
    heroLoaded = false
    info = undefined
    shots = undefined
    ttb = undefined
    steamDlc = []
    confirmingDlc = null
    asking = null
    installPanel = null
    tab = 'overview'
    api.steamDlc(key).then(
      (d) => alive && (steamDlc = d ?? []),
      () => {},
    )
    api.detailArt(key).then((a) => {
      if (!alive) return
      art = a
      if (a.hero) ontint(a.hero)
      // No wide art: the cover still gives the page its colour.
      else api.cover(key).then((c) => alive && ontint(c?.src ?? null))
    })
    api.gameInfo(key).then(
      (i) => {
        if (!alive) return
        info = i
        if (i?.screenshots.length)
          api.screenshots(key, i.screenshots).then(
            (s) => alive && (shots = s),
            () => alive && (shots = []),
          )
      },
      () => alive && (info = null),
    )
    api.timeToBeat(key).then(
      (t) => alive && (ttb = t),
      () => alive && (ttb = null),
    )
    return () => {
      alive = false
    }
  })

  // After the reset above, so a conflict that opened this page survives it.
  $effect(() => {
    if (conflict) asking = { ...conflict, action: 'play' }
  })
  // The question takes focus when it appears, like the Epic uninstall confirm (its Cancel).
  $effect(() => askCancel?.focus())

  // Which copy Play and Install use: the default until the owner picks one. Not stored.
  let chosenKey = $state('')
  let chosenFor = ''
  $effect(() => {
    if (game && game.key !== chosenFor) {
      chosenFor = game.key
      chosenKey = game.chosen
    }
  })
  const copy = $derived(game && (game.copies.find((c) => c.key === chosenKey) ?? game.copies[0]))
  /** Added by the owner: no store, so no store details, links or store actions. */
  const custom = $derived(game?.stores[0] === 'custom')
  // Running state and progress arrive under copy keys.
  const copyKeys = $derived(game ? game.copies.map((c) => c.key) : [gameKey])
  const running = $derived(runningKey !== null && copyKeys.includes(runningKey))
  const progress = $derived(copyKeys.map((k) => progressByKey[k]).find(Boolean))

  async function loadSaves(key: string, alive = () => true) {
    try {
      const s = await commands.saves(key)
      if (alive()) saves = s
    } catch {
      // Quiet: the tab keeps what it showed.
    }
  }

  $effect(() => {
    const key = copy?.cloudSaves ? copy.key : null
    void version
    if (!key) {
      saves = null
      return
    }
    let alive = true
    loadSaves(key, () => alive)
    return () => {
      alive = false
    }
  })

  async function setSaves(choice: boolean | null) {
    const key = copy!.key
    await run(() => commands.setSaves(key, choice))
    await loadSaves(key)
  }

  async function syncNow() {
    const key = copy!.key
    syncing = true
    await run(() => commands.syncSavesNow(key))
    await loadSaves(key)
    syncing = false
  }

  // A string, so a library reload that keeps the copy doesn't reload the options under the cursor.
  const launchKey = $derived(
    copy?.installed &&
      (copy.provider === 'epic' || copy.provider === 'gog' || copy.provider === 'amazon' || copy.provider === 'ea' || copy.provider === 'custom')
      ? copy.key
      : null,
  )
  $effect(() => {
    const key = launchKey
    launch = null
    argsSaved = false
    protons = []
    if (!key) return
    let alive = true
    // Linux only: Windows has no Proton builds, and a native Linux build of a GOG game runs without one.
    commands.protons(key).then(
      (p) => alive && (protons = p),
      () => {},
    )
    commands.launchOptions(key).then(
      (o) => {
        if (!alive) return
        launch = o
        argsText = o?.args ?? ''
      },
      () => {},
    )
    return () => {
      alive = false
    }
  })

  /** Linux: the Proton builds a game may pick, newest first; '' follows Settings. */
  let protons = $state<string[]>([])
  const protonChoices = $derived([{ value: '', label: t('detail.default') }, ...protons.map((p) => ({ value: p, label: p }))])
  const protonLabel = $derived(protonChoices.find((c) => c.value === (launch?.proton ?? ''))?.label ?? t('detail.default'))
  let protonOpen = $state(false)

  /** Saves a change to the launch options; true when it was stored. */
  async function setLaunch(change: Partial<LaunchOptions>) {
    const key = copy!.key
    try {
      const stored = await commands.setLaunchOptions(key, { ...launch!, ...change })
      if (copy?.key === key) launch = stored
      return true
    } catch (e) {
      onerror(api.message(e))
      return false
    }
  }

  let savedTimer: ReturnType<typeof setTimeout> | undefined
  async function saveArgs() {
    if (!launch || argsText === launch.args) return
    if (await setLaunch({ args: argsText })) {
      argsSaved = true
      clearTimeout(savedTimer)
      savedTimer = setTimeout(() => (argsSaved = false), 2000)
    }
  }

  async function chooseExe() {
    const path = await api.pickExe(copy!.installDir ?? '').catch((e) => (onerror(api.message(e)), null))
    if (path) setLaunch({ exe: path })
  }

  const label = (action: Action) => t(`busy.${action}`)
  /** This game's job in the download queue. */
  const job = $derived(api.queueState(queue, copyKeys))
  /** The job Cancel was pressed for, until it leaves the queue. */
  let cancelling = $state<string | null>(null)
  $effect(() => {
    if (cancelling && job?.key !== cancelling) cancelling = null
  })
  // A queued job's state wins over the local busy text (the `act` waiting for it says "Installing…").
  const shown = $derived(
    job
      ? cancelling === job.key
        ? t('detail.cancelling')
        : job.state === 'queued'
          ? t('card.queued')
          : job.state === 'paused'
            ? t('downloads.paused')
            : label(job.action)
      : busy,
  )

  async function act(action: Action, key = copy!.key, options: ActOptions = {}) {
    confirming = false
    menuOpen = false
    busy = label(action)
    keepFocus(() => actionsEl?.querySelector<HTMLButtonElement>('button.pill[aria-disabled]'))
    let ok = true
    try {
      const found = await api.act(action, key, options)
      if (found) asking = { conflict: found, action, key }
    } catch (e) {
      ok = false
      onerror(api.message(e))
    } finally {
      busy = null
      onchanged()
      keepFocus(() => actionsEl?.querySelector<HTMLButtonElement>('button.pill, button.go'))
    }
    return ok
  }

  /** Queued jobs run in Legendary or gogdl, so they can be cancelled. Not Move: stopped halfway, the record and the folder disagree. */
  async function cancel() {
    const key = job!.key
    cancelling = key
    keepFocus(() => actionsEl?.querySelector<HTMLButtonElement>('button.pill[aria-disabled]'))
    if (!(await oncancel(key))) cancelling = null
  }

  /** Epic, GOG and Amazon move into a picked folder; Steam opens its own storage manager. */
  async function move() {
    const key = copy!.key
    if (copy!.provider === 'steam') return act('move', key)
    const dest = await api.pickFolder().catch((e) => (onerror(api.message(e)), null))
    if (dest) act('move', key, { dest })
  }

  // Steam asks in its own dialog; Legendary, gogdl and Nile don't, so the other stores ask here first.
  function uninstall() {
    if (copy?.provider === 'steam') {
      act('uninstall')
      return
    }
    confirming = true
    alsoPrefix = false
    const key = copy!.key
    commands.winePrefixes().then(
      (list) => (hasPrefix = list.some((p) => p.key === key)),
      () => (hasPrefix = false),
    )
    keepFocus(() => confirmCancel)
  }

  /** Linux: whether the copy being uninstalled has a Wine prefix, and whether it goes too (off unless ticked). */
  let hasPrefix = $state(false)
  let alsoPrefix = $state(false)

  async function confirmUninstall() {
    const key = copy!.key
    const prefix = hasPrefix && alsoPrefix
    if ((await act('uninstall')) && prefix) await run(() => commands.removePrefix(key))
  }

  /** Keep cloud downloads it first; Keep local launches as is and uploads afterwards. */
  function keep(side: 'cloud' | 'local') {
    const { action, key } = asking!
    asking = null
    act(action, key, { keep: side })
  }

  function cancelAsk() {
    asking = null
    keepFocus(() => actionsEl?.querySelector<HTMLButtonElement>('button.pill, button.go'))
  }

  function keepUninstalled() {
    confirming = false
    keepFocus(() => moreButton)
  }

  const run = (f: () => Promise<unknown>) => f().catch((e) => onerror(api.message(e)))

  // The chosen copy's own actions; not while it is busy.
  const copyActions = $derived(!shown && !!copy && (copy.installed || !!copy.installDir))
  // Verify, Move and Uninstall: the chosen store copy, installed.
  const manageable = $derived(!!copy?.installed && !custom)

  // Null hides the tile (Epic's unknown playtime, never played).
  const playtime = $derived(game?.playtimeMinutes != null ? playtimeLabel(game.playtimeMinutes) : null)
  const lastPlayed = $derived(game?.lastPlayed != null ? lastPlayedLabel(game.lastPlayed) : null)
  const beatMain = $derived(ttb?.normally != null ? ttbLabel(ttb.normally) : null)
  const beatAll = $derived(ttb?.completely != null ? ttbLabel(ttb.completely) : null)
  const beatMore = $derived(
    ttb
      ? [ttb.hastily !== null && t('detail.hastily', { time: ttbLabel(ttb.hastily) }), beatAll && t('detail.completely', { time: beatAll })].filter(
          (s): s is string => !!s,
        )
      : [],
  )
  const savesError = $derived(saves?.error ? api.message(saves.error) : null)
  const savesLine = $derived(saves ? (savesError ?? syncedLabel(saves.syncedAt)) : null)
  // Decided from `game` alone, never from how long to beat, so a row that appeared never collapses.
  // A Steam game always has its Playtime tile, so its row is never empty; the free slot waits for a time to beat.
  const showTiles = $derived(!!playtime || !!lastPlayed)

  const year = $derived(info?.released ? releaseYear(info.released) : null)
  // The chosen copy's store when it is installed, else the first installed copy's.
  const installedOn = $derived(game && (copy!.installed ? copy! : game.copies.find((c) => c.installed)))
  const meta = $derived(
    game
      ? custom
        ? [t('store.custom'), ...(game.installed ? [] : [t('play.notInstalled')])]
        : [installedOn ? t('detail.installedOn', { store: storeNames[installedOn.provider] }) : t('play.notInstalled'), info?.developer, year].filter(
            (s): s is string => !!s,
          )
      : [],
  )
  // Epic's DLC first; a Steam DLC named like one of them is shown once. Steam installs owned DLC with the game.
  const dlcs = $derived.by((): Dlc[] => {
    if (!game) return []
    const epic = game.dlc
    const steamInstalled = game.copies.some((c) => c.provider === 'steam' && c.installed)
    const steam = steamDlc
      .filter((s) => !epic.some((e) => e.title.toLowerCase() === s.title.toLowerCase()))
      .map((s) => ({ ...s, installed: steamInstalled, copy: null }))
    return [...epic, ...steam]
  })

  function askUninstallDlc(d: Dlc) {
    confirmingDlc = `${d.copy}|${d.id}`
    keepFocus(() => dlcCancel)
  }

  // A tab with nothing for this game is hidden.
  const tabs = $derived.by(() => {
    const ids: Tab[] = ['overview']
    if (dlcs.length) ids.push('dlc')
    if (copy?.cloudSaves) ids.push('saves')
    // An added game's only launch option is its Proton build, so only on Linux.
    if (copy?.installed && (!custom || protons.length)) ids.push('launch')
    if (copy?.installed || copy?.installDir) ids.push('installation')
    return ids.map((id) => ({ id, label: t(`detail.tab.${id}`), count: id === 'dlc' ? dlcs.length : undefined }))
  })
  // A tab that went away (the DLC list emptied, the copy was uninstalled) falls back to Overview.
  const current = $derived(tabs.some((t) => t.id === tab) ? tab : 'overview')

  const showAbout = $derived(!!info && (info.description !== '' || info.screenshots.length > 0))
  // Every store game has a store page, so Details always has something to show.
  const showDetails = $derived(!!game && !custom)
  const storeLinks = $derived(game?.links.filter((k) => k === 'steamStore' || k === 'epicStore') ?? [])
  const infoLinks = $derived(game?.links.filter((k) => k !== 'steamStore' && k !== 'epicStore') ?? [])
</script>

{#snippet copyItems()}
  {#if copy!.installDir}
    <DropdownMenu.Item onSelect={() => run(() => commands.openFolder(copy!.key))}>
      {#snippet child({ props })}
        <button {...props}>{t('detail.openInstallFolder')}</button>
      {/snippet}
    </DropdownMenu.Item>
  {/if}
  {#if copy!.installed && !custom && !running}
    <DropdownMenu.Item onSelect={uninstall}>
      {#snippet child({ props })}
        <button {...props} class="danger">{t('detail.uninstall')}</button>
      {/snippet}
    </DropdownMenu.Item>
  {/if}
{/snippet}

{#snippet chips(label: string, kinds: LinkKind[])}
  {#if kinds.length}
    <dt>{label}</dt>
    <dd class="chips">
      {#each kinds as kind (kind)}
        <button class="chip" onclick={() => run(() => commands.openLink(game!.key, kind))}>
          <LinkIcon {kind} />{linkNames[kind]}
        </button>
      {/each}
    </dd>
  {/if}
{/snippet}

<section class="detail" aria-label={game?.title ?? t('detail.game')}>
  <div class="page">
    <header class="hero">
      <div class="frame">
        {#if art.hero}
          <img src={art.hero} alt="" class:loaded={heroLoaded} onload={() => (heroLoaded = true)} />
        {/if}
      </div>
      <button class="back" bind:this={backButton} onclick={onback}>{t('detail.back')}</button>

      {#if game}
        <div class="bottom">
          <div class="name">
            <h1>{game.title}</h1>
            <div class="meta">
              {#if !custom}{#each game.stores as store}<StoreIcon provider={store} size={14} />{/each}{/if}
              {#each meta as item}<span>{item}</span>{/each}
            </div>
            {#if progress}
              <div class="progress">
                <div
                  class="bar"
                  role="progressbar"
                  aria-label={t('detail.installProgress')}
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={Math.round(progress.fraction * 100)}
                >
                  <span style="width: {progress.fraction * 100}%"></span>
                </div>
                <small
                  >{job?.state === 'paused' ? t('downloads.pausedAt', { percent: `${Math.round(progress.fraction * 100)}%` }) : progressLong(progress)}</small
                >
              </div>
            {/if}
          </div>

          <div class="actions" bind:this={actionsEl}>
            {#if asking}
              <!-- Asks before launching, like the Epic uninstall; never picks a save by itself. -->
              <span class="ask">{asking.conflict === 'localNewer' ? t('detail.localNewer') : t('detail.cloudNewer')}</span>
              <button class="glass round" onclick={() => keep('cloud')}>{t('detail.keepCloud')}</button>
              <button class="glass round" onclick={() => keep('local')}>{t('detail.keepLocal')}</button>
              <button class="glass round" bind:this={askCancel} onclick={cancelAsk}>{t('common.cancel')}</button>
            {:else if confirming}
              <span class="ask">{t('detail.askUninstall', { title: game.title })}</span>
              {#if hasPrefix}
                <label class="also"><input type="checkbox" bind:checked={alsoPrefix} />{t('detail.removePrefix')}</label>
              {/if}
              <button class="glass round danger" onclick={confirmUninstall}>{t('detail.uninstall')}</button>
              <button class="glass round" bind:this={confirmCancel} onclick={keepUninstalled}>{t('common.cancel')}</button>
            {:else}
              <GameMenu
                {game}
                bind:open={menuOpen}
                label={t('detail.moreActions')}
                {onchanged}
                {onerror}
                {onedit}
                onremoved={onback}
                extra={copyActions ? copyItems : undefined}
              >
                {#snippet trigger({ props })}
                  <button {...props} class="glass more" bind:this={moreButton} aria-label={t('detail.moreActions')}>
                    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                      <circle cx="5" cy="12" r="2" /><circle cx="12" cy="12" r="2" /><circle cx="19" cy="12" r="2" />
                    </svg>
                  </button>
                {/snippet}
              </GameMenu>
              <PlayButton
                copies={game.copies}
                chosen={chosenKey}
                {running}
                busy={shown ?? (running && settingUp ? t('busy.settingUp') : null)}
                onchoose={(key) => (chosenKey = key)}
                onplay={() => act('play')}
                onupdate={() => act('update')}
                oninstall={() => {
                  // Steam installs in its own dialog; the other stores' copies ask in the install panel under the pill.
                  if (!/^(epic|gog|amazon|ea)-/.test(copy!.key)) return act('install')
                  installOpener = actionsEl?.querySelector<HTMLElement>('.split, button.pill') ?? null
                  installPanel = installPanel === copy!.key ? null : copy!.key
                }}
              />
              {#if job?.state === 'paused'}
                <button class="glass round" onclick={onresume}>{t('common.resume')}</button>
              {/if}
              {#if job && !cancelling}
                <button class="glass round" onclick={cancel}>{t('common.cancel')}</button>
              {/if}
            {/if}
            {#if installPanel && installPanel === copy?.key && !copy.installed && !job}
              <!-- Over the page, under the pill; a fresh panel per copy, so a late answer never lands on another one. -->
              {#key installPanel}
                <div class="install">
                  <InstallPanel
                    game={{ key: installPanel, title: game.title }}
                    opener={installOpener}
                    oncancel={() => (installPanel = null)}
                    onconfirm={(choice) => {
                      const key = installPanel!
                      installPanel = null
                      act('install', key, { choice })
                    }}
                  />
                </div>
              {/key}
            {/if}
          </div>
        </div>
      {/if}
    </header>

    {#if game === null}
      <p class="gone">{t('detail.gone')}</p>
    {:else if game}
      <!-- Always shown, Overview alone included, so a DLC tab that arrives late doesn't move the page. -->
      <Tabs.Root value={current} onValueChange={(v) => (tab = v as Tab)}>
        <Tabs.List aria-label={t('detail.sections')}>
          {#snippet child({ props })}
            <div {...props} class="tabs">
              {#each tabs as t (t.id)}
                <Tabs.Trigger value={t.id}>
                  {#snippet child({ props })}
                    <button {...props}>
                      {t.label}{#if t.count}<span class="n">{t.count}</span>{/if}
                    </button>
                  {/snippet}
                </Tabs.Trigger>
              {/each}
            </div>
          {/snippet}
        </Tabs.List>

        <Tabs.Content value={current} tabindex={-1}>
          {#if current === 'dlc'}
            <section class="panel" aria-label={t('detail.tab.dlc')}>
              <ul class="dlc">
                {#each dlcs as d (`${d.copy}|${d.id}`)}
                  <li>
                    <span class="dlc-title">{d.title}</span>
                    {#if confirmingDlc === `${d.copy}|${d.id}`}
                      <!-- Asks inline, like the game's own Epic uninstall. -->
                      <span class="dlc-state">{t('detail.dlcAsk')}</span>
                      <button
                        class="chip danger"
                        aria-label={t('detail.uninstallNamed', { title: d.title })}
                        onclick={() => {
                          confirmingDlc = null
                          act('uninstall', d.copy!, { dlc: d.id })
                        }}>{t('detail.uninstall')}</button
                      >
                      <button class="chip" bind:this={dlcCancel} onclick={() => (confirmingDlc = null)}>{t('common.cancel')}</button>
                    {:else}
                      <span class="dlc-state">{d.installed ? t('play.installed') : t('play.notInstalled')}</span>
                      <!-- Legendary and gogdl add a DLC to an installed game only. -->
                      {#if d.copy && !shown && game.copies.find((c) => c.key === d.copy)?.installed}
                        <button
                          class="chip"
                          class:danger={d.installed}
                          aria-label={t(d.installed ? 'detail.uninstallNamed' : 'detail.installNamed', { title: d.title })}
                          onclick={() => (d.installed ? askUninstallDlc(d) : act('install', d.copy!, { dlc: d.id }))}
                        >
                          {d.installed ? t('detail.uninstall') : t('common.install')}
                        </button>
                      {/if}
                    {/if}
                  </li>
                {/each}
              </ul>
            </section>
          {:else if current === 'saves'}
            <!-- The chosen Epic or GOG copy's cloud saves; the store's default is in its Settings card's Options. -->
            <section class="panel rows" aria-label={t('detail.tab.saves')}>
              <div class="row">
                <span class="key" id="saves-cloud">{t('detail.cloudSaves')}</span>
                <div class="chips" role="group" aria-labelledby="saves-cloud">
                  {#each [{ value: null, label: t('detail.default'), title: t('detail.syncSavesHint') }, { value: true, label: t('common.on') }, { value: false, label: t('common.off') }] as option (option.value)}
                    <Tip text={option.title ?? ''}>
                      {#snippet children({ props })}
                        <button {...props} class="chip" aria-pressed={saves !== null && saves.choice === option.value} onclick={() => setSaves(option.value)}>
                          {option.label}
                        </button>
                      {/snippet}
                    </Tip>
                  {/each}
                </div>
              </div>
              <div class="row">
                <span class="key">{t('detail.saves')}</span>
                <Tip text={savesLine ?? ''} truncated>
                  {#snippet children({ props })}
                    <span {...props} class="value" class:failed={!!saves?.error}>{savesLine ?? ' '}</span>
                  {/snippet}
                </Tip>
                <Tip text={t('detail.syncNowHint')}>
                  {#snippet children({ props })}
                    <span {...props} class="tip-anchor">
                      <button class="chip" disabled={!!shown || running || syncing} onclick={syncNow}
                        >{syncing ? t('detail.syncing') : t('detail.syncNow')}</button
                      >
                    </span>
                  {/snippet}
                </Tip>
              </div>
            </section>
          {:else if current === 'launch'}
            <!-- The chosen copy's own options; Epic, GOG and Amazon apply them on Play. GOG and Amazon take only the arguments and executable. -->
            <section class="panel rows" aria-label={t('detail.tab.launch')}>
              {#if copy!.provider === 'steam'}
                <div class="row">
                  <div class="what">{t('detail.steamLaunch')}</div>
                  <button class="chip" onclick={() => run(() => commands.openSteamProperties(copy!.key))}>{t('detail.openInSteam')}</button>
                </div>
              {:else if launch}
                {#if copy!.provider !== 'custom'}
                  <div class="row">
                    <label class="what" for="launch-args">{t('detail.args')}<small>{t('detail.argsHint')}</small></label>
                    {#if argsSaved}<span class="saved-note" role="status">{t('detail.saved')}</span>{/if}
                    <input
                      id="launch-args"
                      class="args"
                      type="text"
                      spellcheck="false"
                      autocomplete="off"
                      bind:value={argsText}
                      onblur={saveArgs}
                      onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
                    />
                  </div>
                  {#if copy!.provider === 'epic'}
                    <div class="row">
                      <div class="what" id="launch-offline">{t('detail.offline')}<small>{t('detail.offlineHint')}</small></div>
                      <button
                        class="chip"
                        aria-pressed={launch.offline}
                        aria-labelledby="launch-offline"
                        onclick={() => setLaunch({ offline: !launch!.offline })}
                      >
                        {launch.offline ? t('common.on') : t('common.off')}
                      </button>
                    </div>
                    <div class="row">
                      <div class="what" id="launch-skip">{t('detail.skipCheck')}</div>
                      <button
                        class="chip"
                        aria-pressed={launch.skipVersionCheck}
                        aria-labelledby="launch-skip"
                        onclick={() => setLaunch({ skipVersionCheck: !launch!.skipVersionCheck })}
                      >
                        {launch.skipVersionCheck ? t('common.on') : t('common.off')}
                      </button>
                    </div>
                  {/if}
                  <div class="row">
                    <div class="what">{t('detail.altExe')}<small class="path">{launch.exe ?? t('detail.default')}</small></div>
                    <button class="chip" onclick={chooseExe}>{t('detail.choose')}</button>
                    {#if launch.exe}<button class="chip" onclick={() => setLaunch({ exe: null })}>{t('common.reset')}</button>{/if}
                  </div>
                {/if}
                {#if protons.length}
                  <div class="row">
                    <div class="what">{t('settings.proton')}<small>{t('detail.protonHint')}</small></div>
                    <div class="choice">
                      <ChoiceMenu
                        bind:value={() => launch?.proton ?? '', (v) => setLaunch({ proton: v || null })}
                        options={protonChoices}
                        label={t('settings.proton')}
                        bind:open={protonOpen}
                      >
                        {#snippet trigger({ props })}
                          <button {...props} class="chip" aria-label={t('settings.protonNow', { name: protonLabel })}>{protonLabel}</button>
                        {/snippet}
                      </ChoiceMenu>
                    </div>
                  </div>
                {/if}
              {/if}
            </section>
          {:else if current === 'installation'}
            <!-- Acts on the chosen copy, like the ⋯ menu. -->
            <section class="panel rows" aria-label={t('detail.tab.installation')}>
              {#if copy!.installDir}
                <div class="row">
                  <span class="key">{t('detail.folder')}</span>
                  <Tip text={copy!.installDir} truncated>
                    {#snippet children({ props })}<span {...props} class="value">{copy!.installDir}</span>{/snippet}
                  </Tip>
                  <button class="chip" onclick={() => run(() => commands.openFolder(copy!.key))}>{t('detail.open')}</button>
                  {#if manageable}
                    <Tip text={copy!.provider === 'steam' ? t('detail.inSteam') : t('detail.moveHint')}>
                      {#snippet children({ props })}
                        <span {...props} class="tip-anchor">
                          <button class="chip" disabled={!!shown || running} onclick={move}>{t('detail.moveButton')}</button>
                        </span>
                      {/snippet}
                    </Tip>
                  {/if}
                </div>
              {/if}
              {#if copy!.sizeBytes !== null || manageable}
                <div class="row">
                  <span class="key">{t('detail.files')}</span>
                  <span class="value">{copy!.sizeBytes !== null ? sizeLabel(copy!.sizeBytes) : ''}</span>
                  {#if manageable}
                    <Tip text={copy!.provider === 'steam' ? t('detail.inSteam') : t('detail.verifyHint')}>
                      {#snippet children({ props })}
                        <span {...props} class="tip-anchor">
                          <button class="chip" disabled={!!shown || running} onclick={() => act('verify')}>{t('detail.verify')}</button>
                        </span>
                      {/snippet}
                    </Tip>
                  {/if}
                </div>
              {/if}
              {#if copy!.updateAvailable}
                <div class="row">
                  <span class="key">{t('detail.version')}</span>
                  <span class="value">{t('detail.updateAvailable')}</span>
                  <button class="chip" disabled={!!shown || running} onclick={() => act('update')}>{t('play.update')}</button>
                  <button class="chip" disabled={!!shown || running} onclick={() => act('playOutdated')}>{t('detail.playOutdated')}</button>
                </div>
              {/if}
            </section>
            {#if manageable}
              <div class="foot">
                <!-- The Epic, GOG and Amazon confirm is in the hero; this chip stays, so focus is moved there by hand. -->
                <Tip text={copy!.provider === 'steam' ? t('detail.inSteam') : ''}>
                  {#snippet children({ props })}
                    <span {...props} class="tip-anchor">
                      <button
                        class="chip danger"
                        disabled={!!shown || running}
                        onclick={() => {
                          uninstall()
                          tick().then(() => confirmCancel?.focus())
                        }}>{t('detail.uninstall')}</button
                      >
                    </span>
                  {/snippet}
                </Tip>
              </div>
            {/if}
          {:else}
            {#if savesError}
              <Tip text={savesError} truncated>
                {#snippet children({ props })}<p {...props} class="sync-note">{savesError}</p>{/snippet}
              </Tip>
            {/if}
            {#if showTiles}
              <!-- Reserved height: tiles that answer late fill their slot without moving the panels below. -->
              <div class="tiles">
                {#if playtime}
                  <div class="tile">
                    <div class="head">
                      <h2>{t('detail.playtime')}</h2>
                      {#if game.playtimeStores.length}<small>{game.playtimeStores.map((p) => storeNames[p]).join(' + ')}</small>{/if}
                    </div>
                    <p><b>{playtime}</b></p>
                  </div>
                {/if}
                {#if lastPlayed}
                  <div class="tile">
                    <div class="head"><h2>{t('detail.lastPlayed')}</h2></div>
                    <p>
                      <b>{lastPlayed}</b>{#if game.lastPlayedStore}<span> · {t('detail.onStore', { store: storeNames[game.lastPlayedStore] })}</span>{/if}
                    </p>
                  </div>
                {/if}
                {#if ttb}
                  <div class="tile">
                    <div class="head">
                      <h2>{t('detail.ttb')}</h2>
                      <Tip text={t('detail.igdbCredit')}>
                        {#snippet children({ props })}
                          <button {...props} class="credit" aria-label={t('detail.igdbCredit')} onclick={() => run(commands.openIgdb)}>IGDB</button>
                        {/snippet}
                      </Tip>
                    </div>
                    <Tip text={[beatMain && t('detail.normally', { time: beatMain }), ...beatMore].filter(Boolean).join(' · ')}>
                      {#snippet children({ props })}
                        <p {...props}>
                          <b>{beatMain ?? beatMore[0]}</b>{#if beatMain && beatAll}<span> · {t('detail.all', { time: beatAll })}</span>{/if}
                        </p>
                      {/snippet}
                    </Tip>
                  </div>
                {/if}
              </div>
            {/if}

            {#if !custom}
              <div class="cols">
                <!-- Skeleton lines hold the panels' places while the store details load. -->
                {#if info === undefined}
                  <section class="panel about" aria-labelledby="about-heading" aria-busy="true">
                    <h2 id="about-heading">{t('detail.about')}</h2>
                    <div class="skeleton" aria-hidden="true"><span></span><span></span><span></span><span></span></div>
                  </section>
                {:else if showAbout && info}
                  <section class="panel about" aria-labelledby="about-heading">
                    <h2 id="about-heading">{t('detail.about')}</h2>
                    {#if info.description}<p class="text">{info.description}</p>{/if}
                    {#if info.screenshots.length && shots?.length !== 0}
                      <div class="shots">
                        {#if shots}
                          {#each shots as shot, i (i)}
                            <img src={shot} alt={t('detail.screenshot', { number: i + 1, title: game.title })} loading="lazy" />
                          {/each}
                        {:else}
                          {#each info.screenshots as _, i (i)}<div class="shot" aria-hidden="true"></div>{/each}
                        {/if}
                      </div>
                    {/if}
                  </section>
                {/if}
                {#if showDetails}
                  <section class="panel details" class:solo={info !== undefined && !showAbout} aria-labelledby="details-heading">
                    <h2 id="details-heading">{t('detail.details')}</h2>
                    <dl>
                      {#if info === undefined}
                        {#each ['genres', 'developer', 'publisher', 'released'] as term (term)}
                          <dt>{t(`detail.${term}`)}</dt>
                          <dd class="skeleton" aria-hidden="true"><span></span></dd>
                        {/each}
                      {/if}
                      {#if info?.genres.length}
                        <dt>{t('detail.genres')}</dt>
                        <dd class="tags">
                          {#each info.genres as genre}<span>{genre}</span>{/each}
                        </dd>
                      {/if}
                      {#if info?.developer}<dt>{t('detail.developer')}</dt>
                        <dd>{info.developer}</dd>{/if}
                      {#if info?.publisher}<dt>{t('detail.publisher')}</dt>
                        <dd>{info.publisher}</dd>{/if}
                      {#if info?.released}<dt>{t('detail.released')}</dt>
                        <dd>{releaseDate(info.released)}</dd>{/if}
                      {#if copy!.sizeBytes !== null}<dt>{t('detail.size')}</dt>
                        <dd>{sizeLabel(copy!.sizeBytes)}</dd>{/if}
                      {@render chips(t('detail.storePages'), storeLinks)}
                      {@render chips(t('detail.info'), infoLinks)}
                    </dl>
                  </section>
                {/if}
              </div>
            {/if}
          {/if}
        </Tabs.Content>
      </Tabs.Root>
    {/if}
  </div>
</section>

<style>
  .detail {
    position: absolute;
    inset: 0;
    overflow-y: auto;
    animation: enter 180ms ease-out;
  }

  @keyframes enter {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }

  .page {
    max-width: 1360px;
    margin: 0 auto;
    padding: 0 24px 24px;
  }

  /* Hero: rounded art banner. The frame clips the art; the hero itself doesn't, so the menu can hang below. */
  .hero {
    position: relative;
    height: 300px;
    border-radius: 14px;
    box-shadow: 0 10px 30px rgb(0 0 0 / 0.45);
  }

  .frame {
    position: absolute;
    inset: 0;
    overflow: hidden;
    border-radius: inherit;
    background: linear-gradient(160deg, var(--surface-2), var(--surface));
  }

  .frame img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: center 30%;
    opacity: 0;
    transition: opacity 300ms ease-out;
  }

  .frame img.loaded {
    opacity: 1;
  }

  /* Darkens the top for the back button and the bottom for the title and actions. */
  .frame::after {
    content: '';
    position: absolute;
    inset: 0;
    background: linear-gradient(180deg, var(--glass) 0%, transparent 30%, transparent 35%, var(--scrim) 100%);
  }

  .back {
    position: absolute;
    top: 16px;
    left: 16px;
    border-radius: var(--radius);
    background: var(--glass-strong);
    border: 1px solid var(--glass-line);
  }

  .back:hover:not(:disabled) {
    background: var(--glass-on);
  }

  .bottom {
    position: absolute;
    left: 24px;
    right: 24px;
    bottom: 20px;
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 16px;
  }

  .name {
    min-width: 0;
  }

  h1 {
    margin: 0;
    font-size: 34px;
    font-weight: 700;
    line-height: 1.1;
    overflow-wrap: break-word;
    text-shadow: 0 1px 12px rgb(0 0 0 / 0.5);
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    color: var(--text-2);
  }

  .meta span::before {
    content: '·';
    margin-right: 8px;
    color: var(--text-3);
  }

  /* No store logo before it (a game the owner added). */
  .meta span:first-child::before {
    content: none;
  }

  .progress {
    display: grid;
    gap: 4px;
    margin-top: 12px;
    max-width: 420px;
  }

  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--glass-on);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 400ms linear;
  }

  .progress small {
    font-size: 12px;
    color: var(--text-2);
  }

  .actions {
    position: relative;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  /* The install panel: over the page under the Install pill, right edges aligned, so nothing below moves. */
  .install {
    position: absolute;
    z-index: 20;
    top: calc(100% + 8px);
    right: 0;
  }

  .round {
    height: 44px;
    padding: 0 20px;
    border-radius: 999px;
  }

  .ask {
    max-width: 260px;
    color: var(--text);
  }

  /* Like the install panel's DLC rows. */
  .also {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    font-size: 14px;
    color: var(--text-2);
    cursor: pointer;
  }

  .also input {
    width: 16px;
    height: 16px;
    margin: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }

  button.danger {
    color: var(--danger-text);
  }

  .more {
    width: 44px;
    height: 44px;
    padding: 0;
    display: grid;
    place-items: center;
    border-radius: 999px;
  }

  .more svg {
    fill: currentColor;
  }

  svg {
    flex: none;
    fill: currentColor;
  }

  /* Quick facts: three fixed slots. Every tile is one head line plus one value line, so a late
     tile is exactly as tall as the ones already there and the row never changes height. */
  .tiles {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 16px;
    margin-top: 16px;
  }

  .tile {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    padding: 12px 16px;
    border-radius: 12px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
  }

  h2 {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-2);
  }

  .tile b {
    font-size: 20px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-height: 24px;
  }

  .tile p {
    margin: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .head small {
    font-size: 12px;
    color: var(--text-2);
  }

  .tile p span {
    color: var(--text-2);
  }

  /* 24px tall: the minimum target size for dense desktop UI. */
  .credit {
    flex: none;
    min-height: 24px;
    padding: 0;
    background: transparent;
    font-size: 12px;
    color: var(--text-3);
    text-decoration: underline;
  }

  .credit:hover:not(:disabled) {
    background: transparent;
    color: var(--text);
  }

  .credit:active:not(:disabled) {
    transform: none;
  }

  .cols {
    display: grid;
    grid-template-columns: minmax(0, 1.7fr) minmax(0, 1fr);
    align-items: start;
    gap: 16px;
    margin-top: 16px;
  }

  .panel {
    padding: 16px;
    border-radius: 12px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
  }

  .panel h2 {
    margin-bottom: 8px;
  }

  .about {
    grid-column: 1;
  }

  /* Right column, or the left one when the game has nothing to say about itself. */
  .details {
    grid-column: 2;
  }

  .details.solo {
    grid-column: 1;
  }

  .text {
    margin: 0;
    line-height: 1.55;
    white-space: pre-line;
  }

  .shots {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 8px;
    margin-top: 12px;
  }

  .shots img,
  .shot {
    display: block;
    width: 100%;
    aspect-ratio: 16 / 9;
    object-fit: cover;
    border-radius: 8px;
    background: var(--surface);
  }

  /* Lines as tall as the text they stand in for, in a quiet glass tone. Flex, so margins don't collapse. */
  .skeleton {
    display: flex;
    flex-direction: column;
  }

  .skeleton span {
    display: block;
    height: 10px;
    margin: 6px 0;
    border-radius: 5px;
    background: var(--glass-hover);
  }

  .skeleton span:last-child:not(:first-child) {
    width: 60%;
  }

  dd.skeleton span {
    width: 70%;
    margin: 5px 0;
  }

  dl {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 8px 16px;
    margin: 0;
  }

  dt {
    color: var(--text-2);
  }

  dd {
    margin: 0;
  }

  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .tags span {
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--glass-on);
    font-size: 12px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .choice {
    position: relative;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    font-size: 13px;
    border-radius: 999px;
    background: var(--glass-hover);
    border: 1px solid var(--glass-line);
  }

  .chip:hover:not(:disabled) {
    background: var(--glass-on);
  }

  .chip[aria-pressed='true'] {
    background: var(--glass-on);
    border-color: var(--glass-on-line);
  }

  .path {
    overflow-wrap: anywhere;
  }

  /* Label-style tabs under the hero; the open one is underlined in `text`. */
  .tabs {
    display: flex;
    gap: 4px;
    margin-top: 16px;
    border-bottom: 1px solid var(--glass-line);
  }

  [role='tab'] {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-bottom: -1px;
    padding: 10px 14px;
    border-radius: 8px 8px 0 0;
    border-bottom: 2px solid transparent;
    background: transparent;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-2);
  }

  [role='tab']:hover:not(:disabled) {
    background: transparent;
    color: var(--text);
  }

  [role='tab']:active:not(:disabled) {
    transform: none;
  }

  [role='tab'][aria-selected='true'] {
    border-bottom-color: var(--text);
    color: var(--text);
  }

  [role='tab']:focus-visible {
    outline-offset: -2px;
  }

  .n {
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--glass-on);
    font-size: 11px;
    letter-spacing: 0;
    font-variant-numeric: tabular-nums;
  }

  [role='tabpanel'] > .panel {
    margin-top: 16px;
  }

  .rows {
    padding-top: 6px;
    padding-bottom: 6px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 0;
  }

  .row + .row {
    border-top: 1px solid var(--glass-line);
  }

  .what {
    flex: 1;
    min-width: 0;
  }

  .args {
    flex: 1;
    min-width: 0;
    height: 28px;
    padding: 0 10px;
    font-size: 13px;
    border-radius: 10px;
    border: 1px solid var(--glass-line);
    background: var(--glass);
    color: var(--text);
  }

  .saved-note {
    font-size: 12px;
    color: var(--text-3);
  }

  .what small {
    display: block;
    font-size: 12px;
    color: var(--text-3);
  }

  /* Installation and Saves rows: what the row is about, its value (one line, full text in the title), then its chips. */
  .key {
    flex: none;
    width: 88px;
    font-size: 13px;
    color: var(--text-3);
  }

  .value {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .value.failed {
    color: var(--danger-text);
  }

  .foot {
    display: flex;
    justify-content: flex-end;
    margin-top: 10px;
  }

  .sync-note {
    color: var(--text-2);
    margin: 16px 0 0;
    font-size: 13px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .dlc {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .dlc li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
  }

  .dlc-title {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .dlc-state {
    flex: none;
    font-size: 12px;
    color: var(--text-2);
  }

  .gone {
    padding: 24px 0;
    color: var(--text-2);
  }
</style>
