<script lang="ts" module>
  import changelogRaw from '../../CHANGELOG.md?raw'
  import { parseNotes, section } from './changelog'

  /** The installed version's CHANGELOG.md section, or null. */
  export const installedNotes = (version: string) => section(changelogRaw, version)
</script>

<script lang="ts">
  import { Popover } from 'bits-ui'
  import type { Snippet } from 'svelte'
  import { t } from './i18n'
  import { keepOnBackForward } from './outside'

  let {
    version,
    sub,
    notes,
    open = $bindable(),
    anchor,
    trigger,
    children,
  }: {
    version: string
    /** After the version, in `text-3`. */
    sub?: string
    /** A CHANGELOG.md section. */
    notes: string
    open: boolean
    /** The caller's own button, which toggles `open`; clicks on it don't count as outside. */
    anchor?: HTMLElement
    /** Or a chip the popover owns as its trigger: spread `props` on it. */
    trigger?: Snippet<[{ props: Record<string, unknown> }]>
    /** The footer. */
    children?: Snippet
  } = $props()

  const groups = $derived(parseNotes(notes))
</script>

<Popover.Root bind:open>
  {#if trigger}
    <Popover.Trigger>
      {#snippet child({ props })}
        {@render trigger({ props })}
      {/snippet}
    </Popover.Trigger>
  {/if}
  <Popover.Portal>
    <!-- Right-aligned under the button, above it when there's more room there; it scrolls when taller than the room. -->
    <Popover.Content customAnchor={anchor ?? null} align="end" sideOffset={8} collisionPadding={16} onInteractOutside={keepOnBackForward}>
      {#snippet child({ props, wrapperProps })}
        <div {...wrapperProps}>
          <div {...props} class="pop" role="dialog" aria-label={t('notes.label', { version })}>
            <div class="body">
              <div class="ver">
                <b>{version}</b>{#if sub}<span>{sub}</span>{/if}
              </div>
              {#each groups as g (g.title)}
                <h3>{g.title}</h3>
                <ul>
                  {#each g.items as item, i (i)}<li>{item}</li>{/each}
                </ul>
              {/each}
              {@render children?.()}
            </div>
          </div>
        </div>
      {/snippet}
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>

<style>
  .pop {
    /* bits-ui copies this onto its portal wrapper: above the Settings backdrop (20). */
    position: relative;
    z-index: 30;
    width: 380px;
    max-height: var(--bits-floating-available-height);
    overflow-y: auto;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: var(--shadow-modal);
    color: var(--text);
    text-align: left;
  }

  .pop:focus-visible {
    outline: none;
  }

  .body {
    padding: 14px;
  }

  .ver {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .ver b {
    font-size: 16px;
    font-weight: 600;
  }

  .ver span {
    font-size: 12px;
    color: var(--text-3);
  }

  h3 {
    margin: 12px 0 4px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-3);
  }

  ul {
    margin: 0;
    padding-left: 18px;
    color: var(--text-2);
    line-height: 1.55;
  }

  li {
    margin: 2px 0;
  }
</style>
