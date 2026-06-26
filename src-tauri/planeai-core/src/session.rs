use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::task::{Task, TaskSource};
use crate::template;

/// Operations that interact with git, tmux, and DB.
/// Injected for testability.
pub trait Backend: Send + Sync {
    fn create_worktree(
        &self,
        repo: &str,
        path: &str,
        branch: &str,
        base: &str,
    ) -> Result<(), String>;
    fn create_tmux_session(
        &self,
        name: &str,
        cwd: &str,
        cmd: &str,
        session_id: &str,
    ) -> Result<(), String>;
    fn create_daemon_session(&self, session_id: &str, cmd: &str, cwd: &str) -> Result<(), String>;
    fn insert_session(&self, session: &NewSession) -> Result<(), String>;
    fn notify_gui(&self, session_id: &str) -> Result<(), String>;
    fn kill_session(&self, session: &NewSession) -> Result<(), String>;
    fn list_active_sessions(&self) -> Result<Vec<NewSession>, String>;
    /// Return task keys that already have a session (active or exited).
    /// Used to prevent re-dispatching tasks that were already worked on.
    fn list_claimed_task_keys(&self) -> Result<HashSet<String>, String>;
    fn fetch_base(&self, repo: &str, base: &str) -> Result<String, String>;
    /// Reload the dispatch config for a provider. Called before each dispatch to pick up config changes.
    fn reload_dispatch_config(&self, provider: &str) -> Option<DispatchConfig>;
}

/// Data needed to insert a session into the DB.
#[derive(Debug, Clone)]
pub struct NewSession {
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub name: String,
    pub tmux_name: Option<String>,
    pub branch: String,
    pub worktree_path: String,
    pub provider: String,
    pub backend: String,
    pub auto_approve: bool,
    pub task_key: String,
    pub base_branch: String,
    pub auto_dispatched: bool,
    pub command: String,
}

/// Configuration for how the orchestrator dispatches sessions.
#[derive(Debug, Clone)]
pub struct DispatchConfig {
    pub provider: String,
    pub provider_command: String,
    pub yolo: bool,
    pub yolo_flag: Option<String>,
    pub worktree_root: String,
    pub base_branch: String,
    pub session_backend: String,
    pub prompt_template: Option<String>,
    pub prompt_command: Option<String>,
    pub prompt_wrapper: Option<String>,
    pub name_template: Option<String>,
}

/// Lifecycle hook config for on_start.
#[derive(Debug, Clone)]
pub struct OnStartHook {
    pub move_to: String,
}

pub struct SessionDispatcher {
    pub task_source: Arc<dyn TaskSource>,
    pub on_start: Option<OnStartHook>,
    pub dispatch_config: DispatchConfig,
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
}

impl SessionDispatcher {
    pub fn dispatch(&self, task: &Task, backend: &dyn Backend) -> Result<NewSession, String> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let short_id = &session_id.replace('-', "")[..8];

        tracing::info!(task_key = %task.key, session_id = %session_id, project = %self.project_name, "dispatching session");

        let effective_parent_key = task
            .parent_key
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&task.key);

        // Build branch name: use parent_key if present, else task key
        let branch = format!(
            "{}/{}",
            effective_parent_key.to_lowercase().replace(' ', "-"),
            short_id
        );

        // Fetch base branch to ensure we have the latest remote ref
        let resolved_base = backend.fetch_base(&self.project_path, &task.base_branch)?;

        // Create worktree
        let wt_path = format!(
            "{}/{}/{}",
            self.dispatch_config.worktree_root, self.project_name, short_id
        );
        backend.create_worktree(&self.project_path, &wt_path, &branch, &resolved_base)?;

        // Build agent launch command via shared helper (autonomous=true for auto-dispatch)
        let rendered_prompt = if let Some(tpl) = &self.dispatch_config.prompt_template {
            let mut vars = HashMap::new();
            vars.insert("key", task.key.as_str());
            vars.insert("title", task.title.as_str());
            vars.insert("description", task.description.as_str());
            vars.insert("parent_key", effective_parent_key);
            Some(template::render(tpl, &vars))
        } else {
            None
        };

        let provider_config = crate::session_launch::ProviderConfig {
            command: self.dispatch_config.provider_command.clone(),
            yolo_flag: self.dispatch_config.yolo_flag.clone(),
            prompt_command: self.dispatch_config.prompt_command.clone(),
            autonomous_prompt_template: self.dispatch_config.prompt_wrapper.clone(),
        };
        let launch_result = crate::session_launch::build_provider_launch_command(
            &provider_config,
            self.dispatch_config.yolo,
            rendered_prompt.as_deref(),
            true, // autonomous: auto-dispatched sessions always use autonomous template
        );
        let cmd = launch_result.command;
        tracing::info!(
            command = %cmd,
            prompt_injected = launch_result.prompt_was_injected,
            approve_applied = launch_result.auto_approve_was_applied,
            prompt_template = ?self.dispatch_config.prompt_template,
            prompt_command = ?self.dispatch_config.prompt_command,
            rendered_prompt = ?rendered_prompt,
            "dispatch command built"
        );

        // Create tmux session
        let tmux_name = format!("planeai-{}-{}", self.project_name, short_id);
        if self.dispatch_config.session_backend == "tmux" {
            backend.create_tmux_session(&tmux_name, &wt_path, &cmd, &session_id)?;
        } else if self.dispatch_config.session_backend == "daemon" {
            backend.create_daemon_session(&session_id, &cmd, &wt_path)?;
        }

        let session_name = if let Some(tpl) = &self.dispatch_config.name_template {
            let mut vars = HashMap::new();
            vars.insert("key", task.key.as_str());
            vars.insert("title", task.title.as_str());
            vars.insert("description", task.description.as_str());
            vars.insert("status", task.status.as_str());
            vars.insert("parent_key", effective_parent_key);
            template::render(tpl, &vars)
        } else {
            format!("{}: {}", task.key, task.title)
        };

        let new_session = NewSession {
            id: session_id.clone(),
            project_id: self.project_id.clone(),
            project_name: self.project_name.clone(),
            name: session_name,
            tmux_name: Some(tmux_name),
            branch,
            worktree_path: wt_path,
            provider: self.dispatch_config.provider.clone(),
            backend: self.dispatch_config.session_backend.clone(),
            auto_approve: self.dispatch_config.yolo,
            task_key: task.key.clone(),
            base_branch: resolved_base,
            auto_dispatched: true,
            command: cmd,
        };

        backend.insert_session(&new_session)?;

        // Fire on_start hook (move task to in_progress)
        if let Some(hook) = &self.on_start {
            tracing::info!(task_key = %task.key, move_to = %hook.move_to, "firing on_start hook");
            let _ = self.task_source.move_task(&task.key, &hook.move_to);
        }

        // Notify GUI
        let _ = backend.notify_gui(&session_id);

        Ok(new_session)
    }
}
