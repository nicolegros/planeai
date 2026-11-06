import { afterEach, describe, expect, it, vi } from "vitest";
import { hitTest, liftOffset, pressTaskRow, taskDrag, taskDragPayload } from "../task-drag.svelte";
import type { SidebarTaskRow } from "../sidebar-model";
import type { Project, TaskItem } from "../types";

const rect = (left: number, top: number, width: number, height: number) =>
  ({ left, top, width, height, right: left + width, bottom: top + height }) as DOMRect;

function pointer(type: string, clientX: number, clientY: number) {
  window.dispatchEvent(new MouseEvent(type, { clientX, clientY, bubbles: true }));
}

const project: Project = {
  id: "alpha",
  name: "Alpha",
  path: "/repos/alpha",
  hidden: false,
  path_missing: false,
};

function row(overrides: Partial<TaskItem> = {}): SidebarTaskRow {
  return {
    kind: "task",
    project,
    linkedSession: undefined,
    task: {
      key: "PLA-1",
      title: "Fix login",
      status: "in_progress",
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
      ...overrides,
    },
  };
}

const NOW = Date.parse("2026-11-06T10:00:00.000Z");

/** A sidebar list spanning x 0-200 with a shelf and a project group, and a pane to its right. */
function layoutDom() {
  document.body.innerHTML = `
    <nav data-task-drag-list id="list">
      <section data-task-lane="todo" id="shelf"><button id="shelf-header">To do</button><ul><li id="shelf-row"></li></ul></section>
      <div data-task-lane="in_review" data-task-lane-project="beta" id="group"></div>
      <div id="gap"></div>
    </nav>
    <div data-pane-drop="p2" id="pane"><div id="xterm"></div></div>`;
  const at: Record<string, string> = {
    "10": "shelf-header",
    "20": "shelf-row",
    "50": "group",
    "90": "gap",
    "300": "xterm",
  };
  document.elementFromPoint = (x: number) => document.getElementById(at[String(x)] ?? "") ?? null;
  document.getElementById("list")!.getBoundingClientRect = () => rect(0, 0, 200, 600);
}

afterEach(() => {
  pointer("pointerup", 90, 0);
  document.body.replaceChildren();
  document.body.className = "";
});

describe("hitTest", () => {
  it("finds a lane from its header or a row inside it, with its project when it has one", () => {
    layoutDom();
    expect(hitTest(10, 5)).toEqual({ kind: "lane", lane: "todo", projectId: null });
    expect(hitTest(20, 5)).toEqual({ kind: "lane", lane: "todo", projectId: null });
    expect(hitTest(50, 5)).toEqual({ kind: "lane", lane: "in_review", projectId: "beta" });
  });

  it("finds a pane from anything inside it, and nothing elsewhere", () => {
    layoutDom();
    expect(hitTest(300, 5)).toEqual({ kind: "pane", paneId: "p2" });
    expect(hitTest(90, 5)).toBeNull();
  });
});

describe("taskDragPayload", () => {
  it("carries what verdicts and the reference text need, with the task's lane", () => {
    expect(taskDragPayload(row(), NOW)).toEqual({
      projectId: "alpha",
      projectName: "Alpha",
      repoPath: "/repos/alpha",
      key: "PLA-1",
      title: "Fix login",
      status: "in_progress",
      lane: "in_progress",
    });
  });

  it("puts a snoozed task in the snoozed lane while keeping its status", () => {
    const snoozed = row({ snoozed_until: "2026-11-07T09:00:00.000Z", snoozed_at: "2026-11-06" });
    expect(taskDragPayload(snoozed, NOW)).toMatchObject({ status: "in_progress", lane: "snoozed" });
  });
});

describe("pressTaskRow", () => {
  function press(x = 20, y = 100) {
    const drop = vi.fn();
    const down = new PointerEvent("pointerdown", { button: 0, clientX: x, clientY: y });
    Object.defineProperty(down, "target", { value: document.getElementById("shelf-row") });
    pressTaskRow(taskDragPayload(row(), NOW), down, drop);
    return drop;
  }

  it("stays inside the sidebar while the pointer is within the list's width", () => {
    layoutDom();
    press();
    pointer("pointermove", 50, 140);
    expect(taskDrag.current?.point).toEqual({ x: 50, y: 140 });
    expect(taskDrag.outside).toBe(false);
    expect(document.body.classList.contains("task-dragging")).toBe(true);
  });

  it("is outside once the pointer leaves the list sideways, and back inside on return", () => {
    layoutDom();
    press();
    pointer("pointermove", 300, 140);
    expect(taskDrag.outside).toBe(true);
    pointer("pointermove", 90, 700);
    expect(taskDrag.outside).toBe(false);
  });

  it("drops the payload on the lane or pane under the pointer", () => {
    layoutDom();
    const drop = press();
    pointer("pointermove", 50, 140);
    pointer("pointerup", 50, 140);
    expect(drop).toHaveBeenCalledWith(expect.objectContaining({ key: "PLA-1" }), {
      kind: "lane",
      lane: "in_review",
      projectId: "beta",
    });
    expect(taskDrag.current).toBeNull();
  });
});

describe("liftOffset", () => {
  const list = { top: 100, bottom: 500 };
  const home = { top: 200, bottom: 232 };

  it("follows the pointer vertically from where the press started", () => {
    expect(liftOffset(150, 140, home, list)).toBe(10);
    expect(liftOffset(120, 140, home, list)).toBe(-20);
  });

  it("stops at the edges of the list", () => {
    expect(liftOffset(0, 140, home, list)).toBe(-100);
    expect(liftOffset(900, 140, home, list)).toBe(268);
  });
});
