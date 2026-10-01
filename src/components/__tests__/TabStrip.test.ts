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

  function mountDraggable(onTabDrop: (e: DragEvent, index: number) => void) {
    const target = document.body.appendChild(document.createElement("div"));
    return mount(TabStrip, {
      target,
      props: {
        tabs: [
          { id: "a", label: "A", icon: "bot", type: "agent" },
          { id: "a:1", label: "Shell", icon: "terminal", type: "shell" },
        ],
        activeTabId: "a",
        draggable: true,
        onSelectTab: vi.fn(),
        onTabDrop,
      },
    });
  }

  function dragTo(element: Element, clientX: number) {
    element.dispatchEvent(new MouseEvent("dragover", { bubbles: true, cancelable: true, clientX }));
    element.dispatchEvent(new MouseEvent("drop", { bubbles: true, cancelable: true, clientX }));
  }

  it("drops at the end anywhere past the last tab", async () => {
    const drop = vi.fn();
    const component = mountDraggable(drop);
    await tick();

    dragTo(document.querySelector('[role="tablist"]')!, 500);
    expect(drop).toHaveBeenCalledWith(expect.anything(), 2);
    unmount(component);
  });

  it("drops before or after a tab by which half of it is under the pointer", async () => {
    const drop = vi.fn();
    const component = mountDraggable(drop);
    await tick();
    const first = document.querySelectorAll('[role="tab"]')[0].parentElement!;
    first.getBoundingClientRect = () => ({ left: 0, width: 100 }) as DOMRect;

    dragTo(first, 20);
    expect(drop).toHaveBeenLastCalledWith(expect.anything(), 0);
    dragTo(first, 80);
    expect(drop).toHaveBeenLastCalledWith(expect.anything(), 1);
    unmount(component);
  });
});
