<script lang="ts">
  import { onMount, tick } from 'svelte'
  import { MediaQuery } from 'svelte/reactivity'
  import { mergeProps } from 'bits-ui'
  import * as api from './api'
  import { onOpenDownloads } from './api'
  import { commands, type Card, type Collection, type Counts, type Progress, type QueueView } from './bindings'
  import { collectionFilter } from './filters'
  import { t } from './i18n'
  import { downloadFraction, fallbackView, RAIL_BELOW, type Pop, type View } from './sidebar'
  import ActionMenu, { type Action } from './ActionMenu.svelte'
  import DownloadsPopover from './DownloadsPopover.svelte'
  import Tip from './Tip.svelte'

  let {
    view = $bindable(),
    counts,
    collections,
    pop = $bindable(),
    modal,
    attention,
    progress,
    queue,
    games,
    coversReady,
    oncancel,
    onpause,
    onresume,
    onmove,
    onsettings,
    onchanged,
    onnavigate,
  }: {
    view: View
    /** Over the whole library, not what the toolbar filters show. */
    counts: Counts
    collections: Collection[]
    /** The one open popover, shared with the toolbar. */
    pop: Pop | null
    /** A modal (Settings, Add a game) is open: the tray's Downloads item opens nothing. */
    modal: boolean
    /** Something in settings needs a look: the dot on Settings. */
    attention: boolean
    /** `poll`'s map, by copy key: Downloads shows while it has anything, or the queue does. */
    progress: Record<string, Progress>
    queue: QueueView
    games: Card[]
    /** The grid's first covers have settled; the stacks ask only after them. */
    coversReady: boolean
    oncancel: (key: string) => void
    onpause: () => void
    onresume: () => void
    onmove: (key: string, up: boolean) => void
    onsettings: () => void
    /** After a collection was created, renamed or deleted. */
    onchanged: () => void
    /** A row was chosen; App closes the game page. */
    onnavigate: () => void
  } = $props()

  let nav = $state<HTMLElement>()
  /** Library is always the first row. */
  const libraryRow = () => nav?.querySelector<HTMLElement>('[data-row]')
  let plusButton = $state<HTMLButtonElement>()
  let downloadsRow = $state<HTMLButtonElement>()
  let settingsRow = $state<HTMLButtonElement>()

  /** The row that is the Tab stop: the view, or Library while a gone row waits for App's fallback. */
  const current = $derived(fallbackView(view, counts, collections))

  const rowName = (name: string, count: number | string) => t('sidebar.rowName', { name, count })

  function choose(v: View, e: MouseEvent) {
    onnavigate()
    view = v
    problem = null
    // WebKitGTK doesn't focus a clicked button.
    ;(e.currentTarget as HTMLElement).focus()
  }

  /** The view rows are one Tab stop: Up, Down, Home and End move between them. */
  function rowKeys(e: KeyboardEvent) {
    if (!['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(e.key) || !nav) return
    const rows = [...nav.querySelectorAll<HTMLElement>('[data-row]')]
    const i = rows.indexOf(e.currentTarget as HTMLElement)
    e.preventDefault()
    const to = e.key === 'Home' ? 0 : e.key === 'End' ? rows.length - 1 : Math.max(0, Math.min(rows.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)))
    rows[to]?.focus()
  }

  // The view fell back to Library (its row went away): focus that was in the sidebar, or lost with the row, goes to Library.
  let lastView = view
  $effect(() => {
    const was = lastView
    lastView = view
    if (view !== null || was === null) return
    const a = document.activeElement
    if (!a || a === document.body || nav?.contains(a)) libraryRow()?.focus()
  })

  const downloading = $derived(Object.keys(progress).length > 0 || queue.jobs.length > 0)
  const jobCount = $derived(new Set([...Object.keys(progress), ...queue.jobs.map((j) => j.key)]).size)
  const fraction = $derived(downloadFraction(progress, queue))
  const showDownloads = (on: boolean) => (pop = on ? 'downloads' : pop === 'downloads' ? null : pop)

  // The last download ended: its popover closes, and focus on the row or in the popover moves to Settings.
  let wasDownloading = false
  $effect.pre(() => {
    const now = downloading
    if (wasDownloading && !now) {
      const a = document.activeElement
      const had = !!a && (a === downloadsRow || (pop === 'downloads' && !!a.closest('[data-popover-content]')))
      if (pop === 'downloads') pop = null
      if (had) tick().then(() => settingsRow?.focus())
    }
    wasDownloading = now
  })

  // The tray's Downloads item: the window is already shown; open the popover only if there is something in it.
  onMount(() => {
    const unlisten = onOpenDownloads(() => {
      if (downloading && !modal) pop = 'downloads'
    })
    return () => void unlisten.then((f) => f())
  })

  /** Cover art by entry key, asked after the grid's first covers and ordered after the grid's cards. */
  let art = $state<Record<string, string | null>>({})
  let asked = 0
  $effect(() => {
    if (!coversReady) return
    for (const c of collections)
      for (const key of c.covers) {
        if (key in art) continue
        art[key] = null
        api.cover(key, games.length + asked++).then((found) => (art[key] = found?.src ?? null))
      }
  })

  let naming = $state(false)
  let name = $state('')
  let renaming = $state<number | null>(null)
  let draft = $state('')
  let deleting = $state<number | null>(null)
  /** Why the last create, rename or delete failed. */
  let problem = $state<string | null>(null)
  /** The row whose ⋯ menu is open. */
  let more = $state<number | null>(null)
  /** The menu a right-click or the Menu key opened; it stays mounted after closing so a picked action still runs. */
  let ctx = $state<{ c: Collection; x: number; y: number } | null>(null)
  let ctxOpen = $state(false)

  const narrow = new MediaQuery(`max-width: ${RAIL_BELOW - 1}px`)
  /** A name field, a delete question or a failure doesn't fit the rail: the full sidebar lies over the content meanwhile, so the grid keeps its width. */
  const editing = $derived(naming || renaming !== null || deleting !== null || problem !== null)
  const rail = $derived(narrow.current && !editing)
  const settingsName = $derived(attention ? t('toolbar.settingsAttention') : t('toolbar.settings'))

  const rowOf = (id: number) => nav?.querySelector<HTMLElement>(`[data-collection="${id}"]`)

  async function startNaming() {
    naming = true
    name = ''
    problem = null
    await tick()
    nav?.querySelector<HTMLInputElement>('.new input')?.focus()
  }

  async function create() {
    try {
      await commands.createCollection(name)
    } catch (e) {
      // The name is empty or taken: the field stays for another try.
      problem = api.message(e)
      return
    }
    naming = false
    problem = null
    onchanged()
    // In a narrow window + moves back into the rail, a new element.
    await tick()
    plusButton?.focus()
  }

  async function nameKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') create()
    else if (e.key === 'Escape') {
      naming = false
      problem = null
      await tick()
      plusButton?.focus()
    }
  }

  async function startRename(c: Collection) {
    renaming = c.id
    draft = c.name
    problem = null
    await tick()
    nav?.querySelector<HTMLInputElement>('.rename')?.focus()
  }

  async function saveRename(c: Collection) {
    if (renaming !== c.id) return
    renaming = null
    if (draft.trim() === c.name) return
    try {
      await commands.renameCollection(c.id, draft)
      problem = null
    } catch (e) {
      problem = api.message(e)
      // Saved by leaving the field onto nothing focusable: focus is lost, so bring it back where Escape closes the note.
      if (!document.activeElement || document.activeElement === document.body) rowOf(c.id)?.focus()
      // Focus went on elsewhere: no note to hold the overlay open; the row shows the old name.
      else if (!nav?.contains(document.activeElement)) problem = null
    }
    onchanged()
  }

  async function renameKeydown(e: KeyboardEvent, c: Collection) {
    if (e.key !== 'Enter' && e.key !== 'Escape') return
    if (e.key === 'Enter') saveRename(c)
    else renaming = null
    await tick()
    rowOf(c.id)?.focus()
  }

  async function askDelete(c: Collection) {
    deleting = c.id
    problem = null
    await tick()
    nav?.querySelector<HTMLElement>('.confirm .cancel')?.focus()
  }

  async function keep(c: Collection) {
    deleting = null
    await tick()
    rowOf(c.id)?.focus()
  }

  async function remove(c: Collection) {
    const chosen = view === collectionFilter(c.id)
    const at = collections.findIndex((x) => x.id === c.id)
    const next = collections[at + 1] ?? collections[at - 1]
    deleting = null
    try {
      await commands.deleteCollection(c.id)
    } catch (e) {
      problem = api.message(e)
      await tick()
      rowOf(c.id)?.focus()
      onchanged()
      return
    }
    if (chosen) libraryRow()?.focus()
    else if (next) rowOf(next.id)?.focus()
    else plusButton?.focus()
    onchanged()
  }

  const actions = (c: Collection): Action[] => [
    { label: t('filters.rename'), run: () => startRename(c), movesFocus: true },
    { label: t('sidebar.deleteCollection'), run: () => askDelete(c), danger: true, movesFocus: true },
  ]

  function openAt(e: MouseEvent, c: Collection) {
    e.preventDefault()
    // The Menu key and Shift+F10 may come without a pointer position.
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect()
    const keyboard = e.clientX === 0 && e.clientY === 0
    ctx = { c, x: keyboard ? r.left + 16 : e.clientX, y: keyboard ? r.bottom : e.clientY }
    ctxOpen = true
  }
