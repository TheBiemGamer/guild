<script lang="ts">
  import * as api from './api'
  import type { Card, Progress, QueuedAction, QueueView } from './api'
  import { popover } from './popover'
  import StoreIcon from './StoreIcon.svelte'

  let { progress, queue, games, anchor, onclose, oncancel, onpause, onresume, onmove }: {
    /** `poll`'s map, by copy key. */
    progress: Record<string, Progress>
    /** The Epic download queue; its first job is the active one. */
    queue: QueueView
    /** The library as loaded, for titles; a game hidden by the search is asked for once. */
    games: Card[]
    anchor: HTMLElement | undefined
    onclose: (refocus: boolean) => void
    oncancel: (key: string) => void
    onpause: () => void
    onresume: () => void
    onmove: (key: string, up: boolean) => void
  } = $props()

  let asked = $state<Record<string, string>>({})
  const known = $derived(new Map(games.flatMap((g) => g.copyKeys.map((k) => [k, g.title] as const))))

  $effect(() => {
    for (const key of Object.keys(progress)) {
      if (known.has(key) || key in asked) continue
      asked[key] = ''
      api.game(key).then(
        (d) => (asked[key] = d?.title ?? ''),
        () => {},
      )
    }
  })

  const queued = $derived(new Map(queue.jobs.map((j) => [j.key, j.title] as const)))
  const title = (key: string) => known.get(key) || queued.get(key) || asked[key] || 'A game'
  const active = $derived(queue.jobs[0])
  const next = $derived(queue.jobs.slice(1))
  const jobs = $derived(Object.entries(progress))
  // Epic jobs outside the queue (Move) show their progress only; Steam's run in Steam.
  const other = $derived(jobs.filter(([k]) => k.startsWith('epic-') && k !== active?.key))
  const steam = $derived(jobs.filter(([k]) => !k.startsWith('epic-')))
  const running: Record<QueuedAction, string> = { install: 'Installing…', update: 'Updating…', verify: 'Verifying…' }
  const actionName: Record<QueuedAction, string> = { install: 'Install', update: 'Update', verify: 'Verify' }

  function activeText(p: Progress | undefined): string {
    if (queue.paused) return p ? `Paused · ${Math.round(p.fraction * 100)}%` : 'Paused'
    return p?.text ?? running[active.action]
  }
</script>

{#snippet bar(key: string, p: Progress | undefined)}
  <div class="bar" role="progressbar" aria-label="{title(key)} progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round((p?.fraction ?? 0) * 100)}>
    <span style="width: {(p?.fraction ?? 0) * 100}%"></span>
  </div>
{/snippet}

{#snippet job(key: string, p: Progress, inSteam: boolean)}
  <div class="job">
    <StoreIcon provider={inSteam ? 'steam' : 'epic'} size={16} />
    <div class="what">
      <b>{title(key)}</b>
      <small>{p.text}</small>
      {@render bar(key, p)}
    </div>
  </div>
{/snippet}

<div class="pop" role="dialog" aria-label="Downloads" tabindex="-1" use:popover={{ anchor, close: onclose }}>
  {#if active || other.length}
    <h3>Downloading</h3>
    {#if active}
      <div class="job">
        <StoreIcon provider="epic" size={16} />
        <div class="what">
          <b>{title(active.key)}</b>
          <small>{activeText(progress[active.key])}</small>
          {@render bar(active.key, progress[active.key])}
        </div>
        {#if queue.paused}
          <button class="glass icon" aria-label="Resume" title="Resume" onclick={onresume}>
            <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path class="solid" d="M7 4.5v15l12.5-7.5z" /></svg>
          </button>
        {:else}
          <button class="glass icon" aria-label="Pause" title="Pause" onclick={onpause}>
            <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path class="solid" d="M6 4h4v16H6zM14 4h4v16h-4z" /></svg>
          </button>
        {/if}
        <button class="glass" onclick={() => oncancel(active.key)}>Cancel</button>
      </div>
    {/if}
    {#each other as [key, p] (key)}{@render job(key, p, false)}{/each}
  {/if}
  {#if next.length}
    <h3>Up next</h3>
    {#each next as j, i (`${j.key}|${j.action}`)}
      <div class="job">
        <StoreIcon provider="epic" size={16} />
        <div class="what">
          <b>{title(j.key)}</b>
          <small>{actionName[j.action]}</small>
        </div>
        <button class="glass icon" aria-label="Move {title(j.key)} up" disabled={i === 0} onclick={() => onmove(j.key, true)}>
          <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path d="M12 19V5M6 11l6-6 6 6" /></svg>
        </button>
        <button class="glass icon" aria-label="Move {title(j.key)} down" disabled={i === next.length - 1} onclick={() => onmove(j.key, false)}>
          <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path d="M12 5v14M6 13l6 6 6-6" /></svg>
        </button>
        <button class="glass icon" aria-label="Remove {title(j.key)} from queue" onclick={() => oncancel(j.key)}>
          <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
        </button>
      </div>
    {/each}
  {/if}
  {#if steam.length}
    <h3>In Steam</h3>
    {#each steam as [key, p] (key)}{@render job(key, p, true)}{/each}
  {/if}
</div>

<style>
  .pop {
    position: absolute;
    z-index: 6;
    top: 44px;
    right: 0;
    width: 380px;
    padding: 6px;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: 0 30px 80px rgb(0 0 0 / 0.6);
  }

  .pop:focus-visible {
    outline: none;
  }

  h3 {
    margin: 8px 10px 0;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-3);
  }

  .job {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px;
  }

  .job + .job {
    border-top: 1px solid var(--glass-line);
  }

  .what {
    flex: 1;
    min-width: 0;
  }

  b {
    display: block;
    font-weight: 600;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  small {
    display: block;
    font-size: 12px;
    color: var(--text-3);
  }

  .bar {
    height: 6px;
    margin-top: 6px;
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

  .glass {
    flex: none;
    height: 32px;
    padding: 0 14px;
    border-radius: 999px;
  }

  .glass.icon {
    width: 32px;
    padding: 0;
    display: grid;
    place-items: center;
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
</style>
