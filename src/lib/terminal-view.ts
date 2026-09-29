/**
 * Terminal view: the live terminal for one pty key (see CONTEXT.md).
 *
 * Invariant: the surface (xterm) and the PTY always have the same size, and
 * both change together, only while the view is visible. Output keeps flowing
 * into the buffer while hidden or unmounted, so showing a view again needs no
 * replay and no redraw from the child process.
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

export interface TerminalViewOptions {
  ptyKey: string;
  kind: TerminalKind;
  initialCommand?: string;
  pty: TerminalPty;
  createSurface: (handlers: TerminalSurfaceHandlers) => TerminalSurface;
  onTitle?: (title: string) => void;
  resizeDebounceMs?: number;
}

export interface TerminalView {
  readonly ptyKey: string;
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
  setEvents(events: TerminalViewEvents | null): void;
  dispose(): void;
}

const FLOW_HIGH = 32_000;
const FLOW_LOW = 8_000;
const FOCUS_REPORTS = /\x1b\[I|\x1b\[O/g;
const encoder = new TextEncoder();

type Phase = "created" | "opening" | "opened" | "disposed";
type Connection = "none" | "connecting" | "live";

export function createTerminalView(options: TerminalViewOptions): TerminalView {
  const { ptyKey, kind, pty } = options;
  const resizeDebounceMs = options.resizeDebounceMs ?? 50;

  const host = document.createElement("div");
  host.style.width = "100%";
  host.style.height = "100%";

  let phase: Phase = "created";
  let container: HTMLElement | null = null;
  let shown = false;
  let focused = false;
  let exited = false;
  let sized = false;
  let connection: Connection = "none";
  // Bumped on every (re)connect so output from a superseded connection is dropped.
  let generation = 0;
  let receivedOutput = false;
  let pendingBytes = 0;
  let paused = false;
  let events: TerminalViewEvents | null = null;
  let resizeTimer: ReturnType<typeof setTimeout> | null = null;

  const surface = options.createSurface({
    onData: (data) => {
      if (exited || connection !== "live") return;
      // Focus reports xterm emits before the child has spoken confuse shells.
      const filtered = receivedOutput ? data : data.replace(FOCUS_REPORTS, "");
      if (filtered) writeUserBytes(Array.from(encoder.encode(filtered)));
    },
    onUserBytes: (bytes) => {
      if (exited || connection !== "live") return;
      writeUserBytes(bytes);
    },
    onReply: (bytes) => {
      if (connection === "live") pty.write(ptyKey, bytes);
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

  function writeUserBytes(bytes: number[]): void {
    events?.userInput?.();
    pty.write(ptyKey, bytes);
  }

  function syncGeometry(): void {
    const next = surface.measure();
    if (!next || next.cols <= 0 || next.rows <= 0) return;
    sized = true;
    if (next.cols === surface.cols && next.rows === surface.rows) return;
    surface.resize(next);
    if (connection === "live") pty.resize(ptyKey, next);
  }

  function receive(gen: number, data: Uint8Array): void {
    if (gen !== generation || phase === "disposed") return;
    receivedOutput = true;
    pendingBytes += data.byteLength;
    if (pendingBytes > FLOW_HIGH && !paused) {
      paused = true;
      pty.pause(ptyKey);
    }
    surface.write(data, () => {
      if (gen !== generation) return;
      pendingBytes = Math.max(pendingBytes - data.byteLength, 0);
      if (paused && pendingBytes < FLOW_LOW) {
        paused = false;
        pty.resume(ptyKey);
      }
    });
  }

  function connect(): void {
    const gen = ++generation;
    connection = "connecting";
    receivedOutput = false;
    pendingBytes = 0;
    paused = false;
    pty
      .connect({ ptyKey, kind, initialCommand: options.initialCommand }, (data) =>
        receive(gen, data),
      )
      .then(() => {
        if (gen !== generation || phase === "disposed") return;
        connection = "live";
        // Bring the PTY to the surface's size; the kernel ignores a no-op.
        pty.resize(ptyKey, { cols: surface.cols, rows: surface.rows });
        events?.attached?.();
      })
      .catch((error) => {
        if (gen !== generation || phase === "disposed") return;
        connection = "none";
        events?.attachError?.(error);
      });
  }

  function reconcile(): void {
    if (phase !== "opened") return;
    const visible = isVisible();
    surface.setGpu(visible);
    if (!visible) return;
    syncGeometry();
    if (sized && connection === "none" && !exited) connect();
    if (focused) surface.focus();
  }

  return {
    ptyKey,

    mount(target) {
      if (phase === "disposed") return;
      if (container && container !== target) {
        // Moving: the new slot decides visibility and focus.
        shown = false;
        focused = false;
      }
      container = target;
      target.appendChild(host);
      if (phase === "created") {
        phase = "opening";
        void surface.ready().then(() => {
          if (phase !== "opening") return;
          surface.open(host);
          phase = "opened";
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
      if (!next && phase === "opened") surface.blur();
      reconcile();
    },

    setExited(next) {
      if (exited === next) return;
      exited = next;
      if (next) return;
      // Restart: same view, clean buffer (CONTEXT.md "Restart"), new connection.
      if (connection !== "none" || receivedOutput) {
        generation++;
        connection = "none";
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
    },

    dispose() {
      if (phase === "disposed") return;
      phase = "disposed";
      generation++;
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeObserver.disconnect();
      host.remove();
      container = null;
      events = null;
      surface.dispose();
    },
  };
}
