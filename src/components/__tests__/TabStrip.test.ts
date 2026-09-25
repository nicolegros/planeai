import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";
import TabStrip from "../TabStrip.svelte";

afterEach(() => {
  document.body.replaceChildren();
});

describe("TabStrip", () => {
  it("uses stable tab IDs when operation indices are shared", async () => {
    const select = vi.fn();
    const target = document.body.appendChild(document.createElement("div"));
    const component = mount(TabStrip, {
      target,
      props: {
        tabs: [
          { id: "session:editor:/one.ts", index: -2, label: "one.ts", icon: "file" },
          { id: "session:editor:/two.ts", index: -2, label: "two.ts", icon: "file" },
        ],
        activeTabIndex: -2,
        activeTabId: "session:editor:/two.ts",
        onSelectTab: select,
      },
    });
    await tick();

    const tabs = Array.from(document.querySelectorAll('[role="tab"]'));
    expect(tabs).toHaveLength(2);
    expect(tabs[0]?.getAttribute("aria-selected")).toBe("false");
    expect(tabs[1]?.getAttribute("aria-selected")).toBe("true");

    (tabs[1] as HTMLButtonElement).click();
    expect(select).toHaveBeenCalledWith(-2, "session:editor:/two.ts");
    unmount(component);
  });

  it("reports a double-clicked tab", async () => {
    const doubleClick = vi.fn();
    const target = document.body.appendChild(document.createElement("div"));
    const component = mount(TabStrip, {
      target,
      props: {
        tabs: [{ id: "session-1", index: 0, label: "Fix login", icon: "bot" }],
        activeTabIndex: 0,
        onSelectTab: vi.fn(),
        onTabDoubleClick: doubleClick,
      },
    });
    await tick();

    document
      .querySelector('[role="tab"]')!
      .dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    expect(doubleClick).toHaveBeenCalledWith(0, "session-1");
    unmount(component);
  });
});
