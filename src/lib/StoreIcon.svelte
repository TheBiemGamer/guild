<script lang="ts" module>
  import steamSvg from './icons/steam.svg?raw'
  import epicSvg from './icons/epicgames.svg?raw'
  // The files are white-filled; `no-inline` keeps them out of data: URLs, which the CSP blocks.
  import steamFile from './icons/steam.svg?no-inline'
  import epicFile from './icons/epicgames.svg?no-inline'
  import type { Store } from './api'

  // Simple Icons ship one path per logo; reading it once keeps {@html} out of every card.
  const path = (svg: string) => /\sd="([^"]+)"/.exec(svg)?.[1] ?? ''
  const paths: Record<Store, string> = { steam: path(steamSvg), epic: path(epicSvg) }
  export const storeNames: Record<Store, string> = { steam: 'Steam', epic: 'Epic Games' }
  /** For places that repeat the logo many times (every cover): an <img> is rasterized once and cached. */
  export const logoFile: Record<Store, string> = { steam: steamFile, epic: epicFile }
</script>

<script lang="ts">
  let { provider, size = 16 }: { provider: Store; size?: number } = $props()
</script>

<svg
  class="store-icon"
  viewBox="0 0 24 24"
  width={size}
  height={size}
  fill="currentColor"
  role="img"
  aria-label={storeNames[provider]}
>
  <path d={paths[provider]} />
</svg>

<style>
  .store-icon {
    display: block;
    flex: none;
  }
</style>
