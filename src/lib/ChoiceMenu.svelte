<script lang="ts" generics="T extends string">
  import { DropdownMenu } from 'bits-ui'
  import type { Snippet } from 'svelte'
  import Check from './Check.svelte'
  import { keepOnBackForward } from './outside'

  let {
    value = $bindable(),
    options,
    label,
    open = $bindable(),
    trigger,
  }: {
    value: T
    options: { value: T; label: string }[]
    /** The menu's accessible name, like "Sort by". */
    label: string
    open: boolean
    /** The caller's chip: spread `props` on it. */
    trigger: Snippet<[{ props: Record<string, unknown> }]>
  } = $props()

  let content = $state<HTMLElement | null>(null)

  /** A menu opens on the chosen item, whether by mouse or keyboard. */
  function focusChosen(e: Event) {
    const chosen = content?.querySelector<HTMLElement>('[aria-checked="true"]')
    if (!chosen) return
    e.preventDefault()
    chosen.focus()
  }
</script>

<DropdownMenu.Root bind:open>
  <DropdownMenu.Trigger>
    {#snippet child({ props })}
      {@render trigger({ props })}
    {/snippet}
  </DropdownMenu.Trigger>
  <DropdownMenu.Portal>
    <DropdownMenu.Content bind:ref={content} align="end" sideOffset={8} aria-label={label} onInteractOutside={keepOnBackForward} onOpenAutoFocus={focusChosen}>
      {#snippet child({ props, wrapperProps })}
        <div {...wrapperProps}>
          <div {...props} class="pop">
            <DropdownMenu.RadioGroup bind:value>
              {#each options as o (o.value)}
                <DropdownMenu.RadioItem value={o.value}>
                  {#snippet child({ props, checked })}
                    <button {...props} class="o">
                      {o.label}
                      {#if checked}<Check />{/if}
                    </button>
                  {/snippet}
                </DropdownMenu.RadioItem>
              {/each}
            </DropdownMenu.RadioGroup>
          </div>
        </div>
      {/snippet}
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>

<style>
  .pop {
    /* bits-ui copies this onto its portal wrapper: above the Settings backdrop (20) and every popup (up to 40). */
    position: relative;
    z-index: 50;
    min-width: 230px;
    padding: 6px;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: var(--shadow-modal);
  }

  .pop:focus-visible {
    outline: none;
  }

  .o {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    padding: 8px 10px;
    border-radius: 8px;
    background: transparent;
    color: var(--text-2);
    text-align: left;
    white-space: nowrap;
  }

  .o:hover:not(:disabled) {
    background: var(--glass-hover);
    color: var(--text);
  }

  .o[aria-checked='true'] {
    color: var(--text);
  }
</style>
