import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn() }));
vi.mock("../../lib/xterm-surface", () => ({
  createXtermSurface: () => ({
    cols: 80,
    rows: 24,
    ready: () => Promise.resolve(),
    open: vi.fn(),
    measure: () => ({ cols: 80, rows: 24 }),
    resize: vi.fn(),
    write: vi.fn(),
    reset: vi.fn(),
    setGpu: vi.fn(),
    refreshAppearance: vi.fn(),
    focus: vi.fn(),
    blur: vi.fn(),
    dispose: vi.fn(),
  }),
}));
vi.mock("../../lib/terminal-pty", () => ({
  tauriTerminalPty: {
    connect: vi.fn().mockResolvedValue(undefined),
    write: vi.fn(),
    resize: vi.fn(),
    pause: vi.fn(),
    resume: vi.fn(),
  },
}));
vi.mock("../../lib/split-tree.svelte", () => ({ updateTabLabel: vi.fn() }));
vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));

import Terminal from "../Terminal.svelte";
import { disposeAllTerminalViews } from "../../lib/terminal-views";

describe("Terminal pointer focus boundary", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount>;
  let onFocused: (event: PointerEvent | FocusEvent) => void;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    onFocused = vi.fn<(event: PointerEvent | FocusEvent) => void>();
    class MockResizeObserver {
      constructor(_callback: ResizeObserverCallback) {}
      observe() {}
      unobserve() {}
      disconnect() {}
      takeRecords(): ResizeObserverEntry[] {
        return [];
      }
    }
    globalThis.ResizeObserver = MockResizeObserver as unknown as typeof ResizeObserver;
    component = mount(Terminal, {
      target,
      props: { sessionId: "s1", visible: true, focused: false, onFocused },
    });
  });

  afterEach(() => {
    unmount(component);
    disposeAllTerminalViews();
    target.remove();
    vi.clearAllMocks();
  });

  it("notifies its host when xterm's surface receives pointer input", () => {
    target.firstElementChild?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));

    expect(onFocused).toHaveBeenCalledWith(expect.objectContaining({ type: "pointerdown" }));
  });

  it("notifies its host when xterm moves keyboard focus into its surface", () => {
    target.firstElementChild?.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));

    expect(onFocused).toHaveBeenCalledWith(expect.objectContaining({ type: "focusin" }));
  });
});
