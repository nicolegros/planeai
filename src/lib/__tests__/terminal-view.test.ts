import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createTerminalView,
  type TerminalConnectRequest,
  type TerminalGeometry,
  type TerminalPty,
  type TerminalSurface,
  type TerminalSurfaceHandlers,
  type TerminalView,
} from "../terminal-view";

// ── Fakes ───────────────────────────────────────────────────────────────

class FakeSurface implements TerminalSurface {
  cols = 80;
  rows = 24;
  opened = false;
  gpu = false;
  focused = false;
  /** What the host can fit; null means "not laid out". */
  available: TerminalGeometry | null = { cols: 120, rows: 40 };
  written: { bytes: string; cols: number; rows: number }[] = [];
  resets = 0;
  disposed = false;
  private fontReady!: () => void;
  private readyPromise = new Promise<void>((resolve) => (this.fontReady = resolve));

  constructor(public handlers: TerminalSurfaceHandlers) {}

  loadFont(): Promise<void> {
    this.fontReady();
    return flush();
  }
  ready() {
    return this.readyPromise;
  }
  open() {
    if (this.opened) throw new Error("open called twice");
    this.opened = true;
  }
  measure() {
    return this.available;
  }
  resize(g: TerminalGeometry) {
    this.cols = g.cols;
    this.rows = g.rows;
  }
  write(data: Uint8Array, done: () => void) {
    this.written.push({ bytes: new TextDecoder().decode(data), cols: this.cols, rows: this.rows });
    done();
  }
  reset() {
    this.resets++;
    this.written = [];
  }
  setGpu(enabled: boolean) {
    this.gpu = enabled;
  }
  refreshAppearance() {}
  focus() {
    this.focused = true;
  }
  blur() {
    this.focused = false;
  }
  dispose() {
    this.disposed = true;
  }
}

/** Models a PTY: the child only redraws (SIGWINCH) when the size really changes. */
class FakePty implements TerminalPty {
  size: TerminalGeometry = { cols: 80, rows: 24 };
  sigwinch = 0;
  connects: TerminalConnectRequest[] = [];
  writes: number[][] = [];
  output: ((data: Uint8Array) => void) | null = null;
  pauses = 0;
  resumes = 0;
  private pendingConnect: { resolve: () => void; reject: (e: unknown) => void } | null = null;

  connect(request: TerminalConnectRequest, onData: (data: Uint8Array) => void) {
    this.connects.push(request);
    this.output = onData;
    return new Promise<void>((resolve, reject) => (this.pendingConnect = { resolve, reject }));
  }
  async acceptConnect() {
    this.pendingConnect?.resolve();
    this.pendingConnect = null;
    await flush();
  }
  async rejectConnect(error: unknown) {
    this.pendingConnect?.reject(error);
    this.pendingConnect = null;
    await flush();
  }
  emit(text: string) {
    this.output?.(new TextEncoder().encode(text));
  }
  write(_key: string, bytes: number[]) {
    this.writes.push(bytes);
  }
  resize(_key: string, g: TerminalGeometry) {
    if (g.cols === this.size.cols && g.rows === this.size.rows) return;
    this.size = g;
    this.sigwinch++;
  }
  pause() {
    this.pauses++;
  }
  resume() {
    this.resumes++;
  }
}

const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

const observers: { callback: ResizeObserverCallback; disconnected: boolean }[] = [];
class FakeResizeObserver {
  private entry: { callback: ResizeObserverCallback; disconnected: boolean };
  constructor(callback: ResizeObserverCallback) {
    this.entry = { callback, disconnected: false };
    observers.push(this.entry);
  }
  observe() {}
  unobserve() {}
  disconnect() {
    this.entry.disconnected = true;
  }
}

async function containerResized(): Promise<void> {
  for (const o of observers) if (!o.disconnected) o.callback([], {} as ResizeObserver);
  await new Promise((resolve) => setTimeout(resolve, 5));
}

// ── Harness ─────────────────────────────────────────────────────────────

let surface: FakeSurface;
let pty: FakePty;
let view: TerminalView;
let containers: HTMLElement[];

function newContainer(): HTMLElement {
  const el = document.createElement("div");
  document.body.appendChild(el);
  containers.push(el);
  return el;
}

function createView(kind: "agent" | "shell" = "agent", initialCommand?: string): TerminalView {
  return createTerminalView({
    ptyKey: "s1",
    kind,
    initialCommand,
    pty,
    createSurface: (handlers) => (surface = new FakeSurface(handlers)),
    resizeDebounceMs: 1,
  });
}

/** Mount, show, load the font, and accept the connection. */
async function liveView(): Promise<void> {
  view.mount(newContainer());
  view.setShown(true);
  await surface.loadFont();
  await pty.acceptConnect();
}

function expectInSync(): void {
  expect({ cols: surface.cols, rows: surface.rows }).toEqual(pty.size);
}

