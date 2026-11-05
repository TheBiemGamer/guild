<script lang="ts">
  import * as api from './api'
  import type { Action, Detail, Progress } from './api'

  let { gameKey, running, progress, version, onback, onerror, onchanged }: {
    gameKey: string
    running: boolean
    progress: Progress | undefined
    /** Bumped whenever the library reloads, so installed state and stats stay current. */
    version: number
    onback: () => void
    onerror: (message: string) => void
    onchanged: () => void
  } = $props()

  let game = $state<Detail | null>(null)
  let art = $state<{ hero: string | null; logo: string | null }>({ hero: null, logo: null })
  let busy = $state<string | null>(null)
  let confirming = $state(false)

  $effect(() => {
    void version
    api.game(gameKey).then((g) => (game = g))
  })

  $effect(() => {
    const key = gameKey
    art = { hero: null, logo: null }
    api.detailArt(key).then((a) => {
      if (gameKey === key) art = a
    })
  })

  const labels: Record<Action, string> = { play: 'Launching…', install: 'Installing…', uninstall: 'Uninstalling…' }

  async function act(action: Action) {
    confirming = false
    busy = labels[action]
    try {
      await api.act(action, gameKey)
    } catch (e) {
      onerror(api.message(e))
    } finally {
      busy = null
      onchanged()
    }
  }

  async function cancel() {
    busy = 'Cancelling…'
    try {
      await api.cancelInstall(gameKey)
    } catch (e) {
      onerror(api.message(e))
    }
  }

  // Steam asks in its own dialog; Legendary doesn't, so Epic asks here first.
  const uninstall = () => (game?.provider === 'epic' ? (confirming = true) : act('uninstall'))
</script>

<section class="detail">
  <header class="hero" style={art.hero ? `background-image: url('${art.hero}')` : ''}>
    <button class="back" onclick={onback}>← Library</button>
    {#if art.logo}
      <img class="logo" src={art.logo} alt={game?.title ?? ''} />
    {:else}
      <h1>{game?.title ?? ''}</h1>
    {/if}
  </header>

  {#if game}
    <div class="body">
      <div class="actions">
        {#if busy}
          <button class="big" disabled>{busy}</button>
          {#if game.provider === 'epic' && busy === 'Installing…'}
            <button onclick={cancel}>Cancel</button>
          {/if}
        {:else if confirming}
          <span>Uninstall {game.title}? Its files will be deleted.</span>
          <button class="danger" onclick={() => act('uninstall')}>Uninstall</button>
          <button onclick={() => (confirming = false)}>Cancel</button>
        {:else if !game.installed}
          <button class="big primary" onclick={() => act('install')}>Install</button>
        {:else if running}
          <button class="big play" disabled>Playing</button>
        {:else}
          <button class="big play" onclick={() => act('play')}>Play</button>
        {/if}
        {#if game.installed && !busy && !confirming}
          <button onclick={uninstall}>Uninstall</button>
        {/if}
        <span class="store">{game.provider === 'epic' ? 'Epic Games' : 'Steam'}</span>
      </div>

      {#if progress}
        <div class="progress">
          <div class="bar"><span style="width: {progress.fraction * 100}%"></span></div>
          <small>{progress.text}</small>
        </div>
      {/if}

      <dl>
        <dt>Playtime</dt>
        <dd>{game.playtime}</dd>
        {#if game.lastPlayed}
          <dt>Last played</dt>
          <dd>{game.lastPlayed.replace('Last played ', '')}</dd>
        {/if}
      </dl>
    </div>
  {:else}
    <p class="gone">This game is no longer in the library.</p>
  {/if}
</section>

<style>
  .detail {
    position: absolute;
    inset: 0;
    overflow-y: auto;
    background: var(--bg);
    animation: enter 180ms ease-out;
  }

  @keyframes enter {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }

  .hero {
    position: relative;
    height: 380px;
    padding: 24px 32px;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    align-items: flex-start;
    background: var(--surface) center / cover no-repeat;
  }

  /* Fade the hero into the page so the actions sit on calm background. */
  .hero::after {
    content: '';
    position: absolute;
    inset: 0;
    background: linear-gradient(to bottom, rgb(13 15 20 / 0.1) 40%, var(--bg));
    pointer-events: none;
  }

  .back,
  .logo,
  h1 {
    position: relative;
    z-index: 1;
  }

  .back {
    background: rgb(0 0 0 / 0.45);
    backdrop-filter: blur(8px);
  }

  .logo {
    max-width: 420px;
    max-height: 140px;
    object-fit: contain;
    filter: drop-shadow(0 4px 16px rgb(0 0 0 / 0.6));
  }

  h1 {
    margin: 0;
    font-size: 40px;
    text-shadow: 0 2px 12px rgb(0 0 0 / 0.7);
  }

  .body {
    padding: 8px 32px 32px;
    display: grid;
    gap: 20px;
    max-width: 900px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .big {
    padding: 12px 40px;
    font-size: 17px;
    font-weight: 600;
  }

  .play {
    background: var(--play);
    color: #04210f;
  }

  .play:hover:not(:disabled) {
    background: #4ade80;
  }

  .danger {
    background: var(--danger);
    color: white;
  }

  .store {
    margin-left: auto;
    color: var(--muted);
  }

  .progress {
    display: grid;
    gap: 6px;
  }

  .bar {
    height: 8px;
    border-radius: 4px;
    background: var(--surface-2);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 400ms linear;
  }

  small {
    color: var(--muted);
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 24px;
    margin: 0;
  }

  dt {
    color: var(--muted);
  }

  dd {
    margin: 0;
  }

  .gone {
    padding: 32px;
    color: var(--muted);
  }
</style>
