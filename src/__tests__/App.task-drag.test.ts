import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { flushSync, mount, tick, unmount } from "svelte";
import type { Project, Session, TaskItem } from "../lib/types";

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

function session(id: string, provider: string, taskKey: string | null): Session {
  return {
    id,
    project_id: alpha.id,
    name: id,
    tmux_name: null,
    branch: id,
    status: "active",
    created_at: "",
    worktree_path: null,
    provider,
    backend: "local",
    base_branch: null,
    task_key: taskKey,
    task_project_id: taskKey ? alpha.id : null,
  };
}

const sessions = [session("claude-1", "claude", "P-1"), session("kiro-1", "kiro", null)];
const tasks = [task("P-1", "in_progress"), task("T-1", "todo"), task("D-1", "done")];
const providers = {
  claude: { command: "claude", yolo_flag: null, mcp_args: "--mcp-config {mcp_url}" },
  kiro: { command: "kiro", yolo_flag: null, mcp_args: "" },
};
let config: Record<string, unknown> = {};

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => {
    if (command === "get_config") {
      return {
        appearance: { mode: "light", theme: "default" },
        terminal: { font_family: "Menlo", font_size: 14 },
        providers,
        default_provider: "claude",
        sidebar_group_by: "status",
        ...config,
      };
    }
    if (command === "list_projects") return [alpha];
    if (command === "list_sessions") return sessions;
    if (command === "list_all_task_items") return tasks;
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
import { selectSession } from "../lib/session-orchestrator.svelte";

async function settle(): Promise<void> {
  for (let i = 0; i < 10; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    await tick();
  }
}

let app: ReturnType<typeof mount> | undefined;

async function mountApp(overrides: Record<string, unknown> = {}): Promise<HTMLElement> {
  config = overrides;
  const target = document.createElement("div");
  document.body.append(target);
  app = mount(App, { target });
  await settle();
  return target;
}

/** The pane showing `sessionId`'s workspace. */
async function openPane(root: HTMLElement, sessionId: string): Promise<HTMLElement> {
  selectSession(sessionId, { explicit: true });
  await settle();
  const pane = root.querySelector<HTMLElement>("[data-pane-drop]");
  if (!pane) throw new Error("no pane shown");
  return pane;
}

function rowButton(root: HTMLElement, title: string): HTMLElement {
  const button = [...root.querySelectorAll<HTMLElement>("nav [data-nav-index]")].find((b) =>
    b.getAttribute("aria-labelledby")
      ? document.getElementById(b.getAttribute("aria-labelledby")!)?.textContent?.includes(title)
      : b.textContent?.includes(title),
  );
  if (!button) throw new Error(`no row titled ${title}`);
  return button;
}

function moveOver(over: () => Element | null): void {
  document.elementFromPoint = over;
  window.dispatchEvent(new MouseEvent("pointermove", { clientX: 50, clientY: 120 }));
  flushSync();
}

/** Presses the row and drags it over whatever `over` returns, once the drag has shown every lane. */
function dragRow(root: HTMLElement, title: string, over: () => Element | null): void {
  rowButton(root, title).dispatchEvent(
    new PointerEvent("pointerdown", { bubbles: true, button: 0, clientX: 50, clientY: 100 }),
  );
  moveOver(() => null);
  moveOver(over);
}

function release(): void {
  window.dispatchEvent(new MouseEvent("pointerup", { clientX: 50, clientY: 120 }));
  flushSync();
}

const refused = () => document.body.classList.contains("task-drop-refused");
const preview = (pane: HTMLElement) => {
  const element = pane.querySelector(".pane-drop-preview");
  return element
    ? element.classList.contains("pane-drop-refused")
      ? "refused"
      : "accepted"
    : null;
};

describe("dragging a task over a session pane", () => {
  beforeAll(() => {
    HTMLElement.prototype.scrollIntoView = vi.fn();
    Object.defineProperty(document, "fonts", {
      value: { load: () => Promise.resolve([]), ready: Promise.resolve() },
    });
    globalThis.ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    };
  });

  afterEach(() => {
    release();
    if (app) unmount(app);
    app = undefined;
    document.body.replaceChildren();
    document.body.className = "";
  });

  it("previews a drop on an agent whose provider has MCP tools", async () => {
    const root = await mountApp();
    const pane = await openPane(root, "claude-1");

    dragRow(root, "Title T-1", () => pane);

    expect([preview(pane), refused()]).toEqual(["accepted", false]);
  });

  it("refuses the pane once the MCP server is turned off", async () => {
    const root = await mountApp({ mcp_enabled: false });
    const pane = await openPane(root, "claude-1");

    dragRow(root, "Title T-1", () => pane);

    expect([preview(pane), refused()]).toEqual(["refused", true]);
    release();
    expect([preview(pane), refused()]).toEqual([null, false]);
  });

  it("refuses an agent whose provider has no MCP arguments", async () => {
    const root = await mountApp();
    const pane = await openPane(root, "kiro-1");

    dragRow(root, "Title T-1", () => pane);

    expect([preview(pane), refused()]).toEqual(["refused", true]);
  });

  it("shows the refusal of a sidebar lane the same way, until the task leaves it", async () => {
    const root = await mountApp();
    await openPane(root, "claude-1");
    root.querySelector<HTMLButtonElement>('[data-task-lane="done"] button')!.click();
    flushSync();

    dragRow(root, "Title D-1", () => root.querySelector('[data-task-lane="snoozed"]'));
    expect(refused()).toBe(true);
    moveOver(() => root.querySelector('[data-task-lane="todo"]'));
    expect(refused()).toBe(false);
  });
});
