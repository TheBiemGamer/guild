// The toolbar popovers' keyboard and pointer rules: focus moves in on open, Tab stays inside,
// Escape closes and returns focus to the chip, a click outside closes.

const FOCUSABLE = 'button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex="-1"])'

export function popover(node: HTMLElement, { anchor, close }: { anchor: HTMLElement | undefined; close: (refocus: boolean) => void }) {
  const items = () => Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE))
  ;(items()[0] ?? node).focus()

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      close(true)
    } else if (e.key === 'Tab') {
      const all = items()
      if (all.length === 0) return e.preventDefault()
      const at = all.indexOf(document.activeElement as HTMLElement)
      const next = e.shiftKey ? (at <= 0 ? all.length - 1 : at - 1) : at === all.length - 1 ? 0 : at + 1
      e.preventDefault()
      all[next].focus()
    }
  }

  // Mouse Back/Forward (3, 4) don't close it: App ignores them while a popover is open, like Settings.
  function onpointerdown(e: PointerEvent) {
    const target = e.target as Node
    if (e.button > 2 || node.contains(target) || anchor?.contains(target)) return
    close(false)
  }

  node.addEventListener('keydown', onkeydown)
  window.addEventListener('pointerdown', onpointerdown, true)
  return {
    destroy() {
      node.removeEventListener('keydown', onkeydown)
      window.removeEventListener('pointerdown', onpointerdown, true)
    },
  }
}
