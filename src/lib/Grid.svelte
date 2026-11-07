<script lang="ts">
  import type { Snippet } from 'svelte'
  import CardView from './Card.svelte'
  import { queueState } from './api'
  import { commands, type Card, type Progress, type QueueView, type Section } from './bindings'
  import { groupLabel } from './format'

  let { games, sections = null, stat = null, resetKey, running, progress, queue, onopen, onplay, oninstall, oncancel, onresume, onfavorite, onmenu, benchSpeed, header, headerHeight = 0 }: {
    games: Card[]
    /** Labelled runs of `games`, each starting on its own row under a label row; null for one plain run. */
    sections?: Section[] | null
    /** The hours each card's badge shows, if any. */
    stat?: 'playtime' | 'ttb' | null
    /** Changes when search, filters or sort change; background reloads keep the scroll position. */
    resetKey: string
    /** Copy keys, as `poll` reports them. */
    running: string | null
    progress: Record<string, Progress>
    queue: QueueView
    onopen: (key: string) => void
    onplay: (key: string) => void
    oninstall: (key: string, anchor: DOMRect) => void
    /** Takes a waiting game out of the download queue, by copy key. */
    oncancel: (key: string) => void
    onresume: () => void
    onfavorite: (key: string, on: boolean) => void
    /** A card's right-click, at the pointer. */
    onmenu: (game: Card, x: number, y: number) => void
    /** px/s; when set, scroll to the bottom once and report frame times (LAUNCHER_BENCH). */
    benchSpeed: number | null
    /** Scrolls with the covers, above them, as wide as the columns. */
    header?: Snippet
    headerHeight?: number
  } = $props()

  const CARD_W = 180
  const CARD_H = 270
  const GAP = 16
  const ROW_H = CARD_H + GAP
  /** Off-screen rows rendered on each side, so fast scrolling doesn't show gaps. */
  const BUFFER = 2
  /** A group label row: the 18px label, then the card gap. */
  const LABEL_H = 18
  /** Extra space above every group label but the first: with the row gap, 24px. */
  const SECTION_GAP = 8

  let el: HTMLDivElement
  let width = $state(0)
  let height = $state(0)
  let scrollTop = $state(0)

  // Only rows near the viewport exist in the DOM; one tall sizer keeps the scrollbar honest.
  const cols = $derived(Math.max(1, Math.floor((width - GAP) / (CARD_W + GAP))))

  /** A row of cards (`games[start..start + count]`), or a group label (`label` set). */
  interface Row { y: number; start: number; count: number; label?: string }

  // Groups flatten into label rows and card rows. Rebuilt when the list or the width changes, not on scroll.
  const layout = $derived.by(() => {
    const rows: Row[] = []
    const runs = sections ?? [{ group: undefined, count: games.length }]
    let y = sections ? headerHeight : headerHeight + GAP
    let start = 0
    runs.forEach((run, i) => {
      if (run.group !== undefined) {
        if (i > 0) y += SECTION_GAP
        rows.push({ y, start, count: run.count, label: groupLabel(run.group) })
        y += LABEL_H + GAP
      }
      for (let at = 0; at < run.count; at += cols) {
        rows.push({ y, start: start + at, count: Math.min(cols, run.count - at) })
        y += ROW_H
      }
      start += run.count
    })
    return { rows, height: y + GAP }
  })

  /** The first row whose bottom is below `y`. */
  function rowAt(y: number) {
    const rows = layout.rows
    let lo = 0
    let hi = rows.length
    while (lo < hi) {
      const mid = (lo + hi) >> 1
      if (rows[mid].y + ROW_H <= y) lo = mid + 1
      else hi = mid
    }
    return lo
  }

  const first = $derived(Math.max(0, rowAt(scrollTop) - BUFFER))
  const last = $derived(Math.min(layout.rows.length, rowAt(scrollTop + height) + 1 + BUFFER))
  const visibleRows = $derived(layout.rows.slice(first, last))
  const labels = $derived(visibleRows.filter((r) => r.label !== undefined))
  const visible = $derived(
    visibleRows.flatMap((r) =>
      r.label !== undefined ? [] : games.slice(r.start, r.start + r.count).map((game, col) => ({ game, index: r.start + col, col, y: r.y })),
    ),
  )
  const left = $derived(Math.max(GAP, (width - cols * (CARD_W + GAP) + GAP) / 2))
  const span = $derived(cols * (CARD_W + GAP) - GAP)

  // A new search, filter or sort starts at the top.
  $effect(() => {
    void resetKey
    if (el) el.scrollTop = 0
  })

  $effect(() => {
    if (!benchSpeed || !el || games.length === 0) return
    const speed = benchSpeed
    const frameMs: number[] = []
    let lastAt: number | null = null
    let frame = requestAnimationFrame(function step(at) {
      if (lastAt !== null) frameMs.push(at - lastAt)
      const dt = lastAt === null ? 0 : (at - lastAt) / 1000
      lastAt = at
      el.scrollTop += Math.max(1, speed * dt)
      if (el.scrollTop + el.clientHeight >= el.scrollHeight - 1 && frameMs.length > 0) {
        commands.benchDone(frameMs, games.length)
        return
      }
      frame = requestAnimationFrame(step)
    })
    return () => cancelAnimationFrame(frame)
  })
</script>

<div
  class="grid"
  bind:this={el}
  bind:clientWidth={width}
  bind:clientHeight={height}
  onscroll={() => (scrollTop = el.scrollTop)}
>
  <div class="sizer" style="height: {layout.height}px">
    {#if header && width > 0}
      <div class="header" style="left: {left}px; width: {span}px; height: {headerHeight}px">
        {@render header()}
      </div>
    {/if}
    {#each labels as row (row.label)}
      <h2 class="label slot" style="transform: translate({left}px, {row.y}px); width: {span}px">
        {row.label}<span>{row.count}</span>
      </h2>
    {/each}
    {#each visible as { game, index, col, y } (game.key)}
      <div class="slot" style="transform: translate({left + col * (CARD_W + GAP)}px, {y}px)">
        <CardView
          {game}
          {index}
          {stat}
          running={running !== null && game.copyKeys.includes(running)}
          progress={game.copyKeys.map((k) => progress[k]).find(Boolean)}
          queued={queue.jobs.length ? queueState(queue, game.copyKeys) : null}
          {onopen}
          {onplay}
          {oninstall}
          {oncancel}
          {onresume}
          {onfavorite}
          {onmenu}
        />
      </div>
    {/each}
  </div>
</div>

<style>
  .grid {
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .sizer {
    position: relative;
  }

  .header {
    position: absolute;
    top: 0;
  }

  .slot {
    position: absolute;
    top: 0;
    left: 0;
  }

  /* The section label style, as App's "Library" label. */
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
</style>
