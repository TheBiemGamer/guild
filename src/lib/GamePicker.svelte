<script lang="ts">
  import { Combobox } from 'bits-ui'
  import Check from './Check.svelte'
  import { t } from './i18n'
  import { titleMatches } from './filters'
  import { keepOnBackForward } from './outside'

  let {
    id,
    games,
    value = $bindable(),
  }: {
    id: string
    games: { key: string; title: string }[]
    /** The picked game's key; '' while nothing is picked. */
    value: string
  } = $props()

  const titleOf = (key: string) => games.find((g) => g.key === key)?.title ?? ''

  let open = $state(false)
  let text = $state(titleOf(value))
  /** What the owner typed; the picked title in the field is not a search. */
  let query = $state('')
  const shown = $derived(games.filter((g) => titleMatches(g.title, query)))

  /** Typing drops the pick, so Import waits for a choice; an emptied field brings back the first game. */
  function typed(e: Event) {
    text = query = (e.currentTarget as HTMLInputElement).value
    value = query.trim() === '' ? games[0].key : ''
  }

  function settle() {
    query = ''
    if (value !== '') text = titleOf(value)
  }
</script>

<Combobox.Root
  type="single"
  bind:value
  bind:open
  items={games.map((g) => ({ value: g.key, label: g.title }))}
  inputValue={text}
  allowDeselect={false}
  onValueChange={settle}
  onOpenChange={(o) => o || settle()}
>
  <Combobox.Input {id} autocomplete="off" oninput={typed} onclick={() => (open = true)}>
    {#snippet child({ props })}<input {...props} />{/snippet}
  </Combobox.Input>
  <Combobox.Portal>
    <Combobox.Content sideOffset={6} onInteractOutside={keepOnBackForward}>
      {#snippet child({ props, wrapperProps })}
        <div {...wrapperProps}>
          <div {...props} class="pop">
            {#each shown as g (g.key)}
              <Combobox.Item value={g.key} label={g.title}>
                {#snippet child({ props, selected })}
                  <div {...props} class="o">
                    <span>{g.title}</span>
                    {#if selected}<Check />{/if}
                  </div>
                {/snippet}
              </Combobox.Item>
            {:else}
              <div class="o none">{t('settings.importNoMatch')}</div>
            {/each}
          </div>
        </div>
      {/snippet}
    </Combobox.Content>
  </Combobox.Portal>
</Combobox.Root>

<style>
  /* The Settings field look; Settings' own styles don't reach into this component. */
  input {
    flex: 1;
    min-width: 0;
    height: 36px;
    padding: 0 12px;
    border-radius: 10px;
    border: 1px solid var(--glass-line);
    background: var(--glass);
    color: var(--text);
  }

  input:focus-visible {
    outline-offset: 0;
  }

  .pop {
    /* bits-ui copies this onto its portal wrapper: above the Settings dialog and every popup (up to 40). */
    position: relative;
    z-index: 50;
    min-width: var(--bits-floating-anchor-width);
    max-width: 90vw;
    max-height: 260px;
    overflow-y: auto;
    padding: 6px;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: var(--shadow-modal);
  }

  .o {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 10px;
    border-radius: 8px;
    color: var(--text-2);
  }

  .o[data-highlighted] {
    background: var(--glass-hover);
    color: var(--text);
  }

  .o[data-selected] {
    color: var(--text);
  }

  .none {
    color: var(--text-3);
  }
</style>
