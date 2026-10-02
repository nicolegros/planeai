/**
 * Tab drag: moves TaskWorkspace tabs with pointer events.
 *
 * HTML5 drag-and-drop cannot be used: Tauri's native drag-drop handler, which
 * file drops from the OS need for real paths, swallows the webview's drag
 * events. Targets are found by hit-testing marked elements instead:
 * `data-tab-strip="<paneId>"` (with `data-tab-count`) holding
 * `data-tab-index` tabs, and `data-pane-drop="<paneId>"` pane bodies.
 */
import type { NavDirection } from "./layout-tree";

export type PaneDropZone = NavDirection | "center";

export type TabDropTarget =
  | { kind: "strip"; paneId: string; index: number }
  | { kind: "pane"; paneId: string; zone: PaneDropZone };

/** A press moves this far before it becomes a drag, so clicks still select. */
const DRAG_THRESHOLD_PX = 4;
/** Share of a pane's size, from an edge, that splits it on that side. */
const EDGE_ZONE = 0.25;

let dragged = $state<string | null>(null);
let target = $state<TabDropTarget | null>(null);
/** Ends the press in progress; a new press must never inherit a stale one. */
let endActivePress: (() => void) | null = null;

/** The tab being dragged and where it would land, for drop indicators. */
export const tabDrag = {
  get dragged(): string | null {
    return dragged;
  },
  get target(): TabDropTarget | null {
    return target;
  },
};

/** Insert position over tab `index`: before or after it by which half is under `x`. */
export function stripIndex(rect: DOMRect, index: number, x: number): number {
  return x > rect.left + rect.width / 2 ? index + 1 : index;
}

/** Where over a pane a tab would land: a split near an edge, else into the pane. */
export function paneZone(rect: DOMRect, x: number, y: number): PaneDropZone {
  const horizontal = (x - rect.left) / rect.width;
  const vertical = (y - rect.top) / rect.height;
  const edges: [NavDirection, number][] = [
    ["left", horizontal],
    ["right", 1 - horizontal],
    ["up", vertical],
    ["down", 1 - vertical],
  ];
  const [side, distance] = edges.reduce((closest, edge) => (edge[1] < closest[1] ? edge : closest));
  return distance < EDGE_ZONE ? side : "center";
}

/** The drop target under a viewport point, if any. */
export function hitTest(x: number, y: number): TabDropTarget | null {
  const element = document.elementFromPoint(x, y);
  const strip = element?.closest<HTMLElement>("[data-tab-strip]");
  if (strip) {
    const tab = element!.closest<HTMLElement>("[data-tab-index]");
    const index = tab
      ? stripIndex(tab.getBoundingClientRect(), Number(tab.dataset.tabIndex), x)
      : Number(strip.dataset.tabCount);
    return { kind: "strip", paneId: strip.dataset.tabStrip!, index };
  }
  const pane = element?.closest<HTMLElement>("[data-pane-drop]");
  if (pane) {
    return {
      kind: "pane",
      paneId: pane.dataset.paneDrop!,
      zone: paneZone(pane.getBoundingClientRect(), x, y),
    };
  }
  return null;
}

/**
 * Track a press on a tab. It turns into a drag once the pointer moves past a
 * small threshold, and `onDrop` runs if it is released over a target. Escape,
 * the window losing focus, or a new press cancels it.
 */
export function pressTab(
  ptyKey: string,
  down: PointerEvent,
  onDrop: (ptyKey: string, target: TabDropTarget) => void,
): void {
  if (down.button !== 0) return;
  endActivePress?.();
  const startX = down.clientX;
  const startY = down.clientY;

  const move = (event: PointerEvent) => {
    if (!dragged) {
      if (Math.hypot(event.clientX - startX, event.clientY - startY) < DRAG_THRESHOLD_PX) return;
      dragged = ptyKey;
      document.body.classList.add("tab-dragging");
    }
    target = hitTest(event.clientX, event.clientY);
  };
  const up = () => {
    const drop = dragged ? target : null;
    finish();
    if (drop) onDrop(ptyKey, drop);
  };
  const key = (event: KeyboardEvent) => {
    if (event.key !== "Escape" || !dragged) return;
    event.preventDefault();
    event.stopPropagation();
    finish();
  };
  const finish = () => {
    endActivePress = null;
    dragged = null;
    target = null;
    document.body.classList.remove("tab-dragging");
    window.removeEventListener("pointermove", move, true);
    window.removeEventListener("pointerup", up, true);
    window.removeEventListener("pointercancel", finish, true);
    window.removeEventListener("keydown", key, true);
    window.removeEventListener("blur", finish);
  };
  endActivePress = finish;

  // Capture phase: a terminal under the pointer must not swallow the events.
  window.addEventListener("pointermove", move, true);
  window.addEventListener("pointerup", up, true);
  window.addEventListener("pointercancel", finish, true);
  window.addEventListener("keydown", key, true);
  window.addEventListener("blur", finish);
}