</script>

{#snippet row(v: View, label: string, count: number, icon: string)}
  <Tip text={rail ? rowName(label, count) : ''}>
    {#snippet children({ props })}
      <button
        {...mergeProps(props, { onclick: (e: MouseEvent) => choose(v, e) })}
        class="item"
        class:on={current === v}
        data-row
        tabindex={current === v ? 0 : -1}
        aria-current={current === v ? 'page' : undefined}
        aria-label={rowName(label, count)}
        onkeydown={rowKeys}
      >
        <svg class="line" viewBox="0 0 24 24" width="17" height="17" aria-hidden="true"><path d={icon} /></svg>
        <span class="name">{label}</span>
        <em>{count}</em>
      </button>
    {/snippet}
  </Tip>
{/snippet}

{#snippet plus()}
  <Tip text={t('sidebar.newCollection')}>
    {#snippet children({ props })}
      <button {...props} class="plus" bind:this={plusButton} aria-label={t('sidebar.newCollection')} onclick={startNaming}>
        <svg class="line" viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
      </button>
    {/snippet}
  </Tip>
{/snippet}

<!-- Nothing else closes a failure note, and in a narrow window it holds the overlay open. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<nav
  class="side"
  class:rail
  class:over={narrow.current && editing}
  aria-label={t('sidebar.label')}
  bind:this={nav}
  onkeydown={(e) => e.key === 'Escape' && (problem = null)}
  onfocusout={(e) => !nav?.contains(e.relatedTarget as Node | null) && (problem = null)}
>
  {@render row(
    null,
    t('app.library'),
    counts.library,
    'M4.5 3h4A1.5 1.5 0 0 1 10 4.5v6A1.5 1.5 0 0 1 8.5 12h-4A1.5 1.5 0 0 1 3 10.5v-6A1.5 1.5 0 0 1 4.5 3zM15.5 3h4A1.5 1.5 0 0 1 21 4.5v6a1.5 1.5 0 0 1-1.5 1.5h-4a1.5 1.5 0 0 1-1.5-1.5v-6A1.5 1.5 0 0 1 15.5 3zM4.5 15h4a1.5 1.5 0 0 1 1.5 1.5v3A1.5 1.5 0 0 1 8.5 21h-4A1.5 1.5 0 0 1 3 19.5v-3A1.5 1.5 0 0 1 4.5 15zM15.5 15h4a1.5 1.5 0 0 1 1.5 1.5v3a1.5 1.5 0 0 1-1.5 1.5h-4a1.5 1.5 0 0 1-1.5-1.5v-3a1.5 1.5 0 0 1 1.5-1.5z',
  )}
  {#if counts.favorites > 0}
    {@render row('favorites', t('filters.favorites'), counts.favorites, 'M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z')}
  {/if}

  <div class="sec">
    <h2 id="sidebar-collections" class:visually-hidden={rail}>{t('sidebar.collections')}</h2>
    {#if !rail}{@render plus()}{/if}
  </div>
  <div class="list" role="group" aria-labelledby="sidebar-collections">
    {#if naming}
      <div class="new">
        <input
          aria-label={t('sidebar.collectionName')}
          placeholder={t('sidebar.collectionName')}
          bind:value={name}
          onkeydown={nameKeydown}
          onblur={() => !name.trim() && (naming = false)}
        />
      </div>
    {/if}
    {#each collections as c (c.id)}
      {@const v = collectionFilter(c.id)}
      <div class="coll">
        {#if deleting === c.id}
          <div class="confirm" role="group" aria-labelledby="ask-{c.id}">
            <span class="ask" id="ask-{c.id}">{t('filters.askDelete', { name: c.name })}</span>
            <div>
              <button class="small danger" aria-label={t('filters.deleteNamed', { name: c.name })} onclick={() => remove(c)}>{t('common.delete')}</button>
              <button class="small cancel" onclick={() => keep(c)} onkeydown={(e) => e.key === 'Escape' && keep(c)}>{t('common.cancel')}</button>
            </div>
          </div>
        {:else if renaming === c.id}
          <input
            class="rename"
            aria-label={t('filters.renameNamed', { name: c.name })}
            bind:value={draft}
            onkeydown={(e) => renameKeydown(e, c)}
            onblur={() => saveRename(c)}
          />
        {:else}
          {@const label = rowName(c.name, t('sidebar.gamesCount', { n: c.count }))}
          <Tip text={rail ? label : ''}>
            {#snippet children({ props })}
              <button
                {...mergeProps(props, { onclick: (e: MouseEvent) => choose(v, e) })}
                class="item two"
                class:on={current === v}
                data-row
                data-collection={c.id}
                tabindex={current === v ? 0 : -1}
                aria-current={current === v ? 'page' : undefined}
                aria-label={label}
                onkeydown={rowKeys}
                oncontextmenu={(e) => openAt(e, c)}
              >
                <span class="stack" aria-hidden="true">
                  {#if c.covers.length === 0}<span class="tile" style="--i: 0"></span>{/if}
                  {#each [...c.covers.entries()].reverse() as [i, key] (key)}
                    {#if art[key]}
                      <img src={art[key]} alt="" width="20" height="30" decoding="async" style="--i: {i}" />
                    {:else}
                      <span class="tile" style="--i: {i}"></span>
                    {/if}
                  {/each}
                </span>
                <span class="text">
                  <span class="name">{c.name}</span>
                  <small>{t('sidebar.gamesCount', { n: c.count })}</small>
                </span>
              </button>
            {/snippet}
          </Tip>
          <ActionMenu label={c.name} actions={actions(c)} bind:open={() => more === c.id, (o) => (more = o ? c.id : more === c.id ? null : more)}>
            {#snippet trigger({ props })}
              <button {...props} class="more" tabindex="-1" aria-label={t('detail.moreActions')}>⋯</button>
            {/snippet}
          </ActionMenu>
        {/if}
      </div>
    {:else}
      {#if !naming && !rail}<p class="note">{t('sidebar.noCollections')}</p>{/if}
    {/each}
    {#if problem}<p class="note problem" role="alert">{problem}</p>{/if}
  </div>
  {#if rail}{@render plus()}{/if}

  {#if counts.hidden > 0}
    {@render row(
      'hidden',
      t('filters.hidden'),
      counts.hidden,
      'M3 3l18 18M10.6 5.1A10 10 0 0 1 12 5c5 0 9 4.5 10 7a13 13 0 0 1-3 4M6.6 6.6A13 13 0 0 0 2 12c1 2.5 5 7 10 7a10 10 0 0 0 4.4-1',
    )}
  {/if}

  <div class="bottom">
    {#if downloading}
      <DownloadsPopover {progress} {queue} {games} bind:open={() => pop === 'downloads', showDownloads} {oncancel} {onpause} {onresume} {onmove}>
        {#snippet trigger({ props })}
          <Tip text={rail ? rowName(t('downloads.title'), jobCount) : ''}>
            {#snippet children({ props: tip })}
              <button
                {...mergeProps(tip, props)}
                class="item downloads"
                bind:this={downloadsRow}
                aria-label={t('toolbar.downloadsAt', { percent: `${Math.round(fraction * 100)}%` })}
              >
                {#if rail}<span class="ring" style="--done: {fraction * 100}%"></span>{/if}
                <svg class="line" viewBox="0 0 24 24" width="17" height="17" aria-hidden="true"><path d="M12 4v11M7 10l5 5 5-5M5 20h14" /></svg>
                <span class="name">{t('downloads.title')}</span>
                <em>{jobCount}</em>
                {#if !rail}<span class="bar" aria-hidden="true"><i style="width: {fraction * 100}%"></i></span>{/if}
              </button>
            {/snippet}
          </Tip>
        {/snippet}
      </DownloadsPopover>
    {/if}
    <Tip text={rail ? settingsName : ''}>
      {#snippet children({ props })}
        <button {...mergeProps(props, { onclick: onsettings })} class="item" bind:this={settingsRow} aria-label={settingsName}>
          <svg class="line" viewBox="0 0 24 24" width="17" height="17" aria-hidden="true">
            <circle cx="12" cy="12" r="3" />
            <path
              d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
            />
          </svg>
          <span class="name">{t('toolbar.settings')}</span>
          {#if attention}<span class="dot"></span>{/if}
        </button>
      {/snippet}
    </Tip>
  </div>
</nav>

{#if ctx}
  {#key ctx}
    <ActionMenu label={ctx.c.name} at={ctx} actions={actions(ctx.c)} bind:open={ctxOpen} />
  {/key}
{/if}

<style>
  .side {
    flex: none;
    --open-width: 224px;
    --rail-width: 64px;
    width: var(--open-width);
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 16px 8px 12px;
    background: var(--glass);
    border-right: 1px solid var(--glass-line);
  }

  .line {
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .item {
    position: relative;
    width: 100%;
    height: 32px;
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 10px;
    border-radius: 8px;
    background: transparent;
    color: var(--text-2);
    font-size: 13px;
    text-align: left;
  }

  .item:hover:not(:disabled) {
    background: var(--glass-hover);
    color: var(--text);
  }

  .item.on,
  .item.on:hover:not(:disabled) {
    background: var(--glass-on);
    color: var(--text);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  em {
    font-style: normal;
    font-size: 11px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }

  .sec {
    display: flex;
    align-items: center;
    padding: 14px 4px 6px 10px;
  }

  h2 {
    flex: 1;
    margin: 0;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-3);
  }

  .plus {
    width: 24px;
    height: 24px;
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 999px;
    background: transparent;
    color: var(--text-2);
  }

  .plus:hover:not(:disabled) {
    background: var(--glass-hover);
    color: var(--text);
  }

  /* Only the collections scroll; the rows above and below stay put. */
  .list {
    flex: 0 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .coll {
    position: relative;
    flex: none;
  }

  .item.two {
    height: 44px;
    padding-right: 34px;
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.25;
  }

  .text small {
    font-size: 11px;
    color: var(--text-3);
  }

  /* Up to three covers, newest in front; the ones behind sit a step up and to the right, dimmer. */
  .stack {
    position: relative;
    width: 26px;
    height: 32px;
    flex: none;
  }

  .stack img,
  .tile {
    position: absolute;
    left: calc(var(--i) * 3px);
    top: calc(2px - var(--i) * 1px);
    width: 20px;
    height: 30px;
    border-radius: 3px;
    opacity: calc(1 - var(--i) * 0.22);
  }

  .stack img {
    object-fit: cover;
    box-shadow: 0 1px 4px rgb(0 0 0 / 0.6);
  }

  .tile {
    background: var(--glass);
    border: 1px solid var(--glass-line);
  }

  .more {
    position: absolute;
    top: 8px;
    right: 6px;
    width: 28px;
    height: 28px;
    padding: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--text-2);
  }

  .more:hover:not(:disabled) {
    background: var(--glass-hover);
    color: var(--text);
  }

  .coll:not(:hover, :focus-within) .more[aria-expanded='false'] {
    opacity: 0;
  }

  input {
    width: 100%;
    height: 32px;
    padding: 0 10px;
    border-radius: 10px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
    color: var(--text);
    font-size: 13px;
  }

  input::placeholder {
    color: var(--text-3);
  }

  .new {
    flex: none;
    padding-bottom: 4px;
  }

  .rename {
    margin: 6px 0;
  }

  .confirm {
    display: grid;
    gap: 8px;
    padding: 8px 10px;
    font-size: 13px;
  }

  .confirm div {
    display: flex;
    gap: 6px;
  }

  .ask {
    color: var(--text);
  }

  .small {
    height: 28px;
    padding: 0 10px;
    border-radius: 999px;
    background: var(--glass-hover);
    border: 1px solid var(--glass-line);
    font-size: 13px;
  }

  .small:hover:not(:disabled) {
    background: var(--glass-on);
  }

  .danger {
    color: var(--danger-text);
  }

  .note {
    flex: none;
    margin: 2px 0 0;
    padding: 12px;
    border-radius: 10px;
    border: 1px solid var(--glass-line);
    font-size: 13px;
    line-height: 1.45;
    color: var(--text-2);
  }

  .problem {
    border-color: rgb(239 68 68 / 0.35);
    color: var(--error-text);
  }

  .bottom {
    margin-top: auto;
    padding-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .downloads {
    height: 40px;
    padding-bottom: 6px;
  }

  /* Overall progress; it changes with each poll, never per frame. */
  .bar {
    position: absolute;
    left: 37px;
    right: 10px;
    bottom: 5px;
    height: 3px;
    border-radius: 2px;
    background: var(--glass-hover);
    overflow: hidden;
  }

  .bar i {
    display: block;
    height: 100%;
    background: var(--accent-2);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--warn);
  }

  /* Instant, no width transition: the virtualised grid's column count follows its width. */
  .rail {
    width: var(--rail-width);
    padding: 14px 10px;
    align-items: center;
  }

  .rail .item {
    width: 44px;
    height: 44px;
    padding: 0;
    justify-content: center;
  }

  .rail .item .line {
    width: 20px;
    height: 20px;
  }

  .rail .name,
  .rail em,
  .rail .text,
  .rail .more {
    display: none;
  }

  .rail .sec {
    width: 28px;
    height: 1px;
    margin: 8px 0;
    padding: 0;
    background: var(--glass-line);
  }

  .rail .list {
    scrollbar-width: none;
  }

  .rail .list::-webkit-scrollbar {
    display: none;
  }

  .rail .stack {
    width: 30px;
    height: 36px;
  }

  .rail .stack img,
  .rail .tile {
    width: 23px;
    height: 34px;
  }

  .rail .plus {
    margin-top: 6px;
  }

  .rail .bottom {
    align-items: center;
  }

  .rail .dot {
    position: absolute;
    top: 8px;
    right: 8px;
  }

  /* Progress as a ring inside the button's edge; it changes with each poll, never per frame. */
  .ring {
    position: absolute;
    inset: 3px;
    padding: 2px;
    border-radius: 8px;
    background: conic-gradient(var(--accent-2) 0 var(--done), transparent var(--done));
    mask:
      linear-gradient(#000 0 0) content-box exclude,
      linear-gradient(#000 0 0);
    pointer-events: none;
  }

  /* Over the content, out of the layout: the grid keeps the rail's width while a field, question or failure shows. */
  .over {
    position: relative;
    z-index: 25;
    margin-right: calc(var(--rail-width) - var(--open-width));
    background: var(--surface-2);
    box-shadow: var(--shadow-modal);
  }
</style>
