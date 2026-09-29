/**
 * Terminal view: the live terminal for one pty key (see CONTEXT.md).
 *
 * Invariant: the surface (xterm) and the PTY always have the same size, and
 * both change together. Sizes are measured only while the view is visible; a
 * hidden view that was never measured connects at a provisional size
 * (GeometryHint). Output keeps flowing into the buffer while hidden or
 * unmounted, so showing a view again needs no replay and no redraw.
 */

export type TerminalKind = "agent" | "shell";

export interface TerminalGeometry {
  cols: number;
  rows: number;
}

export interface TerminalSurfaceHandlers {
  /** Keyboard input decoded by the emulator; may contain focus reports. */
  onData: (data: string) => void;
  /** Bytes produced by paste or terminal shortcuts. */
  onUserBytes: (bytes: number[]) => void;
  /** Protocol replies (e.g. DECRQM) that are not user input. */
  onReply: (bytes: number[]) => void;
  onTitle: (title: string) => void;
}

/** The emulator a view renders into. `open` is called at most once. */
export interface TerminalSurface {
  readonly cols: number;
  readonly rows: number;
  ready(): Promise<void>;
  open(host: HTMLElement): void;
  /** Size the host can fit, or null when it is not laid out. */
  measure(): TerminalGeometry | null;
  resize(geometry: TerminalGeometry): void;
  write(data: Uint8Array, done: () => void): void;
  reset(): void;
  setGpu(enabled: boolean): void;
  refreshAppearance(): void;
  focus(): void;
  blur(): void;
  dispose(): void;
}

export interface TerminalConnectRequest {
  ptyKey: string;
  kind: TerminalKind;
  initialCommand?: string;
}

export interface TerminalPty {
  connect(request: TerminalConnectRequest, onData: (data: Uint8Array) => void): Promise<void>;
  write(ptyKey: string, bytes: number[]): void;
  resize(ptyKey: string, geometry: TerminalGeometry): void;
  pause(ptyKey: string): void;
  resume(ptyKey: string): void;
}

export interface TerminalViewEvents {
  attached?: () => void;
  attachError?: (error: unknown) => void;
  /** Fired before user-originated bytes are written to the PTY. */
  userInput?: () => void;
}

/**
 * Last size any view measured. A view that cannot measure (hidden tab, tab
 * behind an overlay) waits briefly for a visible sibling to measure, then
 * connects at this size, or at the surface default if nothing was measured.
 */
export interface GeometryHint {
  last: TerminalGeometry | null;
}

export interface TerminalViewOptions {
  ptyKey: string;
  kind: TerminalKind;
  initialCommand?: string;
  pty: TerminalPty;
  createSurface: (handlers: TerminalSurfaceHandlers) => TerminalSurface;
  geometryHint?: GeometryHint;
  onTitle?: (title: string) => void;
  resizeDebounceMs?: number;
  provisionalWaitMs?: number;
}

export interface TerminalView {
  readonly ptyKey: string;
  /** Resolves once the surface is open (font loaded). */
  readonly opened: Promise<void>;
  /** Place the view in a container, moving it out of any previous one. */
  mount(container: HTMLElement): void;
  /**
   * Take the view out of `from`. Returns false (and does nothing) when the view
   * has since been mounted elsewhere, so a stale slot cannot evict a newer one.
   */
  unmount(from: HTMLElement): boolean;
  setShown(shown: boolean): void;
  setFocused(focused: boolean): void;
  setExited(exited: boolean): void;
  /** One-shot focus request; ignored unless the view is visible. */
  focus(): void;
  refreshAppearance(): void;
  /** Also delivers an attach outcome that arrived while no events were set. */
  setEvents(events: TerminalViewEvents | null): void;
  dispose(): void;
}