beforeEach(() => {
  observers.length = 0;
  containers = [];
  globalThis.ResizeObserver = FakeResizeObserver as unknown as typeof ResizeObserver;
  pty = new FakePty();
  view = createView();
});

afterEach(() => {
  view.dispose();
  for (const el of containers) el.remove();
});

// ── Tests ───────────────────────────────────────────────────────────────

describe("terminal view: first mount", () => {
  it("does not connect before the surface is opened and sized", async () => {
    view.mount(newContainer());
    view.setShown(true);
    expect(pty.connects).toHaveLength(0);

    await surface.loadFont();
    expect(surface.opened).toBe(true);
    expect({ cols: surface.cols, rows: surface.rows }).toEqual({ cols: 120, rows: 40 });
    expect(pty.connects).toHaveLength(1);
  });

  it("never writes output at the pre-fit default size", async () => {
    view.mount(newContainer());
    view.setShown(true);
    await surface.loadFont();
    pty.emit("replay");
    await pty.acceptConnect();

    expect(surface.written[0]).toEqual({ bytes: "replay", cols: 120, rows: 40 });
  });

  it("brings the PTY to the surface size once connected", async () => {
    await liveView();

    expectInSync();
    expect(pty.sigwinch).toBe(1);
  });

  it("stays unconnected while hidden, and connects on first show", async () => {
    view.mount(newContainer());
    await surface.loadFont();
    expect(pty.connects).toHaveLength(0);

    view.setShown(true);
    expect(pty.connects).toHaveLength(1);
  });

  it("waits for layout before connecting", async () => {
    view.mount(newContainer());
    view.setShown(true);
    surface.available = null;
    await surface.loadFont();
    expect(pty.connects).toHaveLength(0);

    surface.available = { cols: 100, rows: 30 };
    await containerResized();
    expect(pty.connects).toHaveLength(1);
  });

  it("passes kind and initial command to the connection", async () => {
    view.dispose();
    view = createView("shell", "vim file.ts");
    await liveView();

    expect(pty.connects[0]).toEqual({ ptyKey: "s1", kind: "shell", initialCommand: "vim file.ts" });
  });

  it("reports connection failures and retries on the next show", async () => {
    const attachError = vi.fn();
    view.setEvents({ attachError });
    view.mount(newContainer());
    view.setShown(true);
    await surface.loadFont();
    await pty.rejectConnect(new Error("boom"));
    expect(attachError).toHaveBeenCalledWith(new Error("boom"));

    view.setShown(false);
    view.setShown(true);
    expect(pty.connects).toHaveLength(2);
  });
});

describe("terminal view: geometry invariant", () => {
  it("resizes surface and PTY together when the container changes", async () => {
    await liveView();
    surface.available = { cols: 90, rows: 30 };
    await containerResized();

    expectInSync();
    expect(pty.size).toEqual({ cols: 90, rows: 30 });
  });

  it("freezes geometry while hidden", async () => {
    await liveView();
    view.setShown(false);
    surface.available = { cols: 1, rows: 1 };
    await containerResized();

    expect(pty.size).toEqual({ cols: 120, rows: 40 });
    expectInSync();
  });

  it("sends nothing when shown again at the same size", async () => {
    await liveView();
    const before = pty.sigwinch;
    view.setShown(false);
    view.setShown(true);

    expect(pty.sigwinch).toBe(before);
    expectInSync();
  });

  it("resizes both when shown again at a different size, so the child repaints", async () => {
    await liveView();
    view.setShown(false);
    surface.available = { cols: 150, rows: 50 };
    view.setShown(true);

    expect(pty.size).toEqual({ cols: 150, rows: 50 });
    expectInSync();
  });

  it("keeps writing output into the buffer while hidden", async () => {
    await liveView();
    view.setShown(false);
    pty.emit("while hidden");

    expect(surface.written.at(-1)).toEqual({ bytes: "while hidden", cols: 120, rows: 40 });
  });

  it("re-measures after an appearance change", async () => {
    await liveView();
    surface.available = { cols: 100, rows: 35 };
    view.refreshAppearance();

    expect(pty.size).toEqual({ cols: 100, rows: 35 });
    expectInSync();
  });
});

describe("terminal view: moving between containers", () => {
  it("keeps its buffer and connection across unmount and mount", async () => {
    await liveView();
    pty.emit("screen");
    view.unmount();
    pty.emit("more");
    view.mount(newContainer());
    view.setShown(true);

    expect(pty.connects).toHaveLength(1);
    expect(surface.written.map((w) => w.bytes)).toEqual(["screen", "more"]);
  });

  it("re-parents the same host element instead of reopening", async () => {
    const first = newContainer();
    view.mount(first);
    view.setShown(true);
    await surface.loadFont();
    const host = first.firstElementChild;
    view.unmount();
    const second = newContainer();
    view.mount(second);

    expect(first.childElementCount).toBe(0);
    expect(second.firstElementChild).toBe(host);
  });

  it("forgets shown and focused on unmount", async () => {
    await liveView();
    view.setFocused(true);
    view.unmount();
    view.mount(newContainer());

    expect(surface.gpu).toBe(false);
    surface.focused = false;
    await containerResized();
    expect(surface.focused).toBe(false);
  });
});

