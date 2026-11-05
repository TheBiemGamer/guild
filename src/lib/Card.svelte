<script lang="ts">
  import StoreIcon from './StoreIcon.svelte'
  import { cover, type Card, type Progress } from './api'

  let { game, running, progress, onopen, onplay, oninstall }: {
    game: Card
    running: boolean
    progress: Progress | undefined
    onopen: (key: string) => void
    onplay: (key: string) => void
    /** `anchor` is the cover's rectangle, for a popup next to it. */
    oninstall: (key: string, anchor: DOMRect) => void
  } = $props()

  let el: HTMLDivElement
  let src = $state<string | null>(null)
  let loaded = $state(false)
  let portrait = $state(false)

  $effect(() => {
    const key = game.key
    src = null
    loaded = false
    // A card recycled for another game must not show the old game's cover.
    cover(key).then((url) => {
      if (game.key === key) src = url
    })
  })
</script>

<div class="card" class:running class:out={!game.installed} bind:this={el}>
  <button class="open" onclick={() => onopen(game.key)} title={game.title}>
    <span class="name">{game.title}</span>
    {#if src}
      <!-- Portrait art (Steam 2:3, Epic 3:4) fills the card; landscape headers stay whole. -->
      <img
        {src}
        alt=""
        decoding="async"
        class:loaded
        class:fill={portrait}
        onload={(e) => {
          const img = e.currentTarget as HTMLImageElement
          portrait = img.naturalHeight > img.naturalWidth
          loaded = true
        }}
      />
    {/if}
  </button>

  <span class="logo"><StoreIcon provider={game.provider} size={15} /></span>

  {#if running}
    <span class="badge">Playing</span>
  {:else if progress}
    <span class="strip"><span style="width: {progress.fraction * 100}%"></span></span>
  {:else if !game.installed}
    <span class="chip">
      <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true"><path d="M12 4v11M7 10l5 5 5-5M5 20h14" /></svg>
      Install
    </span>
  {/if}

  {#if !running && !progress}
    <div class="hover">
      <b>{game.title}</b>
      {#if game.installed}
        <button class="pill small" onclick={() => onplay(game.key)}>
          <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path class="solid" d="M7 4.5v15l12.5-7.5z" /></svg>
          Play
        </button>
      {:else}
        <button class="pill small" onclick={() => oninstall(game.key, el.getBoundingClientRect())}>
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
    background: linear-gradient(160deg, var(--surface-2), var(--surface));
  }

  .open:hover:not(:disabled) {
    background: linear-gradient(160deg, var(--surface-2), var(--surface));
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

  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
    opacity: 0;
    transition: opacity 200ms;
  }

  img.loaded {
    opacity: 1;
  }

  img.fill {
    object-fit: cover;
  }

  /* Option C: not installed fades a little, and the chip says what's next. */
  .out img {
    filter: saturate(0.5) brightness(0.75);
  }

  .logo {
    position: absolute;
    top: 8px;
    right: 8px;
    width: 26px;
    height: 26px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: rgb(10 10 16 / 0.62);
    border: 1px solid rgb(255 255 255 / 0.12);
    color: #fff;
    pointer-events: none;
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

  .hover .pill {
    pointer-events: auto;
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
