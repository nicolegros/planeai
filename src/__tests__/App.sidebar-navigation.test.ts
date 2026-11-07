import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { flushSync, mount, tick, unmount } from "svelte";
import type { PluginInventory, Project, Session, TaskItem } from "../lib/types";

const alpha: Project = {
  id: "alpha",
  name: "alpha",
  path: "/repos/alpha",
  hidden: false,
  path_missing: false,
};

function task(key: string, status: string): TaskItem {
  return {
    key,
    title: `Title ${key}`,
    status,
    description: "",
    priority: 0,
    blocked_by: [],
    tags: [],
    parent_key: null,
    url: null,
    base_branch: "main",
    last_visited_at: null,
    last_activity_at: null,
    snoozed_until: null,
    snoozed_at: null,
    woke_at: null,
    snooze_rev: 0,
    created_at: "2026-11-01T09:00:00.000Z",
  };
}

function session(id: string, taskKey: string | null): Session {
  return {
    id,
    project_id: alpha.id,
    name: id,
    tmux_name: null,
    branch: id,
    status: "active",
    created_at: "",
    worktree_path: null,
    provider: null,
    backend: "local",
    base_branch: null,
    task_key: taskKey,
    task_project_id: taskKey ? alpha.id : null,
  };
}

// The session list order differs from the sidebar order, and the first session is a Done task.
const sessions = [
  session("done-1", "D-1"),
  session("progress-1", "P-1"),
  session("review-1", "R-1"),
  session("scratch", null),
];
// A running plugin whose page opens with Mod+Shift+Y.
const notesPlugin: PluginInventory = {
  id: "notes",
  name: "Notes",
  version: "1.0.0",
  host_api_version: "1",
  source_kind: "local",
  backend_entrypoint: "index.js",
  capabilities: [],
  ui_contributions: [
    {
      id: "page",
      label: "Page",
      placement: "main-pane",
      entrypoint: "page.html",
      order: null,
      shortcut: "Mod+Shift+Y",
    },
  ],
  providers: [],
  tools: [],
  installed_hash: null,
  installed_path: null,
  original_display_path: null,
  remote_origin: null,
  enabled: true,
  state: "running",
  last_error: null,
  log_path: null,
};
const tasks = [task("D-1", "done"), task("P-1", "in_progress"), task("R-1", "in_review")];

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => {
    if (command === "get_config") {
      return {
        appearance: { mode: "light", theme: "default" },
        terminal: { font_family: "Menlo", font_size: 14 },
        providers: {},
        default_provider: "kiro",
        sidebar_group_by: "status",
      };
    }
    if (command === "list_projects") return [alpha];
    if (command === "list_sessions") return sessions;
    if (command === "list_all_task_items") return tasks;
    if (command === "list_plugins") return [notesPlugin];
    if (command === "mark_task_visited")
      return { last_visited_at: "2026-11-03T12:00:00Z", last_activity_at: null };
    if (command.startsWith("list_")) return [];
    return null;
  }),
  Channel: class {
    onmessage = null;
  },
  convertFileSrc: (path: string) => path,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
  emit: vi.fn().mockResolvedValue(undefined),
  emitTo: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onCloseRequested: vi.fn().mockResolvedValue(() => {}),
    listen: vi.fn().mockResolvedValue(() => {}),
    onFocusChanged: vi.fn().mockResolvedValue(() => {}),
  }),
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({ WebviewWindow: class {} }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: vi.fn().mockResolvedValue(() => {}) }),
}));

import App from "../App.svelte";
import { IS_MAC } from "../lib/keyboard";
import { getActiveSessionId, isSelectionExplicit } from "../lib/session-orchestrator.svelte";

const modifier = IS_MAC ? { metaKey: true } : { ctrlKey: true };
const modifierKey = IS_MAC ? "Meta" : "Control";

function chord(key: string, shiftKey = false): void {
  window.dispatchEvent(
    new KeyboardEvent("keydown", { key, shiftKey, ...modifier, bubbles: true, cancelable: true }),
  );
  flushSync();
}

function completeTaskChord(): void {
  window.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "ß",
      code: "KeyS",
      altKey: true,
      ...modifier,
      bubbles: true,
      cancelable: true,
    }),
  );
  flushSync();
}

function markDoneFromCard(title: string): void {
  const card = [...document.querySelectorAll<HTMLElement>("[id^='sidebar-card-']")].find(
    (element) => element.textContent?.includes(title),
  );
  card
    ?.closest(".group")
    ?.querySelector<HTMLButtonElement>("button[aria-label='Mark done']")
    ?.click();
  flushSync();
}

function clickToastUndo(): void {
  [...document.querySelectorAll<HTMLButtonElement>("[data-toast-host] button")]
    .find((button) => button.textContent === "Undo")
    ?.click();
  flushSync();
}

