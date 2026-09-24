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
 */

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
  /** This tab's leaf is the focused leaf of the split tree. */
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

/**
 * Give up xterm's DOM focus when the terminal may not own the keyboard.
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
  // The DOM probe catches keyboard-owning dialogs whose open state App does not
  // model, such as the BranchCompare form or Preferences; a hand-maintained list
  // of flags drifts as dialogs are added.
  if (terminalMayOwnKeyboard(ownership) && !hasOpenDialog(root)) return;
  const active = root.activeElement;
  if (active instanceof HTMLElement && active.closest(".xterm")) active.blur();
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
