import { describe, it, expect, beforeEach } from "vitest";
import {
  initSession,
  getTabs,
  addTab,
  removeTab,
  setActiveTab,
  getActiveTabIndex,
  getTabCount,
  destroySession,
  setTabTitle,
} from "../session-tabs.svelte";

describe("session tabs", () => {
  beforeEach(() => {
    destroySession("s1");
    destroySession("s2");
  });

  it("new session starts with 1 tab (agent at index 0)", () => {
    initSession("s1");
    const tabs = getTabs("s1");
    expect(tabs).toEqual([{ index: 0, label: "Agent", icon: "bot" }]);
  });

  it("addTab creates helper tabs with incrementing indices", () => {
    initSession("s1");
    const idx1 = addTab("s1");
    const idx2 = addTab("s1");
    expect(idx1).toBe(1);
    expect(idx2).toBe(2);
    expect(getTabs("s1")).toEqual([
      { index: 0, label: "Agent", icon: "bot" },
      { index: 1, label: "Shell 1", icon: "terminal" },
      { index: 2, label: "Shell 2", icon: "terminal" },
    ]);
  });

  it("removeTab on index 0 is a no-op (agent pinned)", () => {
    initSession("s1");
    addTab("s1");
    removeTab("s1", 0);
    expect(getTabs("s1")).toEqual([
      { index: 0, label: "Agent", icon: "bot" },
      { index: 1, label: "Shell 1", icon: "terminal" },
    ]);
  });

  it("removeTab on helper tab removes it", () => {
    initSession("s1");
    addTab("s1");
    addTab("s1");
    removeTab("s1", 1);
    expect(getTabs("s1")).toEqual([
      { index: 0, label: "Agent", icon: "bot" },
      { index: 2, label: "Shell 1", icon: "terminal" },
    ]);
  });

  it("setActiveTab changes active tab index", () => {
    initSession("s1");
    addTab("s1");
    expect(getActiveTabIndex("s1")).toBe(0);
    setActiveTab("s1", 1);
    expect(getActiveTabIndex("s1")).toBe(1);
  });

  it("active tab is remembered per session", () => {
    initSession("s1");
    initSession("s2");
    addTab("s1");
    addTab("s2");
    setActiveTab("s1", 1);
    setActiveTab("s2", 1);
    // Switching back to s1 should still show tab 1
    expect(getActiveTabIndex("s1")).toBe(1);
    expect(getActiveTabIndex("s2")).toBe(1);
    setActiveTab("s2", 0);
    expect(getActiveTabIndex("s1")).toBe(1);
    expect(getActiveTabIndex("s2")).toBe(0);
  });

  it("getTabCount reflects current state", () => {
    initSession("s1");
    expect(getTabCount("s1")).toBe(1);
    addTab("s1");
    expect(getTabCount("s1")).toBe(2);
    addTab("s1");
    expect(getTabCount("s1")).toBe(3);
    removeTab("s1", 1);
    expect(getTabCount("s1")).toBe(2);
  });

  it("initSession with tabCount > 1 pre-creates helper tabs", () => {
    initSession("s1", 3);
    expect(getTabs("s1")).toEqual([
      { index: 0, label: "Agent", icon: "bot" },
      { index: 1, label: "Shell 1", icon: "terminal" },
      { index: 2, label: "Shell 2", icon: "terminal" },
    ]);
    expect(getTabCount("s1")).toBe(3);
  });

  it("removing active helper tab falls back to previous tab or tab 0", () => {
    initSession("s1", 3);
    setActiveTab("s1", 2);
    removeTab("s1", 2);
    // Should fall back to the previous tab (index 1)
    expect(getActiveTabIndex("s1")).toBe(1);

    // If we remove the last remaining helper, fall back to 0
    setActiveTab("s1", 1);
    removeTab("s1", 1);
    expect(getActiveTabIndex("s1")).toBe(0);
  });

  it("setTabTitle updates a shell tab label but not the agent tab", () => {
    initSession("s1", 2);
    setTabTitle("s1", 1, "cargo");
    const tabs = getTabs("s1");
    expect(tabs[0].label).toBe("Agent");
    expect(tabs[1].label).toBe("cargo");
  });

  it("setTabTitle on agent tab (index 0) is a no-op", () => {
    initSession("s1");
    setTabTitle("s1", 0, "vim");
    expect(getTabs("s1")[0].label).toBe("Agent");
  });

  it("setTabTitle is a no-op for unknown session or tab", () => {
    initSession("s1", 2);
    // Unknown session
    setTabTitle("unknown", 1, "vim");
    // Unknown tab index
    setTabTitle("s1", 99, "vim");
    // Should not throw, tabs unchanged
    expect(getTabs("s1")[1].label).toBe("Shell 1");
  });
});
