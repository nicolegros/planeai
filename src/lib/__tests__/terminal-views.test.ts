import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../xterm-surface", () => ({ createXtermSurface: vi.fn() }));
vi.mock("../terminal-pty", () => ({ tauriTerminalPty: {} }));

import type { TerminalPty, TerminalSurface } from "../terminal-view";
import {
  acquireTerminalView,
  disposeAllTerminalViews,
  disposeSessionTerminalViews,
  disposeTerminalView,
} from "../terminal-views";

const disposed: string[] = [];

function acquire(ptyKey: string) {
  return acquireTerminalView({
    ptyKey,
    kind: ptyKey.includes(":") ? "shell" : "agent",
    pty: {} as TerminalPty,
    createSurface: () => ({ dispose: () => disposed.push(ptyKey) }) as unknown as TerminalSurface,
  });
}

beforeEach(() => {
  disposed.length = 0;
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
});

afterEach(() => disposeAllTerminalViews());

describe("terminal view registry", () => {
  it("returns the same view for a pty key across acquisitions", () => {
    expect(acquire("s1")).toBe(acquire("s1"));
  });

  it("creates a fresh view after disposal", () => {
    const first = acquire("s1:1");
    disposeTerminalView("s1:1");

    expect(disposed).toEqual(["s1:1"]);
    expect(acquire("s1:1")).not.toBe(first);
  });

  it("disposes a session's agent and shell views only", () => {
    const s10 = acquire("s10");
    const s10Shell = acquire("s10:1");
    for (const key of ["s1", "s1:1", "s1:2"]) acquire(key);
    disposeSessionTerminalViews("s1");

    expect(disposed.sort()).toEqual(["s1", "s1:1", "s1:2"]);
    expect(acquire("s10")).toBe(s10);
    expect(acquire("s10:1")).toBe(s10Shell);
  });
});
