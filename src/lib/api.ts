import { invoke } from "@tauri-apps/api/core";
import type { Channel } from "@tauri-apps/api/core";
import type { Session, Project, TaskItem, DirEntry, ChangedFile, FileDiff, CiCheck } from "./types";
import type { AppConfig } from "./settings.svelte";

export interface LaunchSessionParams {
  projectId: string;
  projectName: string;
  repoPath: string;
  branch: string;
  isNewBranch: boolean;
  name: string;
  useWorktree: boolean;
  baseBranch: string | null;
  autoApprove: boolean;
  provider: string;
  taskKey: string | null;
  taskPrompt: string | null;
}

export const sessions = {
  list: () => invoke<Session[]>("list_sessions"),
  listArchived: () => invoke<Session[]>("list_archived_sessions"),
  launch: (params: LaunchSessionParams) =>
    invoke<Session>("launch_session", params as unknown as Record<string, unknown>),
  destroy: (id: string) => invoke("destroy_session", { id }),
  archive: (id: string) => invoke("archive_session", { id }),
  restore: (id: string) => invoke("restore_session", { id }),
  rename: (id: string, name: string) => invoke("rename_session", { id, name }),
  restart: (sessionId: string) => invoke<Session>("restart_session", { sessionId }),
  markExited: (sessionId: string) => invoke("mark_exited", { sessionId }),
  acknowledge: (sessionId: string) => invoke("acknowledge_session", { sessionId }),
  saveMruOrder: (sessionIds: string[]) => invoke("save_mru_order", { sessionIds }),
};

export const projects = {
  list: () => invoke<Project[]>("list_projects"),
  listArchived: () => invoke<Project[]>("list_archived_projects"),
  create: (name: string, path: string) => invoke("create_project", { name, path }),
  delete: (id: string) => invoke("delete_project", { id }),
  archive: (id: string) => invoke("archive_project", { id }),
  restore: (id: string) => invoke("restore_project", { id }),
  setAutoMode: (id: string, enabled: boolean) => invoke("set_project_auto_mode", { id, enabled }),
  getAutoMode: (id: string) => invoke<boolean>("get_project_auto_mode", { id }),
  validateGitRepo: (path: string) => invoke<boolean>("validate_git_repo", { path }),
  listBranches: (repoPath: string) => invoke<string[]>("list_branches", { repoPath }),
};

export const pty = {
  write: (sessionId: string, data: number[]) => invoke("write_to_pty", { sessionId, data }),
  attach: (
    sessionId: string,
    darkMode: boolean,
    onData: Channel<ArrayBuffer>,
    useResume?: boolean,
  ) => invoke("attach_session", { sessionId, darkMode, useResume: useResume ?? null, onData }),
  spawnTab: (
    sessionId: string,
    tabIndex: number,
    darkMode: boolean,
    onData: Channel<ArrayBuffer>,
  ) => invoke("spawn_tab", { sessionId, tabIndex, darkMode, onData }),
  resize: (sessionId: string, rows: number, cols: number) =>
    invoke("resize_pty", { sessionId, rows, cols }),
  pause: (sessionId: string) => invoke("pause_pty", { sessionId }),
  resume: (sessionId: string) => invoke("resume_pty", { sessionId }),
  closeTab: (sessionId: string, tabIndex: number) => invoke("close_tab", { sessionId, tabIndex }),
  incrementTabCount: (sessionId: string) => invoke("increment_tab_count", { sessionId }),
};

export const config = {
  get: () => invoke<AppConfig>("get_config"),
  update: (newConfig: AppConfig) => invoke("update_config", { newConfig }),
};

export const tasks = {
  list: (repoPath: string) => invoke<TaskItem[]>("list_task_items", { repoPath }),
  listAll: (repoPath: string) => invoke<TaskItem[]>("list_all_task_items", { repoPath }),
  create: (params: {
    repoPath: string;
    title: string;
    description: string;
    priority: number;
    tags: string[];
    blockedBy: string[];
    baseBranch?: string;
  }) => invoke("create_task_item", params),
  edit: (params: {
    repoPath: string;
    key: string;
    title: string;
    description: string;
    priority: number;
    tags: string[] | null;
    blockedBy: string[] | null;
    parentKey?: string | null;
    baseBranch?: string | null;
  }) => invoke("edit_task_item", params),
  move: (key: string, status: string, repoPath: string) =>
    invoke("move_task_item", { key, status, repoPath }),
  fireNotifyHook: (sessionId: string) => invoke("fire_task_notify_hook", { sessionId }),
};

