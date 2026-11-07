<script lang="ts">
  import type { UpdateInfo } from './api'
  import NotesPopover from './NotesPopover.svelte'

  let { update, current, running, installing, error, anchor, onclose, oninstall }: {
    update: UpdateInfo
    /** The installed version. */
    current: string
    /** A game is running: the installer would close it. */
    running: boolean
    installing: boolean
    /** The last install's failure. */
    error: string | null
    anchor: HTMLElement | undefined
    onclose: (refocus: boolean) => void
    oninstall: () => void
  } = $props()
</script>

<NotesPopover version={update.version} sub="you have {current}" notes={update.notes} {anchor} {onclose}>
  {#if error}<p class="error">{error}</p>{/if}
  {#if running}<p>Close the game to update.</p>{/if}
  <div class="foot">
    <button class="glass" disabled={installing} onclick={() => onclose(true)}>Later</button>
    <button class="pill" disabled={running} aria-disabled={installing} onclick={() => installing || oninstall()}>
      {installing ? 'Downloading…' : 'Update and restart'}
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
