<script lang="ts">
  import { Popover } from 'bits-ui'
  import type { Snippet } from 'svelte'
  import { commands, type Card, type Progress, type QueueView, type QueuedAction, type Store } from './bindings'
  import { progressShort } from './format'
  import { t } from './i18n'
  import { keepOnBackForward } from './outside'
  import StoreIcon from './StoreIcon.svelte'
  import Tip from './Tip.svelte'

  let {
    progress,
    queue,
    games,
    open = $bindable(),
    trigger,
    oncancel,
    onpause,
    onresume,
    onmove,
  }: {
    /** `poll`'s map, by copy key. */
    progress: Record<string, Progress>
    /** The Epic, GOG and Amazon download queue; its first job is the active one. */
    queue: QueueView
    /** The library as loaded, for titles; a game hidden by the search is asked for once. */
    games: Card[]
    open: boolean
    /** The Downloads chip: spread `props` on it. */
    trigger: Snippet<[{ props: Record<string, unknown> }]>
    oncancel: (key: string) => void
    onpause: () => void
    onresume: () => void
    onmove: (key: string, up: boolean) => void
  } = $props()

  let asked = $state<Record<string, string>>({})
  const known = $derived(new Map(games.flatMap((g) => g.copyKeys.map((k) => [k, g.title] as const))))

  $effect(() => {
    if (!open) return
    for (const key of Object.keys(progress)) {
      if (known.has(key) || key in asked) continue
      asked[key] = ''
      commands.game(key).then(
        (d) => (asked[key] = d?.title ?? ''),
        () => {},
      )
    }
  })

  const queued = $derived(new Map(queue.jobs.map((j) => [j.key, j.title] as const)))
  const title = (key: string) => known.get(key) || queued.get(key) || asked[key] || t('downloads.aGame')
  const active = $derived(queue.jobs[0])
  const next = $derived(queue.jobs.slice(1))
  const jobs = $derived(Object.entries(progress))
  const storeOf = (key: string): Store => (key.startsWith('gog-') ? 'gog' : key.startsWith('epic-') ? 'epic' : key.startsWith('amazon-') ? 'amazon' : 'steam')
  // Epic and GOG jobs outside the queue (Move, a GOG DLC uninstall) show their progress only; Steam's run in Steam.
  const other = $derived(jobs.filter(([k]) => storeOf(k) !== 'steam' && k !== active?.key))
  const steam = $derived(jobs.filter(([k]) => storeOf(k) === 'steam'))
  const running = (action: QueuedAction) => t(`queue.running.${action}`)
  const actionName = (action: QueuedAction) => t(`queue.action.${action}`)

  function activeText(p: Progress | undefined): string {
    if (queue.paused) return p ? t('downloads.pausedAt', { percent: `${Math.round(p.fraction * 100)}%` }) : t('downloads.paused')
    return p ? progressShort(p) : running(active.action)
  }
</script>

