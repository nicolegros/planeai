import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

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
  getSettings: () => ({ vim_mode: false }),
}));

vi.mock("../../lib/focus.svelte", () => ({
  getActiveZone: () => "explorer",
}));

import FileExplorer from "../FileExplorer.svelte";

describe("FileExplorer real Trees focus", () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.append(target);
  });

  afterEach(() => {
    if (component) unmount(component);
    target.remove();
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

    window.dispatchEvent(new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      key: "ArrowRight",
    }));
    await new Promise((resolve) => setTimeout(resolve, 20));

    expect(shadowRoot?.querySelector('[data-item-path="src/index.ts"]')).not.toBeNull();
  });
});
