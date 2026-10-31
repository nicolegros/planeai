import { describe, expect, it } from "vitest";
import {
  folderUrgency,
  projectHue,
  rowActions,
  sessionActivity,
  splitBranch,
  type SessionActivity,
} from "../sidebar-cards";
import type { SidebarRow } from "../sidebar-model";
import type { Project, Session, TaskItem } from "../types";

const project: Project = {
  id: "alpha",
  name: "alpha",
  path: "/repos/alpha",
  hidden: false,
  path_missing: false,
};

function session(id: string, status: Session["status"] = "active"): Session {
  return {
    id,
    project_id: "alpha",
    name: id,
    tmux_name: null,
    branch: id,
    status,
    created_at: "",
    worktree_path: null,
    provider: null,
    backend: "local",
    base_branch: null,
    task_key: null,
    task_project_id: null,
  };
}

function taskRow(key: string, status: string, linkedSession?: Session): SidebarRow {
  const task: TaskItem = {
    key,
    title: key,
    status,
    description: "",
    priority: 0,
    blocked_by: [],
    tags: [],
    parent_key: null,
    url: null,
    base_branch: "main",
  };
  return { kind: "task", task, project, linkedSession };
}

function sessionRow(s: Session): SidebarRow {
  return { kind: "session", session: s, project };
}

describe("sessionActivity", () => {
  it("ranks a busy agent over review-ready over exited, and shows nothing when idle", () => {
    const busyExited = session("a", "exited");
    const readyExited = session("b", "exited");
    const states = { a: "Busy", b: "Idle", d: "Idle" };
    const ready = { a: true, b: true };
    expect(
      [busyExited, readyExited, session("c", "exited"), session("d"), undefined].map((s) =>
        sessionActivity(s, states, ready),
      ),
    ).toEqual(["working", "ready", "exited", null, null]);
  });
});

describe("rowActions", () => {
  it("offers review or restart before Done on cards, and one action on slim rows", () => {
    expect([
      rowActions(sessionRow(session("live"))),
      rowActions(taskRow("A-1", "in_review", session("gone", "exited"))),
      rowActions(taskRow("A-2", "in_review")),
      rowActions(taskRow("A-3", "todo"), "todo"),
      rowActions(taskRow("A-4", "done", session("old")), "done"),
    ]).toEqual([["review", "done"], ["restart", "done"], ["done"], ["start"], ["reopen"]]);
  });
});

describe("folderUrgency", () => {
  const activities: Record<string, SessionActivity | null> = {
    busy: "working",
    ready: "ready",
    exited: "exited",
    idle: null,
  };
  const activityOf = (row: SidebarRow) =>
    row.kind === "session" ? activities[row.session.id] : null;

  it("picks the most urgent row: exited, then review, then working", () => {
    expect([
      folderUrgency([sessionRow(session("busy")), sessionRow(session("exited"))], activityOf),
      folderUrgency([sessionRow(session("busy")), sessionRow(session("ready"))], activityOf),
      folderUrgency([sessionRow(session("busy")), taskRow("A-1", "in_review")], activityOf),
      folderUrgency([sessionRow(session("idle")), sessionRow(session("busy"))], activityOf),
      folderUrgency([sessionRow(session("idle"))], activityOf),
      folderUrgency([], activityOf),
    ]).toEqual(["exited", "review", "review", "running", null, null]);
  });
});

describe("projectHue", () => {
  it("maps a project id to a stable hue", () => {
    expect(["alpha", "beta", "planeai", ""].map(projectHue)).toEqual([107, 191, 311, 61]);
  });
});

describe("splitBranch", () => {
  it("keeps the last characters whole so only the head truncates", () => {
    expect([splitBranch("pla-371/feat-t3-code-sidebar"), splitBranch("main")]).toEqual([
      ["pla-371/feat-t3-co", "de-sidebar"],
      ["", "main"],
    ]);
  });
});
