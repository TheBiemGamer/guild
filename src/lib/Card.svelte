<script lang="ts">
  import { untrack } from 'svelte'
  import { logoFile, storeNames } from './StoreIcon.svelte'
  import { cover, prefetch, type Card, type Cover, type Progress, type QueueState } from './api'

  let { game, index, stat = null, running, progress, queued, onopen, onplay, oninstall, oncancel, onresume, onfavorite, onmenu }: {
    game: Card
    /** Place in the grid; covers are asked in this order. */
    index: number
    /** Which hours the bottom-left badge shows; it takes Install's place on a game that isn't installed. */
    stat?: 'playtime' | 'ttb' | null
    running: boolean
    progress: Progress | undefined
    /** Where this game is in the download queue; null when it isn't. */
    queued: QueueState | null
    onopen: (key: string) => void
    /** Gets the chosen copy's key, like `oninstall`. */
    onplay: (key: string) => void
    /** `anchor` is the cover's rectangle, for a popup next to it. */
    oninstall: (key: string, anchor: DOMRect) => void
    /** Takes the waiting copy (its key) out of the queue. */
    oncancel: (key: string) => void
    onresume: () => void
    onfavorite: (key: string, on: boolean) => void
    /** A right-click: the game menu opens at the pointer. */
    onmenu: (game: Card, x: number, y: number) => void
  } = $props()

  /** "38 h played", "45 min played", "~25 h to beat"; null when never played, unknown or off. */
  const statText = $derived.by(() => {
    if (stat === 'ttb') return game.ttbHours === null ? null : `~${game.ttbHours} h to beat`
    const m = stat === 'playtime' ? game.playtimeMinutes : null
    return !m ? null : m < 60 ? `${m} min played` : `${Math.floor(m / 60)} h played`
  })

  /** Waiting in the queue, or first but paused: the hover shows the busy pill with Cancel or Resume. */
  const waiting = $derived(queued && queued.state !== 'active' ? queued : null)

  let el: HTMLDivElement
  // undefined: still asking. null: no art, so the title shows.
  let art = $state<Cover | null | undefined>(undefined)
  let loaded = $state(false)
  let portrait = $state(false)

  // The grid hands every card a fresh `game` each time a row scrolls in or out; only a new key may
  // reset the cover, or every card drops and refades its art on each row.
  const gameKey = $derived(game.key)

  $effect(() => {
    const key = gameKey
    art = undefined
    loaded = false
    // A card recycled for another game must not show the old game's cover.
    cover(key, untrack(() => index)).then((c) => {
      if (game.key === key) art = c
    })
  })

  /** The game page's data is asked once the pointer rests here this long. */
  const REST_MS = 150
  let resting: ReturnType<typeof setTimeout> | undefined
  $effect(() => () => clearTimeout(resting))
</script>

<!-- The right-click (or the menu key on the focused card) opens the game page's ⋯ menu here. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="card"
  class:running
  class:out={!game.installed}
  bind:this={el}
  oncontextmenu={(e) => {
    e.preventDefault()
    onmenu(game, e.clientX, e.clientY)
  }}