{#snippet fill(key: string, p: Progress | undefined, paused = false)}
  {@const pct = Math.round((p?.fraction ?? 0) * 100)}
  <span
    class="fill"
    class:paused
    style="width: {pct}%"
    role="progressbar"
    aria-label={t('downloads.progress', { title: title(key) })}
    aria-valuemin={0}
    aria-valuemax={100}
    aria-valuenow={pct}
  ></span>
{/snippet}

{#snippet job(key: string, p: Progress)}
  <div class="job">
    {@render fill(key, p)}
    <StoreIcon provider={storeOf(key)} size={16} />
    <div class="what">
      <b>{title(key)}</b>
      <small>{progressShort(p)}</small>
    </div>
  </div>
{/snippet}

<Popover.Root bind:open>
  <Popover.Trigger>
    {#snippet child({ props })}
      {@render trigger({ props })}
    {/snippet}
  </Popover.Trigger>
  <Popover.Portal>
    <Popover.Content align="end" sideOffset={8} collisionPadding={8} onInteractOutside={keepOnBackForward}>
      {#snippet child({ props, wrapperProps })}
        <div {...wrapperProps}>
          <div {...props} class="pop" role="dialog" aria-label={t('downloads.title')}>
            {#if active}
              <div class="job">
                {@render fill(active.key, progress[active.key], queue.paused)}
                <StoreIcon provider={storeOf(active.key)} size={16} />
                <div class="what">
                  <b>{title(active.key)}</b>
                  <small>{activeText(progress[active.key])}</small>
                </div>
                {#if queue.paused}
                  <Tip text={t('common.resume')}>
                    {#snippet children({ props })}
                      <button {...props} class="glass icon" aria-label={t('common.resume')} onclick={onresume}>
                        <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path class="solid" d="M7 4.5v15l12.5-7.5z" /></svg>
                      </button>
                    {/snippet}
                  </Tip>
                {:else}
                  <Tip text={t('common.pause')}>
                    {#snippet children({ props })}
                      <button {...props} class="glass icon" aria-label={t('common.pause')} onclick={onpause}>
                        <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path class="solid" d="M6 4h4v16H6zM14 4h4v16h-4z" /></svg>
                      </button>
                    {/snippet}
                  </Tip>
                {/if}
                <Tip text={t('common.cancel')}>
                  {#snippet children({ props })}
                    <button
                      {...props}
                      class="glass icon"
                      aria-label={t('downloads.cancelGame', { title: title(active.key) })}
                      onclick={() => oncancel(active.key)}
                    >
                      <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
                    </button>
                  {/snippet}
                </Tip>
              </div>
            {/if}
            {#each other as [key, p] (key)}{@render job(key, p)}{/each}
            {#each steam as [key, p] (key)}{@render job(key, p)}{/each}
            {#if next.length}
              <h3>{t('downloads.upNext')}</h3>
              {#each next as j, i (`${j.key}|${j.action}`)}
                <div class="job">
                  <StoreIcon provider={storeOf(j.key)} size={16} />
                  <div class="what">
                    <b>{title(j.key)}</b>
                    <small class="quiet">{actionName(j.action)}</small>
                  </div>
                  <button class="glass icon" aria-label={t('downloads.moveUp', { title: title(j.key) })} disabled={i === 0} onclick={() => onmove(j.key, true)}>
                    <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path d="M12 19V5M6 11l6-6 6 6" /></svg>
                  </button>
                  <button
                    class="glass icon"
                    aria-label={t('downloads.moveDown', { title: title(j.key) })}
                    disabled={i === next.length - 1}
                    onclick={() => onmove(j.key, false)}
                  >
                    <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path d="M12 5v14M6 13l6 6 6-6" /></svg>
                  </button>
                  <button class="glass icon" aria-label={t('downloads.remove', { title: title(j.key) })} onclick={() => oncancel(j.key)}>
                    <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
                  </button>
                </div>
              {/each}
            {/if}
          </div>
        </div>
      {/snippet}
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>

<style>
  .pop {
    /* bits-ui copies this onto its portal wrapper: above the Settings backdrop (20) and every popup (up to 40). */
    position: relative;
    z-index: 50;
    width: 380px;
    padding: 6px;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: var(--shadow-modal);
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
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 8px;
    overflow: hidden;
  }

  .job + .job {
    margin-top: 2px;
  }

  /* The row is the progress bar: a faint fill with a 2px line along its bottom edge. */
  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    background: rgb(124 92 255 / 0.18);
    border-bottom: 2px solid var(--accent);
    transition: width 400ms linear;
    pointer-events: none;
  }

  .fill.paused {
    background: rgb(255 255 255 / 0.08);
    border-bottom-color: var(--text-3);
  }

  .job > :not(.fill) {
    position: relative;
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
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  small.quiet {
    color: var(--text-3);
  }

  .glass.icon {
    flex: none;
    width: 28px;
    height: 28px;
    padding: 0;
    border-radius: 999px;
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
