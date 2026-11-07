<script lang="ts" module>
  /** The year in a store date: Steam "28 Nov, 2016", Epic "2019-06-26". */
  export function releaseYear(date: string): string | null {
    return /\b(\d{4})\b/.exec(date)?.[1] ?? null
  }

  // Fixed English names: toLocaleDateString's short months vary by ICU version ("Sept").
  const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

  /** Month number 1–12 from a name like "Nov" or "November"; null when it isn't one. */
  function monthNumber(name: string): number | null {
    const at = MONTHS.indexOf(name.charAt(0).toUpperCase() + name.slice(1, 3).toLowerCase())
    return at < 0 ? null : at + 1
  }

  const dayMonthYear = (day: number, month: number, year: string) => `${day} ${MONTHS[month - 1]} ${year}`

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
  import { onMount, tick } from 'svelte'
  import * as api from './api'
  import type { ActOptions, Action, Conflict, Detail, Dlc, GameInfo, LinkKind, Progress, QueueView, SavesView, TimeToBeat } from './api'
  import PlayButton from './PlayButton.svelte'
  import LinkIcon, { linkNames } from './LinkIcon.svelte'
  import StoreIcon, { storeNames } from './StoreIcon.svelte'

  let { gameKey, runningKey, progressByKey, queue, oncancel, onresume, version, onback, onerror, onchanged, ontint, onconfirminstall, conflict = null }: {
    gameKey: string
    /** Copy keys, as `poll` reports them. */
    runningKey: string | null
    progressByKey: Record<string, Progress>
    /** The Epic download queue; this game's job in it decides the busy pill. */
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
    /** Install through the library's flow: an Epic copy asks in the install popup first, then calls `go`. */
    onconfirminstall: (key: string, title: string, anchor: DOMRect, go: (withDlc: boolean) => void) => void
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
  /** The Epic DLC whose uninstall is being confirmed, as `copy|id`. */
  let confirmingDlc = $state<string | null>(null)
  let dlcCancel = $state<HTMLButtonElement>()
  let busy = $state<string | null>(null)
  let confirming = $state(false)
  let menuOpen = $state(false)
  let moreButton = $state<HTMLButtonElement>()
  let menu = $state<HTMLDivElement>()
  let backButton = $state<HTMLButtonElement>()
  let confirmCancel = $state<HTMLButtonElement>()
  let actionsEl = $state<HTMLDivElement>()
  type Tab = 'overview' | 'dlc' | 'saves' | 'installation'
  let tab = $state<Tab>('overview')
  let tabList = $state<HTMLDivElement>()
  /** The chosen copy's Saves tab; null without cloud saves. */
  let saves = $state<SavesView | null>(null)
  let syncing = $state(false)
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
    api.game(key).then(
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
  // Running state and progress arrive under copy keys.
  const copyKeys = $derived(game ? game.copies.map((c) => c.key) : [gameKey])
  const running = $derived(runningKey !== null && copyKeys.includes(runningKey))
  const progress = $derived(copyKeys.map((k) => progressByKey[k]).find(Boolean))

  async function loadSaves(key: string, alive = () => true) {
    try {
      const s = await api.saves(key)
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
    await run(() => api.setSaves(key, choice))
    await loadSaves(key)
  }

  async function syncNow() {
    const key = copy!.key
    syncing = true
    await run(() => api.syncSavesNow(key))
    await loadSaves(key)
    syncing = false
  }

  const labels: Record<Action, string> = {
    play: 'Launching…',
    playOutdated: 'Launching…',
    install: 'Installing…',
    update: 'Updating…',
    uninstall: 'Uninstalling…',
    verify: 'Verifying…',
    move: 'Moving…',
  }
  /** This game's Epic job in the download queue. */
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
        ? 'Cancelling…'
        : job.state === 'queued'
          ? 'Queued'
          : job.state === 'paused'
            ? 'Paused'
            : labels[job.action]
      : busy,
  )

  async function act(action: Action, key = copy!.key, options: ActOptions = {}) {
    confirming = false
    menuOpen = false
    busy = labels[action]
    keepFocus(() => actionsEl?.querySelector<HTMLButtonElement>('button.pill[aria-disabled]'))
    try {
      const found = await api.act(action, key, options)
      if (found) asking = { conflict: found, action, key }
    } catch (e) {
      onerror(api.message(e))
    } finally {
      busy = null
      onchanged()
      keepFocus(() => actionsEl?.querySelector<HTMLButtonElement>('button.pill, button.go'))
    }
  }

  /** Epic's queued jobs run in Legendary, so they can be cancelled. Not Move: stopped halfway, Legendary's record and the folder disagree. */
  async function cancel() {
    const key = job!.key
    cancelling = key
    keepFocus(() => actionsEl?.querySelector<HTMLButtonElement>('button.pill[aria-disabled]'))
    if (!(await oncancel(key))) cancelling = null
  }

  /** Epic moves into a picked folder; Steam opens its own storage manager. */
  async function move() {
    const key = copy!.key
    if (copy!.provider !== 'epic') return act('move', key)
    const dest = await api.pickFolder().catch((e) => (onerror(api.message(e)), null))
    if (dest) act('move', key, { dest })
  }

  // Steam asks in its own dialog; Legendary doesn't, so Epic asks here first.
  function uninstall() {
    if (copy?.provider !== 'epic') {
      act('uninstall')
      return
    }
    confirming = true
    keepFocus(() => confirmCancel)
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

  const run = (f: () => Promise<void>) => f().catch((e) => onerror(api.message(e)))

  async function openMenu() {
    menuOpen = true
    await tick()
    menu?.querySelector('button')?.focus()
  }

  function closeMenu(refocus: boolean) {
    menuOpen = false
    if (refocus) moreButton?.focus()
  }

  function pick(f: () => void) {
    closeMenu(true)
    f()
  }

  function menuKeydown(e: KeyboardEvent) {
    if (!menu) return
    const items = Array.from(menu.querySelectorAll('button'))
    const at = items.indexOf(document.activeElement as HTMLButtonElement)
    const go = (i: number) => {
      e.preventDefault()
      items[(i + items.length) % items.length]?.focus()
    }
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      closeMenu(true)
    } else if (e.key === 'ArrowDown') go(at + 1)
    else if (e.key === 'ArrowUp') go(at - 1)
    else if (e.key === 'Home') go(0)
    else if (e.key === 'End') go(-1)
    else if (e.key === 'Tab') closeMenu(false)
  }

  function onpointerdown(e: PointerEvent) {
    if (!menuOpen) return
    const target = e.target as Node
    if (menu?.contains(target) || moreButton?.contains(target)) return
    closeMenu(false)
  }

  // Tile text comes ready from Rust; null hides the tile (Epic's unknown playtime, never played).
  const playtime = $derived(game?.playtimeTile ?? null)
  const lastPlayed = $derived(game?.lastPlayedTile ?? null)
  const beatMain = $derived(ttb ? (ttb.normally ?? null) : null)
  const beatMore = $derived(
    ttb
      ? [ttb.hastily && `${ttb.hastily} hastily`, ttb.completely && `${ttb.completely} completely`].filter(
          (s): s is string => !!s,
        )
      : [],
  )
  // Decided from `game` alone, never from how long to beat, so a row that appeared never collapses.
  // A Steam game always has its Playtime tile, so its row is never empty; the free slot waits for a time to beat.
  const showTiles = $derived(!!playtime || !!lastPlayed)

  const year = $derived(info?.released ? releaseYear(info.released) : null)
  // The chosen copy's store when it is installed, else the first installed copy's.
  const installedOn = $derived(game && (copy!.installed ? copy! : game.copies.find((c) => c.installed)))
  const meta = $derived(
    game
      ? [installedOn ? `Installed on ${storeNames[installedOn.provider]}` : 'Not installed', info?.developer, year].filter(
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

  // Launch options (Task 25) joins when it exists; a tab with nothing for this game is hidden.
  const tabs = $derived.by(() => {
    const list: { id: Tab; label: string; count?: number }[] = [{ id: 'overview', label: 'Overview' }]
    if (dlcs.length) list.push({ id: 'dlc', label: 'DLC', count: dlcs.length })
    if (copy?.cloudSaves) list.push({ id: 'saves', label: 'Saves' })
    if (copy?.installed || copy?.installDir) list.push({ id: 'installation', label: 'Installation' })
    return list
  })
  // A tab that went away (the DLC list emptied, the copy was uninstalled) falls back to Overview.
  const current = $derived(tabs.some((t) => t.id === tab) ? tab : 'overview')

  /** Arrow keys, Home and End move between tabs (roving tabindex); the focused tab opens. */
  function tabKeydown(e: KeyboardEvent) {
    const at = tabs.findIndex((t) => t.id === current)
    const to =
      e.key === 'ArrowRight' ? at + 1 : e.key === 'ArrowLeft' ? at - 1 : e.key === 'Home' ? 0 : e.key === 'End' ? tabs.length - 1 : null
    if (to === null) return
    e.preventDefault()
    tab = tabs[(to + tabs.length) % tabs.length].id
    tick().then(() => tabList?.querySelector<HTMLButtonElement>('[aria-selected="true"]')?.focus())
  }

  const showAbout = $derived(!!info && (info.description !== '' || info.screenshots.length > 0))
  // Every game has a store page, so Details always has something to show.
  const showDetails = $derived(!!game)
  const storeLinks = $derived(game?.links.filter((k) => k === 'steamStore' || k === 'epicStore') ?? [])
  const infoLinks = $derived(game?.links.filter((k) => k !== 'steamStore' && k !== 'epicStore') ?? [])
</script>

{#snippet chips(label: string, kinds: LinkKind[])}
  {#if kinds.length}
    <dt>{label}</dt>
    <dd class="chips">
      {#each kinds as kind (kind)}
        <button class="chip" onclick={() => run(() => api.openLink(game!.key, kind))}>
          <LinkIcon {kind} />{linkNames[kind]}
        </button>
      {/each}
    </dd>
  {/if}
{/snippet}

<svelte:window {onpointerdown} />

<section class="detail" aria-label={game?.title ?? 'Game'}>
  <div class="page">
    <header class="hero">
      <div class="frame">
        {#if art.hero}
          <img src={art.hero} alt="" class:loaded={heroLoaded} onload={() => (heroLoaded = true)} />
        {/if}
      </div>
      <button class="back" bind:this={backButton} onclick={onback}>← Library</button>

      {#if game}
        <div class="bottom">
          <div class="name">
            <h1>{game.title}</h1>
            <div class="meta">
              {#each game.stores as store}<StoreIcon provider={store} size={14} />{/each}
              {#each meta as item}<span>{item}</span>{/each}
            </div>
            {#if progress}
              <div class="progress">
                <div
                  class="bar"
                  role="progressbar"
                  aria-label="Install progress"
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={Math.round(progress.fraction * 100)}
                >
                  <span style="width: {progress.fraction * 100}%"></span>
                </div>
                <small>{job?.state === 'paused' ? `Paused · ${Math.round(progress.fraction * 100)}%` : progress.text}</small>
              </div>
            {/if}
          </div>

          <div class="actions" bind:this={actionsEl}>
            {#if asking}
              <!-- Asks before launching, like the Epic uninstall; never picks a save by itself. -->
              <span class="ask">{asking.conflict === 'localNewer' ? 'Local save is newer than the cloud save.' : 'Cloud save is newer than the local save.'}</span>
              <button class="glass round" onclick={() => keep('cloud')}>Keep cloud</button>
              <button class="glass round" onclick={() => keep('local')}>Keep local</button>
              <button class="glass round" bind:this={askCancel} onclick={cancelAsk}>Cancel</button>
            {:else if confirming}
              <span class="ask">Uninstall {game.title}? Its files will be deleted.</span>
              <button class="glass round danger" onclick={() => act('uninstall')}>Uninstall</button>
              <button class="glass round" bind:this={confirmCancel} onclick={keepUninstalled}>Cancel</button>
            {:else}
              {#if !shown && (copy!.installed || copy!.installDir)}
                <div class="more-wrap">
                  <button
                    class="glass more"
                    bind:this={moreButton}
                    aria-label="More actions"
                    aria-haspopup="menu"
                    aria-expanded={menuOpen}
                    onclick={() => (menuOpen ? closeMenu(true) : openMenu())}
                  >
                    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                      <circle cx="5" cy="12" r="2" /><circle cx="12" cy="12" r="2" /><circle cx="19" cy="12" r="2" />
                    </svg>
                  </button>
                  {#if menuOpen}
                    <div class="menu" role="menu" aria-label="More actions" tabindex="-1" bind:this={menu} onkeydown={menuKeydown}>
                      {#if copy!.installDir}
                        <button role="menuitem" tabindex="-1" onclick={() => pick(() => run(() => api.openFolder(copy!.key)))}>
                          Open install folder
                        </button>
                      {/if}
                      {#if copy!.installed}
                        <button role="menuitem" tabindex="-1" class="danger" onclick={() => pick(uninstall)}>Uninstall</button>
                      {/if}
                    </div>
                  {/if}
                </div>
              {/if}
              <PlayButton
                copies={game.copies}
                chosen={chosenKey}
                {running}
                busy={shown}
                onchoose={(key) => (chosenKey = key)}
                onplay={() => act('play')}
                onupdate={() => act('update')}
                oninstall={() => {
                  const key = copy!.key // the copy shown when the popup opens, not when it is confirmed
                  onconfirminstall(key, game!.title, actionsEl!.getBoundingClientRect(), (withDlc) =>
                    act('install', key, { withDlc }),
                  )
                }}
              />
              {#if job?.state === 'paused'}
                <button class="glass round" onclick={onresume}>Resume</button>
              {/if}
              {#if job && !cancelling}
                <button class="glass round" onclick={cancel}>Cancel</button>
              {/if}
            {/if}
          </div>
        </div>
      {/if}
    </header>

    {#if game === null}
      <p class="gone">This game is no longer in the library.</p>
    {:else if game}
      <!-- Always shown, Overview alone included, so a DLC tab that arrives late doesn't move the page. -->
      <div class="tabs" role="tablist" aria-label="Game sections" tabindex="-1" bind:this={tabList} onkeydown={tabKeydown}>
        {#each tabs as t (t.id)}
          <button
            role="tab"
            id="tab-{t.id}"
            aria-selected={current === t.id}
            aria-controls="tab-panel"
            tabindex={current === t.id ? 0 : -1}
            onclick={() => (tab = t.id)}
          >
            {t.label}{#if t.count}<span class="n">{t.count}</span>{/if}
          </button>
        {/each}
      </div>

      <div id="tab-panel" role="tabpanel" aria-labelledby="tab-{current}">
      {#if current === 'dlc'}
        <section class="panel" aria-label="DLC">
          <ul class="dlc">
            {#each dlcs as d (`${d.copy}|${d.id}`)}
              <li>
                <span class="dlc-title">{d.title}</span>
                {#if confirmingDlc === `${d.copy}|${d.id}`}
                  <!-- Asks inline, like the game's own Epic uninstall. -->
                  <span class="dlc-state">Its files will be deleted.</span>
                  <button
                    class="chip danger"
                    aria-label="Uninstall {d.title}"
                    onclick={() => {
                      confirmingDlc = null
                      act('uninstall', d.copy!, { dlc: d.id })
                    }}>Uninstall</button
                  >
                  <button class="chip" bind:this={dlcCancel} onclick={() => (confirmingDlc = null)}>Cancel</button>
                {:else}
                  <span class="dlc-state">{d.installed ? 'Installed' : 'Not installed'}</span>
                  <!-- Legendary adds a DLC to an installed game only. -->
                  {#if d.copy && !shown && game.copies.find((c) => c.key === d.copy)?.installed}
                    <button
                      class="chip"
                      class:danger={d.installed}
                      aria-label="{d.installed ? 'Uninstall' : 'Install'} {d.title}"
                      onclick={() => (d.installed ? askUninstallDlc(d) : act('install', d.copy!, { dlc: d.id }))}
                    >
                      {d.installed ? 'Uninstall' : 'Install'}
                    </button>
                  {/if}
                {/if}
              </li>
            {/each}
          </ul>
        </section>
      {:else if current === 'saves'}
        <!-- The chosen Epic copy's cloud saves; the global default is in Settings → Library. -->
        <section class="panel rows" aria-label="Saves">
          <div class="row">
            <div class="what">Sync cloud saves<small>Downloads before you play and uploads after. Default follows Settings.</small></div>
            <div class="chips" role="group" aria-label="Sync cloud saves">
              {#each [{ value: null, label: 'Default' }, { value: true, label: 'On' }, { value: false, label: 'Off' }] as option (option.label)}
                <button class="chip" aria-pressed={saves !== null && saves.choice === option.value} onclick={() => setSaves(option.value)}>
                  {option.label}
                </button>
              {/each}
            </div>
          </div>
          <div class="row">
            <div class="what">Last sync<small class="last" class:failed={saves?.failed} title={saves?.last}>{saves?.last ?? ' '}</small></div>
          </div>
          <div class="row">
            <div class="what">Sync now<small>The newer save wins.</small></div>
            <button class="chip" disabled={!!shown || running || syncing} onclick={syncNow}>{syncing ? 'Syncing…' : 'Sync now'}</button>
          </div>
        </section>
      {:else if current === 'installation'}
        <!-- Acts on the chosen copy, like the ⋯ menu. -->
        <section class="panel rows" aria-label="Installation">
          {#if copy!.installDir}
            <div class="row">
              <div class="what">Folder<small class="path">{copy!.installDir}{#if copy!.size}{' · '}{copy!.size}{/if}</small></div>
              <button class="chip" onclick={() => run(() => api.openFolder(copy!.key))}>Open folder</button>
            </div>
          {/if}
          {#if copy!.updateAvailable}
            <div class="row">
              <div class="what">Update available<small>A newer version is ready to download.</small></div>
              <button class="chip" disabled={!!shown || running} onclick={() => act('update')}>Update</button>
              <button class="chip" disabled={!!shown || running} onclick={() => act('playOutdated')}>Play without updating</button>
            </div>
          {/if}
          {#if copy!.installed}
            <div class="row">
              <div class="what">
                Verify files<small>{copy!.provider === 'steam' ? 'Steam checks the files in its own window.' : 'Checks every file and repairs bad or missing ones.'}</small>
              </div>
              <button class="chip" disabled={!!shown || running} onclick={() => act('verify')}>Verify files</button>
            </div>
            <div class="row">
              <div class="what">
                Move<small>{copy!.provider === 'steam' ? 'Opens Steam’s storage manager.' : 'Moves the game folder into another folder on the same drive.'}</small>
              </div>
              <button class="chip" disabled={!!shown || running} onclick={move}>Move…</button>
            </div>
            <div class="row">
              <div class="what">
                Uninstall<small>Removes the {storeNames[copy!.provider]} copy{copy!.provider === 'steam' ? ' in Steam’s own dialog' : ' and deletes its files'}.</small>
              </div>
              <!-- The Epic confirm is in the hero; this chip stays, so focus is moved there by hand. -->
              <button
                class="chip danger"
                disabled={!!shown || running}
                onclick={() => {
                  uninstall()
                  tick().then(() => confirmCancel?.focus())
                }}>Uninstall</button
              >
            </div>
          {/if}
        </section>
      {:else}
      {#if saves?.failed}
        <p class="sync-note" title={saves.last}>{saves.last}</p>
      {/if}
      {#if showTiles}
        <!-- Reserved height: tiles that answer late fill their slot without moving the panels below. -->
        <div class="tiles">
          {#if playtime}
            <div class="tile"><div class="head">
                <h2>Playtime</h2>
                {#if game.playtimeNote}<small>{game.playtimeNote}</small>{/if}
              </div><p><b>{playtime}</b></p></div>
          {/if}
          {#if lastPlayed}
            <div class="tile"><div class="head"><h2>Last played</h2></div><p><b>{lastPlayed}</b>{#if game.lastPlayedStore}<span> · {game.lastPlayedStore}</span>{/if}</p></div>
          {/if}
          {#if ttb}
            <div class="tile">
              <div class="head">
                <h2>How long to beat</h2>
                <button class="credit" title="Data from IGDB" aria-label="Data from IGDB" onclick={() => run(api.openIgdb)}>IGDB</button>
              </div>
              <p title={[beatMain && `${beatMain} normally`, ...beatMore].filter(Boolean).join(' · ')}>
                <b>{beatMain ?? beatMore[0]}</b>{#if beatMain && ttb.completely}<span> · {ttb.completely} all</span>{/if}
              </p>
            </div>
          {/if}
        </div>
      {/if}

      <div class="cols">
        <!-- Skeleton lines hold the panels' places while the store details load. -->
        {#if info === undefined}
          <section class="panel about" aria-labelledby="about-heading" aria-busy="true">
            <h2 id="about-heading">About</h2>
            <div class="skeleton" aria-hidden="true"><span></span><span></span><span></span><span></span></div>
          </section>
        {:else if showAbout && info}
          <section class="panel about" aria-labelledby="about-heading">
            <h2 id="about-heading">About</h2>
            {#if info.description}<p class="text">{info.description}</p>{/if}
            {#if info.screenshots.length && shots?.length !== 0}
              <div class="shots">
                {#if shots}
                  {#each shots as shot, i (i)}
                    <img src={shot} alt="Screenshot {i + 1} of {game.title}" loading="lazy" />
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
            <h2 id="details-heading">Details</h2>
            <dl>
              {#if info === undefined}
                {#each ['Genres', 'Developer', 'Publisher', 'Released'] as term (term)}
                  <dt>{term}</dt><dd class="skeleton" aria-hidden="true"><span></span></dd>
                {/each}
              {/if}
              {#if info?.genres.length}
                <dt>Genres</dt>
                <dd class="tags">{#each info.genres as genre}<span>{genre}</span>{/each}</dd>
              {/if}
              {#if info?.developer}<dt>Developer</dt><dd>{info.developer}</dd>{/if}
              {#if info?.publisher}<dt>Publisher</dt><dd>{info.publisher}</dd>{/if}
              {#if info?.released}<dt>Released</dt><dd>{releaseDate(info.released)}</dd>{/if}
              {#if copy!.size}<dt>Size</dt><dd>{copy!.size}</dd>{/if}
              {@render chips('Store pages', storeLinks)}
              {@render chips('Info', infoLinks)}
            </dl>
          </section>
        {/if}
      </div>
      {/if}
      </div>
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
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 8px;
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

  button.danger {
    color: var(--danger-text);
  }

  .more-wrap {
    position: relative;
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

  .menu {
    position: absolute;
    z-index: 5;
    top: calc(100% + 8px);
    right: 0;
    min-width: 220px;
    padding: 4px;
    display: grid;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: 0 30px 80px rgb(0 0 0 / 0.6);
  }

  .menu button {
    padding: 8px 12px;
    text-align: left;
    background: transparent;
  }

  .menu button:hover:not(:disabled),
  .menu button:focus-visible {
    background: var(--glass-hover);
  }

  .menu button:focus-visible {
    outline-offset: -2px;
  }

  .menu button:active:not(:disabled) {
    transform: none;
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

  .tabs:focus-visible {
    outline: none;
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

  .what small {
    display: block;
    font-size: 12px;
    color: var(--text-3);
  }

  /* One quiet line; the full text is in the title. */
  .what small.last {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .what small.failed,
  .sync-note {
    color: var(--text-2);
  }

  .sync-note {
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