>
  <button
    class="open"
    style:background={art?.color}
    onclick={() => onopen(game.key)}
    onpointerenter={() => (resting = setTimeout(() => prefetch(game), REST_MS))}
    onpointerleave={() => clearTimeout(resting)}
    onfocus={() => prefetch(game)}
    title={game.title}
  >
    {#if art === null}
      <span class="name">{game.title}</span>
    {:else if art}
      <!-- Portrait art (Steam 2:3, Epic 3:4) fills the card; landscape headers stay whole. -->
      <img
        src={art.src}
        alt=""
        decoding="async"
        class:loaded
        class:fill={portrait}
        onload={(e) => {
          const img = e.currentTarget as HTMLImageElement
          portrait = img.naturalHeight > img.naturalWidth
          loaded = true
        }}
        onerror={() => (art = null)}
      />
    {/if}
  </button>

  <button
    class="heart"
    aria-pressed={game.favorite}
    aria-label="Favorite {game.title}"
    onclick={(e) => {
      e.stopPropagation()
      onfavorite(game.key, !game.favorite)
    }}
  >
    <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z" /></svg>
  </button>

  <span class="logos">
    {#each game.stores as store (store)}
      <span class="logo"><img src={logoFile[store]} alt={storeNames[store]} width="15" height="15" /></span>
    {/each}
  </span>

  {#if running}
    <span class="badge">Playing</span>
  {:else if waiting}
    <span class="chip">{waiting.state === 'paused' ? 'Paused' : 'Queued'}</span>
  {:else if progress}
    <span class="strip"><span style="width: {progress.fraction * 100}%"></span></span>
  {:else if game.installed && game.update}
    <span class="chip">
      <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true"><path d="M12 4v11M7 10l5 5 5-5M5 20h14" /></svg>
      Update
    </span>
  {:else if statText}
    <span class="chip stat">{statText}</span>
  {:else if !game.installed}
    <span class="chip">
      <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true"><path d="M12 4v11M7 10l5 5 5-5M5 20h14" /></svg>
      Install
    </span>
  {/if}

  {#if !running && (!progress || waiting)}
    <div class="hover">
      <b>{game.title}</b>
      {#if waiting}
        <span class="btns">
          <button class="pill small" aria-disabled="true">{waiting.state === 'paused' ? 'Paused' : 'Queued'}</button>
          {#if waiting.state === 'paused'}
            <button class="glass small" onclick={onresume}>Resume</button>
          {:else}
            <button class="glass small" onclick={() => oncancel(waiting.key)}>Cancel</button>
          {/if}
        </span>
      {:else if game.installed}
        <button class="pill small" onclick={() => onplay(game.chosen)}>
          <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path class="solid" d="M7 4.5v15l12.5-7.5z" /></svg>
          Play
        </button>
      {:else if !game.installed}
        <button class="pill small" onclick={() => oninstall(game.chosen, el.getBoundingClientRect())}>
          <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path d="M12 4v11M7 10l5 5 5-5M5 20h14" /></svg>
          Install
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .card {
    position: relative;
    width: 180px;
    height: 270px;
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: 0 4px 12px rgb(0 0 0 / 0.45);
    transition: transform 160ms ease-out, box-shadow 160ms ease-out;
  }

  .card:hover,
  .card:focus-within {
    transform: translateY(-4px) scale(1.03);
    box-shadow: 0 16px 34px rgb(0 0 0 / 0.6), 0 0 0 2px rgb(255 255 255 / 0.85);
  }

  .card.running {
    box-shadow: 0 4px 12px rgb(0 0 0 / 0.45), 0 0 0 2px var(--play);
  }

  .open {
    position: absolute;
    inset: 0;
    padding: 0;
    overflow: hidden;
    border-radius: inherit;
    /* Until the cover's own colour is known. */
    background: var(--surface-2);
  }

  .open:hover:not(:disabled) {
    background: var(--surface-2);
  }

  .open:active:not(:disabled) {
    transform: none;
  }

  /* The card itself shows focus; the outline would sit under the hover ring. */
  .open:focus-visible {
    outline: none;
  }

  .name {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 16px;
    font-size: 15px;
    font-weight: 600;
    text-align: center;
    color: var(--muted);
  }

  .open img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
    opacity: 0;
    transition: opacity 150ms ease-out;
  }

  .open img.loaded {
    opacity: 1;
  }

  .open img.fill {
    object-fit: cover;
  }

  /* Option C: not installed fades a little, and the chip says what's next. */
  .out .open img {
    filter: saturate(0.5) brightness(0.75);
  }

  /* Shown on hover or focus; a favorite keeps its filled heart. Absent otherwise, like the hover overlay. */
  .heart {
    position: absolute;
    z-index: 1;
    top: 8px;
    left: 8px;
    width: 28px;
    height: 28px;
    padding: 0;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: rgb(0 0 0 / 0.5);
    border: 1px solid var(--glass-line);
    color: #fff;
  }

  .heart:hover:not(:disabled) {
    background: rgb(0 0 0 / 0.5);
  }

  .card:not(:hover, :focus-within) .heart[aria-pressed='false'] {
    display: none;
  }

  .heart svg {
    stroke-width: 2;
  }

  .heart[aria-pressed='true'] svg {
    fill: currentColor;
  }

  /* One glass circle per store, stacked down from the corner. */
  .logos {
    position: absolute;
    top: 8px;
    right: 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    pointer-events: none;
  }

  .logo {
    width: 26px;
    height: 26px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: rgb(10 10 16 / 0.62);
    border: 1px solid rgb(255 255 255 / 0.12);
  }

  .logo img {
    display: block;
  }

  .chip,
  .badge {
    position: absolute;
    left: 8px;
    bottom: 8px;
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 4px 9px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 600;
    pointer-events: none;
  }

  .chip {
    background: rgb(10 10 16 / 0.68);
    border: 1px solid rgb(255 255 255 / 0.12);
    color: #fff;
  }

  .stat {
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }

  .badge {
    background: var(--play);
    color: #04210f;
  }

  svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 2.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .solid {
    fill: currentColor;
    stroke: none;
  }

  .hover {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    align-items: flex-start;
    gap: 8px;
    padding: 10px;
    border-radius: inherit;
    background: linear-gradient(180deg, transparent 45%, rgb(0 0 0 / 0.85));
    pointer-events: none;
    transition: opacity 140ms ease-out;
  }

  /* Absent (not just transparent) until hovered: idle cards stay cheap for the scroller to paint. */
  .card:not(:hover, :focus-within) .hover {
    display: none;
  }

  @starting-style {
    .card:hover .hover,
    .card:focus-within .hover {
      opacity: 0;
    }
  }

  .card:hover .chip,
  .card:focus-within .chip {
    opacity: 0;
  }

  .hover b {
    max-width: 100%;
    font-size: 13px;
    line-height: 1.3;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    text-shadow: 0 1px 6px rgb(0 0 0 / 0.6);
  }

  .hover .pill,
  .hover .glass {
    pointer-events: auto;
  }

  .btns {
    display: flex;
    gap: 6px;
  }

  .glass.small {
    padding: 6px 14px;
    border-radius: 999px;
    font-size: 12px;
  }

  .strip {
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: 8px;
    height: 6px;
    border-radius: 3px;
    background: rgb(0 0 0 / 0.6);
    overflow: hidden;
  }

  .strip span {
    display: block;
    height: 100%;
    background: var(--text);
    transition: width 400ms linear;
  }
</style>
