<script lang="ts">
  import { filterGroups, filterNames, toggleFilter, type Filter } from './filters'
  import { popover } from './popover'

  let { filters = $bindable(), anchor, onclose }: {
    filters: Filter[]
    /** The Filters chip: clicks on it toggle instead of closing from outside. */
    anchor: HTMLElement | undefined
    onclose: (refocus: boolean) => void
  } = $props()
</script>

<div class="pop" role="dialog" aria-label="Filters" tabindex="-1" use:popover={{ anchor, close: onclose }}>
  {#each filterGroups as group (group.label)}
    <h3 id="filter-{group.label}">{group.label}</h3>
    <div class="toggles" role="group" aria-labelledby="filter-{group.label}">
      {#each group.filters as f (f)}
        <button aria-pressed={filters.includes(f)} onclick={() => (filters = toggleFilter(filters, f))}>{filterNames[f]}</button>
      {/each}
    </div>
  {/each}
  <!-- The right slot is for Collections / "Save as collection" later. -->
  <div class="foot">
    <button class="link" onclick={() => (filters = [])}>Clear</button>
  </div>
</div>

<style>
  .pop {
    position: absolute;
    z-index: 6;
    top: 44px;
    left: 0;
    width: 360px;
    padding: 12px;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: 0 30px 80px rgb(0 0 0 / 0.6);
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
    height: 30px;
    padding: 0 12px;
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
