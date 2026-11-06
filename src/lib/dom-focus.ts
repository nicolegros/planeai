/** The focused element, looking into the shadow roots plugin frames live in. */
export function deepActiveElement(root: Document = document): Element | null {
  let active = root.activeElement;
  while (active?.shadowRoot?.activeElement) active = active.shadowRoot.activeElement;
  return active;
}

/** Takes keyboard focus back from a focused frame. */
export function releaseFrameFocus(frame: HTMLIFrameElement, root: Document = document): void {
  frame.blur();
  // WebKit keeps sending keys to a blurred frame until the app's own window takes focus.
  root.defaultView?.focus();
}