function openPluginPage(): void {
  window.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Y",
      code: "KeyY",
      shiftKey: true,
      ...modifier,
      bubbles: true,
      cancelable: true,
    }),
  );
  flushSync();
}

const pluginPageOpen = () =>
  [...document.querySelectorAll("button")].some((button) =>
    button.textContent?.includes("Back to workspace"),
  );

const showsTaskWorkspace = (title: string) =>
  [...document.querySelectorAll("h2")].some((heading) => heading.textContent?.trim() === title);

function releaseModifier(): void {
  window.dispatchEvent(new KeyboardEvent("keyup", { key: modifierKey, bubbles: true }));
  flushSync();
}

async function settle(): Promise<void> {
  for (let i = 0; i < 10; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    await tick();
  }
}

describe("App sidebar navigation in the Cards layout", () => {
  let app: ReturnType<typeof mount>;

  beforeAll(async () => {
    HTMLElement.prototype.scrollIntoView = vi.fn();
    Object.defineProperty(document, "fonts", {
      value: { load: () => Promise.resolve([]), ready: Promise.resolve() },
    });
    globalThis.ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    };
    const target = document.createElement("div");
    document.body.append(target);
    app = mount(App, { target });
    await settle();
  });

  afterAll(() => unmount(app));

  it("opens the Nth sidebar row from the top with Mod+1-9, skipping the folded Done shelf", () => {
    const jumps = ["1", "2", "3"].map((key) => {
      chord(key);
      return getActiveSessionId();
    });
    chord("4");

    expect([...jumps, getActiveSessionId()]).toEqual([
      "scratch",
      "review-1",
      "progress-1",
      "progress-1",
    ]);
  });

  it("selects an unlinked session explicitly and a task row like a click", () => {
    chord("2");
    chord("1");
    const scratch = isSelectionExplicit("scratch");
    chord("2");

    expect([scratch, isSelectionExplicit("review-1")]).toEqual([true, false]);
  });

  it("cycles Mod+{/} over the rows the sidebar shows, skipping the folded Done shelf", () => {
    chord("1");

    chord("{", true);
    releaseModifier();
    const previous = getActiveSessionId();
    chord("}", true);
    releaseModifier();

    expect([previous, getActiveSessionId()]).toEqual(["progress-1", "scratch"]);
  });

  describe("marking a task Done", () => {
    // Undoes any pending Done, so the next test starts from the same sidebar.
    afterEach(async () => {
      chord("z");
      await settle();
    });

    it("opens the Mod+1 row when the viewed task is marked Done", async () => {
      chord("3");
      const viewed = getActiveSessionId();
      completeTaskChord();
      await settle();

      expect([viewed, getActiveSessionId()]).toEqual(["progress-1", "scratch"]);
    });

    it("returns to the task it left when Mod+Z undoes its Done", async () => {
      chord("3");
      completeTaskChord();
      await settle();
      const left = getActiveSessionId();
      chord("z");
      await settle();

      expect([left, getActiveSessionId()]).toEqual(["scratch", "progress-1"]);
    });

    it("returns to the task it left when the toast's Undo is clicked", async () => {
      chord("3");
      completeTaskChord();
      await settle();
      const left = getActiveSessionId();
      clickToastUndo();
      await settle();

      expect([left, getActiveSessionId()]).toEqual(["scratch", "progress-1"]);
    });

    it("leaves a plugin page opened meanwhile to return to a task without sessions", async () => {
      // W-1 has no session, so returning to it cannot go through session selection.
      tasks.push(task("W-1", "in_progress"));
      chord("r");
      await settle();
      try {
        chord("4");
        completeTaskChord();
        await settle();
        openPluginPage();
        await settle();
        const onPluginPage = pluginPageOpen();
        clickToastUndo();
        await settle();

        expect([onPluginPage, pluginPageOpen(), showsTaskWorkspace("Title W-1")]).toEqual([
          true,
          false,
          true,
        ]);
      } finally {
        tasks.pop();
        chord("r");
        await settle();
      }
    });

    it("stays put when Mod+Z undoes the Done of a task it did not leave", async () => {
      chord("1");
      markDoneFromCard("Title R-1");
      await settle();
      chord("z");
      await settle();

      expect([
        getActiveSessionId(),
        document.body.textContent?.includes("R-1 moved back to Needs review"),
      ]).toEqual(["scratch", true]);
    });

    it("stays on the viewed task when another task is marked Done", async () => {
      chord("3");
      markDoneFromCard("Title R-1");
      await settle();

      expect([
        getActiveSessionId(),
        document.body.textContent?.includes("R-1 moved to Done"),
      ]).toEqual(["progress-1", true]);
    });
  });
});
