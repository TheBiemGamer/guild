<script lang="ts" module>
  import steamSvg from './icons/steam.svg?raw'
  import epicSvg from './icons/epicgames.svg?raw'
  import wikiSvg from './icons/pcgamingwiki.svg?raw'
  import dbSvg from './icons/steamdb.svg?raw'
  import protonSvg from './icons/protondb.svg?raw'
  import type { LinkKind } from './api'

  // Simple Icons ship one path per logo, like StoreIcon reads them.
  const path = (svg: string) => /\sd="([^"]+)"/.exec(svg)?.[1] ?? ''
  const paths: Record<LinkKind, string> = {
    steamStore: path(steamSvg),
    epicStore: path(epicSvg),
    pcGamingWiki: path(wikiSvg),
    steamDb: path(dbSvg),
    protonDb: path(protonSvg),
  }
  export const linkNames: Record<LinkKind, string> = {
    steamStore: 'Steam',
    epicStore: 'Epic Games',
    pcGamingWiki: 'PCGamingWiki',
    steamDb: 'SteamDB',
    protonDb: 'ProtonDB',
  }
</script>

<script lang="ts">
  let { kind, size = 14 }: { kind: LinkKind; size?: number } = $props()
</script>

<svg viewBox="0 0 24 24" width={size} height={size} fill="currentColor" aria-hidden="true">
  <path d={paths[kind]} />
</svg>
