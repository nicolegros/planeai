export interface Session {
  id: string;
  project_id: string;
  name: string;
  tmux_name: string | null;
  branch: string;
  status: string;
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
  done: number;
  errors: number;
}
