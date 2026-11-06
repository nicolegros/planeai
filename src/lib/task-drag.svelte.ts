/**
 * Task drag: a sidebar task row dragged onto a lane, to change its status, or onto a session pane,
 * to tell that agent about it. Targets are found by hit-testing marked elements:
 * `data-task-lane="<lane>"` sections (with `data-task-lane-project` when the lane belongs to one
 * project) and `data-pane-drop="<paneId>"` pane bodies. The sidebar's scroll container carries
 * `data-task-drag-list`: inside its width the row follows the pointer, outside it a ghost does.
 */
import type { Attachment } from "svelte/attachments";
import { definePointerDrag, type PointerDrag } from "./pointer-drag.svelte";
import { normalizeTaskStatus, taskLane, type SidebarTaskRow, type TaskLane } from "./sidebar-model";
import {
  laneVerdict,
  type DropVerdict,
  type LaneOutcome,
  type PaneOutcome,
  type TaskDragPayload,
  type TaskDropTarget,
} from "./task-drop";

export type { TaskDragPayload, TaskDropTarget } from "./task-drop";

/** The drop target under a viewport point, if any. */
export function hitTest(x: number, y: number): TaskDropTarget | null {
  const element = document.elementFromPoint(x, y);
  const lane = element?.closest<HTMLElement>("[data-task-lane]");
  if (lane) {
    return {
      kind: "lane",
      lane: lane.dataset.taskLane as TaskLane,
      projectId: lane.dataset.taskLaneProject ?? null,
    };
  }
  const pane = element?.closest<HTMLElement>("[data-pane-drop]");
  if (pane) return { kind: "pane", paneId: pane.dataset.paneDrop! };
  return null;
}

const drag = definePointerDrag<TaskDragPayload, TaskDropTarget>({
  hitTest,
  bodyClass: "task-dragging",
});

/** Horizontal bounds of the list the press started in. */
let listBounds: { left: number; right: number } | null = null;

type PaneVerdicts = (task: TaskDragPayload, paneId: string) => DropVerdict<PaneOutcome>;
/** Pane verdicts need the layout and the sessions, which the app provides. */
let paneVerdicts: PaneVerdicts | null = null;

export function resolvePaneDrops(resolve: PaneVerdicts): void {
  paneVerdicts = resolve;
}

export const taskDrag = {
  get current(): PointerDrag<TaskDragPayload, TaskDropTarget> | null {
    return drag.current;
  },
  /** The pointer left the sidebar sideways: the row goes home and a ghost follows the pointer. */
  get outside(): boolean {
    const current = drag.current;
    if (!current || !listBounds) return false;
    return current.point.x < listBounds.left || current.point.x > listBounds.right;
  },
  /** What dropping now would do, for the target under the pointer. */
  get verdict(): DropVerdict<LaneOutcome> | DropVerdict<PaneOutcome> | null {
    const current = drag.current;
    if (!current?.target) return null;
    if (current.target.kind === "lane") return laneVerdict(current.payload, current.target);
    return paneVerdicts?.(current.payload, current.target.paneId) ?? null;
  },
};

/**
 * The verdict a lane section shows while the dragged task hovers it; null when the pointer is
 * elsewhere, or the task is dropped back where it came from.
 */
export function laneHover(
  lane: TaskLane,
  projectId: string | null,
): DropVerdict<LaneOutcome> | null {
  const current = drag.current;
  const target = current?.target;
  if (
    !current ||
    target?.kind !== "lane" ||
    target.lane !== lane ||
    target.projectId !== projectId
  ) {
    return null;
  }
  const verdict = laneVerdict(current.payload, target);
  return verdict.accepts && verdict.outcome.kind === "stay" ? null : verdict;
}

export function taskDragPayload(row: SidebarTaskRow, now: number): TaskDragPayload {
  const { task, project } = row;
  return {
    projectId: project.id,
    projectName: project.name,
    repoPath: project.path,
    key: task.key,
    title: task.title,
    status: normalizeTaskStatus(task.status),
    lane: taskLane(task, now),
  };
}

/** Track a press on a task row; dragging it past the threshold and releasing over a target runs `onDrop`. */
export function pressTaskRow(
  payload: TaskDragPayload,
  down: PointerEvent,
  onDrop: (payload: TaskDragPayload, target: TaskDropTarget) => void,
): void {
  const list = (down.target as Element | null)?.closest("[data-task-drag-list]");
  const rect = list?.getBoundingClientRect();
  listBounds = rect ? { left: rect.left, right: rect.right } : null;
  drag.press(payload, down, onDrop);
}

type Span = { top: number; bottom: number };

/** How far a lifted row moves from its slot: with the pointer, but never past the list's edges. */
export function liftOffset(pointerY: number, originY: number, row: Span, list: Span): number {
  return Math.min(Math.max(pointerY - originY, list.top - row.top), list.bottom - row.bottom);
}

const isDragged = (id: { projectId: string; key: string }) => {
  const payload = drag.current?.payload;
  return payload?.projectId === id.projectId && payload.key === id.key;
};

const reducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/**
 * Lifts a row while it is the one dragged: it follows the pointer inside the list and glides home
 * when the pointer leaves sideways. The row is measured once the drag starts, after the sidebar
 * has shown every lane it can land in.
 */
export function liftWhileDragged(id: { projectId: string; key: string }): Attachment<HTMLElement> {
  let home: { row: Span; list: Span } | null = null;
  let measuring = false;
  return (node) => {
    const current = drag.current;
    if (!current || !isDragged(id)) {
      home = null;
      measuring = false;
      node.classList.remove("task-drag-lifted");
      node.style.transform = "";
      node.style.transition = "";
      return;
    }
    if (!home) {
      if (measuring) return;
      measuring = true;
      node.classList.add("task-drag-lifted");
      // The next pointer move applies what is measured here; the row has not moved yet.
      requestAnimationFrame(() => {
        if (!measuring) return;
        measuring = false;
        const list = node.closest("[data-task-drag-list]")?.getBoundingClientRect();
        if (list) home = { row: node.getBoundingClientRect(), list };
      });
      return;
    }
    if (taskDrag.outside) {
      node.style.transition = reducedMotion() ? "" : "transform 150ms ease-out";
      node.style.transform = "translateY(0)";
      return;
    }
    const dy = liftOffset(current.point.y, current.origin.y, home.row, home.list);
    node.style.transition = "none";
    node.style.transform = `translateY(${dy}px)`;
  };
}
