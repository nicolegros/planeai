import { afterEach, describe, expect, it, vi } from "vitest";
import { definePointerDrag } from "../pointer-drag.svelte";

function pointer(type: string, clientX: number, clientY: number) {
  window.dispatchEvent(new MouseEvent(type, { clientX, clientY, bubbles: true }));
}

const down = (x: number, y: number, button = 0) =>
  new PointerEvent("pointerdown", { button, clientX: x, clientY: y });

type Target = { id: string };
/** How far the targets scrolled to the left of their place. */
let scrolled = 0;
/** Targets by x: "a" left of 100, "b" past 200, nothing between. */
const hitTest = (x: number): Target | null => {
  const at = x + scrolled;
  return at < 100 ? { id: "a" } : at > 200 ? { id: "b" } : null;
};

const rows = definePointerDrag<string, Target>({ hitTest, bodyClass: "rows-dragging" });
const cards = definePointerDrag<number, Target>({ hitTest, bodyClass: "cards-dragging" });

afterEach(() => {
  pointer("pointerup", 150, 0);
  document.body.className = "";
  scrolled = 0;
});

describe("definePointerDrag", () => {
  it("is a click, not a drag, until the pointer moves past a threshold", () => {
    const drop = vi.fn();
    rows.press("r1", down(10, 5), drop);
    pointer("pointermove", 12, 6);
    expect(rows.current).toBeNull();
    expect(document.body.classList.contains("rows-dragging")).toBe(false);
    pointer("pointerup", 12, 6);
    expect(drop).not.toHaveBeenCalled();
  });

  it("tracks the payload, the target under the pointer and the point once dragging", () => {
    rows.press("r1", down(10, 5), vi.fn());
    pointer("pointermove", 250, 40);
    expect(rows.current).toEqual({
      payload: "r1",
      target: { id: "b" },
      point: { x: 250, y: 40 },
      origin: { x: 10, y: 5 },
    });
    expect(document.body.classList.contains("rows-dragging")).toBe(true);
    pointer("pointermove", 150, 40);
    expect(rows.current?.target).toBeNull();
  });

  it("re-reads the target under a still pointer when something scrolls", () => {
    const drop = vi.fn();
    rows.press("r1", down(10, 5), drop);
    pointer("pointermove", 150, 40);
    expect(rows.current?.target).toBeNull();
    scrolled = 100;
    document.body.dispatchEvent(new Event("scroll"));
    expect(rows.current).toEqual({
      payload: "r1",
      target: { id: "b" },
      point: { x: 150, y: 40 },
      origin: { x: 10, y: 5 },
    });
    pointer("pointerup", 150, 40);
    expect(drop).toHaveBeenCalledWith("r1", { id: "b" });
  });

  it("drops on the target under the pointer and clears the drag", () => {
    const drop = vi.fn();
    rows.press("r1", down(10, 5), drop);
    pointer("pointermove", 250, 40);
    pointer("pointerup", 250, 40);
    expect(drop).toHaveBeenCalledWith("r1", { id: "b" });
    expect(rows.current).toBeNull();
    expect(document.body.classList.contains("rows-dragging")).toBe(false);
  });

  it("does not drop when released over nothing", () => {
    const drop = vi.fn();
    rows.press("r1", down(10, 5), drop);
    pointer("pointermove", 150, 40);
    pointer("pointerup", 150, 40);
    expect(drop).not.toHaveBeenCalled();
  });

  it("cancels on Escape without dropping", () => {
    const drop = vi.fn();
    rows.press("r1", down(10, 5), drop);
    pointer("pointermove", 250, 40);
    const escape = new KeyboardEvent("keydown", { key: "Escape", cancelable: true });
    window.dispatchEvent(escape);
    expect(escape.defaultPrevented).toBe(true);
    expect(rows.current).toBeNull();
    pointer("pointerup", 250, 40);
    expect(drop).not.toHaveBeenCalled();
  });

  it("exposes the payload alone, so readers of it do not follow every move", () => {
    rows.press("r1", down(10, 5), vi.fn());
    expect(rows.payload).toBeNull();
    pointer("pointermove", 250, 40);
    expect(rows.payload).toBe("r1");
    pointer("pointerup", 250, 40);
    expect(rows.payload).toBeNull();
  });

  it("swallows the click that follows a release after Escape, so nothing gets selected", () => {
    const clicked = vi.fn();
    const button = document.body.appendChild(document.createElement("button"));
    button.addEventListener("click", clicked);
    rows.press("r1", down(10, 5), vi.fn());
    pointer("pointermove", 250, 40);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true }));
    pointer("pointerup", 250, 40);
    button.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    expect(clicked).not.toHaveBeenCalled();

    button.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    expect(clicked).toHaveBeenCalledTimes(1);
    button.remove();
  });

  it("cancels when the pointer is cancelled or the window loses focus", () => {
    const drop = vi.fn();
    for (const end of [
      () => pointer("pointercancel", 250, 40),
      () => window.dispatchEvent(new Event("blur")),
    ]) {
      rows.press("r1", down(10, 5), drop);
      pointer("pointermove", 250, 40);
      end();
      expect(rows.current).toBeNull();
      expect(document.body.classList.contains("rows-dragging")).toBe(false);
      pointer("pointerup", 250, 40);
    }
    expect(drop).not.toHaveBeenCalled();
  });

  it("lets a new press of any kind end the one before it", () => {
    const dropRow = vi.fn();
    const dropCard = vi.fn();
    rows.press("stale", down(10, 5), dropRow);
    pointer("pointermove", 250, 40);
    cards.press(7, down(10, 5), dropCard);
    expect(rows.current).toBeNull();
    expect(document.body.classList.contains("rows-dragging")).toBe(false);
    pointer("pointermove", 250, 40);
    expect(cards.current?.payload).toBe(7);
    pointer("pointerup", 250, 40);
    expect(dropRow).not.toHaveBeenCalled();
    expect(dropCard).toHaveBeenCalledWith(7, { id: "b" });
  });

  it("ignores presses with other buttons", () => {
    const drop = vi.fn();
    rows.press("r1", down(10, 5, 2), drop);
    pointer("pointermove", 250, 40);
    expect(rows.current).toBeNull();
    pointer("pointerup", 250, 40);
    expect(drop).not.toHaveBeenCalled();
  });
});
