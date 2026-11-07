/**
 * Pointer drag: the press-and-move mechanics every drag in the app shares.
 *
 * HTML5 drag-and-drop cannot be used: Tauri's native drag-drop handler, which
 * file drops from the OS need for real paths, swallows the webview's drag
 * events. A drag kind names what it carries and how a viewport point maps to a
 * target; this module turns a press into a drag past a small threshold, tracks
 * the target under the pointer, and ends it on release, Escape, a cancelled
 * pointer, a lost window focus or a new press of any kind. One press is live at
 * a time across every kind.
 */

interface Point {
  x: number;
  y: number;
}

export interface PointerDrag<P, T> {
  payload: P;
  /** What is under the pointer, or null over nothing that takes this kind. */
  target: T | null;
  point: Point;
  /** Where the press started. */
  origin: Point;
}

export interface PointerDragKind<P, T> {
  /** The drag of this kind in progress, once its press moved past the threshold. */
  readonly current: PointerDrag<P, T> | null;
  /** Its payload alone, for readers that need not follow the pointer. */
  readonly payload: P | null;
  /** What is under the pointer; it changes only when a different target is found. */
  readonly target: T | null;
  /** Track a press; `onDrop` runs if the drag is released over a target. */
  press(payload: P, down: PointerEvent, onDrop: (payload: P, target: T) => void): void;
}

/** A press moves this far before it becomes a drag, so clicks still select. */
const DRAG_THRESHOLD_PX = 4;

interface ActiveDrag {
  kind: object;
  payload: unknown;
  origin: Point;
}

let active = $state.raw<ActiveDrag | null>(null);
let target = $state.raw<unknown>(null);
let point = $state.raw<Point>({ x: 0, y: 0 });

/** Targets are flat records, so two with the same fields are the same target. */
function sameTarget(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (!a || !b || typeof a !== "object" || typeof b !== "object") return false;
  const keys = Object.keys(a);
  return (
    keys.length === Object.keys(b).length &&
    keys.every((key) =>
      Object.is((a as Record<string, unknown>)[key], (b as Record<string, unknown>)[key]),
    )
  );
}
/** Ends the press in progress; a new press must never inherit a stale one. */
let endActivePress: (() => void) | null = null;

/** Swallows the click a release fires, in the same task; a release that fires none leaves nothing armed. */
function swallowNextClick(): void {
  const swallow = (event: Event) => {
    event.stopPropagation();
    event.preventDefault();
  };
  window.addEventListener("click", swallow, { capture: true, once: true });
  setTimeout(() => window.removeEventListener("click", swallow, true), 0);
}

export function definePointerDrag<P, T>(options: {
  hitTest: (x: number, y: number) => T | null;
  /** Put on `<body>` while a drag of this kind lasts, for cursor and hover rules. */
  bodyClass: string;
}): PointerDragKind<P, T> {
  const kind: PointerDragKind<P, T> = {
    get current() {
      if (active?.kind !== kind) return null;
      return {
        payload: active.payload as P,
        target: target as T | null,
        point,
        origin: active.origin,
      };
    },
    get payload() {
      return active?.kind === kind ? (active.payload as P) : null;
    },
    get target() {
      return active?.kind === kind ? (target as T | null) : null;
    },
    press(payload, down, onDrop) {
      if (down.button !== 0) return;
      endActivePress?.();
      const origin = { x: down.clientX, y: down.clientY };

      const hit = (x: number, y: number) => {
        point = { x, y };
        const found = options.hitTest(x, y);
        if (!sameTarget(target, found)) target = found;
      };
      const move = (event: PointerEvent) => {
        if (!active) {
          if (Math.hypot(event.clientX - origin.x, event.clientY - origin.y) < DRAG_THRESHOLD_PX) {
            return;
          }
          active = { kind, payload, origin };
          document.body.classList.add(options.bodyClass);
        }
        hit(event.clientX, event.clientY);
      };
      // A scroll moves what is under a still pointer, so the drag re-reads its point and target.
      const scroll = () => {
        if (active) hit(point.x, point.y);
      };
      const up = () => {
        const drop = active ? (target as T | null) : null;
        finish();
        if (drop) onDrop(payload, drop);
      };
      const key = (event: KeyboardEvent) => {
        if (event.key !== "Escape" || !active) return;
        event.preventDefault();
        event.stopPropagation();
        finish();
        // The pointer is still down: its release would click whatever is under it.
        window.addEventListener("pointerup", swallowNextClick, { capture: true, once: true });
      };
      const finish = () => {
        endActivePress = null;
        active = null;
        target = null;
        document.body.classList.remove(options.bodyClass);
        window.removeEventListener("pointermove", move, true);
        window.removeEventListener("pointerup", up, true);
        window.removeEventListener("pointercancel", finish, true);
        window.removeEventListener("keydown", key, true);
        window.removeEventListener("scroll", scroll, true);
        window.removeEventListener("blur", finish);
      };
      endActivePress = finish;

      // Capture phase: a terminal under the pointer must not swallow the events, and scrolls do not bubble.
      window.addEventListener("pointermove", move, true);
      window.addEventListener("pointerup", up, true);
      window.addEventListener("pointercancel", finish, true);
      window.addEventListener("keydown", key, true);
      window.addEventListener("scroll", scroll, true);
      window.addEventListener("blur", finish);
    },
  };
  return kind;
}
