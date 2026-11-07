import { describe, expect, it } from "vitest";
import {
  laneVerdict,
  paneVerdict,
  providerConnectsMcp,
  taskReferenceText,
  type DropVerdict,
  type LaneOutcome,
  type PaneFacts,
  type PaneOutcome,
  type TaskDragPayload,
} from "../task-drop";
import type { TaskLane } from "../sidebar-model";

function dragged(overrides: Partial<TaskDragPayload> = {}): TaskDragPayload {
  return {
    projectId: "alpha",
    projectName: "Alpha",
    repoPath: "/repos/alpha",
    key: "PLA-123",
    title: "Fix the login form",
    status: "todo",
    lane: "todo",
    ...overrides,
  };
}

const lane = (lane: TaskLane, projectId: string | null = null) =>
  ({ kind: "lane", lane, projectId }) as const;

describe("laneVerdict", () => {
  const cases: [string, TaskDragPayload, ReturnType<typeof lane>, DropVerdict<LaneOutcome>][] = [
    [
      "moves a task to another status lane",
      dragged(),
      lane("in_progress"),
      { accepts: true, outcome: { kind: "status", status: "in_progress" }, label: "Running" },
    ],
    [
      "moves a task to Done through the same lane label",
      dragged({ status: "in_review", lane: "in_review" }),
      lane("done", "alpha"),
      { accepts: true, outcome: { kind: "status", status: "done" }, label: "Done" },
    ],
    [
      "reopens a done task",
      dragged({ status: "done", lane: "done" }),
      lane("todo"),
      { accepts: true, outcome: { kind: "status", status: "todo" }, label: "To do" },
    ],
    [
      "leaves a task dropped back on its own lane alone",
      dragged(),
      lane("todo"),
      { accepts: true, outcome: { kind: "stay" }, label: "To do" },
    ],
    [
      "offers the snooze picker on the Snoozed lane",
      dragged(),
      lane("snoozed"),
      { accepts: true, outcome: { kind: "snooze" }, label: "Snooze…" },
    ],
    [
      "refuses to snooze a done task",
      dragged({ status: "done", lane: "done" }),
      lane("snoozed"),
      { accepts: false, reason: "Done tasks cannot be snoozed." },
    ],
    [
      "wakes a snoozed task dropped on its own status lane",
      dragged({ status: "in_progress", lane: "snoozed" }),
      lane("in_progress"),
      { accepts: true, outcome: { kind: "wake" }, label: "Wake" },
    ],
    [
      "wakes and moves a snoozed task dropped on another status lane",
      dragged({ status: "in_progress", lane: "snoozed" }),
      lane("in_review"),
      {
        accepts: true,
        outcome: { kind: "wake_status", status: "in_review" },
        label: "Wake to Needs review",
      },
    ],
    [
      "leaves a snoozed task dropped back on Snoozed alone",
      dragged({ status: "in_progress", lane: "snoozed" }),
      lane("snoozed"),
      { accepts: true, outcome: { kind: "stay" }, label: "Snoozed" },
    ],
    [
      "refuses a lane of another project",
      dragged(),
      lane("in_progress", "beta"),
      { accepts: false, reason: "PLA-123 belongs to Alpha." },
    ],
    [
      "accepts the lane of the task's own project",
      dragged(),
      lane("in_progress", "alpha"),
      { accepts: true, outcome: { kind: "status", status: "in_progress" }, label: "Running" },
    ],
  ];

  it.each(cases)("%s", (_name, task, target, verdict) => {
    expect(laneVerdict(task, target)).toEqual(verdict);
  });
});

