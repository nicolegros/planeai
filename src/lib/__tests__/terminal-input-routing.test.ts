import { describe, expect, it } from "vitest";
import appSource from "../../App.svelte?raw";
import prPanelSource from "../../components/PrPanel.svelte?raw";
import reviewTabSource from "../../components/ReviewTab.svelte?raw";
import terminalSource from "../../components/Terminal.svelte?raw";

describe("user-input invalidation routing", () => {
  it("routes terminal-originated input through the invalidate-before-write helper", () => {
    expect(terminalSource).toContain(
      "writeTerminalUserInput(bytes, () => onUserInput?.(), queueWrite)",
    );
  });

  it("invalidates before every direct user-initiated PTY write", () => {
    expect(appSource).toMatch(/recordUserInput\(activeSessionId\);\s*pty\.write\(activeSessionId/);
    expect(reviewTabSource).toMatch(
      /recordUserInput\(sessionId\);\s*await pty\.write\(sessionId, bytes\)/,
    );
    expect(prPanelSource).toMatch(/recordUserInput\(sessionId\);\s*await pty\.write\(sessionId/);
  });
});
