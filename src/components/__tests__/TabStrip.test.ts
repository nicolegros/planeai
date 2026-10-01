import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";
import TabStrip from "../TabStrip.svelte";

afterEach(() => {
  document.body.replaceChildren();
});

describe("TabStrip", () => {
  it("selects tabs by their pty key", async () => {
    const select = vi.fn();
    const target = document.body.appendChild(document.createElement("div"));
    const component = mount(TabStrip, {
      target,
      props: {
        tabs: [
          { id: "session:editor:/one.ts", label: "one.ts", icon: "file", type: "editor" },
          { id: "session:editor:/two.ts", label: "two.ts", icon: "file", type: "editor" },
        ],
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
    expect(select).toHaveBeenCalledWith("session:editor:/two.ts");
    unmount(component);
  });

  it("shows a tab's detail after its label", async () => {
    const target = document.body.appendChild(document.createElement("div"));
    const component = mount(TabStrip, {
      target,
      props: {
        tabs: [{ id: "s1", label: "Fix bug (2)", detail: "Other", icon: "bot", type: "agent" }],
        activeTabId: "s1",
        onSelectTab: vi.fn(),
      },
    });
    await tick();

    expect(document.querySelector('[role="tab"]')?.textContent?.trim()).toBe("Fix bug (2) · Other");
    unmount(component);
  });

  it("reports a double-clicked tab", async () => {
    const doubleClick = vi.fn();
    const target = document.body.appendChild(document.createElement("div"));
    const component = mount(TabStrip, {
      target,
      props: {
        tabs: [{ id: "session-1", label: "Fix login", icon: "bot", type: "agent" }],
        activeTabId: "session-1",
        onSelectTab: vi.fn(),
        onTabDoubleClick: doubleClick,
      },
    });
    await tick();

    document
      .querySelector('[role="tab"]')!
      .dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    expect(doubleClick).toHaveBeenCalledWith("session-1");
    unmount(component);
  });
});
