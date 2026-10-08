import type { FocusZone } from "./focus.svelte";

/**
 * Which terminal pane owns keyboard focus, and when xterm must give up DOM focus.
 *
 * Two rules are in tension. A pane only takes focus when it belongs to the
 * selected session, otherwise the outgoing terminal reclaims focus while a
 * workspace layout is being swapped. But Terminal.svelte blurs xterm only on a
 * `focused` true -> false transition, so when that guard leaves *no* pane focused
 * the terminal keeps DOM focus forever — and xterm calls stopPropagation() on
 * keys it consumes, so sidebar navigation never reaches its window key handler.
 * `releaseTerminalDomFocus` therefore enforces the release directly.
 *
 * A provider session's chat frame owns the keyboard the same way: keys typed in
 * it stay in the frame's document, so it follows the same rules.
 */

/** Marks the iframe of a provider session's UI, which owns the keyboard like a terminal. */
export const PROVIDER_FRAME_ATTRIBUTE = "data-provider-session-frame";

/** Conditions under which the terminal is allowed to own the keyboard at all. */
export interface TerminalKeyboardOwnership {
  /** The app-level keyboard focus zone. */
  zone: FocusZone;
  /** Any modal that must own the keyboard is open. */
  modalOpen: boolean;
  /** A plugin workspace is covering the workspace. */
  pluginOverlayActive: boolean;
}

export function terminalMayOwnKeyboard(ownership: TerminalKeyboardOwnership): boolean {
  return ownership.zone === "terminal" && !ownership.modalOpen && !ownership.pluginOverlayActive;
}

export interface TerminalPaneFocusInput extends TerminalKeyboardOwnership {
  /** This tab is the active tab of its leaf. */
  isActiveTabInLeaf: boolean;
  /** This tab's pane is the focused pane of the layout. */
  isFocusedLeaf: boolean;
  /** This tab's session is the selected session (guards layout transitions). */
  belongsToActiveSession: boolean;
}

export function isTerminalPaneFocused(input: TerminalPaneFocusInput): boolean {
  return (
    input.isActiveTabInLeaf &&
    input.belongsToActiveSession &&
    input.isFocusedLeaf &&
    terminalMayOwnKeyboard(input)
  );
}

/** The focused element, looking into the shadow roots plugin frames live in. */
function deepActiveElement(root: Document): Element | null {
  let active = root.activeElement;
  while (active?.shadowRoot?.activeElement) active = active.shadowRoot.activeElement;
  return active;
}

/**
 * Give up xterm's, or a provider session frame's, DOM focus when it may not own the keyboard.
 * Independent of any pane's `focused` prop, which may never transition.
 *
 * Must run on ownership change *and* on focus arrival: Terminal.svelte's
 * focusRequest effect focuses xterm regardless of `focused` and suppresses the
 * focusin that would move the zone, so focus can land in a terminal behind an
 * open modal or while the zone is already elsewhere.
 */
export function releaseTerminalDomFocus(
  ownership: TerminalKeyboardOwnership,
  root: Document = document,
): void {
  // Cheapest discriminating check first: this runs on every focusin in the app,
  // and `closest` walks up from one node while `hasOpenDialog` scans the whole
  // document — which grows with the number of mounted terminals.
  const active = deepActiveElement(root);
  if (
    !(active instanceof HTMLElement) ||
    !(active.closest(".xterm") || active.hasAttribute(PROVIDER_FRAME_ATTRIBUTE))
  )
    return;
  // The DOM probe catches keyboard-owning dialogs whose open state App does not
  // model, such as the BranchCompare form or Preferences; a hand-maintained list
  // of flags drifts as dialogs are added.
  if (terminalMayOwnKeyboard(ownership) && !hasOpenDialog(root)) return;
  active.blur();
  // WebKit keeps sending keys to a blurred frame until the app's own window takes focus.
  if (active.hasAttribute(PROVIDER_FRAME_ATTRIBUTE)) root.defaultView?.focus();
}

/**
 * Any modal dialog is open, whoever owns its state. Covers both bits-ui dialogs
 * and hand-rolled `role="dialog"` overlays such as the sidebar's task modal, which
 * have no bits-ui attributes at all.
 *
 * bits-ui sets role/aria-modal unconditionally and keeps its content node mounted
 * for a frame after closing, so `data-state` is authoritative for those nodes and
 * the role-based clauses must exclude them — otherwise closing a dialog blurs the
 * terminal that is being handed focus back.
 */
export function hasOpenDialog(root: Document = document): boolean {
  return !!root.querySelector(
    '[data-dialog-content][data-state="open"], [role="dialog"][aria-modal="true"]:not([data-dialog-content]), [role="alertdialog"][aria-modal="true"]:not([data-dialog-content]), dialog[open]',
  );
}
