<script lang="ts">
  import type { Featured } from './api'

  let { featured, running, onplay, onopen }: {
    featured: Featured
    running: boolean
    onplay: (key: string) => void
    onopen: (key: string) => void
  } = $props()

  let loaded = $state<string | null>(null)
</script>

<section class="continue" aria-label="Continue playing">
  {#if featured.hero}
    <img src={featured.hero} alt="" class:loaded={loaded === featured.hero} onload={() => (loaded = featured.hero)} />
  {/if}
  <button class="open" onclick={() => onopen(featured.key)} aria-label="Open {featured.title}"></button>
  <div class="text">
    <small>Continue playing</small>
    <h2>{featured.title}</h2>
    <button class="pill" disabled={running} onclick={() => onplay(featured.key)}>
      {#if running}
        Playing
      {:else}
        <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path d="M7 4.5v15l12.5-7.5z" /></svg>
        Play
      {/if}
    </button>
  </div>
</section>

<style>
  .continue {
    position: relative;
    height: 100%;
    overflow: hidden;
    border-radius: 14px;
    background: var(--glass-strong);
    box-shadow: 0 10px 30px rgb(0 0 0 / 0.45);
  }

  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: center 30%;
    opacity: 0;
    transition: opacity 300ms ease-out;
  }

  img.loaded {
    opacity: 1;
  }

  /* Darkens the left side so the title reads on any art. */
  .open {
    position: absolute;
    inset: 0;
    padding: 0;
    border-radius: 0;
    background: linear-gradient(90deg, rgb(10 10 16 / 0.85) 0%, rgb(10 10 16 / 0.35) 45%, rgb(10 10 16 / 0.05) 75%);
  }

  .open:hover:not(:disabled) {
    background: linear-gradient(90deg, rgb(10 10 16 / 0.8) 0%, rgb(10 10 16 / 0.25) 45%, transparent 75%);
  }

  .open:active:not(:disabled) {
    transform: none;
  }

  .open:focus-visible {
    outline-offset: -3px;
    border-radius: 14px;
  }

  .text {
    position: absolute;
    left: 24px;
    bottom: 20px;
    right: 24px;
    pointer-events: none;
  }

  small {
    color: var(--text-2);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-size: 11px;
    font-weight: 600;
  }

  h2 {
    margin: 2px 0 12px;
    font-size: 26px;
    font-weight: 700;
    line-height: 1.15;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    text-shadow: 0 1px 12px rgb(0 0 0 / 0.5);
  }

  .pill {
    pointer-events: auto;
    padding: 9px 22px;
  }

  svg {
    fill: currentColor;
  }
</style>
