<script lang="ts">
  import { onMount } from 'svelte'
  import * as api from './api'

  let { onclose, onconnected }: {
    onclose: () => void
    /** Steam approved the login; the app re-reads settings and the library. */
    onconnected: () => void
  } = $props()

  const POLL_MS = 5000
  /** Polls that may fail in a row (a network blip) before the dialog gives up. */
  const MAX_FAILURES = 3

  let closeButton: HTMLButtonElement
  let svg = $state<string | null>(null)
  let scanned = $state(false)
  let failure = $state<string | null>(null)

  onMount(() => {
    const before = document.activeElement
    closeButton.focus()
    // Answers arriving after the dialog is gone are dropped.
    let alive = true
    let timer: ReturnType<typeof setTimeout> | undefined
    let failures = 0

    async function tick() {
      try {
        const step = await api.steamConnectPoll()
        if (!alive) return
        failures = 0
        if (step.step === 'connected') {
          alive = false
          onconnected()
          return
        }
        if (step.step === 'rotated') svg = step.svg
        if (step.step === 'scanned') scanned = true
      } catch (e) {
        if (!alive) return
        if (++failures >= MAX_FAILURES) {
          failure = api.message(e)
          return
        }
      }
      timer = setTimeout(tick, POLL_MS)
    }

    api.steamConnectStart().then(
      (code) => {
        if (!alive) return
        svg = code
        timer = setTimeout(tick, POLL_MS)
      },
      (e) => alive && (failure = api.message(e)),
    )

    return () => {
      alive = false
      clearTimeout(timer)
      api.steamConnectCancel().catch(() => {})
      if (before instanceof HTMLElement && before.isConnected) before.focus()
    }
  })

  // Settings also closes on Escape; this dialog takes the key first.
  function onkeydown(e: KeyboardEvent) {
    if (e.key !== 'Escape') return
    e.stopImmediatePropagation()
    onclose()
  }
</script>

<svelte:window onkeydowncapture={onkeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Connect Steam">
    <button class="close" bind:this={closeButton} onclick={onclose} aria-label="Close">✕</button>
    <h4>Connect Steam</h4>
    {#if failure}
      <p class="fail" role="alert">{failure}</p>
    {:else}
      <div class="qr" aria-hidden="true">
        {#if svg}{@html svg}{/if}
      </div>
      <p>{scanned ? 'Approve the login in the Steam app' : 'Open the Steam app → Steam Guard → scan this code.'}</p>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.5);
  }

  .dialog {
    position: relative;
    box-sizing: border-box;
    width: 300px;
    padding: 16px;
    border-radius: 14px;
    background: var(--surface-2);
    border: 1px solid var(--glass-on-line);
    box-shadow: 0 20px 50px rgb(0 0 0 / 0.65);
  }

  .close {
    position: absolute;
    top: 10px;
    right: 10px;
    width: 28px;
    height: 28px;
    padding: 0;
    border-radius: 10px;
    background: rgb(255 255 255 / 0.08);
  }

  h4 {
    margin: 0 0 12px;
    font-size: 15px;
    font-weight: 600;
  }

  /* Reserved so the dialog doesn't jump when the code arrives. */
  .qr {
    width: 240px;
    height: 240px;
    margin: 0 auto 12px;
    border-radius: 8px;
    overflow: hidden;
    background: rgb(255 255 255 / 0.05);
  }

  .qr :global(svg) {
    display: block;
    width: 100%;
    height: 100%;
  }

  p {
    margin: 0;
    font-size: 14px;
    color: var(--text-2);
  }

  .fail {
    color: var(--danger-text);
    overflow-wrap: anywhere;
  }
</style>
