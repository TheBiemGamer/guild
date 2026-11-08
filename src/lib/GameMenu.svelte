<script lang="ts">
  import { DropdownMenu } from 'bits-ui'
  import { tick, type Snippet } from 'svelte'
  import * as api from './api'
  import type { Listed } from './api'
  import { commands, type Collection } from './bindings'
  import { t } from './i18n'
  import Check from './Check.svelte'
  import { keepOnBackForward } from './outside'

  let {
    game,
    at = null,
    open = $bindable(),
    trigger,
    label,
    onchanged,
    onerror,
    onedit,
    onremoved,
    extra,
  }: {
    game: Listed
    /** Opens at the pointer (a card's right-click); otherwise it hangs below `trigger`, right-aligned. */
    at?: { x: number; y: number } | null
    open: boolean
    /** The caller's button: spread `props` on it. */
    trigger?: Snippet<[{ props: Record<string, unknown> }]>
    label: string
    /** After a change, so the library and the page reload. */
    onchanged: () => void
    onerror: (message: string) => void
    /** Opens the Edit dialog for a game the owner added. */
    onedit?: (key: string) => void
    /** A game the owner added was removed from the library. */
    onremoved?: () => void
    /** The page's own items (`DropdownMenu.Item`s), after a separator. */
    extra?: Snippet
  } = $props()

  /** Added by the owner: it can be edited and removed. */
  const custom = $derived(game.key.startsWith('custom-'))
  /** Remove asks first, in the menu. */
  let removing = $state(false)
  let removeCancel = $state<HTMLButtonElement>()
  const askId = $props.id()

  let content = $state<HTMLElement | null>(null)
  let sub = $state<HTMLElement | null>(null)
  let subTrigger = $state<HTMLButtonElement>()
  let collections = $state<Collection[]>([])
  let naming = $state(false)
  let name = $state('')
  let newButton = $state<HTMLButtonElement>()
  /** Edit opens its dialog only once focus is back on the opener, so the dialog can return it there. */
  let editing = false
  /** Closed by a grid scroll: focus stays where it is. */
  let scrolled = false

  $effect(() => {
    if (!open) return
    scrolled = false
    commands.collections().then(
      (c) => (collections = c),
      (e) => onerror(api.message(e)),
    )
  })

  /** Focus starts on the first item, by mouse or keyboard. */
  function focusFirst(e: Event) {
    const first = content?.querySelector<HTMLElement>('[role^="menuitem"]')
    if (!first) return
    e.preventDefault()
    first.focus()
  }

  /** bits-ui returns focus right after this runs: to the trigger, or to what had focus when it opened. */
  function closeFocus(e: Event) {
    if (scrolled) e.preventDefault()
    if (editing) {
      editing = false
      const key = game.key
      queueMicrotask(() => onedit?.(key))
    }
  }

  async function askRemove() {
    removing = true
    await tick()
    removeCancel?.focus()
  }

  async function remove() {
    await change(() => commands.removeCustomGame(game.key))
    onremoved?.()
  }

  async function change(f: () => Promise<unknown>) {
    open = false
    try {
      await f()
    } catch (e) {
      onerror(api.message(e))
    }
    onchanged()
  }

  async function startNaming() {
    naming = true
    name = ''
    await tick()
    sub?.querySelector('input')?.focus()
  }

  async function create() {
    try {
      const id = await commands.createCollection(name)
      await change(() => commands.setInCollection(game.key, id, true))
    } catch (e) {
      // The name is empty or taken: the field stays for another try.
      onerror(api.message(e))
    }
  }

  /** Keys typed into the name stay in the field: no typeahead, item or submenu keys. Escape (`subEscape`) and Tab go to the menu. */
  function nameKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault()
      create()
    } else if (e.key !== 'Escape' && e.key !== 'Tab') {
      e.stopPropagation()
    }
  }

  /** Escape in the submenu: while naming it only cancels the name; otherwise the submenu closes and focus goes back to its row. */
  async function subEscape(e: KeyboardEvent) {
    if (!naming) return subTrigger?.focus()
    e.preventDefault()
    naming = false
    await tick()
    newButton?.focus()
  }

  /** While naming, hovering must not pull focus out of the field or close the submenu. Stopped on the wrappers, above every bits-ui pointer listener. */
  function holdWhileNaming(e: PointerEvent) {
    if (naming) e.stopPropagation()
  }

  /** Opened at the pointer, it would stay put while the grid scrolls the card away. */
  function onscroll(e: Event) {
    const target = e.target as Node
    if (!at || !open || content?.contains(target) || sub?.contains(target)) return
    scrolled = true
    open = false
  }
