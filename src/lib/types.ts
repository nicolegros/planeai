export interface Session {
  id: string;
  project_id: string;
  name: string;
  tmux_name: string | null;
  branch: string;
  status: "active" | "exited" | "archived";
  created_at: string;
  worktree_path: string | null;
  provider: string | null;
  backend: string;
  base_branch: string | null;
  task_key: string | null;
  /** Project owning the linked task; null means the session's own project. */
  task_project_id: string | null;
}

/** The project whose TaskWorkspace a session belongs to. */
export function sessionTaskProjectId(
  session: Pick<Session, "project_id" | "task_project_id">,
): string {
  return session.task_project_id ?? session.project_id;
}

export interface LaunchResult {
  session: Session;
  warning: string | null;
}

export interface Project {
  id: string;
  name: string;
  path: string;
  hidden: boolean;
  /** The folder is gone from disk, e.g. moved outside PlaneAI. */
  path_missing: boolean;
}

export interface TaskItem {
  key: string;
  title: string;
  status: string;
  description: string;
  priority: number;
  blocked_by: string[];
  tags: string[];
  parent_key: string | null;
  url: string | null;
  base_branch: string;
  /** RFC 3339; together with `last_activity_at` decides whether the task is unread. */
  last_visited_at: string | null;
  last_activity_at: string | null;
  /** RFC 3339; see `src/lib/task-snooze.ts` for what they mean together. */
  snoozed_until: string | null;
  snoozed_at: string | null;
  woke_at: string | null;
  /** Bumped by every write to the snooze; orders the copies of it the store receives. */
  snooze_rev: number;
  /** Tells a task apart from an earlier one created under the same key. */
  created_at: string;
}

export type TaskVisit = Pick<TaskItem, "last_visited_at" | "last_activity_at">;
export type TaskSnooze = Pick<TaskItem, "snoozed_until" | "snoozed_at" | "woke_at">;
/** A snooze as the backend wrote it, with its revision. */
export type StoredSnooze = TaskSnooze & Pick<TaskItem, "snooze_rev">;

import type { ReviewFileDiff } from "./review-diff";

export interface DirEntry {
  name: string;
  path: string;
  is_dir: boolean;
}

export interface FsChangeEvent {
  session_id: string;
  path: string;
  kind: "create" | "remove" | "modify" | "rename";
}

export interface ChangedFile {
  path: string;
  status: string;
  additions: number;
  deletions: number;
  old_path: string | null;
}

export type FileDiff = ReviewFileDiff;

// ─── Plugin runtime types ───────────────────────────────────────────────────

export type PluginSourceKind = "builtin" | "local";
export type PluginRuntimeState = "disabled" | "starting" | "running" | "stopping" | "error";

export type PluginUiPlacement =
  | "sidebar.header"
  | "sidebar.navigation"
  | "sidebar.section"
  | "sidebar.footer"
  | "preferences"
  | "main-pane"
  | "dialog"
  | "session.panel"
  | "session.indicator"
  | "session.meta"
  | "titlebar"
  | "interaction"
  /** Host-synthesized placement for a provider's session UI; never declared in manifests. */
  | "session.main";

export interface PluginUiContribution {
  id: string;
  label: string;
  placement: PluginUiPlacement;
  entrypoint: string;
  order: number | null;
  shortcut: string | null;
}

/** A running plugin's UI contribution, as hosts render it. */
export type PluginSlot = { plugin: PluginInventory; contribution: PluginUiContribution };

export type ProviderFeature = "auto_approve" | "handoff";

/** What a terminal tab runs when it starts; a program runs without a shell to parse it. */
export type TabSpec =
  | { kind: "shell" }
  | { kind: "command"; command: string }
  | { kind: "program"; argv: readonly string[] };

/** Why a terminal tab ended, as its one `tab-ended` event reports it (ADR-0015). */
export type TabEndReason = "closed" | "exited" | "failed_to_start" | "session_ended";

export interface TabEnded {
  pty_key: string;
  reason: TabEndReason;
  /** Why it failed to start. */
  error?: string;
}

/** A session runtime declared by a plugin; its UI replaces the agent terminal (ADR-0014). */
export interface PluginProvider {
  id: string;
  label: string;
  entrypoint: string;
  supports: ProviderFeature[];
}

/** An opaque provider event forwarded from the sidecar to the session's UI. */
export interface ProviderSessionEvent {
  plugin_id: string;
  session_id: string;
  seq: number;
  payload: unknown;
}

