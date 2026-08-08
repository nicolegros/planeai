import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

const { mockFocusFirstItem, mockCleanUp, mockListAllPaths, mockWatch, mockUnwatch } = vi.hoisted(() => ({
  mockFocusFirstItem: vi.fn(),
  mockCleanUp: vi.fn(),
  mockListAllPaths: vi.fn((rootPath: string) => Promise.resolve(
    rootPath === "/tmp/project-b" ? ["project-b/"] : ["project-a/"],
  )),
  mockWatch: vi.fn(() => Promise.resolve()),
  mockUnwatch: vi.fn(() => Promise.resolve()),
}));

vi.mock("@pierre/trees", () => {
  class FileTree {
    private container: HTMLElement | undefined;

    constructor(_options: unknown) {}

    cleanUp = mockCleanUp;
    focusFirstItem = mockFocusFirstItem;
    getVisibleCount = () => 1;
    getFileTreeContainer = () => this.container;
    getFocusedPath = () => "src/";
    setGitStatus = vi.fn();

    render = ({ fileTreeContainer }: { fileTreeContainer: HTMLElement }) => {
      this.container = fileTreeContainer;
      const shadowRoot = fileTreeContainer.shadowRoot ?? fileTreeContainer.attachShadow({ mode: "open" });
      if (shadowRoot.querySelector('[data-item-focused="true"]')) return;

      const row = document.createElement("button");
      row.dataset.itemPath = "src/";
      shadowRoot.append(row);
    };
  }

  return {
    FileTree,
    prepareFileTreeInput: (paths: string[]) => paths,
  };
});

vi.mock("../../lib/api", () => ({
  fileExplorer: {
    listAllPaths: mockListAllPaths,
    watch: mockWatch,
    unwatch: mockUnwatch,
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

import { setActiveZone } from "../../lib/focus.svelte";
import FileExplorer from "../FileExplorer.svelte";
import FileExplorerHarness from "./FileExplorerHarness.svelte";

describe("FileExplorer focus", () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount> | undefined;
  let harness: { switchSession: (sessionId: string, rootPath: string) => void } | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    setActiveZone("explorer");
    target = document.createElement("div");
    document.body.append(target);
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = undefined;
    }
    if (harness) {
      unmount(harness);
      harness = undefined;
    }
    target.remove();
  });

  it("activates Explorer and focuses the first path-identified tree row once", async () => {
    const onFocus = vi.fn();
    component = mount(FileExplorer, {
      target,
      props: {
        rootPath: "/tmp/project",
        sessionId: "session-1",
        visible: true,
        onFocus,
        onOpenFile: vi.fn(),
        onPinFile: vi.fn(),
      },
    });

    await new Promise((resolve) => setTimeout(resolve, 50));

    const host = target.querySelector<HTMLElement>(".file-tree-host");
    const focusedRow = host?.shadowRoot?.querySelector<HTMLElement>('[data-item-path="src/"]');

    expect(onFocus).toHaveBeenCalled();
    expect(mockFocusFirstItem).toHaveBeenCalledTimes(1);
    expect(host?.shadowRoot?.activeElement).toBe(focusedRow);
  });

  it("preserves terminal focus on session reload and restores tree focus only for Explorer", async () => {
    harness = mount(FileExplorerHarness, { target }) as typeof harness;
    await new Promise((resolve) => setTimeout(resolve, 50));

    expect(mockListAllPaths).toHaveBeenCalledWith("/tmp/project-a");
    expect(mockWatch).toHaveBeenCalledWith("session-a", "/tmp/project-a");
    expect(mockFocusFirstItem).toHaveBeenCalledTimes(1);

    setActiveZone("terminal");
    harness?.switchSession("session-b", "/tmp/project-b");
    await new Promise((resolve) => setTimeout(resolve, 50));

    expect(mockUnwatch).toHaveBeenCalledWith("session-a");
    expect(mockCleanUp).toHaveBeenCalled();
    expect(mockListAllPaths).toHaveBeenLastCalledWith("/tmp/project-b");
    expect(mockWatch).toHaveBeenLastCalledWith("session-b", "/tmp/project-b");
    expect(mockFocusFirstItem).toHaveBeenCalledTimes(1);

    setActiveZone("explorer");
    harness?.switchSession("session-c", "/tmp/project-c");
    await new Promise((resolve) => setTimeout(resolve, 50));

    // Reactivating Explorer focuses the current tree, then its replacement on reload.
    expect(mockFocusFirstItem).toHaveBeenCalledTimes(3);
  });
});
