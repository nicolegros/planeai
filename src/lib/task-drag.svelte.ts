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
  /** The task being dragged, for readers that need not follow the pointer. */
  get payload(): TaskDragPayload | null {
    return drag.payload;
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
  const payload = drag.payload;
  return payload?.projectId === id.projectId && payload.key === id.key;
};

const reducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** The lifted row's slot and the list, as measured when its drag started. */
interface Lift {
  payload: TaskDragPayload;
  row: Span;
  list: Span;
  pointerY: number;
  scrollTop: number;
}

let lift: Lift | null = null;
/** The drag whose row is being measured. */
let measuring: TaskDragPayload | null = null;

function listOf(node: HTMLElement): HTMLElement | null {
  return node.closest<HTMLElement>("[data-task-drag-list]");
}

/** Measures `node`'s slot once the sidebar has shown every lane, which may have moved it. */
function measure(node: HTMLElement, payload: TaskDragPayload): void {
  if (measuring === payload) return;
  measuring = payload;
  requestAnimationFrame(() => {
    if (measuring === payload) measuring = null;
    const current = drag.current;
    const list = listOf(node);
    if (current?.payload !== payload || !list || !node.isConnected) return;
    node.style.transform = "";
    lift = {
      payload,
      row: node.getBoundingClientRect(),
      list: list.getBoundingClientRect(),
      pointerY: current.point.y,
      scrollTop: list.scrollTop,
    };
    place(node, current.point.y);
  });
}

function place(node: HTMLElement, pointerY: number): void {
  if (!lift) return;
  if (taskDrag.outside) {
    node.style.transition = reducedMotion() ? "" : "transform 150ms ease-out";
    node.style.transform = "translateY(0)";
    return;
  }
  // The slot moves with the list's scroll; the row stays under the pointer.
  const scrolled = (listOf(node)?.scrollTop ?? lift.scrollTop) - lift.scrollTop;
  const dy = scrolled + liftOffset(pointerY, lift.pointerY, lift.row, lift.list);
  node.style.transition = "none";
  node.style.transform = `translateY(${dy}px)`;
}

function dropLift(node: HTMLElement): void {
  node.classList.remove("task-drag-lifted");
  node.style.transform = "";
  node.style.transition = "";
}

/** Lifts a row while it is the one dragged: it follows the pointer inside the list and glides home when the pointer leaves sideways. */
export function liftWhileDragged(id: { projectId: string; key: string }): Attachment<HTMLElement> {
  return (node) => {
    if (!isDragged(id)) return;
    const current = drag.current;
    if (!current) return;
    node.classList.add("task-drag-lifted");
    if (lift?.payload === current.payload) place(node, current.point.y);
    else measure(node, current.payload);
    return () => dropLift(node);
  };
}
