<script lang="ts">
  import { Tooltip } from 'bits-ui'
  import type { Snippet } from 'svelte'

  let { text, truncated = false, children }: { text: string; truncated?: boolean; children: Snippet<[{ props: Record<string, unknown> }]> } = $props()

  let open = $state(false)
  let trigger = $state<HTMLElement | null>(null)

  /** A truncated text only gets a tooltip when it is cut off right now. */
  function change(next: boolean) {
    open = next && !!text && (!truncated || (!!trigger && trigger.scrollWidth > trigger.clientWidth))
  }
</script>

<Tooltip.Root {open} onOpenChange={(next) => change} disableHoverableContent>
  <Tooltip.Trigger bind:ref={trigger}>
    {#snippet child({ props })}
      <!-- The caller's element keeps its own focus, type and disabled; only the hover and describedby props pass on. -->
      {@const { tabindex: _tabindex, type: _type, disabled: _disabled, ...rest } = props}
      {@render children({ props: rest })}
    {/snippet}
  </Tooltip.Trigger>
  <Tooltip.Portal>
    <Tooltip.Content class="tip" sideOffset={6} collisionPadding={8}>{text}</Tooltip.Content>
  </Tooltip.Portal>
</Tooltip.Root>

<style>
  /* A disabled button fires no pointer events, so the tooltip hangs on this wrapper. */
  :global(.tip-anchor) {
    display: inline-flex;
  }
  :global(.tip) {
    max-width: 320px;
    padding: 5px 9px;
    border: 1px solid var(--glass-on-line);
    border-radius: 8px;
    background: var(--surface-2);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.5);
    color: var(--text);
    font-size: 12px;
    overflow-wrap: anywhere;
    transition: opacity 120ms;
  }
  :global(.tip[data-starting-style]),
  :global(.tip[data-ending-style]) {
    opacity: 0;
  }
  @media (prefers-reduced-motion: reduce) {
    :global(.tip) {
      transition: none;
    }
  }
</style>
