/** Whether a key or focus target sits inside a dialog, which then owns the keyboard. */
export function isInDialog(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    !!target.closest("[role='dialog'], [role='alertdialog'], dialog[open]")
  );
}