describe("terminal view: GPU renderer", () => {
  it("holds WebGL only while visible", async () => {
    await liveView();
    expect(surface.gpu).toBe(true);

    view.setShown(false);
    expect(surface.gpu).toBe(false);

    view.setShown(true);
    view.unmount();
    expect(surface.gpu).toBe(false);
  });
});

describe("terminal view: input", () => {
  it("writes typed input and notifies before writing", async () => {
    const order: string[] = [];
    view.setEvents({ userInput: () => order.push("invalidate") });
    await liveView();
    pty.emit("$ ");
    const originalWrite = pty.write.bind(pty);
    pty.write = (key, bytes) => {
      order.push("write");
      originalWrite(key, bytes);
    };
    surface.handlers.onData("ls");

    expect(order).toEqual(["invalidate", "write"]);
    expect(pty.writes).toEqual([[108, 115]]);
  });

  it("strips focus reports until the child has produced output", async () => {
    await liveView();
    surface.handlers.onData("\x1b[I");
    expect(pty.writes).toHaveLength(0);

    pty.emit("$ ");
    surface.handlers.onData("\x1b[I");
    expect(pty.writes).toHaveLength(1);
  });

  it("drops input while exited", async () => {
    await liveView();
    view.setExited(true);
    surface.handlers.onData("x");
    surface.handlers.onUserBytes([1]);

    expect(pty.writes).toHaveLength(0);
  });

  it("sends protocol replies without notifying user input", async () => {
    const userInput = vi.fn();
    view.setEvents({ userInput });
    await liveView();
    surface.handlers.onReply([1, 2]);

    expect(pty.writes).toEqual([[1, 2]]);
    expect(userInput).not.toHaveBeenCalled();
  });
});

describe("terminal view: flow control", () => {
  it("pauses above the high watermark and resumes once drained", async () => {
    await liveView();
    let release: (() => void)[] = [];
    surface.write = (_data, done) => {
      release.push(done);
    };
    pty.emit("x".repeat(33_000));
    expect(pty.pauses).toBe(1);

    release.forEach((done) => done());
    release = [];
    expect(pty.resumes).toBe(1);
  });
});

describe("terminal view: exit and restart", () => {
  it("does not connect when mounted exited", async () => {
    view.setExited(true);
    await liveView();

    expect(pty.connects).toHaveLength(0);
  });

  it("keeps the frozen buffer while exited", async () => {
    await liveView();
    pty.emit("last words");
    view.setExited(true);

    expect(surface.resets).toBe(0);
    expect(surface.written.at(-1)?.bytes).toBe("last words");
  });

  it("resets the buffer and reconnects on restart", async () => {
    await liveView();
    pty.emit("old");
    view.setExited(true);
    view.setExited(false);

    expect(surface.resets).toBe(1);
    expect(pty.connects).toHaveLength(2);
  });

  it("drops output from the superseded connection", async () => {
    await liveView();
    const oldOutput = pty.output!;
    view.setExited(true);
    view.setExited(false);
    oldOutput(new TextEncoder().encode("stale"));

    expect(surface.written.map((w) => w.bytes)).not.toContain("stale");
  });

  it("connects on restart of a view mounted exited, without a reset", async () => {
    view.setExited(true);
    await liveView();
    view.setExited(false);

    expect(surface.resets).toBe(0);
    expect(pty.connects).toHaveLength(1);
  });
});

describe("terminal view: focus", () => {
  it("applies focus once the surface opens", async () => {
    view.setFocused(true);
    view.mount(newContainer());
    view.setShown(true);
    expect(surface.focused).toBe(false);

    await surface.loadFont();
    expect(surface.focused).toBe(true);
  });

  it("blurs when focus is withdrawn", async () => {
    await liveView();
    view.setFocused(true);
    view.setFocused(false);

    expect(surface.focused).toBe(false);
  });

  it("ignores one-shot focus while hidden", async () => {
    await liveView();
    view.setShown(false);
    view.focus();

    expect(surface.focused).toBe(false);
  });
});

describe("terminal view: dispose", () => {
  it("disposes the surface and ignores late connections", async () => {
    const attached = vi.fn();
    view.setEvents({ attached });
    view.mount(newContainer());
    view.setShown(true);
    await surface.loadFont();
    view.dispose();
    await pty.acceptConnect();

    expect(surface.disposed).toBe(true);
    expect(attached).not.toHaveBeenCalled();
  });

  it("never opens a surface disposed before its font loaded", async () => {
    view.mount(newContainer());
    view.dispose();
    await surface.loadFont();

    expect(surface.opened).toBe(false);
  });
});