/** A sidecar-registered action rendered in a host session context menu. */
export interface PluginSessionAction {
  plugin_id: string;
  id: string;
  label: string;
  /** Empty means all providers; otherwise the action is shown only for these provider IDs. */
  providers: string[];
}

export type PluginSessionAdvisorySeverity = "info" | "warning" | "error";

export interface PluginSessionAdvisory {
  session_id: string;
  message: string;
  severity: PluginSessionAdvisorySeverity;
}

export interface PluginSessionCompletion {
  session_id: string;
  message: string | null;
}

export interface PluginDiscoveryCandidate {
  full_name: string;
  description: string | null;
  url: string;
  updated_at: string;
  stargazers_count: number;
  language: string | null;
}

/** Where a local plugin installed from Discovery came from (ADR-0018). */
export interface PluginRemoteOrigin {
  repo: string;
  release_tag: string | null;
  source_url: string;
  archive_sha256: string;
}

export interface PluginInventory {
  id: string;
  name: string;
  version: string;
  host_api_version: string;
  source_kind: PluginSourceKind;
  backend_entrypoint: string;
  capabilities: string[];
  background_service?: {
    method: string;
    interval_setting: string;
    default_interval_ms: number;
  } | null;
  ui_contributions: PluginUiContribution[];
  providers: PluginProvider[];
  tools: PluginTool[];
  installed_hash: string | null;
  installed_path: string | null;
  original_display_path: string | null;
  remote_origin: PluginRemoteOrigin | null;
  enabled: boolean;
  state: PluginRuntimeState;
  last_error: string | null;
  log_path: string | null;
}

/** What a Discovery card can offer for this host, resolved from GitHub and the installed inventory. */
export type DiscoveryCardState =
  | { kind: "unavailable"; reason: string }
  | { kind: "install"; latest_version: string | null }
  | { kind: "up_to_date"; installed_version: string }
  | { kind: "update"; installed_version: string; latest_version: string }
  | { kind: "reinstall"; installed_version: string }
  | { kind: "installed_locally"; installed_version: string; latest_version: string | null }
  | {
      kind: "installed_from_other_repo";
      repo: string;
      installed_version: string;
      latest_version: string | null;
    };

export type PackageIntegrity =
  | { kind: "verified"; sha256: string }
  | { kind: "unverified"; sha256: string };

export type StagedPluginReplacement =
  | { kind: "none" }
  | { kind: "repo"; installed_version: string }
  | { kind: "other_repo"; repo: string; installed_version: string }
  | { kind: "local"; installed_version: string; original_display_path: string | null };

/** A manifest tool declaration as the backend serializes it (camelCase, ADR-0017). */
export interface PluginTool {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
  annotations: Record<string, unknown>;
  timeoutMs?: number;
}

/** A downloaded, verified and extracted package awaiting "Install and enable" or discard. */
export interface StagedPluginInstall {
  staging_id: string;
  repo: string;
  source_url: string;
  release_tag: string | null;
  integrity: PackageIntegrity;
  manifest: {
    id: string;
    name: string;
    version: string;
    capabilities: string[];
    ui_contributions: PluginUiContribution[];
    tools: PluginTool[];
  };
  replaces: StagedPluginReplacement;
  version_mismatch: { release_tag: string; manifest_version: string } | null;
}

export type JiraMigrationState = "not_needed" | "available" | "importing" | "failed" | "completed";
export type GithubMigrationState = JiraMigrationState;

export interface GithubMigrationStatus {
  state: GithubMigrationState;
  legacy_detected: boolean;
  can_migrate: boolean;
  message: string;
  error: string | null;
  imported_pull_requests: number;
  skipped_state_only: number;
  snapshot_path: string | null;
}

export interface JiraMigrationStatus {
  state: JiraMigrationState;
  legacy_detected: boolean;
  can_migrate: boolean;
  message: string;
  error: string | null;
  imported_issues: number;
  imported_links: number;
  snapshot_path: string | null;
}

export interface JiraPluginStatus {
  plugin_id: string;
  plugin_name: string;
  plugin_version: string;
  host_api_version: string;
  runtime_state: PluginRuntimeState;
  last_error: string | null;
}

export interface CommitEntry {
  sha: string;
  short_sha: string;
  subject: string;
}

export interface JiraStatus {
  connected: boolean;
  site: string | null;
}

export interface SyncResult {
  created: number;
  updated: number;
  departed: number;
  errors: number;
}

export interface JiraTasksResponse {
  tasks: TaskItem[];
  child_counts: Record<string, number>;
}
