import { afterEach, describe, expect, it, vi } from "vitest";
import { paneZone, pressTab, stripIndex, tabDrag } from "../tab-drag.svelte";

const rect = (left: number, top: number, width: number, height: number) =>
  ({ left, top, width, height }) as DOMRect;

function pointer(type: string, clientX: number, clientY: number) {
  window.dispatchEvent(new MouseEvent(type, { clientX, clientY, bubbles: true }));
}

afterEach(() => {
  document.body.replaceChildren();
  pointer("pointerup", 0, 0);
});

describe("stripIndex", () => {
  it("inserts before or after a tab by which half is under the pointer", () => {
    expect(stripIndex(rect(100, 0, 80, 38), 2, 120)).toBe(2);
    expect(stripIndex(rect(100, 0, 80, 38), 2, 170)).toBe(3);
  });
});

describe("paneZone", () => {
  const pane = rect(0, 0, 400, 200);

  it("splits on the side of the nearest edge within a quarter of the pane", () => {
    expect(paneZone(pane, 20, 100)).toBe("left");
    expect(paneZone(pane, 390, 100)).toBe("right");
    expect(paneZone(pane, 200, 10)).toBe("up");
    expect(paneZone(pane, 200, 190)).toBe("down");
  });

  it("moves into the pane anywhere else", () => {
    expect(paneZone(pane, 200, 100)).toBe("center");
  });
});

describe("pressTab", () => {
  function layoutDom() {
    document.body.innerHTML = `
      <div data-tab-strip="p1" data-tab-count="2">
        <div data-tab-index="0" id="tab0"></div><div data-tab-index="1" id="tab1"></div><div id="rest"></div>
      </div>
      <div data-pane-drop="p2" id="pane"></div>`;
    const at: Record<string, string> = { "10": "tab0", "60": "rest", "250": "pane" };
    document.elementFromPoint = (x: number) => document.getElementById(at[String(x)] ?? "") ?? null;
    for (const id of ["tab0", "tab1"]) {
      document.getElementById(id)!.getBoundingClientRect = () =>
        rect(id === "tab0" ? 0 : 30, 0, 30, 38);
    }
    document.getElementById("pane")!.getBoundingClientRect = () => rect(200, 0, 400, 400);
  }

  it("is a click, not a drag, until the pointer moves past a threshold", () => {
    const drop = vi.fn();
    pressTab("a", new PointerEvent("pointerdown", { button: 0, clientX: 10, clientY: 5 }), drop);
    pointer("pointermove", 12, 6);
    expect(tabDrag.dragged).toBeNull();
    pointer("pointerup", 12, 6);
    expect(drop).not.toHaveBeenCalled();
  });

  it("drops at the end of a strip when released past its tabs", () => {
    layoutDom();
    const drop = vi.fn();
    pressTab("a", new PointerEvent("pointerdown", { button: 0, clientX: 10, clientY: 5 }), drop);
    pointer("pointermove", 60, 5);
    expect(tabDrag.target).toEqual({ kind: "strip", paneId: "p1", index: 2 });
    pointer("pointerup", 60, 5);
    expect(drop).toHaveBeenCalledWith("a", { kind: "strip", paneId: "p1", index: 2 });
    expect(tabDrag.dragged).toBeNull();
  });

  it("splits a pane when released near its edge", () => {
    layoutDom();
    const drop = vi.fn();
    pressTab("a", new PointerEvent("pointerdown", { button: 0, clientX: 10, clientY: 5 }), drop);
    pointer("pointermove", 250, 200);
    pointer("pointerup", 250, 200);
    expect(drop).toHaveBeenCalledWith("a", { kind: "pane", paneId: "p2", zone: "left" });
  });

  it("cancels on Escape", () => {
    layoutDom();
    const drop = vi.fn();
    pressTab("a", new PointerEvent("pointerdown", { button: 0, clientX: 10, clientY: 5 }), drop);
    pointer("pointermove", 250, 200);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(tabDrag.target).toBeNull();
    pointer("pointerup", 250, 200);
    expect(drop).not.toHaveBeenCalled();
  });

  it("ignores presses with other buttons", () => {
    const drop = vi.fn();
    pressTab("a", new PointerEvent("pointerdown", { button: 2, clientX: 10, clientY: 5 }), drop);
    pointer("pointermove", 250, 200);
    expect(tabDrag.dragged).toBeNull();
  });
});
