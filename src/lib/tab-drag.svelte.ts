/**
 * Tab drag: moves TaskWorkspace tabs. Targets are found by hit-testing marked
 * elements: `data-tab-strip="<paneId>"` (with `data-tab-count`) holding
 * `data-tab-index` tabs, and `data-pane-drop="<paneId>"` pane bodies.
 */
import type { NavDirection } from "./layout-tree";
import { definePointerDrag } from "./pointer-drag.svelte";

export type PaneDropZone = NavDirection | "center";

export type TabDropTarget =
  | { kind: "strip"; paneId: string; index: number }
  | { kind: "pane"; paneId: string; zone: PaneDropZone };

/** Share of a pane's size, from an edge, that splits it on that side. */
const EDGE_ZONE = 0.25;

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

const drag = definePointerDrag<string, TabDropTarget>({ hitTest, bodyClass: "tab-dragging" });

/** The tab being dragged and where it would land, for drop indicators. */
export const tabDrag = {
  get dragged(): string | null {
    return drag.current?.payload ?? null;
  },
  get target(): TabDropTarget | null {
    return drag.current?.target ?? null;
  },
};

/** Track a press on a tab; dragging it past the threshold and releasing over a target runs `onDrop`. */
export function pressTab(
  ptyKey: string,
  down: PointerEvent,
  onDrop: (ptyKey: string, target: TabDropTarget) => void,
): void {
  drag.press(ptyKey, down, onDrop);
}