export const fileExplorer = {
  listDir: (path: string) => invoke<DirEntry[]>("fe_list_directory", { path }),
  rename: (oldPath: string, newPath: string) => invoke("fe_rename_entry", { oldPath, newPath }),
  deleteToTrash: (path: string) => invoke("fe_delete_to_trash", { path }),
  createDir: (path: string) => invoke("fe_create_directory", { path }),
  createFile: (path: string) => invoke("fe_create_file", { path }),
  watch: (sessionId: string, path: string) => invoke("fe_watch_directory", { sessionId, path }),
  unwatch: (sessionId: string) => invoke("fe_unwatch_directory", { sessionId }),
};

export const git = {
  getChangedFiles: (repoPath: string, baseBranch: string) =>
    invoke<ChangedFile[]>("get_changed_files", { repoPath, baseBranch }),
  getFileDiff: (repoPath: string, baseBranch: string, filePath: string, oldPath: string | null) =>
    invoke<FileDiff>("get_file_diff", { repoPath, baseBranch, filePath, oldPath }),
  getFilePatch: (repoPath: string, baseBranch: string, filePath: string, oldPath: string | null) =>
    invoke<string>("get_file_patch", { repoPath, baseBranch, filePath, oldPath }),
  getAllFilePatches: (repoPath: string, baseBranch: string, files: [string, string | null][]) =>
    invoke<string[]>("get_all_file_patches", { repoPath, baseBranch, files }),
  listFiles: (repoPath: string) => invoke<string[]>("list_files", { repoPath }),
  readFile: (filePath: string) => invoke<string>("read_file", { filePath }),
  writeFile: (filePath: string, content: string) => invoke("write_file", { filePath, content }),
  fetchPrUrl: (sessionId: string) => invoke<string | null>("fetch_pr_url", { sessionId }),
};

export const pr = {
  create: (sessionId: string, title: string, body: string, baseBranch: string, draft: boolean) =>
    invoke<string>("create_pr", { sessionId, title, body, baseBranch, draft }),
  generateDefaults: (sessionId: string) =>
    invoke<{ title: string; body: string; base_branch: string }>("generate_pr_defaults", {
      sessionId,
    }),
  getCiChecks: (sessionId: string) => invoke<CiCheck[]>("get_ci_checks", { sessionId }),
  getAllowedStrategies: (sessionId: string) =>
    invoke<string[]>("get_allowed_merge_strategies", { sessionId }),
  merge: (sessionId: string, strategy: string) => invoke("merge_pr", { sessionId, strategy }),
  markReady: (sessionId: string) => invoke("mark_pr_ready", { sessionId }),
  getMergeState: (sessionId: string) =>
    invoke<{ blocked: boolean; reasons: string[]; settingsUrl: string | null }>("get_merge_state", {
      sessionId,
    }),
  getCiFailureLogs: (sessionId: string) => invoke<string>("get_ci_failure_logs", { sessionId }),
};

export const notify = {
  isInstalled: () => invoke<boolean>("is_notify_hook_installed"),
  install: () => invoke("install_notify_hook"),
};

export const symphony = {
  getStatus: () => invoke<string>("get_symphony_status"),
};

export const preferences = {
  listMonospaceFonts: () => invoke<string[]>("list_monospace_fonts"),
  listThemes: () => invoke<string[]>("list_themes"),
  checkTmuxAvailable: () => invoke<boolean>("check_tmux_available"),
  checkCliInstalled: () => invoke<boolean>("check_cli_installed"),
  installCli: () => invoke("install_cli"),
  getLogDir: () => invoke<string>("get_log_dir"),
  getThemeCss: () => invoke<string>("get_theme_css"),
  listStaleWorktrees: () =>
    invoke<{ session_name: string; worktree_path: string; branch: string }[]>(
      "list_stale_worktrees",
    ),
  runStaleWorktreeCleanup: () => invoke<string[]>("run_stale_worktree_cleanup"),
};

export interface SessionLogEntry {
  session_id: string;
  started_at: string;
  ended_at: string | null;
  status: string;
  pty_core: string;
  ansi_log_path: string;
  meta_path: string;
  bytes_written: number;
  bytes_dropped: number;
  command: string;
  cwd: string;
}

export const sessionLogs = {
  isEnabled: () => invoke<boolean>("is_dogfood_log_viewer_enabled"),
  getDir: () => invoke<string>("get_session_log_dir"),
  list: () => invoke<SessionLogEntry[]>("list_session_logs"),
  getMetadata: (sessionId: string) =>
    invoke<SessionLogEntry>("get_session_log_metadata", { sessionId }),
  readChunk: (path: string, offset: number, length: number) =>
    invoke<number[]>("read_session_log_chunk", { path, offset, length }),
  openFolder: (path: string) => invoke("open_session_log_folder", { path }),
  delete: (sessionId: string) => invoke("delete_session_log", { sessionId }),
};
