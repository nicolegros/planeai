import { afterEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import ContextMenu, { type MenuItem } from "../ContextMenu.svelte";

function key(target: Element, key: string, init: KeyboardEventInit = {}) {
  target.dispatchEvent(
    new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init }),
  );
  flushSync();
}

function focusedLabel(): string | undefined {
  return document.activeElement?.textContent?.replace(/›/, "").trim();
}

describe("ContextMenu keyboard", () => {
  let component: ReturnType<typeof mount> | undefined;
  const onClose = vi.fn();
  const moveToDone = vi.fn();

  function render(items: MenuItem[]) {
    component = mount(ContextMenu, {
      target: document.body,
      props: { x: 0, y: 0, items, onClose },
    });
    flushSync();
  }

  const items: MenuItem[] = [
    { label: "Edit task", onSelect: vi.fn() },
    { label: "Change status", children: [{ label: "Done", onSelect: moveToDone }] },
    { separator: true },
    { label: "Unavailable", disabled: true, onSelect: vi.fn() },
    { label: "Delete", danger: true, onSelect: vi.fn() },
  ];

  afterEach(() => {
    if (component) unmount(component);
    component = undefined;
    document.body.innerHTML = "";
    vi.clearAllMocks();
  });

  it("focuses the first item and keeps Tab and arrows inside the menu, skipping disabled items", () => {
    render(items);
    expect(focusedLabel()).toBe("Edit task");

    key(document.activeElement!, "ArrowUp");
    expect(focusedLabel()).toBe("Delete");
    key(document.activeElement!, "Tab");
    expect(focusedLabel()).toBe("Edit task");
    key(document.activeElement!, "Tab", { shiftKey: true });
    expect(focusedLabel()).toBe("Delete");
  });

  it("moves to the last item on ArrowUp when the menu container has focus", () => {
    render(items);
    const menu = document.querySelector<HTMLElement>("[role='menu']")!;
    menu.focus();
    key(menu, "ArrowUp");
    expect(focusedLabel()).toBe("Delete");
  });

  it("opens a submenu from the keyboard and returns to its trigger", () => {
    render(items);
    key(document.activeElement!, "ArrowDown");
    expect(focusedLabel()).toBe("Change status");

    key(document.activeElement!, "ArrowRight");
    expect(focusedLabel()).toBe("Done");

    key(document.activeElement!, "ArrowLeft");
    expect(focusedLabel()).toBe("Change status");
    expect(document.querySelectorAll("[role='menu']")).toHaveLength(1);

    key(document.activeElement!, "Enter");
    (document.activeElement as HTMLButtonElement).click();
    expect(moveToDone).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("moves focus to the hovered row so Enter acts on it", () => {
    render(items);
    const remove = [...document.querySelectorAll<HTMLButtonElement>("[role='menuitem']")].find(
      (b) => b.textContent?.trim() === "Delete",
    )!;
    remove.dispatchEvent(new MouseEvent("mouseenter"));
    expect(focusedLabel()).toBe("Delete");
  });

  it("renders submenu items with the same roles, checks and disabled handling", () => {
    render([
      {
        label: "Group",
        children: [
          { label: "By status", checked: true, radio: true, onSelect: vi.fn() },
          { label: "Unavailable", disabled: true, onSelect: vi.fn() },
          { label: "By project", checked: false, radio: true, onSelect: vi.fn() },
        ],
      },
    ]);
    key(document.activeElement!, "ArrowRight");

    const [main, submenu] = document.querySelectorAll<HTMLElement>("[role='menu']");
    const status = submenu.querySelector<HTMLButtonElement>(
      "[role='menuitemradio'][aria-checked='true']",
    )!;
    expect(status.textContent?.trim()).toBe("By status");
    expect(status.querySelector("svg")).toBeTruthy();
    expect(submenu.querySelector<HTMLButtonElement>("button[disabled]")?.textContent?.trim()).toBe(
      "Unavailable",
    );
    // The main list has no checkable items, so it reserves no check column.
    expect(main.querySelector("button span.size-3\\.5")).toBeNull();

    expect(focusedLabel()).toBe("By status");
    key(document.activeElement!, "ArrowDown");
    expect(focusedLabel()).toBe("By project");
  });

  it("closes on Escape without letting keys reach window shortcuts", () => {
    const windowKeydown = vi.fn();
    window.addEventListener("keydown", windowKeydown);
    render(items);

    key(document.activeElement!, "ArrowDown");
    key(document.activeElement!, "Escape");

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(windowKeydown).not.toHaveBeenCalled();
    window.removeEventListener("keydown", windowKeydown);
  });
});
