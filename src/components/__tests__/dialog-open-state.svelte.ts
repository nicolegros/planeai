/** Reactive open flag so tests can exercise a dialog's close window. */
export function createDialogOpenState(): { open: boolean } {
  const state = $state({ open: true });
  return state;
}
