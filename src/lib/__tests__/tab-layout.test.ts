import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({ close: vi.fn() })),
}));

vi.mock("../api", () => ({
  pty: {
    closeTab: vi.fn(() => Promise.resolve()),
    incrementTabCount: vi.fn(() => Promise.resolve()),
  },
}));

vi.mock("../session-orchestrator.svelte", () => ({
  getActiveSessionId: vi.fn(() => "s1"),
}));

vi.mock("../focus.svelte", () => ({
  refocusTerminal: vi.fn(),
}));

import { pty } from "../api";
import { refocusTerminal } from "../focus.svelte";
import { initSession, getTabs, destroySession, setActiveTab } from "../session-tabs.svelte";
import { handleCloseTab, closeShellTab, handleNewTab } from "../tab-layout.svelte";

describe("handleCloseTab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    destroySession("s1");
    initSession("s1", 2);
  });

  it("calls pty.closeTab before removing tab from local state", async () => {
    setActiveTab("s1", 1);

    let tabCountAtCloseCall = -1;
    vi.mocked(pty.closeTab).mockImplementation(() => {
      tabCountAtCloseCall = getTabs("s1").length;
      return Promise.resolve();
    });

    await handleCloseTab();

    expect(pty.closeTab).toHaveBeenCalledWith("s1", 1);
    expect(tabCountAtCloseCall).toBe(2);
    expect(getTabs("s1").length).toBe(1);
  });

  it("does not call pty.closeTab when active tab is 0 (closes window instead)", async () => {
    await handleCloseTab();
    expect(pty.closeTab).not.toHaveBeenCalled();
  });
});

describe("closeShellTab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    destroySession("s1");
    initSession("s1", 3);
  });

  it("calls pty.closeTab then removes tab", async () => {
    let tabCountAtCloseCall = -1;
    vi.mocked(pty.closeTab).mockImplementation(() => {
      tabCountAtCloseCall = getTabs("s1").length;
      return Promise.resolve();
    });

    await closeShellTab("s1", 2);

    expect(pty.closeTab).toHaveBeenCalledWith("s1", 2);
    expect(tabCountAtCloseCall).toBe(3);
    expect(getTabs("s1").length).toBe(2);
  });
});

describe("handleNewTab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    destroySession("s1");
    initSession("s1", 1);
  });

  it("increments tab count in db", async () => {
    await handleNewTab();
    expect(pty.incrementTabCount).toHaveBeenCalledWith("s1");
  });

  it("calls refocusTerminal so the new tab receives focus (PLA-235)", async () => {
    await handleNewTab();
    expect(refocusTerminal).toHaveBeenCalled();
  });
});
