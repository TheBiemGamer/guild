<script lang="ts">
  import { Dialog } from 'bits-ui'
  import { untrack } from 'svelte'
  import * as api from './api'
  import type { CustomGame, GridCover } from './bindings'
  import { nameFromExe } from './format'
  import Tip from './Tip.svelte'
  import { around, t } from './i18n'
  import { keepOnBackForward } from './outside'

  let {
    edit = null,
    onclose,
    onsaved,
    onsettings,
  }: {
    /** The game being edited, as saved; null adds a new one. */
    edit?: ({ key: string } & CustomGame) | null
    onclose: () => void
    /** After a save: the library reloads. The new game's page doesn't open. */
    onsaved: () => void
    /** Opens Settings at Extra metadata, where the SteamGridDB key goes. */
    onsettings: () => void
  } = $props()

  const start = untrack(() => edit)
  let exe = $state(start?.exe ?? '')
  let name = $state(start?.name ?? '')
  let args = $state(start?.args ?? '')
  /** undefined while searching; null without a SteamGridDB key. */
  let covers = $state<GridCover[] | null | undefined>(undefined)
  /** The picked SteamGridDB cover; -1 for none: a file, or (editing) the cover it has. */
  let picked = $state(-1)
  let file = $state<string | null>(null)
  let saving = $state(false)
  let error = $state<string | null>(null)
  /** The name last filled in from a program: picking another program replaces it, never a typed name. */
  let suggested = ''
  let first = true
  let open = $state(true)

  // Adding waits for the search, so the cover it picks first isn't left out.
  const ready = $derived(exe !== '' && name.trim() !== '' && (covers !== undefined || !!start || !!file))
  const fileName = $derived(file?.split(/[\\/]/).pop() ?? '')

  // The name drives the search, 400 ms after it stops changing (at once when the dialog opens).
  $effect(() => {
    const term = name.trim()
    let alive = true
    if (untrack(() => covers) !== null) covers = undefined
    const timer = setTimeout(
      async () => {
        const found = await api.gridSearch(term).catch(() => [])
        if (!alive) return
        covers = found
        // Adding, the first cover is picked unless a file is; editing keeps the cover until one is picked.
        picked = found?.length && !file && !start ? 0 : -1
      },
      first ? 0 : 400,
    )
    first = false
    return () => {
      alive = false
      clearTimeout(timer)
    }
  })

  async function browse() {
    const path = await api.pickProgram().catch((e) => ((error = api.message(e)), null))
    if (!path) return
    exe = path
    if (!name.trim() || name === suggested) name = suggested = nameFromExe(path)
  }

  async function pickFile() {
    const path = await api.pickImage().catch((e) => ((error = api.message(e)), null))
    if (!path) return
    file = path
    picked = -1
  }

  function pickCover(i: number) {
    picked = i
    file = null
  }

  async function save() {
    if (!ready || saving) return
    saving = true
    error = null
    const cover = picked >= 0 && covers ? { url: covers[picked].url } : file ? { file } : null
    try {
      await api.saveCustomGame(start?.key ?? null, { name: name.trim(), exe, args }, cover)
      onsaved()
      onclose()
    } catch (e) {
      error = api.message(e)
    }
    saving = false
  }
</script>

