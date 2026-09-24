/**
 * Regression tests for the sidebar keyboard-navigation trap.
 *
 * State captured by instrumentation in the running app: `activeSessionId` was
 * session A while the focused leaf's active tab was session B's agent, and
 * session B's xterm textarea owned `document.activeElement`. No pane had
 * `focused: true`, so Terminal.svelte's true -> false blur never fired, session
 * B kept DOM focus through every zone change, and xterm's stopPropagation() meant
 * `k` never reached the sidebar's window key handler.
 */
import { afterEach, describe, it, expect, vi } from "vitest";
import {
  hasOpenDialog,
  isTerminalPaneFocused,
  releaseTerminalDomFocus,
  terminalMayOwnKeyboard,
} from "../terminal-focus";
import type { TerminalPaneFocusInput } from "../terminal-focus";
import type { FocusZone } from "../focus.svelte";

const nonTerminalZones: FocusZone[] = ["sidebar", "editor", "explorer", "none"];

const base: TerminalPaneFocusInput = {
  isActiveTabInLeaf: true,
  isFocusedLeaf: true,
  belongsToActiveSession: true,
  zone: "terminal",
  pluginOverlayActive: false,
  modalOpen: false,
};

describe("isTerminalPaneFocused", () => {
  it("focuses the focused leaf's active tab while the terminal zone is active", () => {
    expect(isTerminalPaneFocused(base)).toBe(true);
  });

  it("releases focus for every non-terminal zone", () => {
    for (const zone of nonTerminalZones) {
      expect(isTerminalPaneFocused({ ...base, zone })).toBe(false);
    }
  });

  it("does not focus a pane in an unfocused leaf or an inactive tab", () => {
    expect(isTerminalPaneFocused({ ...base, isFocusedLeaf: false })).toBe(false);
    expect(isTerminalPaneFocused({ ...base, isActiveTabInLeaf: false })).toBe(false);
  });

  it("does not let a pane from the previous layout reclaim focus mid-transition", () => {
    // Deliberate guard: during a workspace swap the split tree still holds the
    // outgoing workspace's tabs while the selected session has already moved.
    expect(isTerminalPaneFocused({ ...base, belongsToActiveSession: false })).toBe(false);
  });

  it("never focuses a pane behind a plugin overlay or a modal", () => {
    expect(isTerminalPaneFocused({ ...base, pluginOverlayActive: true })).toBe(false);
    expect(isTerminalPaneFocused({ ...base, modalOpen: true })).toBe(false);
  });
});

describe("terminalMayOwnKeyboard", () => {
  const owned = { zone: "terminal" as FocusZone, modalOpen: false, pluginOverlayActive: false };

  it("allows ownership only in the terminal zone with nothing covering it", () => {
    expect(terminalMayOwnKeyboard(owned)).toBe(true);
  });

  it("denies ownership for every non-terminal zone", () => {
    for (const zone of nonTerminalZones) {
      expect(terminalMayOwnKeyboard({ ...owned, zone })).toBe(false);
    }
  });

  it("denies ownership behind a modal or plugin overlay even in the terminal zone", () => {
    // A modal opening during an async layout load can otherwise be handed DOM
    // focus by the trailing focus request, and xterm then swallows its keys.
    expect(terminalMayOwnKeyboard({ ...owned, modalOpen: true })).toBe(false);
    expect(terminalMayOwnKeyboard({ ...owned, pluginOverlayActive: true })).toBe(false);
  });

  it("shares its conditions with isTerminalPaneFocused so the two cannot drift", () => {
    for (const modalOpen of [true, false]) {
      for (const pluginOverlayActive of [true, false]) {
        for (const zone of ["terminal", ...nonTerminalZones] as FocusZone[]) {
          const ownership = { zone, modalOpen, pluginOverlayActive };
          const focused = isTerminalPaneFocused({ ...base, ...ownership });
          if (!terminalMayOwnKeyboard(ownership)) expect(focused).toBe(false);
        }
      }
    }
  });
});

