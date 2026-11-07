/**
 * Task drops: what dropping a dragged task on a lane or a session pane does. The verdicts are pure
 * so the sidebar can show the outcome while the pointer hovers, and the drop handlers can trust it.
 */
import { LANES, type TaskLane, type TaskStatus } from "./sidebar-model";
import { DONE_CANNOT_SNOOZE } from "./task-snooze";
import type { TabType } from "./layout-tree";

/** The task a sidebar row is dragged as. */
export interface TaskDragPayload {
  projectId: string;
  projectName: string;
  repoPath: string;
  key: string;
  title: string;
  status: TaskStatus;
  /** Where it sits in the sidebar: its status, or snoozed. */
  lane: TaskLane;
}

export type TaskDropTarget =
  /** A lane section; `projectId` is null for a lane shared by every project. */
  { kind: "lane"; lane: TaskLane; projectId: string | null } | { kind: "pane"; paneId: string };

export type LaneDropTarget = Extract<TaskDropTarget, { kind: "lane" }>;

export type DropVerdict<Outcome> =
  | { accepts: true; outcome: Outcome; label: string }
  | { accepts: false; reason: string };

export type LaneOutcome =
  /** The task's own lane: nothing changes. */
  | { kind: "stay" }
  | { kind: "status"; status: TaskStatus }
  /** A snoozed task back on its own status lane. */
  | { kind: "wake" }
  | { kind: "wake_status"; status: TaskStatus }
  /** Opens the snooze picker. */
  | { kind: "snooze" };

export function laneVerdict(
  task: TaskDragPayload,
  target: LaneDropTarget,
): DropVerdict<LaneOutcome> {
  if (target.projectId !== null && target.projectId !== task.projectId) {
    return { accepts: false, reason: `${task.key} belongs to ${task.projectName}.` };
  }
  const label = LANES[target.lane].label;
  if (target.lane === task.lane) return { accepts: true, outcome: { kind: "stay" }, label };
  if (target.lane === "snoozed") {
    if (task.status === "done") return { accepts: false, reason: DONE_CANNOT_SNOOZE };
    return { accepts: true, outcome: { kind: "snooze" }, label: "Snooze…" };
  }
  if (task.lane === "snoozed") {
    if (target.lane === task.status) {
      return { accepts: true, outcome: { kind: "wake" }, label: "Wake" };
    }
    return {
      accepts: true,
      outcome: { kind: "wake_status", status: target.lane },
      label: `Wake to ${label}`,
    };
  }
  return { accepts: true, outcome: { kind: "status", status: target.lane }, label };
}

/** What the drop handler needs to know about a pane; the app resolves it from its layout and sessions. */
export interface PaneFacts {
  /** The pane's front tab, or null for an empty pane. */
  tab: TabType | null;
  /** The front tab's session; null when it is gone. */
  session: {
    plugin: boolean;
    provider: string | null;
    providerHasMcp: boolean;
    taskProjectId: string;
    taskKey: string | null;
  } | null;
}

export type PaneOutcome = { kind: "reference" };

/**
 * Whether a session of `provider` runs with PlaneAI's MCP tools, as the backend decides at launch:
 * the server is on and the provider has arguments for it.
 */
export function providerConnectsMcp(
  config: {
    mcp_enabled?: boolean | null;
    providers: Record<string, { mcp_args?: string | null } | undefined>;
  },
  provider: string | null,
): boolean {
  if (config.mcp_enabled === false || provider === null) return false;
  return !!config.providers[provider]?.mcp_args?.trim();
}

const TAB_NAMES: Record<Exclude<TabType, "agent">, string> = {
  shell: "a shell",
  diff: "a diff",
  editor: "an editor",
};

export function paneVerdict(task: TaskDragPayload, pane: PaneFacts): DropVerdict<PaneOutcome> {
  if (pane.tab === null) return { accepts: false, reason: "This pane has no tab to type into." };
  if (pane.tab !== "agent") {
    return {
      accepts: false,
      reason: `Drop the task on an agent tab, not ${TAB_NAMES[pane.tab]} tab.`,
    };
  }
  const session = pane.session;
  if (!session) return { accepts: false, reason: "This pane's session is gone." };
  if (session.plugin) {
    return {
      accepts: false,
      reason: "Chats do not take dropped tasks yet. Drop it on an agent terminal.",
    };
  }
  if (!session.providerHasMcp) {
    const agent = session.provider ?? "This agent";
    return {
      accepts: false,
      reason: `${agent} can't look up tasks: its provider has no MCP connection.`,
    };
  }
  if (session.taskProjectId === task.projectId && session.taskKey === task.key) {
    return { accepts: false, reason: `${task.key} is already this session's task.` };
  }
  return {
    accepts: true,
    outcome: { kind: "reference" },
    label: `Tell the agent about ${task.key}`,
  };
}

/**
 * Typed bytes reach the agent's line editor as keystrokes, so a control character in a title
 * (an escape sequence, a newline) could rewrite or submit the line. Each run becomes one space.
 */
const CONTROL_RUN = /[\x00-\x1f\x7f-\x9f]+/g;

function typeable(text: string): string {
  return text.replace(CONTROL_RUN, " ").replace(/\s+/g, " ").trim();
}

/**
 * The reference typed into an agent for a dropped task, which it resolves with the `task_show`
 * MCP tool. The project is named when the session works on another project's task. No newline:
 * the user adds their ask and submits.
 */
export function taskReferenceText(
  task: Pick<TaskDragPayload, "key" | "title" | "projectId" | "projectName">,
  sessionTaskProjectId: string,
): string {
  const where =
    task.projectId === sessionTaskProjectId ? "" : ` in project ${typeable(task.projectName)}`;
  return `Task ${typeable(task.key)}${where}: ${typeable(task.title)}`;
}