<Dialog.Root bind:open onOpenChange={(o) => o || onclose()}>
  <Dialog.Portal>
    <Dialog.Overlay>
      {#snippet child({ props })}<div {...props} class="backdrop"></div>{/snippet}
    </Dialog.Overlay>
    <Dialog.Content onInteractOutside={keepOnBackForward}>
      {#snippet child({ props })}
        <div {...props} class="dialog" aria-labelledby="add-title">
          <h3 id="add-title">{start ? t('addGame.editTitle') : t('toolbar.addGame')}</h3>

          <div class="f">
            <label for="add-exe">{t('addGame.program')}</label>
            <div class="in">
              <Tip text={exe} truncated>
                {#snippet children({ props })}
                  <input {...props} id="add-exe" readonly value={exe} spellcheck="false" onclick={browse} />
                {/snippet}
              </Tip>
              <button class="glass" onclick={browse}>{t('common.browse')}</button>
            </div>
          </div>
          <div class="f">
            <label for="add-name">{t('menu.name')}</label>
            <input id="add-name" type="text" spellcheck="false" autocomplete="off" bind:value={name} />
          </div>
          <div class="f">
            <label for="add-args">{t('addGame.arguments')}</label>
            <input id="add-args" type="text" spellcheck="false" autocomplete="off" placeholder={t('addGame.optional')} bind:value={args} />
          </div>

          <div class="f">
            <span class="label" id="add-cover">{t('addGame.cover')}</span>
            {#if covers === null}
              {@const [before, after] = around('addGame.noKey', 'link')}
              <p class="note">{before}<button class="link" onclick={onsettings}>{t('addGame.noKeyLink')}</button>{after}</p>
              <div class="in file">
                <button class="glass" onclick={pickFile}>{t('addGame.useFile')}</button>
                {#if file}<Tip text={file}>
                    {#snippet children({ props })}<span {...props}>{fileName}</span>{/snippet}
                  </Tip>{/if}
              </div>
            {:else}
              <!-- Four slots always, so nothing moves while the search runs. -->
              <div class="covers" role="group" aria-labelledby="add-cover">
                {#each [0, 1, 2, 3] as i (i)}
                  {@const c = covers?.[i]}
                  {#if c}
                    <button class="c" aria-pressed={picked === i} aria-label={t('addGame.coverNumber', { number: i + 1 })} onclick={() => pickCover(i)}>
                      <img src={c.thumb} alt="" />
                    </button>
                  {:else}
                    <span class="c" aria-hidden="true"></span>
                  {/if}
                {/each}
              </div>
              <small
                ><Tip text={file ?? ''}>
                  {#snippet children({ props })}<span {...props}>{file ? fileName : t('addGame.fromGrid')}</span>{/snippet}
                </Tip>
                ·
                <button class="link" onclick={pickFile}>{t('addGame.useFile')}</button></small
              >
            {/if}
          </div>

          {#if error}<p class="error" role="alert">{error}</p>{/if}

          <div class="foot">
            <button class="glass" onclick={onclose}>{t('common.cancel')}</button>
            <button class="pill" disabled={!ready || saving} onclick={save}>{start ? t('common.save') : t('addGame.add')}</button>
          </div>
        </div>
      {/snippet}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  /* The same scrim as Settings. */
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    background: rgb(8 8 12 / 0.55);
  }

  .dialog {
    position: fixed;
    inset: 0;
    z-index: 20;
    margin: auto;
    height: fit-content;
    width: min(520px, calc(100% - 48px));
    max-height: calc(100% - 48px);
    overflow-y: auto;
    padding: 18px;
    border-radius: var(--radius-modal);
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: var(--shadow-modal);
  }

  h3 {
    margin: 0 0 14px;
    font-size: 16px;
    font-weight: 700;
  }

  .f {
    margin-bottom: 12px;
  }

  .f > label,
  .label {
    display: block;
    margin-bottom: 5px;
    font-size: 12px;
    color: var(--text-2);
  }

  .in {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  input {
    width: 100%;
    min-width: 0;
    height: 36px;
    padding: 0 10px;
    border-radius: 10px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
    color: var(--text);
  }

  .in input {
    flex: 1;
  }

  input[readonly] {
    cursor: pointer;
    text-overflow: ellipsis;
  }

  input::placeholder {
    color: var(--text-3);
  }

  button.glass {
    flex: none;
  }

  .covers {
    display: flex;
    gap: 8px;
  }

  .c {
    width: 76px;
    height: 114px;
    padding: 0;
    overflow: hidden;
    border-radius: 8px;
    background: var(--surface);
    border: 2px solid transparent;
  }

  button.c:hover:not(:disabled) {
    background: var(--surface);
    border-color: var(--glass-on-line);
  }

  button.c[aria-pressed='true'] {
    border-color: #fff;
  }

  .c img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  small {
    display: block;
    margin-top: 6px;
    font-size: 12px;
    color: var(--text-3);
  }

  .link {
    padding: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--accent-2);
    font-size: inherit;
    text-decoration: underline;
  }

  .link:hover:not(:disabled) {
    background: transparent;
    color: #fff;
  }

  .note {
    margin: 0;
    padding: 10px 12px;
    border-radius: 10px;
    background: var(--glass);
    border: 1px solid var(--glass-line);
    font-size: 12px;
    color: var(--text-3);
  }

  .file {
    margin-top: 8px;
    font-size: 12px;
    color: var(--text-2);
  }

  .file span,
  small span {
    display: inline-block;
    max-width: 320px;
    vertical-align: bottom;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .error {
    margin: 0 0 12px;
    padding: 8px 12px;
    border-radius: 10px;
    border: 1px solid rgb(239 68 68 / 0.35);
    color: var(--error-text);
  }

  .foot {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
    padding-top: 12px;
    border-top: 1px solid var(--glass-line);
  }
</style>
