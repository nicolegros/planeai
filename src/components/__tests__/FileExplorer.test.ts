import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

const mockFocusFirstItem = vi.fn();

vi.mock("@pierre/trees", () => {
  class FileTree {
    private container: HTMLElement | undefined;

    constructor(_options: unknown) {}

    cleanUp = vi.fn();
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
    listAllPaths: vi.fn(() => Promise.resolve(["src/"])),
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

import FileExplorer from "../FileExplorer.svelte";

describe("FileExplorer focus", () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    target = document.createElement("div");
    document.body.append(target);
  });

  afterEach(() => {
    if (component) unmount(component);
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
});
