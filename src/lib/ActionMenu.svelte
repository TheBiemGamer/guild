<script lang="ts" module>
  export type Action = {
    label: string
    run: () => void
    /** Listed but can't run now; the caller says why elsewhere. */
    disabled?: boolean
    danger?: boolean
    /** A line above it, setting it apart from the items before. */
    apart?: boolean
    /** It moves focus itself, so focus doesn't go back to the ⋯ button. */
    movesFocus?: boolean
  }
</script>

<script lang="ts">
  import { DropdownMenu } from 'bits-ui'
  import type { Snippet } from 'svelte'
  import { keepOnBackForward } from './outside'

  let {
    actions,
    label,
    open = $bindable(),
    trigger,
  }: {
    actions: Action[]
    /** The menu's accessible name. */
    label: string
    open: boolean
    /** The caller's ⋯ button: spread `props` on it. */
    trigger: Snippet<[{ props: Record<string, unknown> }]>
  } = $props()

  let content = $state<HTMLElement | null>(null)
  /** Runs once the menu has closed, so an action that changes the row doesn't unmount the menu mid-close. */
  let picked: Action | null = null

  /** Focus starts on the first item that can run, by mouse or keyboard. */
  function focusFirst(e: Event) {
    const first = content?.querySelector<HTMLElement>('[role="menuitem"]:not([data-disabled])')
    if (!first) return
    e.preventDefault()
    first.focus()
  }

  function closeFocus(e: Event) {
    const a = picked
    picked = null
    if (!a) return
    if (a.movesFocus) e.preventDefault()
    queueMicrotask(a.run)
  }
</script>

<DropdownMenu.Root bind:open>
  <DropdownMenu.Trigger>
    {#snippet child({ props })}
      {@render trigger({ props })}
    {/snippet}
  </DropdownMenu.Trigger>
  <DropdownMenu.Portal>
    <DropdownMenu.Content
      bind:ref={content}
      align="end"
      sideOffset={8}
      collisionPadding={8}
      preventScroll={false}
      aria-label={label}
      onInteractOutside={keepOnBackForward}
      onOpenAutoFocus={focusFirst}
      onCloseAutoFocus={closeFocus}
    >
      {#snippet child({ props, wrapperProps })}
        <div {...wrapperProps}>
          <div {...props} class="pop">
            {#each actions as a (a.label)}
              {#if a.apart}<DropdownMenu.Separator class="sep" />{/if}
              <DropdownMenu.Item disabled={a.disabled} onSelect={() => (picked = a)}>
                {#snippet child({ props })}
                  <button {...props} class="o" class:danger={a.danger} disabled={a.disabled}>{a.label}</button>
                {/snippet}
              </DropdownMenu.Item>
            {/each}
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
    min-width: 180px;
    padding: 6px;
    display: grid;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: var(--shadow-modal);
  }

  .pop:focus-visible {
    outline: none;
  }

  .o {
    width: 100%;
    padding: 8px 10px;
    border-radius: 8px;
    background: transparent;
    color: var(--text-2);
    text-align: left;
    white-space: nowrap;
  }

  .o:hover:not(:disabled),
  .o:focus-visible {
    background: var(--glass-hover);
    color: var(--text);
  }

  .o:focus-visible {
    outline-offset: -2px;
  }

  .o.danger,
  .o.danger:hover:not(:disabled) {
    color: var(--danger-text);
  }

  .pop :global(.sep) {
    height: 1px;
    margin: 4px 6px;
    background: var(--glass-line);
  }
</style>
