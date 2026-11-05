<script lang="ts">
  import type { Snippet } from 'svelte'
  import CardView from './Card.svelte'
  import { benchDone, type Card, type Progress } from './api'

  let { games, resetKey, running, progress, onopen, onplay, oninstall, benchSpeed, header, headerHeight = 0 }: {
    games: Card[]
    /** Changes when search, filters or sort change; background reloads keep the scroll position. */
    resetKey: string
    running: string | null
    progress: Record<string, Progress>
    onopen: (key: string) => void
    onplay: (key: string) => void
    oninstall: (key: string, anchor: DOMRect) => void
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

  let el: HTMLDivElement
  let width = $state(0)
  let height = $state(0)
  let scrollTop = $state(0)

  // Only rows near the viewport exist in the DOM; one tall sizer keeps the scrollbar honest.
  const cols = $derived(Math.max(1, Math.floor((width - GAP) / (CARD_W + GAP))))
  const rows = $derived(Math.ceil(games.length / cols))
  const top = $derived(headerHeight + GAP)
  const first = $derived(Math.max(0, Math.floor((scrollTop - top) / ROW_H) - BUFFER))
  const last = $derived(Math.min(rows, Math.ceil((scrollTop - top + height) / ROW_H) + BUFFER))
  const visible = $derived(games.slice(first * cols, last * cols))
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
        benchDone(frameMs, games.length)
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
  <div class="sizer" style="height: {top + rows * ROW_H + GAP}px">
    {#if header && width > 0}
      <div class="header" style="left: {left}px; width: {span}px; height: {headerHeight}px">
        {@render header()}
      </div>
    {/if}
    {#each visible as game, i (game.key)}
      {@const index = first * cols + i}
      <div
        class="slot"
        style="transform: translate({left + (index % cols) * (CARD_W + GAP)}px, {top + Math.floor(index / cols) * ROW_H}px)"
      >
        <CardView
          {game}
          running={running === game.key}
          progress={progress[game.key]}
          {onopen}
          {onplay}
          {oninstall}
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
</style>