const FLOW_HIGH = 32_000;
const FLOW_LOW = 8_000;
const FOCUS_REPORTS = /\x1b\[I|\x1b\[O/g;
const encoder = new TextEncoder();

type Phase = "created" | "opening" | "opened" | "disposed";

/** One attempt to connect; replaced wholesale on every (re)connect. */
interface Connection {
  // "failed" is not retried by reconcile; only a restart or a move to a new container retries.
  status: "connecting" | "live" | "failed";
  receivedOutput: boolean;
  pendingBytes: number;
  paused: boolean;
  /** Attach outcome that arrived while no events were set. */
  undelivered: ((events: TerminalViewEvents) => void) | null;
}

export function createTerminalView(options: TerminalViewOptions): TerminalView {
  const { ptyKey, kind, pty } = options;
  const resizeDebounceMs = options.resizeDebounceMs ?? 50;
  const provisionalWaitMs = options.provisionalWaitMs ?? 100;

  const host = document.createElement("div");
  host.style.width = "100%";
  host.style.height = "100%";

  let phase: Phase = "created";
  let container: HTMLElement | null = null;
  let shown = false;
  let wasVisible = false;
  let focused = false;
  let focusPending = false;
  let exited = false;
  let sized = false;
  let provisionalWaited = false;
  let provisionalTimer: ReturnType<typeof setTimeout> | null = null;
  let connection: Connection | null = null;
  let events: TerminalViewEvents | null = null;
  let resizeTimer: ReturnType<typeof setTimeout> | null = null;
  let resolveOpened!: () => void;
  const opened = new Promise<void>((resolve) => (resolveOpened = resolve));

  const isLive = () => connection?.status === "live";

  const surface = options.createSurface({
    onData: (data) => {
      if (exited || !connection || connection.status !== "live") return;
      // Focus reports xterm emits before the child has spoken confuse shells.
      const filtered = connection.receivedOutput ? data : data.replace(FOCUS_REPORTS, "");
      if (filtered) writeUserBytes(Array.from(encoder.encode(filtered)));
    },
    onUserBytes: (bytes) => {
      if (exited || !isLive()) return;
      writeUserBytes(bytes);
    },
    onReply: (bytes) => {
      if (isLive()) pty.write(ptyKey, bytes);
    },
    onTitle: (title) => options.onTitle?.(title),
  });

  const resizeObserver = new ResizeObserver(() => {
    if (resizeTimer) clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => {
      resizeTimer = null;
      reconcile();
    }, resizeDebounceMs);
  });
  resizeObserver.observe(host);

  function isVisible(): boolean {
    return phase === "opened" && container !== null && shown;
  }

  function currentGeometry(): TerminalGeometry {
    return { cols: surface.cols, rows: surface.rows };
  }

  function writeUserBytes(bytes: number[]): void {
    events?.userInput?.();
    pty.write(ptyKey, bytes);
  }

  /** Returns whether the surface (and PTY) were resized. */
  function syncGeometry(): boolean {
    const next = surface.measure();
    if (!next || next.cols <= 0 || next.rows <= 0) return false;
    sized = true;
    if (options.geometryHint) options.geometryHint.last = next;
    if (next.cols === surface.cols && next.rows === surface.rows) return false;
    surface.resize(next);
    if (isLive()) pty.resize(ptyKey, next);
    return true;
  }

  /** Hidden and never measured: give a visible sibling a moment to measure first. */
  function waitThenAdoptProvisionalGeometry(): void {
    if (!provisionalWaited) {
      if (!provisionalTimer) {
        provisionalTimer = setTimeout(() => {
          provisionalTimer = null;
          provisionalWaited = true;
          reconcile();
        }, provisionalWaitMs);
      }
      return;
    }
    const hint = options.geometryHint?.last;
    if (hint && (hint.cols !== surface.cols || hint.rows !== surface.rows)) surface.resize(hint);
    sized = true;
  }

  function deliver(target: Connection, fire: (events: TerminalViewEvents) => void): void {
    if (events) fire(events);
    else target.undelivered = fire;
  }

  function receive(target: Connection, data: Uint8Array): void {
    if (target !== connection || phase === "disposed") return;
    target.receivedOutput = true;
    target.pendingBytes += data.byteLength;
    if (target.pendingBytes > FLOW_HIGH && !target.paused) {
      target.paused = true;
      pty.pause(ptyKey);
    }
    surface.write(data, () => {
      if (target !== connection) return;
      target.pendingBytes = Math.max(target.pendingBytes - data.byteLength, 0);
      if (target.paused && target.pendingBytes < FLOW_LOW) {
        target.paused = false;
        pty.resume(ptyKey);
      }
    });
  }

  function connect(): void {
    const attempt: Connection = {
      status: "connecting",
      receivedOutput: false,
      pendingBytes: 0,
      paused: false,
      undelivered: null,
    };
    connection = attempt;
    pty
      .connect({ ptyKey, kind, initialCommand: options.initialCommand }, (data) =>
        receive(attempt, data),
      )
      .then(() => {
        if (attempt !== connection || phase === "disposed") return;
        attempt.status = "live";
        // Bring the PTY to the surface's size; the kernel ignores a no-op.
        pty.resize(ptyKey, currentGeometry());
        deliver(attempt, (e) => e.attached?.());
      })
      .catch((error) => {
        if (attempt !== connection || phase === "disposed") return;
        attempt.status = "failed";
        deliver(attempt, (e) => e.attachError?.(error));
      });
  }

  function reconcile(): void {
    if (phase !== "opened") return;
    const visible = isVisible();
    const becameVisible = visible && !wasVisible;
    wasVisible = visible;
    surface.setGpu(visible);
    if (becameVisible) focusPending = true;

    if (visible) {
      const resized = syncGeometry();
      // Another client may have resized the PTY while this view was hidden.
      if (becameVisible && !resized && isLive()) pty.resize(ptyKey, currentGeometry());
    }

    if (container && !connection && !exited) {
      if (!sized && !visible) waitThenAdoptProvisionalGeometry();
      if (sized) connect();
    }

    if (visible && focused && focusPending) {
      focusPending = false;
      surface.focus();
    }
  }

  return {
    ptyKey,
    opened,

    mount(target) {
      if (phase === "disposed") return;
      if (container && container !== target) {
        // Moving: the new slot decides visibility and focus.
        shown = false;
        focused = false;
      }
      // Retry a failure on a move, unless no slot has seen it yet: the next
      // slot's handler decides (e.g. rolling back a terminal editor tab).
      if (container !== target && connection?.status === "failed" && !connection.undelivered) {
        connection = null;
      }
      container = target;
      target.appendChild(host);
      if (phase === "created") {
        phase = "opening";
        void surface.ready().then(() => {
          if (phase !== "opening") return;
          surface.open(host);
          phase = "opened";
          resolveOpened();
          reconcile();
        });
        return;
      }
      reconcile();
    },

    unmount(from) {
      if (container !== from) return false;
      host.remove();
      container = null;
      shown = false;
      focused = false;
      reconcile();
      return true;
    },

    setShown(next) {
      if (shown === next) return;
      shown = next;
      reconcile();
    },

    setFocused(next) {
      if (focused === next) return;
      focused = next;
      focusPending = next;
      if (!next && phase === "opened") surface.blur();
      reconcile();
    },

    setExited(next) {
      if (exited === next) return;
      exited = next;
      if (next) return;
      // Restart: same view, clean buffer (CONTEXT.md "Restart"), new connection.
      if (connection) {
        connection = null;
        if (phase === "opened") surface.reset();
      }
      reconcile();
    },

    focus() {
      if (isVisible()) surface.focus();
    },

    refreshAppearance() {
      if (phase === "disposed") return;
      surface.refreshAppearance();
      if (isVisible()) syncGeometry();
    },

    setEvents(next) {
      events = next;
      const fire = connection?.undelivered;
      if (!next || !connection || !fire) return;
      connection.undelivered = null;
      fire(next);
    },

    dispose() {
      if (phase === "disposed") return;
      phase = "disposed";
      connection = null;
      if (resizeTimer) clearTimeout(resizeTimer);
      if (provisionalTimer) clearTimeout(provisionalTimer);
      resizeObserver.disconnect();
      host.remove();
      container = null;
      events = null;
      surface.dispose();
    },
  };
}
