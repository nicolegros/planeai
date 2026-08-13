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
  tab_count: number;
  base_branch: string | null;
  task_key: string | null;
  pr_url: string | null;
  pr_state: string | null;
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
}

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

export interface FileDiff {
  original: string;
  modified: string;
  language: string;
}

export interface CiCheck {
  name: string;
  status: string;
  conclusion: string | null;
  url: string | null;
}

// ─── Plugin runtime types ───────────────────────────────────────────────────

export type PluginSourceKind = "builtin" | "local";
export type PluginRuntimeState = "disabled" | "starting" | "running" | "stopping" | "error";

export type PluginUiPlacement =
  | "sidebar.header"
  | "sidebar.navigation"
  | "sidebar.footer"
  | "main-pane";

export interface PluginUiContribution {
  id: string;
  label: string;
  placement: PluginUiPlacement;
  entrypoint: string;
  order: number | null;
  shortcut: string | null;
}

export interface PluginInventory {
  id: string;
  name: string;
  version: string;
  host_api_version: string;
  source_kind: PluginSourceKind;
  backend_entrypoint: string;
  ui_contributions: PluginUiContribution[];
  installed_hash: string | null;
  installed_path: string | null;
  original_display_path: string | null;
  enabled: boolean;
  state: PluginRuntimeState;
  last_error: string | null;
  log_path: string | null;
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

export interface PrStatus {
  checks: CiCheck[];
  conflicting: boolean;
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

// ─── Loop types ──────────────────────────────────────────────────────────────

import type { LoopStatusValue } from "./loop-status";

export interface LoopRunSummary {
  id: string;
  project_id: string;
  task_key: string | null;
  strategy: string;
  goal: string;
  status: LoopStatusValue;
  max_rounds: number;
  created_at: string;
  updated_at: string;
}

export interface LoopSessionItem {
  session_id: string;
  role: string;
  round: number;
  provider: string | null;
  status: string;
  created_at: string;
}

export interface LoopEventItem {
  id: number;
  ts: string;
  kind: string;
  payload_json: unknown;
}

export interface LoopArtifactItem {
  id: string;
  session_id: string | null;
  kind: string;
  path: string | null;
  content_json: unknown | null;
  created_at: string;
}

export interface VerifierRunItem {
  id: string;
  session_id: string | null;
  verifier_type: string;
  name: string;
  command: string;
  status: string;
  exit_code: number | null;
  output_path: string | null;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
}

export interface LoopRunDetail {
  run: LoopRunSummary;
  sessions: LoopSessionItem[];
  events: LoopEventItem[];
  artifacts: LoopArtifactItem[];
  verifier_runs: VerifierRunItem[];
  recipe_snapshot?: RecipeSnapshot | null;
}

export interface RecipeSummary {
  id: string;
  name: string;
  description: string | null;
  source: string;
  inputs: Record<string, RecipeInputDef>;
}

export interface SelectOption {
  value: string;
  label: string;
}

export interface RecipeInputDef {
  required: boolean;
  type: string;
  label?: string | null;
  description?: string | null;
  default?: unknown;
  options?: SelectOption[];
}

export interface RecipeSnapshot {
  recipe_schema: string;
  recipe_id: string;
  recipe_name?: string | null;
  recipe_description?: string | null;
  recipe_source: string;
  recipe_path?: string | null;
  inputs: Record<string, unknown>;
  input_defs?: Record<string, RecipeInputDef>;
  runtime: RecipeRuntime;
  policy: RecipeSnapshotPolicy;
  roles: Record<string, RecipeRole>;
  steps: RecipeStepDef[];
  knowledge: RecipeKnowledge;
  tools: RecipeTools;
}

export interface RecipeRuntime {
  current_step: string;
  tick_count: number;
  round: number;
  created_session_ids?: Record<string, string[]>;
  last_error?: string | null;
}

export interface RecipeSnapshotPolicy {
  max_rounds: number;
  max_ticks: number;
  max_sessions: number;
  merge_policy: string;
  auto_approve: boolean;
}

export interface RecipeRole {
  provider: string;
  mode: string;
  isolation: string;
  instructions?: string | null;
}

export interface RecipeStepDef {
  id: string;
  kind: string;
  role?: string | null;
  prompt?: string | null;
  branch?: string | null;
  from?: string | null;
  on?: Record<string, string> | null;
  status?: string | null;
  next?: string | null;
  select?: string | null;
  gates?: RecipeGateDef[];
}

export interface RecipeGateDef {
  name: string;
  command: string;
}

export interface RecipeKnowledge {
  files: string[];
  instructions: string[];
}

export interface RecipeTools {
  required: string[];
  optional: string[];
}
