<script lang="ts">
  import type { Snippet } from 'svelte'
  import type { UpdateInfo } from './bindings'
  import NotesPopover from './NotesPopover.svelte'
  import { t } from './i18n'

  let {
    update,
    current,
    running,
    installing,
    error,
    open = $bindable(),
    trigger,
    oninstall,
  }: {
    update: UpdateInfo
    /** The installed version. */
    current: string
    /** A game is running: the installer would close it. */
    running: boolean
    installing: boolean
    /** The last install's failure. */
    error: string | null
    open: boolean
    /** The Update chip: spread `props` on it. */
    trigger: Snippet<[{ props: Record<string, unknown> }]>
    oninstall: () => void
  } = $props()
</script>

<NotesPopover version={update.version} sub={t('update.youHave', { version: current })} notes={update.notes} bind:open {trigger}>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if running}<p>{t('update.closeGame')}</p>{/if}
  <div class="foot">
    <button class="glass" disabled={installing} onclick={() => (open = false)}>{t('update.later')}</button>
    <button class="pill" disabled={running} aria-disabled={installing} onclick={() => installing || oninstall()}>
      {installing ? t('update.downloading') : t('update.restart')}
    </button>
  </div>
</NotesPopover>

<style>
  p {
    margin: 12px 0 0;
    font-size: 13px;
    color: var(--text-3);
  }

  .error {
    color: var(--danger-text);
  }

  .foot {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 14px;
    padding-top: 12px;
    border-top: 1px solid var(--glass-line);
  }
</style>
