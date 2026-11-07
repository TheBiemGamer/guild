<script lang="ts">
  import { onMount } from 'svelte'
  import { downloadSize } from './api'
  import { sizeLabel } from './format'

  let { game, anchor, folder, onconfirm, oncancel }: {
    /** The Epic copy's key and the game's title. */
    game: { key: string; title: string }
    /** The cover's rectangle; the popup opens beside it. */
    anchor: DOMRect
    /** The saved install folder; null means Legendary's own, undefined that settings aren't read yet. */
    folder: string | null | undefined
    /** `withDlc`: the "Include DLC" box, on by default. */
    onconfirm: (withDlc: boolean) => void
    oncancel: () => void
  } = $props()

  const WIDTH = 270
  const GAP = 8

  let el: HTMLDivElement
  let installButton: HTMLButtonElement
  let height = $state(0)
  /** undefined while Legendary is looking it up, null when it couldn't tell. */
  let size = $state<number | null | undefined>(undefined)
  let withDlc = $state(true)

  // Right of the cover, or left when the window's edge is in the way; kept inside the window.
  const x = $derived(
    anchor.right + GAP + WIDTH <= window.innerWidth - GAP ? anchor.right + GAP : Math.max(GAP, anchor.left - GAP - WIDTH),
  )
  const y = $derived(Math.max(GAP, Math.min(anchor.top, window.innerHeight - height - GAP)))

  onMount(() => {
    const before = document.activeElement
    installButton.focus()
    // Answers arriving after the popup is gone are dropped.
    let alive = true
    downloadSize(game.key).then((bytes) => alive && (size = bytes), () => alive && (size = null))

    // The grid scrolls under a fixed popup, so a scroll anywhere closes it.
    const onscroll = () => oncancel()
    window.addEventListener('scroll', onscroll, true)
    return () => {
      alive = false
      window.removeEventListener('scroll', onscroll, true)
      if (before instanceof HTMLElement && before.isConnected) before.focus()
    }
  })

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') oncancel()
  }

  // Clicks on the cover itself are left to the cover (its Install button asks again).
  function onpointerdown(e: PointerEvent) {
    if (el.contains(e.target as Node)) return
    const inside = e.clientX >= anchor.left && e.clientX <= anchor.right && e.clientY >= anchor.top && e.clientY <= anchor.bottom
    if (!inside) oncancel()
  }
</script>

<svelte:window {onkeydown} {onpointerdown} />

<div
  class="popup"
  bind:this={el}
  bind:clientHeight={height}
  role="dialog"
  aria-label="Install {game.title}"
  style="left: {x}px; top: {y}px; width: {WIDTH}px"
>
  <h4>Install {game.title}?</h4>
  <p>{size === undefined ? 'Checking size…' : size === null ? 'Size unavailable' : `${sizeLabel(size)} download`}</p>
  <p class="folder">{folder === undefined ? '' : `To ${folder ?? "the default folder"}`}</p>
  <label class="dlc"><input type="checkbox" bind:checked={withDlc} />Include DLC</label>
  <div class="buttons">
    <button class="pill" bind:this={installButton} onclick={() => onconfirm(withDlc)}>Install</button>
    <button class="glass" onclick={oncancel}>Cancel</button>
  </div>
</div>

<style>
  .popup {
    position: fixed;
    z-index: 30;
    box-sizing: border-box;
    padding: 16px;
    border-radius: 14px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: 0 20px 50px rgb(0 0 0 / 0.65);
  }

  h4 {
    margin: 0 0 8px;
    font-size: 15px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  p {
    margin: 0 0 4px;
    font-size: 14px;
    color: var(--text-2);
  }

  /* Reserved so the popup doesn't grow when settings arrive. */
  .folder {
    min-height: 1.4em;
    overflow-wrap: anywhere;
  }

  .dlc {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 24px;
    margin-top: 4px;
    font-size: 14px;
    color: var(--text-2);
  }

  .dlc input {
    width: 16px;
    height: 16px;
    margin: 0;
    accent-color: var(--accent);
  }

  .buttons {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }

  .glass {
    border-radius: 999px;
  }
</style>
