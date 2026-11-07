<script lang="ts">
  import { onMount } from 'svelte'
  import * as api from './api'
  import type { LoginStep } from './api'

  let { onclose, onconnected }: {
    onclose: () => void
    /** Steam approved the login; the app re-reads settings and the library. */
    onconnected: () => void
  } = $props()

  const POLL_MS = 5000
  /** Polls that may fail in a row (a network blip) before the QR side gives up. */
  const MAX_FAILURES = 3

  // Answers arriving after the dialog is gone (or the other side won) are dropped.
  let alive = true

  let accountInput = $state<HTMLInputElement>()
  let svg = $state<string | null>(null)
  let scanned = $state(false)
  let qrFailure = $state<string | null>(null)

  let account = $state('')
  let password = $state('')
  let code = $state('')
  let phase = $state<'form' | 'busy' | 'approve' | 'code' | 'checking'>('form')
  let codeKind = $state<'email' | 'app'>('app')
  let approveByEmail = $state(false)
  let error = $state<string | null>(null)

  function finish() {
    alive = false
    onconnected()
  }

  /** Follows one answer of the password login. */
  async function follow(answer: Promise<LoginStep>) {
    let step: LoginStep
    try {
      step = await answer
    } catch (e) {
      step = { step: 'failed', message: api.message(e) }
    }
    if (!alive) return
    switch (step.step) {
      case 'connected':
        return finish()
      case 'failed':
        phase = 'form'
        error = step.message
        return
      case 'approve':
        phase = 'approve'
        approveByEmail = step.email
        error = null
        return follow(api.steamPasswordNext())
      case 'code':
        // Steam asks again after a code it didn't take.
        error = phase === 'checking' ? "That code didn't work. Try again." : null
        phase = 'code'
        codeKind = step.kind
        code = ''
        return
    }
  }

  function signIn(e: SubmitEvent) {
    e.preventDefault()
    if (!account.trim() || !password) return
    error = null
    phase = 'busy'
    const answer = api.steamPasswordLogin(account, password)
    password = ''
    follow(answer)
  }

  function sendCode(e: SubmitEvent) {
    e.preventDefault()
    if (!code.trim()) return
    phase = 'checking'
    follow(api.steamPasswordCode(code))
  }

  onMount(() => {
    const before = document.activeElement
    accountInput?.focus()
    let timer: ReturnType<typeof setTimeout> | undefined
    let failures = 0

    async function tick() {
      try {
        const step = await api.steamConnectPoll()
        if (!alive) return
        failures = 0
        if (step.step === 'connected') return finish()
        if (step.step === 'failed') {
          qrFailure = step.message
          return
        }
        if (step.step === 'rotated') svg = step.svg
        if (step.step === 'scanned') scanned = true
      } catch (e) {
        if (!alive) return
        if (++failures >= MAX_FAILURES) {
          qrFailure = api.message(e)
          return
        }
      }
      timer = setTimeout(tick, POLL_MS)
    }

    api.steamConnectStart().then(
      (svgCode) => {
        if (!alive) return
        svg = svgCode
        timer = setTimeout(tick, POLL_MS)
      },
      (e) => alive && (qrFailure = api.message(e)),
    )

    return () => {
      alive = false
      clearTimeout(timer)
      api.steamConnectCancel().catch(() => {})
      api.steamLoginCancel().catch(() => {})
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
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="steam-login-title">
    <button class="close" onclick={onclose} aria-label="Close">✕</button>
    <h4 id="steam-login-title">Log in to Steam</h4>
    <div class="cols">
      <div class="side">
        {#if phase === 'approve'}
          <p>{approveByEmail ? 'Open the link Steam emailed you to approve this login.' : 'Approve the login in the Steam app on your phone.'}</p>
        {:else if phase === 'code' || phase === 'checking'}
          <form onsubmit={sendCode}>
            <label for="steam-code">
              {codeKind === 'email' ? 'Code Steam emailed you' : 'Code from the Steam app'}
            </label>
            <!-- svelte-ignore a11y_autofocus -->
            <input id="steam-code" type="text" autocomplete="off" spellcheck="false" autofocus bind:value={code} />
            <button class="pill" disabled={phase === 'checking' || !code.trim()}>
              {phase === 'checking' ? 'Checking…' : 'Submit'}
            </button>
          </form>
          {#if phase === 'checking'}<p class="hint">This can take up to 30 seconds.</p>{/if}
        {:else}
          <form onsubmit={signIn}>
            <label for="steam-account">Account name</label>
            <input
              id="steam-account"
              type="text"
              autocomplete="off"
              spellcheck="false"
              bind:this={accountInput}
              bind:value={account}
            />
            <label for="steam-password">Password</label>
            <input id="steam-password" type="password" autocomplete="off" bind:value={password} />
            <button class="pill" disabled={phase === 'busy' || !account.trim() || !password}>
              {phase === 'busy' ? 'Logging in…' : 'Log in'}
            </button>
          </form>
        {/if}
        {#if error}<p class="fail" role="alert">{error}</p>{/if}
      </div>

      <div class="side qr-side">
        {#if qrFailure}
          <p class="fail" role="alert">{qrFailure}</p>
        {:else}
          <div class="qr" aria-hidden="true">
            {#if svg}{@html svg}{/if}
          </div>
          <p>{scanned ? 'Approve the login in the Steam app.' : 'Or scan with the Steam app → Steam Guard.'}</p>
        {/if}
      </div>
    </div>
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
    width: min(560px, calc(100% - 32px));
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

  .cols {
    display: grid;
    grid-template-columns: 1fr 240px;
    gap: 20px;
  }

  .side {
    min-width: 0;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  label {
    font-size: 13px;
    color: var(--text-2);
  }

  input {
    height: 36px;
    margin-bottom: 6px;
    padding: 0 12px;
    border-radius: 10px;
    border: 1px solid var(--glass-line);
    background: var(--glass);
    color: var(--text);
  }

  input:focus-visible {
    outline-offset: 0;
  }

  .pill {
    align-self: flex-start;
  }

  /* Reserved so the dialog doesn't jump when the code arrives. */
  .qr {
    width: 240px;
    height: 240px;
    margin: 0 0 12px;
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

  form + .fail,
  form + .hint {
    margin-top: 10px;
  }

  .fail {
    color: var(--danger-text);
    overflow-wrap: anywhere;
  }

  @media (max-width: 600px) {
    .cols {
      grid-template-columns: 1fr;
    }
  }
</style>
