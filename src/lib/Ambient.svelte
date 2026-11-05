<script lang="ts">
  let { hero, enabled }: { hero: string | null; enabled: boolean } = $props()

  // The art is blurred once into a small canvas that the browser scales up; a CSS blur filter
  // would be re-applied by the compositor on every scrolled frame.
  const W = 192
  const H = 120
  /** How far the art reaches past each edge, so the blur doesn't fade out at the window border. */
  const BLEED = 0.15

  let canvas: HTMLCanvasElement
  let shown = $state(false)

  $effect(() => {
    shown = false
    if (!enabled || !hero) return
    const img = new Image()
    let live = true
    img.onload = () => {
      if (!live) return
      const ctx = canvas.getContext('2d')
      if (!ctx) return
      // Cover-fit the art into the bled-out area.
      const w = W * (1 + 2 * BLEED)
      const h = H * (1 + 2 * BLEED)
      const scale = Math.max(w / img.naturalWidth, h / img.naturalHeight)
      const dw = img.naturalWidth * scale
      const dh = img.naturalHeight * scale
      ctx.clearRect(0, 0, W, H)
      ctx.filter = 'blur(10px) saturate(1.5)'
      ctx.drawImage(img, (W - dw) / 2, (H - dh) / 2, dw, dh)
      shown = true
    }
    img.src = hero
    return () => {
      live = false
    }
  })
</script>

<!-- One static layer behind the whole window, outside the scroller and never animated while scrolling. -->
<div class="ambient" aria-hidden="true">
  <canvas bind:this={canvas} width={W} height={H} class:shown></canvas>
  <div class="shade"></div>
</div>

<style>
  .ambient {
    position: fixed;
    inset: 0;
    z-index: 0;
    overflow: hidden;
    pointer-events: none;
    /* Neutral: a faint cool lift in the top-left corner, otherwise near-black. */
    background:
      radial-gradient(90% 70% at 15% -10%, rgb(70 76 104 / 0.35), transparent 70%),
      var(--bg);
  }

  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    transition: opacity 600ms ease-out;
  }

  canvas.shown {
    opacity: 0.6;
  }

  /* Darkens toward the bottom so the grid always sits on calm, readable ground. */
  .shade {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      180deg,
      rgb(12 12 18 / 0.15) 0%,
      rgb(12 12 18 / 0.72) 42%,
      rgb(12 12 18 / 0.94) 78%
    );
  }
</style>
