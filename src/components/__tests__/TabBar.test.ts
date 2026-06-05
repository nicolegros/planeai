import { describe, it, expect, vi } from "vitest";
import { mount } from "svelte";
import TabBar from "../TabBar.svelte";
import type { Tab } from "../../lib/session-tabs.svelte";

interface TabBarProps {
  tabs: Tab[];
  activeTabIndex: number;
  onSelect: (index: number) => void;
  onClose: (index: number) => void;
  onAdd: () => void;
}

function renderTabBar(props: TabBarProps) {
  const target = document.createElement("div");
  mount(TabBar, { target, props });
  return target;
}

describe("TabBar", () => {
  it("renders tabs even when there is only one tab", () => {
    const target = renderTabBar({
      tabs: [{ index: 0, label: "Agent" }],
      activeTabIndex: 0,
      onSelect: vi.fn(),
      onClose: vi.fn(),
      onAdd: vi.fn(),
    });

    const tablist = target.querySelector("[role='tablist']");
    expect(tablist).not.toBeNull();

    const tabs = target.querySelectorAll("[role='tab']");
    expect(tabs.length).toBe(1);
    expect(tabs[0].textContent).toContain("Agent");
  });

  it("renders a + button that fires onAdd when clicked", () => {
    const onAdd = vi.fn();
    const target = renderTabBar({
      tabs: [{ index: 0, label: "Agent" }],
      activeTabIndex: 0,
      onSelect: vi.fn(),
      onClose: vi.fn(),
      onAdd,
    });

    const addBtn = target.querySelector("[aria-label='New tab']") as HTMLElement;
    expect(addBtn).not.toBeNull();
    addBtn.click();
    expect(onAdd).toHaveBeenCalledOnce();
  });

  it("does not render a close button on the Agent tab (index 0)", () => {
    const target = renderTabBar({
      tabs: [{ index: 0, label: "Agent" }, { index: 1, label: "Shell 1" }],
      activeTabIndex: 0,
      onSelect: vi.fn(),
      onClose: vi.fn(),
      onAdd: vi.fn(),
    });

    const closeAgent = target.querySelector("[aria-label='Close Agent']");
    expect(closeAgent).toBeNull();
  });

  it("renders close buttons on shell tabs that fire onClose", () => {
    const onClose = vi.fn();
    const target = renderTabBar({
      tabs: [{ index: 0, label: "Agent" }, { index: 1, label: "Shell 1" }],
      activeTabIndex: 0,
      onSelect: vi.fn(),
      onClose,
      onAdd: vi.fn(),
    });

    const closeShell = target.querySelector("[aria-label='Close Shell 1']") as HTMLElement;
    expect(closeShell).not.toBeNull();
    closeShell.click();
    expect(onClose).toHaveBeenCalledWith(1);
  });

  it("sets aria-selected=true on the active tab", () => {
    const target = renderTabBar({
      tabs: [{ index: 0, label: "Agent" }, { index: 1, label: "Shell 1" }],
      activeTabIndex: 1,
      onSelect: vi.fn(),
      onClose: vi.fn(),
      onAdd: vi.fn(),
    });

    const tabs = target.querySelectorAll("[role='tab']");
    expect(tabs[0].getAttribute("aria-selected")).toBe("false");
    expect(tabs[1].getAttribute("aria-selected")).toBe("true");
  });
});
