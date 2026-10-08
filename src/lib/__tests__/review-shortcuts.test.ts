import { describe, expect, it } from "vitest";
import { reviewShortcutsApply } from "../review-shortcuts";

describe("reviewShortcutsApply", () => {
  const owning = {
    visible: true,
    focused: true,
    defaultPrevented: false,
    zone: "terminal",
  } as const;

  it("applies while the visible Review tab's pane owns the keyboard", () => {
    expect(reviewShortcutsApply(owning)).toBe(true);
  });

  it("leaves keys to another pane, to keys already handled, and to other zones", () => {
    // A chord replayed from another pane's plugin frame must not send this tab's feedback.
    expect(reviewShortcutsApply({ ...owning, focused: false })).toBe(false);
    expect(reviewShortcutsApply({ ...owning, defaultPrevented: true })).toBe(false);
    expect(reviewShortcutsApply({ ...owning, visible: false })).toBe(false);
    expect(reviewShortcutsApply({ ...owning, zone: "sidebar" })).toBe(false);
  });
});
