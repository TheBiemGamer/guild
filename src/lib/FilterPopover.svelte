<script lang="ts">
  import { tick } from 'svelte'
  import * as api from './api'
  import { collectionFilter, filterGroups, filterName, toggleFilter, type Collection, type Filter } from './filters'
  import { popover } from './popover'

  let { filters = $bindable(), collections, anchor, onclose, onchanged }: {
    filters: Filter[]
    collections: Collection[]
    /** The Filters chip: clicks on it toggle instead of closing from outside. */
    anchor: HTMLElement | undefined
    onclose: (refocus: boolean) => void
    /** After a collection was renamed or deleted. */
    onchanged: () => void
  } = $props()

  const groups = $derived(filterGroups(collections))

  let managing = $state(false)
  let manageButton = $state<HTMLButtonElement>()
  let renaming = $state<number | null>(null)
  let draft = $state('')
  let deleting = $state<number | null>(null)
  /** Why the last rename or delete failed. */
  let problem = $state<string | null>(null)
  let panel = $state<HTMLDivElement>()

  async function focusIn(selector: string) {
    await tick()
    panel?.querySelector<HTMLElement>(selector)?.focus()
  }

  function startRename(c: Collection) {
    renaming = c.id
    draft = c.name
    problem = null
    focusIn('input')
  }

  async function saveRename(c: Collection) {
    if (renaming !== c.id) return
    renaming = null
    if (draft.trim() === c.name) return
    try {
      await api.renameCollection(c.id, draft)
      problem = null
    } catch (e) {
      problem = api.message(e)
    }
    onchanged()
  }

  function renameKeydown(e: KeyboardEvent, c: Collection) {
    if (e.key === 'Enter') saveRename(c)
    else if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      renaming = null
      focusIn(`[data-name="${c.id}"]`)
    }
  }

  function askDelete(c: Collection) {
    deleting = c.id
    problem = null
    focusIn('.cancel')
  }

  function keep(c: Collection) {
    deleting = null
    focusIn(`[data-delete="${c.id}"]`)
  }

  async function remove(c: Collection) {
    deleting = null
    try {
      await api.deleteCollection(c.id)
      filters = filters.filter((f) => f !== collectionFilter(c.id))
    } catch (e) {
      problem = api.message(e)
    }
    manageButton?.focus()
    onchanged()
  }
</script>

<div class="pop" role="dialog" aria-label="Filters" tabindex="-1" use:popover={{ anchor, close: onclose }}>
  {#each groups as group (group.label)}
    <h3 id="filter-{group.label}">{group.label}</h3>
    <div class="toggles" role="group" aria-labelledby="filter-{group.label}">
      {#each group.filters as f (f)}
        <button aria-pressed={filters.includes(f)} onclick={() => (filters = toggleFilter(filters, f))}>{filterName(f, collections)}</button>
      {/each}
    </div>
  {/each}
  <div class="foot">
    <button class="link" onclick={() => (filters = [])}>Clear</button>
    <button class="link" bind:this={manageButton} aria-expanded={managing} aria-controls="manage-collections" onclick={() => (managing = !managing)}>
      Manage collections…
    </button>
  </div>
  {#if managing}
    <div class="manage" id="manage-collections" bind:this={panel}>
      {#each collections as c (c.id)}
        <div class="row">
          {#if deleting === c.id}
            <span class="ask">Delete {c.name}? Games stay in your library.</span>
            <button class="small danger" onclick={() => remove(c)}>Delete</button>
            <button class="small cancel" onclick={() => keep(c)}>Cancel</button>
          {:else if renaming === c.id}
            <input
              aria-label="Rename {c.name}"
              bind:value={draft}
              onkeydown={(e) => renameKeydown(e, c)}
              onblur={() => saveRename(c)}
            />
          {:else}
            <button class="name" data-name={c.id} title="Rename" onclick={() => startRename(c)}>{c.name}</button>
            <button class="x" data-delete={c.id} aria-label="Delete {c.name}" onclick={() => askDelete(c)}>✕</button>
          {/if}
        </div>
      {:else}
        <p class="note">No collections yet. Add games to one from a game's menu.</p>
      {/each}
      {#if problem}<p class="note problem" title={problem}>{problem}</p>{/if}
    </div>
  {/if}
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

  .manage {
    display: grid;
    gap: 2px;
    margin-top: 8px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 32px;
    font-size: 13px;
  }

  .name {
    flex: 1;
    min-width: 0;
    padding: 6px 8px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
    background: transparent;
  }

  .name:hover:not(:disabled),
  .x:hover:not(:disabled) {
    background: var(--glass-hover);
  }

  .x {
    width: 28px;
    height: 28px;
    padding: 0;
    flex: none;
    border-radius: 999px;
    background: transparent;
    color: var(--text-3);
    font-size: 11px;
  }

  .x:hover:not(:disabled) {
    color: var(--text);
  }

  input {
    flex: 1;
    height: 28px;
    padding: 0 10px;
    border-radius: 10px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
    font-size: 13px;
  }

  .ask {
    flex: 1;
    min-width: 0;
    color: var(--text);
  }

  .small {
    flex: none;
    height: 28px;
    padding: 0 10px;
    border-radius: 999px;
    background: var(--glass-hover);
    border: 1px solid var(--glass-line);
    font-size: 13px;
  }

  .small:hover:not(:disabled) {
    background: var(--glass-on);
  }

  .danger {
    color: var(--danger-text);
  }

  .note {
    margin: 4px 8px;
    font-size: 13px;
    color: var(--text-2);
  }

  .problem {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
