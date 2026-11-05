<script lang="ts">
  import { cover, type Card, type Progress } from './api'

  let { game, running, progress, onopen }: {
    game: Card
    running: boolean
    progress: Progress | undefined
    onopen: (key: string) => void
  } = $props()

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

<button class="card" class:running class:dim={!game.installed} onclick={() => onopen(game.key)} title={game.title}>
  <span class="title">{game.title}</span>
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
  {#if running}<span class="badge">Playing</span>{/if}
  {#if progress}
    <span class="strip"><span style="width: {progress.fraction * 100}%"></span></span>
  {/if}
</button>

<style>
  .card {
    position: relative;
    width: 180px;
    height: 270px;
    padding: 0;
    overflow: hidden;
    border-radius: var(--radius);
    background: linear-gradient(160deg, var(--surface-2), var(--surface));
    box-shadow: 0 2px 8px rgb(0 0 0 / 0.35);
    transition: transform 160ms ease-out, box-shadow 160ms ease-out;
  }

  .card:hover {
    background: linear-gradient(160deg, var(--surface-2), var(--surface));
    transform: translateY(-4px) scale(1.03);
    box-shadow: 0 12px 28px rgb(0 0 0 / 0.55), 0 0 0 2px var(--accent);
  }

  .card.running {
    box-shadow: 0 0 0 3px var(--play), 0 8px 24px rgb(34 197 94 / 0.25);
  }

  .card.dim img {
    filter: saturate(0.55) brightness(0.8);
  }

  .title {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 16px;
    font-size: 16px;
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

  .badge {
    position: absolute;
    top: 8px;
    left: 8px;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--play);
    color: #04210f;
    font-size: 12px;
    font-weight: 700;
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
    background: var(--accent);
    transition: width 400ms linear;
  }
</style>
