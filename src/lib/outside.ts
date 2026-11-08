// App owns Back/Forward (mouse buttons 4 and 5), so they must not dismiss a popover.
// Structural type so the node tests compile without the DOM lib; a PointerEvent fits it.
export const keepOnBackForward = (e: { button: number; preventDefault(): void }): void => {
  if (e.button > 2) e.preventDefault()
}
