/**
 * jsdom has no layout, so every element reports zero size. The `tabbable`
 * library that bits-ui's focus scope relies on treats zero-sized elements as
 * untabbable, which makes the focus scope take a fallback path that never runs
 * in a real browser. Stubbing layout keeps focus-behaviour tests honest.
 *
 * Returns a restore function: these are prototype-level patches, so a test file
 * that installs them must undo them rather than relying on vitest's per-file
 * isolation staying enabled.
 */
export function stubLayoutAsVisible(): () => void {
  const originals = {
    getClientRects: Object.getOwnPropertyDescriptor(Element.prototype, "getClientRects"),
    getBoundingClientRect: Object.getOwnPropertyDescriptor(
      Element.prototype,
      "getBoundingClientRect",
    ),
    offsetParent: Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetParent"),
  };

  const rect = {
    x: 0,
    y: 0,
    width: 100,
    height: 20,
    top: 0,
    left: 0,
    right: 100,
    bottom: 20,
  };

  Object.defineProperty(Element.prototype, "getClientRects", {
    configurable: true,
    value(): DOMRectList {
      const list = [rect] as unknown as DOMRectList;
      Object.defineProperty(list, "item", { configurable: true, value: () => rect });
      return list;
    },
  });

  Object.defineProperty(Element.prototype, "getBoundingClientRect", {
    configurable: true,
    value: () => ({ ...rect, toJSON: () => rect }),
  });

  Object.defineProperty(HTMLElement.prototype, "offsetParent", {
    configurable: true,
    get(): Element | null {
      return this.parentElement;
    },
  });

  return () => {
    if (originals.getClientRects) {
      Object.defineProperty(Element.prototype, "getClientRects", originals.getClientRects);
    }
    if (originals.getBoundingClientRect) {
      Object.defineProperty(
        Element.prototype,
        "getBoundingClientRect",
        originals.getBoundingClientRect,
      );
    }
    if (originals.offsetParent) {
      Object.defineProperty(HTMLElement.prototype, "offsetParent", originals.offsetParent);
    }
  };
}

/** Resolve after `frames` animation frames, letting deferred focus work settle. */
export async function flushFrames(frames = 3): Promise<void> {
  for (let i = 0; i < frames; i += 1) {
    await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));
  }
}
