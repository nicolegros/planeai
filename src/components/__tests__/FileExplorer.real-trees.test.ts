import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

const { mockFocusTerminal } = vi.hoisted(() => ({
  mockFocusTerminal: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  fileExplorer: {
    listAllPaths: vi.fn(() => Promise.resolve(["src/", "src/index.ts", "src/main.ts"])),
    watch: vi.fn(() => Promise.resolve()),
    unwatch: vi.fn(() => Promise.resolve()),
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("../../lib/layout-state", () => ({
  getLayoutWidth: () => 220,
  setLayoutWidth: vi.fn(),
}));

vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => ({ vim_mode: true }),
}));

vi.mock("../../lib/focus.svelte", () => ({
  getActiveZone: () => "explorer",
  focusTerminal: mockFocusTerminal,
}));

import { installKeyboardRouter } from "../../lib/keyboard";
import FileExplorer from "../FileExplorer.svelte";

describe("FileExplorer real Trees focus", () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount> | undefined;
  let cleanupKeyboardRouter: (() => void) | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    target = document.createElement("div");
    document.body.append(target);
  });

  afterEach(() => {
    cleanupKeyboardRouter?.();
    cleanupKeyboardRouter = undefined;
    if (component) unmount(component);
    target.remove();
  });

  it("does not consume Vim characters from the shadow-DOM search input", async () => {
    component = mount(FileExplorer, {
      target,
      props: {
        rootPath: "/tmp/project",
        sessionId: "session-1",
        visible: true,
        onFocus: vi.fn(),
        onOpenFile: vi.fn(),
        onPinFile: vi.fn(),
      },
    });

    await new Promise((resolve) => setTimeout(resolve, 100));

    window.dispatchEvent(
      new KeyboardEvent("keydown", {
        bubbles: true,
        cancelable: true,
        key: "/",
      }),
    );
    await new Promise((resolve) => setTimeout(resolve, 20));

    const host = target.querySelector<HTMLElement>(".file-tree-host");
    const searchInput = host?.shadowRoot?.querySelector<HTMLInputElement>(
      "[data-file-tree-search-input]",
    );
    expect(searchInput).toBeDefined();
    expect(host?.shadowRoot?.activeElement).toBe(searchInput);

    const keyEvent = new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      composed: true,
      key: "j",
    });
    searchInput?.dispatchEvent(keyEvent);

    expect(keyEvent.defaultPrevented).toBe(false);

    const escapeEvent = new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      composed: true,
      key: "Escape",
    });
    cleanupKeyboardRouter = installKeyboardRouter(vi.fn());
    searchInput?.dispatchEvent(escapeEvent);
    await new Promise((resolve) => setTimeout(resolve, 20));

    expect(mockFocusTerminal).not.toHaveBeenCalled();
    expect(escapeEvent.defaultPrevented).toBe(true);
    expect(
      host?.shadowRoot
        ?.querySelector("[data-file-tree-search-container]")
        ?.getAttribute("data-open"),
    ).toBe("false");
    expect(host?.shadowRoot?.activeElement).toBe(
      host?.shadowRoot?.querySelector('[data-item-focused="true"]'),
    );
  });

  it("gives the rendered tree row focus so it receives ArrowRight", async () => {
    component = mount(FileExplorer, {
      target,
      props: {
        rootPath: "/tmp/project",
        sessionId: "session-1",
        visible: true,
        onFocus: vi.fn(),
        onOpenFile: vi.fn(),
        onPinFile: vi.fn(),
      },
    });

    await new Promise((resolve) => setTimeout(resolve, 100));

    const host = target.querySelector<HTMLElement>(".file-tree-host");
    const shadowRoot = host?.shadowRoot;
    const focusedRow = shadowRoot?.querySelector<HTMLButtonElement>('[data-item-focused="true"]');

    expect(focusedRow).toBeDefined();
    expect(shadowRoot?.activeElement).toBe(focusedRow);

    window.dispatchEvent(
      new KeyboardEvent("keydown", {
        bubbles: true,
        cancelable: true,
        key: "ArrowRight",
      }),
    );
    await new Promise((resolve) => setTimeout(resolve, 20));

    expect(shadowRoot?.querySelector('[data-item-path="src/index.ts"]')).not.toBeNull();
  });
});