describe("paneVerdict", () => {
  const agent = (overrides: Partial<NonNullable<PaneFacts["session"]>> = {}): PaneFacts => ({
    tab: "agent",
    session: {
      plugin: false,
      provider: "claude",
      providerHasMcp: true,
      taskProjectId: "alpha",
      taskKey: "PLA-7",
      ...overrides,
    },
  });

  const cases: [string, PaneFacts, DropVerdict<PaneOutcome>][] = [
    [
      "accepts an agent terminal whose provider has MCP",
      agent(),
      {
        accepts: true,
        outcome: { kind: "reference", sessionTaskProjectId: "alpha" },
        label: "Tell the agent about PLA-123",
      },
    ],
    [
      "refuses an empty pane",
      { tab: null, session: null },
      { accepts: false, reason: "This pane has no tab to type into." },
    ],
    [
      "refuses a shell tab",
      { ...agent(), tab: "shell" },
      { accepts: false, reason: "Drop the task on an agent tab, not a shell tab." },
    ],
    [
      "refuses an editor tab",
      { ...agent(), tab: "editor" },
      { accepts: false, reason: "Drop the task on an agent tab, not an editor tab." },
    ],
    [
      "refuses a diff tab",
      { ...agent(), tab: "diff" },
      { accepts: false, reason: "Drop the task on an agent tab, not a diff tab." },
    ],
    [
      "refuses a plugin chat",
      agent({ plugin: true }),
      {
        accepts: false,
        reason: "Chats do not take dropped tasks yet. Drop it on an agent terminal.",
      },
    ],
    [
      "refuses a provider without MCP, naming it",
      agent({ provider: "kiro", providerHasMcp: false }),
      { accepts: false, reason: "kiro can't look up tasks: its provider has no MCP connection." },
    ],
    [
      "refuses a session without a provider the same way",
      agent({ provider: null, providerHasMcp: false }),
      {
        accepts: false,
        reason: "This agent can't look up tasks: its provider has no MCP connection.",
      },
    ],
    [
      "refuses the task's own session",
      agent({ taskKey: "PLA-123" }),
      { accepts: false, reason: "PLA-123 is already this session's task." },
    ],
    [
      "accepts a session on a same-key task of another project",
      agent({ taskProjectId: "beta", taskKey: "PLA-123" }),
      {
        accepts: true,
        outcome: { kind: "reference", sessionTaskProjectId: "beta" },
        label: "Tell the agent about PLA-123",
      },
    ],
    [
      "refuses a tab whose session is gone",
      { tab: "agent", session: null },
      { accepts: false, reason: "This pane's session is gone." },
    ],
  ];

  it.each(cases)("%s", (_name, facts, verdict) => {
    expect(paneVerdict(dragged(), facts)).toEqual(verdict);
  });
});

describe("providerConnectsMcp", () => {
  const providers = {
    claude: { mcp_args: "--mcp-config {mcp_url}" },
    kiro: { mcp_args: "" },
    custom: { mcp_args: "  " },
    legacy: {},
  };

  it("holds for a provider with MCP arguments while the server is on, which unset means", () => {
    expect(providerConnectsMcp({ providers }, "claude")).toBe(true);
    expect(providerConnectsMcp({ mcp_enabled: true, providers }, "claude")).toBe(true);
    expect(providerConnectsMcp({ mcp_enabled: null, providers }, "claude")).toBe(true);
  });

  it("fails once the MCP server is turned off, whatever the provider's arguments", () => {
    expect(providerConnectsMcp({ mcp_enabled: false, providers }, "claude")).toBe(false);
  });

  it("fails for a provider whose arguments are empty, blank or missing, and for no provider", () => {
    expect(providerConnectsMcp({ providers }, "kiro")).toBe(false);
    expect(providerConnectsMcp({ providers }, "custom")).toBe(false);
    expect(providerConnectsMcp({ providers }, "legacy")).toBe(false);
    expect(providerConnectsMcp({ providers }, "unknown")).toBe(false);
    expect(providerConnectsMcp({ providers }, null)).toBe(false);
  });
});

describe("taskReferenceText", () => {
  it("names the task by key and title for a session of the same project", () => {
    expect(taskReferenceText(dragged(), "alpha")).toBe("Task PLA-123: Fix the login form");
  });

  it("names the project when the session works on another project's task", () => {
    expect(taskReferenceText(dragged(), "beta")).toBe(
      "Task PLA-123 in project Alpha: Fix the login form",
    );
  });

  it("types no control characters, whatever the title and project hold", () => {
    const task = dragged({
      title: "Fix\x1b the\nlogin\r\x7f form\u0085\t\x00now",
      projectName: "Al\x15pha",
    });
    expect(taskReferenceText(task, "beta")).toBe(
      "Task PLA-123 in project Al pha: Fix the login form now",
    );
  });

  it("never ends with a newline, so nothing is submitted", () => {
    expect(taskReferenceText(dragged({ title: "Trailing\n" }), "alpha")).toBe(
      "Task PLA-123: Trailing",
    );
  });
});
