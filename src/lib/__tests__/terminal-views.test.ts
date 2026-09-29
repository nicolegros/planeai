import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../xterm-surface", () => ({ createXtermSurface: vi.fn() }));
vi.mock("../terminal-pty", () => ({ tauriTerminalPty: {} }));

import type { TerminalPty, TerminalSurface } from "../terminal-view";
import {
  acquireTerminalView,
  disposeAllTerminalViews,
  disposeSessionTerminalViews,
  disposeTerminalView,
  hasTerminalView,
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
    expect(hasTerminalView("s1:1")).toBe(false);
    expect(acquire("s1:1")).not.toBe(first);
  });

  it("disposes a session's agent and shell views only", () => {
    for (const key of ["s1", "s1:1", "s1:2", "s10", "s10:1"]) acquire(key);
    disposeSessionTerminalViews("s1");

    expect(disposed.sort()).toEqual(["s1", "s1:1", "s1:2"]);
    expect(hasTerminalView("s10")).toBe(true);
    expect(hasTerminalView("s10:1")).toBe(true);
  });
});
