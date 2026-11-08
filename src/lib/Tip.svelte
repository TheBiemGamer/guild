<script lang="ts">
  import { Tooltip } from 'bits-ui'
  import type { Snippet } from 'svelte'

  let { text, truncated = false, children }: { text: string; truncated?: boolean; children: Snippet<[{ props: Record<string, unknown> }]> } = $props()

  let open = $state(false)
  /** The element the pointer or focus last reached; measured when the tooltip is about to open. */
  let hovered: HTMLElement | null = null

  /** A truncated text only gets a tooltip when it is cut off right now. Runs after bits-ui set `open`, so the last word is ours. */
  function change(next: boolean) {
    open = next && !!text && (!truncated || (!!hovered && hovered.scrollWidth > hovered.clientWidth))
  }

  type Handler = (e: Event) => void
  /** Remember the element before bits-ui's handler can open the tooltip. */
  const track =
    (handler: unknown): Handler =>
    (e) => {
      hovered = e.currentTarget as HTMLElement
      ;(handler as Handler | undefined)?.(e)
    }
</script>

<Tooltip.Root bind:open onOpenChange={change} disableHoverableContent>
  <Tooltip.Trigger>
    {#snippet child({ props })}
      <!-- The caller's element keeps its own focus, type and disabled. focusin and focusout instead of focus and blur, so a wrapper opens when the button inside it is focused. -->
      {@const { tabindex: _tabindex, type: _type, disabled: _disabled, onfocus, onblur, onpointerenter, onpointermove, ...rest } = props}
      {@render children({
        props: { ...rest, onpointerenter: track(onpointerenter), onpointermove: track(onpointermove), onfocusin: track(onfocus), onfocusout: onblur },
      })}
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