describe("releaseTerminalDomFocus", () => {
  function mountTerminalWithFocus(): HTMLTextAreaElement {
    document.body.innerHTML = "";
    const xterm = document.createElement("div");
    xterm.className = "xterm";
    const textarea = document.createElement("textarea");
    textarea.className = "xterm-helper-textarea";
    xterm.append(textarea);
    document.body.append(xterm);
    textarea.focus();
    return textarea;
  }

  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("blurs an xterm textarea when the zone has moved to the sidebar", () => {
    const textarea = mountTerminalWithFocus();
    expect(document.activeElement).toBe(textarea);

    releaseTerminalDomFocus({ zone: "sidebar", modalOpen: false, pluginOverlayActive: false });

    expect(document.activeElement).toBe(document.body);
  });

  it("leaves the xterm textarea focused while the terminal zone is active", () => {
    const textarea = mountTerminalWithFocus();

    releaseTerminalDomFocus({ zone: "terminal", modalOpen: false, pluginOverlayActive: false });

    expect(document.activeElement).toBe(textarea);
  });

  it("leaves focus alone when it is outside any terminal", () => {
    document.body.innerHTML = "";
    const input = document.createElement("input");
    document.body.append(input);
    input.focus();

    releaseTerminalDomFocus({ zone: "sidebar", modalOpen: false, pluginOverlayActive: false });

    expect(document.activeElement).toBe(input);
  });

  it("blurs a terminal that was handed focus behind an open modal", () => {
    const textarea = mountTerminalWithFocus();

    releaseTerminalDomFocus({ zone: "terminal", modalOpen: true, pluginOverlayActive: false });

    expect(document.activeElement).not.toBe(textarea);
  });

  it("does nothing when nothing is focused", () => {
    document.body.innerHTML = "";
    expect(() =>
      releaseTerminalDomFocus({ zone: "sidebar", modalOpen: false, pluginOverlayActive: false }),
    ).not.toThrow();
  });
});

describe("hasOpenDialog", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("detects an open bits-ui dialog", () => {
    document.body.innerHTML = '<div data-dialog-content data-state="open"></div>';
    expect(hasOpenDialog()).toBe(true);
  });

  it("ignores a closed dialog", () => {
    document.body.innerHTML = '<div data-dialog-content data-state="closed"></div>';
    expect(hasOpenDialog()).toBe(false);
  });

  it("detects a hand-rolled modal overlay with no bits-ui attributes", () => {
    // The sidebar's task modal is a plain div with role/aria-modal only.
    document.body.innerHTML = '<div role="dialog" aria-modal="true" tabindex="-1"></div>';
    expect(hasOpenDialog()).toBe(true);
  });

  it("ignores a non-modal role=dialog element", () => {
    document.body.innerHTML = '<div role="dialog"></div>';
    expect(hasOpenDialog()).toBe(false);
  });

  it("releases terminal focus for a dialog App does not model, even in the terminal zone", () => {
    // A hand-maintained list of App flags drifts as dialogs are added, and xterm
    // would swallow the keys the dialog needs.
    document.body.innerHTML =
      '<div data-dialog-content data-state="open"></div><div class="xterm"><textarea></textarea></div>';
    const textarea = document.querySelector("textarea")!;
    textarea.focus();
    expect(document.activeElement).toBe(textarea);

    releaseTerminalDomFocus({ zone: "terminal", modalOpen: false, pluginOverlayActive: false });

    expect(document.activeElement).not.toBe(textarea);
  });
});

describe("releaseTerminalDomFocus cost", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("does not scan the document when focus is outside a terminal", () => {
    // Runs on every focusin in the app. `zone: "terminal"` is the case that does
    // NOT short-circuit on ownership, so without the cheap gate every focus move
    // between non-terminal elements scanned the whole document — a scan that
    // grows with the number of mounted terminals.
    document.body.innerHTML =
      '<div class="xterm"><textarea></textarea></div><input id="outside" />';
    (document.querySelector("#outside") as HTMLInputElement).focus();

    const querySelector = vi.spyOn(Document.prototype, "querySelector");
    try {
      releaseTerminalDomFocus({ zone: "terminal", modalOpen: false, pluginOverlayActive: false });
      expect(querySelector).not.toHaveBeenCalled();
    } finally {
      querySelector.mockRestore();
    }
  });

  it("still scans when focus is inside a terminal that may own the keyboard", () => {
    document.body.innerHTML = '<div class="xterm"><textarea></textarea></div>';
    (document.querySelector("textarea") as HTMLTextAreaElement).focus();

    const querySelector = vi.spyOn(Document.prototype, "querySelector");
    try {
      releaseTerminalDomFocus({ zone: "terminal", modalOpen: false, pluginOverlayActive: false });
      expect(querySelector).toHaveBeenCalled();
    } finally {
      querySelector.mockRestore();
    }
  });
});