</script>

<!-- At the pointer it leaves the page live (no scroll lock), so the grid can still scroll and close it. -->
<svelte:window onscrollcapture={onscroll} />

<!-- The ⋯ menu stays mounted between opens, so it starts over each time. -->
<DropdownMenu.Root bind:open onOpenChangeComplete={(o) => !o && (removing = naming = false)}>
  {#if trigger}
    <DropdownMenu.Trigger>
      {#snippet child({ props })}
        {@render trigger({ props })}
      {/snippet}
    </DropdownMenu.Trigger>
  {/if}
  <DropdownMenu.Portal>
    <DropdownMenu.Content
      bind:ref={content}
      customAnchor={at ? { getBoundingClientRect: () => new DOMRect(at.x, at.y, 0, 0) } : null}
      align={at ? 'start' : 'end'}
      sideOffset={at ? 0 : 8}
      collisionPadding={8}
      preventScroll={!at}
      aria-label={label}
      onInteractOutside={keepOnBackForward}
      onOpenAutoFocus={focusFirst}
      onCloseAutoFocus={closeFocus}
    >
      {#snippet child({ props, wrapperProps })}
        <div {...wrapperProps} onpointermovecapture={holdWhileNaming} onpointerleavecapture={holdWhileNaming}>
          <div {...props} class="menu">
            {#if removing}
              <!-- Asks inline, like the Epic uninstall: a group of two menu items, named by the question. -->
              <div role="group" aria-labelledby={askId}>
                <p class="ask" id={askId}>{t('menu.askRemove', { title: game.title })}</p>
                <div class="confirm">
                  <DropdownMenu.Item onSelect={remove}>
                    {#snippet child({ props })}
                      <button {...props} class="danger">{t('menu.remove')}</button>
                    {/snippet}
                  </DropdownMenu.Item>
                  <DropdownMenu.Item>
                    {#snippet child({ props })}
                      <button {...props} bind:this={removeCancel}>{t('common.cancel')}</button>
                    {/snippet}
                  </DropdownMenu.Item>
                </div>
              </div>
            {:else}
              <DropdownMenu.CheckboxItem checked={game.favorite} onCheckedChange={(on) => change(() => commands.setFavorite(game.key, on))}>
                {#snippet child({ props })}
                  <button {...props}>
                    {t('menu.favorite')}
                    {#if game.favorite}<Check />{/if}
                  </button>
                {/snippet}
              </DropdownMenu.CheckboxItem>
              <DropdownMenu.Sub onOpenChange={(o) => !o && (naming = false)}>
                <DropdownMenu.SubTrigger>
                  {#snippet child({ props })}
                    <button {...props} bind:this={subTrigger}>
                      {t('menu.addToCollection')}<span class="more" aria-hidden="true">›</span>
                    </button>
                  {/snippet}
                </DropdownMenu.SubTrigger>
                <!-- Escape closes only the submenu, as Left does (or only the name field, see subEscape). -->
                <DropdownMenu.SubContent
                  bind:ref={sub}
                  sideOffset={10}
                  alignOffset={-6}
                  collisionPadding={8}
                  aria-label={t('menu.addToCollection')}
                  escapeKeydownBehavior="close"
                  onEscapeKeydown={subEscape}
                  onInteractOutside={keepOnBackForward}
                >
                  {#snippet child({ props, wrapperProps })}
                    <div {...wrapperProps} onpointermovecapture={holdWhileNaming} onpointerleavecapture={holdWhileNaming}>
                      <div {...props} class="menu sub">
                        {#each collections as c (c.id)}
                          {@const on = game.collections.includes(c.id)}
                          <DropdownMenu.CheckboxItem checked={on} onCheckedChange={(next) => change(() => commands.setInCollection(game.key, c.id, next))}>
                            {#snippet child({ props })}
                              <button {...props}>
                                {c.name}
                                {#if on}<Check />{/if}
                              </button>
                            {/snippet}
                          </DropdownMenu.CheckboxItem>
                        {/each}
                        {#if collections.length}<DropdownMenu.Separator class="sep" />{/if}
                        {#if naming}
                          <input aria-label={t('menu.collectionName')} placeholder={t('menu.name')} bind:value={name} onkeydown={nameKeydown} />
                        {:else}
                          <DropdownMenu.Item closeOnSelect={false} onSelect={startNaming}>
                            {#snippet child({ props })}
                              <button {...props} bind:this={newButton}>{t('menu.newCollection')}</button>
                            {/snippet}
                          </DropdownMenu.Item>
                        {/if}
                      </div>
                    </div>
                  {/snippet}
                </DropdownMenu.SubContent>
              </DropdownMenu.Sub>
              <DropdownMenu.Item onSelect={() => change(() => commands.setHidden(game.key, !game.hidden))}>
                {#snippet child({ props })}
                  <button {...props}>{game.hidden ? t('menu.show') : t('menu.hide')}</button>
                {/snippet}
              </DropdownMenu.Item>
              {#if custom || extra}
                <DropdownMenu.Separator class="sep" />
              {/if}
              {#if custom}
                <DropdownMenu.Item onSelect={() => (editing = true)}>
                  {#snippet child({ props })}
                    <button {...props}>{t('menu.edit')}</button>
                  {/snippet}
                </DropdownMenu.Item>
              {/if}
              {#if extra}
                {@render extra()}
              {/if}
              {#if custom}
                <DropdownMenu.Item closeOnSelect={false} onSelect={askRemove}>
                  {#snippet child({ props })}
                    <button {...props}>{t('menu.removeFromLibrary')}</button>
                  {/snippet}
                </DropdownMenu.Item>
              {/if}
            {/if}
          </div>
        </div>
      {/snippet}
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>

<style>
  .menu {
    /* bits-ui copies this onto its portal wrapper: above the Settings backdrop (20) and every popup (up to 40). */
    position: relative;
    z-index: 50;
    min-width: 240px;
    padding: 6px;
    display: grid;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: var(--shadow-modal);
    color: var(--text);
    text-align: left;
  }

  .menu:focus-visible {
    outline: none;
  }

  /* Buttons from `extra` follow the same rules. A check sits at the right end. */
  .menu :global(button) {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 8px 10px;
    border-radius: 8px;
    text-align: left;
    white-space: nowrap;
    background: transparent;
  }

  .menu :global(button:hover:not(:disabled)),
  .menu :global(button:focus-visible),
  .menu :global(button[aria-expanded='true']) {
    background: var(--glass-hover);
  }

  .menu :global(button:focus-visible) {
    outline-offset: -2px;
  }

  .menu :global(button:active:not(:disabled)) {
    transform: none;
  }

  .more {
    margin-left: auto;
    padding-left: 4px;
    color: var(--text-3);
  }

  .sub {
    max-height: 320px;
    overflow-y: auto;
  }

  .menu :global(.sep) {
    height: 1px;
    margin: 4px 6px;
    background: var(--glass-line);
  }

  .ask {
    max-width: 280px;
    margin: 4px 10px 8px;
    color: var(--text-2);
    font-size: 13px;
  }

  /* Remove and Cancel: round chips, like the collection delete confirm. */
  .confirm {
    display: flex;
    gap: 8px;
    padding: 0 10px 4px;
  }

  .menu .confirm button {
    width: auto;
    height: 28px;
    padding: 0 12px;
    border-radius: 999px;
    background: var(--glass-hover);
    border: 1px solid var(--glass-line);
  }

  .menu .confirm button:hover:not(:disabled) {
    background: var(--glass-on);
  }

  .confirm .danger {
    color: var(--danger-text);
  }

  input {
    height: 34px;
    margin: 2px 0;
    padding: 0 10px;
    border-radius: 8px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
    color: var(--text);
    font: inherit;
  }

  input::placeholder {
    color: var(--text-3);
  }
</style>
