// App owns Back/Forward (button 3 and 4), so they must not dismiss a popover.
// Structural type so the node tests compile without the DOM lib; a PointerEvent fits it.
export const keepOnBackForward = (e: { button: number; preventDefault(): void }): void => {
  if (e.button > 2) e.preventDefault()
}
