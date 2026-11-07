<script lang="ts">
  import * as api from './api'
  import type { Card, Progress } from './api'
  import { popover } from './popover'
  import StoreIcon from './StoreIcon.svelte'

  // shortcut: the installs and updates running now, from `poll`; Task 24's queue adds Pause and "Up next".
  let { progress, games, anchor, onclose, oncancel }: {
    /** `poll`'s map, by copy key. */
    progress: Record<string, Progress>
    /** The library as loaded, for titles; a game hidden by the search is asked for once. */
    games: Card[]
    anchor: HTMLElement | undefined
    onclose: (refocus: boolean) => void
    oncancel: (key: string) => void
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

  const title = (key: string) => known.get(key) || asked[key] || 'A game'
  const jobs = $derived(Object.entries(progress))
  // Legendary's installs are ours to cancel; Steam's run in Steam.
  const ours = $derived(jobs.filter(([k]) => k.startsWith('epic-')))
  const steam = $derived(jobs.filter(([k]) => !k.startsWith('epic-')))
</script>

{#snippet job(key: string, p: Progress, cancel: boolean)}
  <div class="job">
    <StoreIcon provider={key.startsWith('epic-') ? 'epic' : 'steam'} size={16} />
    <div class="what">
      <b>{title(key)}</b>
      <small>{cancel ? p.text : `In Steam · ${p.text}`}</small>
      <div class="bar" role="progressbar" aria-label="{title(key)} progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(p.fraction * 100)}>
        <span style="width: {p.fraction * 100}%"></span>
      </div>
    </div>
    {#if cancel}<button class="glass" onclick={() => oncancel(key)}>Cancel</button>{/if}
  </div>
{/snippet}

<div class="pop" role="dialog" aria-label="Downloads" tabindex="-1" use:popover={{ anchor, close: onclose }}>
  {#if ours.length}
    <h3>Downloading</h3>
    {#each ours as [key, p] (key)}{@render job(key, p, true)}{/each}
  {/if}
  {#if steam.length}
    <h3>In Steam</h3>
    {#each steam as [key, p] (key)}{@render job(key, p, false)}{/each}
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
</style>
