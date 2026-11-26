<script lang="ts">
  import { Popover } from 'bits-ui'
  import type { Snippet } from 'svelte'
  import { filterGroups, filterName, toggleFilter, type Filter } from './filters'
  import { t } from './i18n'
  import { keepOnBackForward } from './outside'

  let {
    filters = $bindable(),
    multiStore,
    open = $bindable(),
    trigger,
  }: {
    filters: Filter[]
    /** The library uses more than one store: the Has group shows. */
    multiStore: boolean
    open: boolean
    /** The Filters chip: spread `props` on it. */
    trigger: Snippet<[{ props: Record<string, unknown> }]>
  } = $props()

  const groups = $derived(filterGroups(multiStore))
</script>

<Popover.Root bind:open>
  <Popover.Trigger>
    {#snippet child({ props })}
      {@render trigger({ props })}
    {/snippet}
  </Popover.Trigger>
  <Popover.Portal>
    <Popover.Content align="start" sideOffset={8} collisionPadding={8} onInteractOutside={keepOnBackForward}>
      {#snippet child({ props, wrapperProps })}
        <div {...wrapperProps}>
          <div {...props} class="pop" role="dialog" aria-label={t('filters.title')}>
            {#each groups as group, i (group.label)}
              <h3 id="filter-group-{i}">{group.label}</h3>
              <div class="toggles" role="group" aria-labelledby="filter-group-{i}">
                {#each group.filters as f (f)}
                  <button aria-pressed={filters.includes(f)} onclick={() => (filters = toggleFilter(filters, f))}>{filterName(f)}</button>
                {/each}
              </div>
            {/each}
            <div class="foot">
              <button class="link" onclick={() => (filters = [])}>{t('filters.clear')}</button>
            </div>
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
    width: 360px;
    padding: 12px;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: var(--shadow-modal);
  }

  .pop:focus-visible {
    outline: none;
  }

  h3 {
    margin: 10px 4px 6px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-3);
  }

  h3:first-child {
    margin-top: 2px;
  }

  .toggles {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .toggles button {
    max-width: 100%;
    height: 30px;
    padding: 0 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    border-radius: 999px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
    color: var(--text-2);
    font-size: 13px;
  }

  .toggles button:hover:not(:disabled) {
    background: var(--glass-hover);
    color: var(--text);
  }

  .toggles button[aria-pressed='true'] {
    background: var(--glass-on);
    border-color: var(--glass-on-line);
    color: var(--text);
  }

  .foot {
    display: flex;
    justify-content: space-between;
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid var(--glass-line);
  }

  .link {
    padding: 4px 0;
    background: transparent;
    color: var(--text-2);
    font-size: 13px;
  }

  .link:hover:not(:disabled) {
    background: transparent;
    color: var(--text);
  }
</style>
