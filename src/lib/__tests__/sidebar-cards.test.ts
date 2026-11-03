import { describe, expect, it } from "vitest";
import {
  folderUrgency,
  rowActions,
  rowActivity,
  splitBranch,
  type RowActivity,
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

function taskRow(
  key: string,
  status: string,
  linkedSession?: Session,
  visit: Partial<TaskItem> = {},
): SidebarRow {
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
    last_visited_at: null,
    last_activity_at: null,
    snoozed_until: null,
    snoozed_at: null,
    woke_at: null,
    snooze_rev: 0,
    created_at: "2026-11-01T09:00:00.000Z",
    ...visit,
  };
  return { kind: "task", task, project, linkedSession };
}

function sessionRow(s: Session): SidebarRow {
  return { kind: "session", session: s, project };
}

describe("rowActivity", () => {
  const states = { busy: "Busy", idle: "Idle", "busy-gone": "Busy" };
  const unreadSessions = { busy: true, finished: true, "finished-gone": true, linked: true };
  const NOW = Date.parse("2026-11-03T12:00:00Z");
  const activityOf = (row: SidebarRow) => rowActivity(row, states, unreadSessions, NOW);

  it("ranks a busy agent over unread over exited for sessions, and shows nothing when idle", () => {
    expect(
      [
        session("busy"),
        session("finished"),
        session("finished-gone", "exited"),
        session("gone", "exited"),
        session("idle"),
      ].map((s) => activityOf(sessionRow(s))),
    ).toEqual(["working", "unread", "unread", "exited", null]);
  });

  it("reads a task's unread from its visit, not from its session", () => {
    const seen = {
      last_activity_at: "2026-11-03T10:00:00Z",
      last_visited_at: "2026-11-03T10:05:00Z",
    };
    const unseen = { last_activity_at: "2026-11-03T10:00:00Z", last_visited_at: null };
    expect([
      activityOf(taskRow("A-1", "in_review", session("busy-gone", "exited"), unseen)),
      activityOf(taskRow("A-2", "in_review", session("linked"), seen)),
      activityOf(taskRow("A-3", "in_review", session("gone", "exited"), unseen)),
      activityOf(taskRow("A-4", "in_review", undefined, unseen)),
      activityOf(taskRow("A-5", "in_review", session("gone", "exited"), seen)),
      activityOf(taskRow("A-6", "todo")),
      activityOf(taskRow("A-7", "done", undefined, unseen)),
    ]).toEqual(["working", null, "unread", "unread", "exited", null, null]);
  });

  it("shows a snoozed task as snoozed only, and a woken one as Woke over anything else until visited", () => {
    const snoozedAt = "2026-11-03T09:00:00Z";
    const unseen = { last_activity_at: "2026-11-03T11:00:00Z", last_visited_at: null };
    const asleep = { ...unseen, snoozed_until: "2026-11-03T13:00:00Z", snoozed_at: snoozedAt };
    const wokeEarly = { ...asleep, woke_at: "2026-11-03T11:00:00Z" };
    const wokeOnTime = { snoozed_until: "2026-11-03T12:00:00Z", snoozed_at: snoozedAt };
    const visitedSince = { ...wokeEarly, last_visited_at: "2026-11-03T11:30:00Z" };
    expect([
      activityOf(taskRow("A-1", "in_progress", session("busy"), asleep)),
      activityOf(taskRow("A-2", "in_progress", session("busy"), wokeEarly)),
      activityOf(taskRow("A-3", "in_review", session("gone", "exited"), wokeOnTime)),
      activityOf(taskRow("A-4", "in_review", session("gone", "exited"), visitedSince)),
      activityOf(taskRow("A-5", "done", undefined, wokeEarly)),
    ]).toEqual(["snoozed", "woke", "woke", "exited", null]);
  });
});

describe("rowActions", () => {
  it("offers review or restart before Done or Archive on cards, and one action on slim rows", () => {
    expect([
      rowActions(sessionRow(session("live"))),
      rowActions(taskRow("A-1", "in_review", session("gone", "exited"))),
      rowActions(taskRow("A-2", "in_review")),
      rowActions(taskRow("A-3", "todo"), "todo"),
      rowActions(taskRow("A-4", "done", session("old")), "done"),
      rowActions(taskRow("A-5", "in_progress", session("live")), "snoozed"),
      rowActions(taskRow("A-6", "todo"), "todo", "woke"),
    ]).toEqual([
      ["review", "archive"],
      ["restart", "done"],
      ["done"],
      ["start"],
      ["reopen"],
      ["wake"],
      ["seen", "start"],
    ]);
  });
});

describe("folderUrgency", () => {
  const activities: Record<string, RowActivity | null> = {
    busy: "working",
    unread: "unread",
    exited: "exited",
    idle: null,
    woke: "woke",
  };
  const activityOf = (row: SidebarRow) =>
    row.kind === "session" ? activities[row.session.id] : null;

  it("picks the most urgent row: exited, then review, then working", () => {
    expect([
      folderUrgency([sessionRow(session("busy")), sessionRow(session("exited"))], activityOf),
      folderUrgency([sessionRow(session("busy")), sessionRow(session("unread"))], activityOf),
      folderUrgency([sessionRow(session("busy")), taskRow("A-1", "in_review")], activityOf),
      folderUrgency([sessionRow(session("idle")), sessionRow(session("busy"))], activityOf),
      folderUrgency([sessionRow(session("idle"))], activityOf),
      folderUrgency([], activityOf),
      folderUrgency([sessionRow(session("busy")), sessionRow(session("woke"))], activityOf),
    ]).toEqual(["exited", "review", "review", "running", null, null, "review"]);
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
