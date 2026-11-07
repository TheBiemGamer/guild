<script lang="ts" module>
  import steamSvg from './icons/steam.svg?raw'
  import epicSvg from './icons/epicgames.svg?raw'
  // The files are white-filled; `no-inline` keeps them out of data: URLs, which the CSP blocks.
  import steamFile from './icons/steam.svg?no-inline'
  import epicFile from './icons/epicgames.svg?no-inline'
  import padSvg from './icons/gamepad.svg?raw'
  import padFile from './icons/gamepad.svg?no-inline'
  import type { Store } from './api'

  // Simple Icons ship one path per logo; reading it once keeps {@html} out of every card.
  const path = (svg: string) => /\sd="([^"]+)"/.exec(svg)?.[1] ?? ''
  const paths: Record<Store, string> = { steam: path(steamSvg), epic: path(epicSvg), custom: path(padSvg) }
  export const storeNames: Record<Store, string> = { steam: 'Steam', epic: 'Epic Games', custom: 'Added by you' }
  /** For places that repeat the logo many times (every cover): an <img> is rasterized once and cached. */
  export const logoFile: Record<Store, string> = { steam: steamFile, epic: epicFile, custom: padFile }
</script>

<script lang="ts">
  let { provider, size = 16 }: { provider: Store; size?: number } = $props()
</script>

<svg
  class="store-icon"
  viewBox="0 0 24 24"
  width={size}
  height={size}
  role="img"
  aria-label={storeNames[provider]}
>
  <!-- The gamepad is a line glyph; the store logos are filled. -->
  {#if provider === 'custom'}
    <path d={paths.custom} fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
  {:else}
    <path d={paths[provider]} fill="currentColor" />
  {/if}
</svg>

<style>
  .store-icon {
    display: block;
    flex: none;
  }
</style>
