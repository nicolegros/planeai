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

export interface Project {
  id: string;
  name: string;
  path: string;
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
  current_round: number;
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
}

export interface RecipeSummary {
  id: string;
  name: string;
  description: string | null;
  source: string;
}
