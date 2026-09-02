import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

const { mockPty, mockTerm } = vi.hoisted(() => {
  const term = {
    rows: 24,
    cols: 80,
    unicode: { activeVersion: "" },
    parser: {},
    options: {},
    attachCustomKeyEventHandler: vi.fn(),
    blur: vi.fn(),
    dispose: vi.fn(),
    focus: vi.fn(),
    loadAddon: vi.fn(),
    onData: vi.fn(),
    onTitleChange: vi.fn(),
    open: vi.fn(),
    write: vi.fn(),
  };
  return {
    mockPty: {
      attach: vi.fn().mockResolvedValue(undefined),
      pause: vi.fn(),
      resize: vi.fn(),
      resume: vi.fn(),
      write: vi.fn(),
    },
    mockTerm: term,
  };
});

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage?: (value: ArrayBuffer) => void;
  },
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    constructor() {
      return mockTerm;
    }
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit = vi.fn();
  },
}));
vi.mock("@xterm/addon-web-links", () => ({
  WebLinksAddon: class {},
}));
vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class {
    clearTextureAtlas = vi.fn();
    dispose = vi.fn();
    onContextLoss = vi.fn();
  },
}));
vi.mock("@xterm/addon-unicode11", () => ({
  Unicode11Addon: class {},
}));
vi.mock("../../lib/api", () => ({ pty: mockPty }));
vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => ({
    terminal: { font_size: 14, font_family: "monospace", option_as_meta: false },
    scrollback_lines: 1000,
  }),
  getTerminalSettings: () => ({ font_size: 14, font_family: "monospace", option_as_meta: false }),
  isDark: () => true,
}));
vi.mock("../../lib/theme-loader", () => ({ extractTerminalTheme: () => ({ background: "#000" }) }));
vi.mock("../../lib/terminal-keys", () => ({ matchTerminalKey: () => null }));
vi.mock("../../lib/shell-title", () => ({ extractCommandName: () => null }));
vi.mock("../../lib/split-tree.svelte", () => ({ updateTabLabel: vi.fn() }));
vi.mock("../../lib/terminal-input", () => ({ writeUserInput: vi.fn() }));
vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));

import Terminal from "../Terminal.svelte";

describe("Terminal pointer focus boundary", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount>;
  let onFocused: (event: PointerEvent | FocusEvent) => void;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    onFocused = vi.fn<(event: PointerEvent | FocusEvent) => void>();
    Object.defineProperty(document, "fonts", {
      configurable: true,
      value: { load: vi.fn().mockResolvedValue([]), ready: Promise.resolve() },
    });
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
