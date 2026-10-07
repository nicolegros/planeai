//! Starting a session for a task, the one owner of what "Start session immediately" means:
//! the desktop task form and plugin-created routine tasks both go through here.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use planeai_tasks::model::{Status, Task};
use planeai_tasks::provider::TaskProvider;
use planeai_tasks::sqlite::SqliteRepository;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::sessions::launch::{launch, LaunchRequest, LaunchResult};
use crate::config::{Config, TaskManagerTemplates};
use crate::plugins::{PluginHostCapability, PluginInventory, PluginRuntimeState};
use planeai_plugin_contract::ProviderFeature;

/// A provider a new session can run on, as the session and task forms offer it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderOption {
    pub key: String,
    pub label: String,
    /// Whether the session honors auto-approve; plugin providers declare it.
    pub auto_approve: bool,
}

/// The providers a session may start on and the configured default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderCatalog {
    pub default: String,
    pub providers: Vec<ProviderOption>,
}

impl ProviderCatalog {
    /// Configured providers, then those of running plugins holding the providers capability.
    pub fn new(cfg: &Config, inventory: &[PluginInventory]) -> Self {
        let mut configured = cfg
            .providers
            .keys()
            // Saving the config refuses these anyway; a plugin's provider is listed below.
            .filter(|key| !planeai_plugin_contract::provider::is_reserved_provider_key(key))
            .map(|key| ProviderOption {
                key: key.clone(),
                label: key.clone(),
                auto_approve: true,
            })
            .collect::<Vec<_>>();
        configured.sort_by(|a, b| a.key.cmp(&b.key));
        let plugins = inventory
            .iter()
            .filter(|plugin| {
                plugin.state == PluginRuntimeState::Running
                    && plugin
                        .capabilities
                        .contains(&PluginHostCapability::Providers)
            })
            .flat_map(|plugin| {
                plugin.providers.iter().map(|provider| ProviderOption {
                    key: format!("{}:{}", plugin.id, provider.id),
                    label: provider.label.clone(),
                    auto_approve: provider.supports(ProviderFeature::AutoApprove),
                })
            });
        Self {
            default: cfg.default_provider.clone(),
            providers: configured.into_iter().chain(plugins).collect(),
        }
    }

    /// Auto-approve as the session gets it: the request, when the provider supports it.
    fn auto_approve(&self, provider: &str, requested: bool) -> bool {
        requested
            && self
                .providers
                .iter()
                .find(|option| option.key == provider)
                .is_none_or(|option| option.auto_approve)
    }
}

/// What the caller chose; everything else comes from the task and the configured templates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartOptions {
    /// `None` starts on the configured default provider.
    pub provider: Option<String>,
    pub use_worktree: bool,
    pub auto_approve: bool,
    /// Overrides the branch template; empty means unset.
    pub branch: Option<String>,
    /// Overrides the prompt template; empty means unset.
    pub prompt: Option<String>,
}

impl Default for StartOptions {
    fn default() -> Self {
        Self {
            provider: None,
            use_worktree: true,
            auto_approve: true,
            branch: None,
            prompt: None,
        }
    }
}

/// Everything `launch` needs for the task's session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSessionPlan {
    pub provider: String,
    pub branch: String,
    pub is_new_branch: bool,
    /// The task's base branch when the branch is created, else `None`.
    pub base_branch: Option<String>,
    pub name: String,
    pub prompt: String,
    pub use_worktree: bool,
    pub auto_approve: bool,
}

pub const NO_PROVIDER: &str = "No provider is configured. Select a provider to start a session.";

/// Apply the session templates, or their defaults, to the task. `existing_branches` is
/// `list_branches` output, remote branches prefixed with `remote:`.
pub fn plan_task_session(
    templates: Option<&TaskManagerTemplates>,
    catalog: &ProviderCatalog,
    task: &Task,
    existing_branches: &[String],
    options: &StartOptions,
) -> Result<TaskSessionPlan, String> {
    let provider = options
        .provider
        .clone()
        .filter(|key| !key.is_empty())
        .unwrap_or_else(|| catalog.default.clone());
    if provider.is_empty() {
        return Err(NO_PROVIDER.to_string());
    }
    let vars = template_vars(task);
    let vars = vars
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect::<HashMap<_, _>>();
    // An empty template counts as unset, as the forms have always read it.
    let render = |template: Option<&String>| {
        template
            .filter(|template| !template.is_empty())
            .map(|template| planeai_core::template::render(template, &vars))
    };
    let set = |value: &Option<String>| value.clone().filter(|value| !value.is_empty());

    let branch = set(&options.branch)
        .or_else(|| render(templates.and_then(|t| t.branch.as_ref())))
        .unwrap_or_else(|| format!("{}/{}", task.key.to_lowercase(), title_slug(&task.title)));
    let is_new_branch = !existing_branches
        .iter()
        .any(|existing| existing.strip_prefix("remote:").unwrap_or(existing) == branch);
    let prompt = set(&options.prompt)
        .or_else(|| render(templates.and_then(|t| t.prompt.as_ref())))
        .unwrap_or_else(|| {
            let first_line = format!("Implement task {}: {}", task.key, task.title);
            if task.description.is_empty() {
                first_line
            } else {
                format!("{first_line}\n\n{}", task.description)
            }
        });
    let name = render(templates.and_then(|t| t.name.as_ref()))
        .unwrap_or_else(|| format!("{}: {}", task.key, task.title));
    Ok(TaskSessionPlan {
        auto_approve: catalog.auto_approve(&provider, options.auto_approve),
        provider,
        base_branch: is_new_branch.then(|| task.base_branch.clone()),
        branch,
        is_new_branch,
        name,
        prompt,
        use_worktree: options.use_worktree,
    })
}

/// Start the task's session the way the task form always has: launch it, move the task to
/// `in_progress` whatever `on_start` says, and refresh the windows' session and task lists.
pub async fn start(
    app: &AppHandle,
    project_id: &str,
    task_key: &str,
    options: StartOptions,
) -> Result<LaunchResult, String> {
    let (project, task, branches) = crate::commands::blocking({
        let db = app.state::<crate::state::DbState>().0.clone();
        let project_id = project_id.to_string();
        let task_key = task_key.to_string();
        move || {
            let project = {
                let conn = db.lock().map_err(|e| e.to_string())?;
                crate::db::get_project(&conn, &project_id)
                    .map_err(|e| e.to_string())?
                    .ok_or("Project not found or archived.")?
            };
            let task = task_repository(&project.prefix)?
                .get(&task_key)
                .map_err(|e| format!("task {task_key}: {e}"))?;
            // Unknown branches only make the branch read as new, as in the task form.
            let branches = crate::git::list_branches(&project.path).unwrap_or_default();
            Ok((project, task, branches))
        }
    })
    .await?;
    let catalog = provider_catalog(app).await?;
    let templates = {
        let cfg = app
            .state::<crate::state::ConfigState>()
            .0
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        cfg.task_management.and_then(|tm| tm.templates)
    };
    let plan = plan_task_session(templates.as_ref(), &catalog, &task, &branches, &options)?;
    let mut result = launch(
        app,
        LaunchRequest {
            project_id: project.id.clone(),
            branch: plan.branch,
            is_new_branch: plan.is_new_branch,
            name: plan.name,
            use_worktree: plan.use_worktree,
            base_branch: plan.base_branch,
            auto_approve: plan.auto_approve,
            provider: Some(plan.provider),
            task_key: Some(task.key.clone()),
            task_project_id: None,
            task_prompt: Some(plan.prompt),
        },
    )
    .await?;
    let moved = crate::commands::blocking(move || move_in_progress(&project, &task.key)).await;
    if let Err(error) = moved {
        tracing::warn!(task_key, %error, "started a task session but could not move its task");
        let failed = "Session started but failed to update task status.";
        result.warning = Some(match result.warning {
            Some(warning) => format!("{warning} {failed}"),
            None => failed.to_string(),
        });
    }
    for event in ["sessions-changed", "tasks-changed"] {
        if let Err(error) = app.emit(event, ()) {
            tracing::warn!(event, %error, "failed to announce a started task session");
        }
    }
    Ok(result)
}

/// A created task whose session a plugin asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingTaskStart {
    /// The asking plugin's name, for the messages the user sees.
    pub plugin: String,
    pub project_id: String,
    pub task_key: String,
    pub options: StartOptions,
}

/// Where a plugin task's requested session stands. Stored with the plugin's task operation,
/// so it outlives an app restart and each task's session is started at most once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum RequestedStart {
    Pending(PendingTaskStart),
    Started,
    Failed { error: String },
}

/// Where a plugin's session start stands when the plugin is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStart {
    Starting,
    Exists,
    Failed,
}

/// Task keys whose session is starting in the background.
#[derive(Debug, Clone, Default)]
pub struct StartClaims(Arc<Mutex<HashSet<String>>>);

/// A task whose session is starting; dropping it lets the task be started again.
#[derive(Debug)]
pub struct StartClaim {
    claims: StartClaims,
    task_key: String,
}

impl Drop for StartClaim {
    fn drop(&mut self) {
        self.claims
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.task_key);
    }
}

#[derive(Debug)]
pub enum Admission {
    Exists,
    Starting,
    Start(StartClaim),
}

impl StartClaims {
    /// Claim the task's start unless one is in flight or the task already has a session.
    pub fn admit(
        &self,
        task_key: &str,
        has_session: impl FnOnce() -> Result<bool, String>,
    ) -> Result<Admission, String> {
        let claimed = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(task_key.to_string());
        if !claimed {
            return Ok(Admission::Starting);
        }
        let claim = StartClaim {
            claims: self.clone(),
            task_key: task_key.to_string(),
        };
        Ok(if has_session()? {
            Admission::Exists
        } else {
            Admission::Start(claim)
        })
    }
}

static IN_FLIGHT: LazyLock<StartClaims> = LazyLock::new(StartClaims::default);

/// Start the task's session unless it has one or one is starting, without waiting for it, so
/// a retried request converges on one session. The outcome is recorded with the plugin's task
/// operation and a failure or warning reaches the user as `app-error`.
pub async fn start_in_background(
    app: &AppHandle,
    pending: PendingTaskStart,
) -> Result<SessionStart, String> {
    let db = app.state::<crate::state::DbState>().0.clone();
    let task_key = pending.task_key.clone();
    let admission = crate::commands::blocking(move || {
        IN_FLIGHT.admit(&task_key, || {
            let conn = db.lock().map_err(|e| e.to_string())?;
            let exists = has_session(&conn, &task_key)?;
            if exists {
                record_start(&conn, &task_key, &Ok(()))?;
            }
            Ok(exists)
        })
    })
    .await?;
    let claim = match admission {
        Admission::Exists => return Ok(SessionStart::Exists),
        Admission::Starting => return Ok(SessionStart::Starting),
        Admission::Start(claim) => claim,
    };
    spawn_start(app.clone(), pending, claim);
    Ok(SessionStart::Starting)
}

/// Start every session a plugin asked for that has not started or failed yet, as after the
/// app quit between creating a task and creating its session.
pub async fn resume_pending_starts(app: &AppHandle) {
    let db = app.state::<crate::state::DbState>().0.clone();
    let pending = crate::commands::blocking(move || {
        let conn = db.lock().map_err(|e| e.to_string())?;
        pending_starts(&conn)
    })
    .await;
    match pending {
        Ok(pending) => {
            for start in pending {
                let task_key = start.task_key.clone();
                if let Err(error) = start_in_background(app, start).await {
                    tracing::warn!(task_key, %error, "could not resume a task session start");
                }
            }
        }
        Err(error) => tracing::warn!(%error, "could not read pending task session starts"),
    }
}

// Not async: `start` reaches plugin RPC, which serves the callbacks that call
// `start_in_background`, so its future type must not contain this one.
fn spawn_start(app: AppHandle, pending: PendingTaskStart, claim: StartClaim) {
    tauri::async_runtime::spawn(async move {
        let outcome = start(
            &app,
            &pending.project_id,
            &pending.task_key,
            pending.options.clone(),
        )
        .await;
        let db = app.state::<crate::state::DbState>().0.clone();
        let message = crate::commands::blocking(move || {
            let conn = db.lock().map_err(|e| e.to_string())?;
            finish_start(&conn, &pending, outcome.map(|result| result.warning))
        })
        .await;
        drop(claim);
        match message {
            Ok(Some(message)) => {
                if let Err(error) = app.emit("app-error", message) {
                    tracing::warn!(%error, "failed to report a task session start");
                }
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "failed to record a task session start"),
        }
    });
}

/// Record how a background start ended and say what the user should hear about it: a
/// failure, or a session that started with a warning.
pub(crate) fn finish_start(
    conn: &rusqlite::Connection,
    pending: &PendingTaskStart,
    outcome: Result<Option<String>, String>,
) -> Result<Option<String>, String> {
    let PendingTaskStart {
        plugin, task_key, ..
    } = pending;
    record_start(
        conn,
        task_key,
        &outcome.as_ref().map(|_| ()).map_err(String::as_str),
    )?;
    Ok(match outcome {
        Ok(None) => None,
        Ok(Some(warning)) => Some(format!(
            "{plugin} started a session for {task_key}: {warning}"
        )),
        Err(error) => {
            tracing::warn!(task_key, %error, "background task session start failed");
            Some(format!(
                "{plugin} created {task_key}, but its session could not start: {error}"
            ))
        }
    })
}

/// Settle a requested start; one already settled stays as it is.
fn record_start(
    conn: &rusqlite::Connection,
    task_key: &str,
    outcome: &Result<(), &str>,
) -> Result<(), String> {
    let (state, error) = match outcome {
        Ok(()) => ("started", None),
        Err(error) => ("failed", Some(*error)),
    };
    conn.execute(
        "UPDATE plugin_task_operations SET start_state = ?2, start_error = ?3 WHERE task_key = ?1 AND start_state = 'pending'",
        rusqlite::params![task_key, state, error],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// The requested starts still pending, in the order their tasks were created.
pub(crate) fn pending_starts(conn: &rusqlite::Connection) -> Result<Vec<PendingTaskStart>, String> {
    let mut statement = conn
        .prepare(
            "SELECT COALESCE(i.name, o.plugin_id), p.id, o.task_key, o.start_options
             FROM plugin_task_operations o
             JOIN tasks t ON t.key = o.task_key
             JOIN projects p ON p.prefix = t.project_prefix
             LEFT JOIN plugin_inventory i ON i.id = o.plugin_id
             WHERE o.start_state = 'pending'
             ORDER BY t.created_at",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    rows.map(|row| {
        let (plugin, project_id, task_key, options) = row.map_err(|e| e.to_string())?;
        Ok(PendingTaskStart {
            plugin,
            project_id,
            task_key,
            options: serde_json::from_str(&options).map_err(|e| e.to_string())?,
        })
    })
    .collect()
}

/// Whether any session, whatever its status, was ever started for the task.
fn has_session(conn: &rusqlite::Connection, task_key: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE task_key = ?1)",
        [task_key],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

/// The providers `start` accepts, as the session and task forms list them.
pub async fn provider_catalog(app: &AppHandle) -> Result<ProviderCatalog, String> {
    let inventory = app
        .state::<crate::plugins::PluginRuntimeHandle>()
        .0
        .list()
        .await?;
    let config = app.state::<crate::state::ConfigState>();
    let cfg = config.0.lock().map_err(|e| e.to_string())?;
    Ok(ProviderCatalog::new(&cfg, &inventory))
}

fn task_repository(prefix: &str) -> Result<SqliteRepository, String> {
    let path = planeai_paths::db_path();
    SqliteRepository::open(
        path.to_str().ok_or("invalid PlaneAI task database path")?,
        prefix,
    )
    .map_err(|e| e.to_string())
}

fn move_in_progress(
    project: &planeai_core::services::Project,
    task_key: &str,
) -> Result<(), String> {
    let (_task, events) = planeai_core::task_lifecycle::move_task_with_lifecycle(
        &task_repository(&project.prefix)?,
        task_key,
        Status::InProgress,
    )
    .map_err(|e| e.to_string())?;
    if !events.is_empty() {
        planeai::task_cli::notify_task_lifecycle(&crate::task_lifecycle::TaskLifecycleBatch::new(
            crate::task_lifecycle::TaskLifecycleOrigin::Ui,
            project.id.clone(),
            project.prefix.clone(),
            events,
        ));
    }
    Ok(())
}

/// The task as `renderTemplate` sees it: an empty parent renders as the task's own key.
fn template_vars(task: &Task) -> [(&'static str, String); 9] {
    let parent_key = task
        .parent_key
        .clone()
        .filter(|key| !key.is_empty())
        .unwrap_or_else(|| task.key.clone());
    [
        ("key", task.key.clone()),
        ("title", task.title.clone()),
        ("description", task.description.clone()),
        ("priority", task.priority.to_string()),
        ("blocked_by", task.blocked_by.join(", ")),
        // A JavaScript array renders comma-joined.
        ("tags", task.tags.join(",")),
        ("parent_key", parent_key),
        ("base_branch", task.base_branch.clone()),
        ("status", task.status.as_str().to_string()),
    ]
}

/// The task form's branch slug, laxer than the template `slug` transform.
fn title_slug(title: &str) -> String {
    let mut slug = String::new();
    let mut in_whitespace = false;
    for c in title.to_lowercase().chars() {
        if c.is_whitespace() {
            if !in_whitespace {
                slug.push('-');
            }
            in_whitespace = true;
            continue;
        }
        in_whitespace = false;
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '/' {
            slug.push(c);
        }
    }
    slug.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::{PluginProvider, PluginSourceKind};

    fn task(description: &str) -> Task {
        Task {
            key: "PLA-12".into(),
            title: "Fix Login Redirect".into(),
            description: description.into(),
            status: Status::Todo,
            priority: 2,
            parent_key: None,
            blocked_by: vec!["PLA-1".into(), "PLA-2".into()],
            tags: vec!["web".into(), "auth".into()],
            base_branch: "develop".into(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn catalog() -> ProviderCatalog {
        ProviderCatalog {
            default: "claude".into(),
            providers: vec![
                ProviderOption {
                    key: "claude".into(),
                    label: "claude".into(),
                    auto_approve: true,
                },
                ProviderOption {
                    key: "chat:claude".into(),
                    label: "Claude Chat".into(),
                    auto_approve: false,
                },
            ],
        }
    }

    fn templates(branch: &str, name: &str, prompt: &str) -> TaskManagerTemplates {
        TaskManagerTemplates {
            branch: Some(branch.into()),
            name: Some(name.into()),
            prompt: Some(prompt.into()),
        }
    }

    fn plan(
        templates: Option<&TaskManagerTemplates>,
        task: &Task,
        branches: &[&str],
        options: StartOptions,
    ) -> TaskSessionPlan {
        let branches = branches.iter().map(|b| b.to_string()).collect::<Vec<_>>();
        plan_task_session(templates, &catalog(), task, &branches, &options).unwrap()
    }

    #[test]
    fn without_templates_the_task_form_defaults_apply() {
        assert_eq!(
            plan(
                None,
                &task("Users land on /home."),
                &["main"],
                StartOptions::default()
            ),
            TaskSessionPlan {
                provider: "claude".into(),
                branch: "pla-12/fix-login-redirect".into(),
                is_new_branch: true,
                base_branch: Some("develop".into()),
                name: "PLA-12: Fix Login Redirect".into(),
                prompt: "Implement task PLA-12: Fix Login Redirect\n\nUsers land on /home.".into(),
                use_worktree: true,
                auto_approve: true,
            }
        );
    }

    /// The cases `taskSessionDefaults` runs too, so the forms preview what the planner starts.
    #[test]
    fn the_shared_cases_plan_as_the_forms_preview_them() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("task_start_cases.json")).unwrap();
        for case in cases {
            let name = case["case"].as_str().unwrap();
            let fields = &case["task"];
            let text = |field: &str| fields[field].as_str().unwrap_or_default().to_string();
            let list = |field: &str| serde_json::from_value(fields[field].clone()).unwrap();
            let task = Task {
                key: text("key"),
                title: text("title"),
                description: text("description"),
                status: Status::parse(&text("status")).unwrap(),
                priority: fields["priority"].as_i64().unwrap() as i32,
                parent_key: fields["parent_key"].as_str().map(str::to_string),
                blocked_by: list("blocked_by"),
                tags: list("tags"),
                base_branch: text("base_branch"),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };
            let templates: Option<TaskManagerTemplates> =
                serde_json::from_value(case["templates"].clone()).unwrap();
            let plan = plan(templates.as_ref(), &task, &[], StartOptions::default());
            assert_eq!(
                serde_json::json!({
                    "branch": plan.branch,
                    "name": plan.name,
                    "prompt": plan.prompt,
                }),
                case["expected"],
                "{name}"
            );
        }
    }

    #[test]
    fn an_empty_description_leaves_the_prompt_at_its_first_line() {
        let plan = plan(None, &task(""), &[], StartOptions::default());
        assert_eq!(plan.prompt, "Implement task PLA-12: Fix Login Redirect");
    }

    #[test]
    fn a_parent_key_renders_when_present() {
        let mut child = task("");
        child.parent_key = Some("PLA-7".into());
        let templates = templates("{parent_key:lower}/{key:lower}", "{title}", "{description}");
        assert_eq!(
            plan(Some(&templates), &child, &[], StartOptions::default()).branch,
            "pla-7/pla-12"
        );
    }

    #[test]
    fn an_existing_branch_is_checked_out_without_a_base() {
        for existing in [
            "pla-12/fix-login-redirect",
            "remote:pla-12/fix-login-redirect",
        ] {
            let plan = plan(
                None,
                &task(""),
                &["main", existing],
                StartOptions::default(),
            );
            assert!(!plan.is_new_branch, "{existing}");
            assert_eq!(plan.base_branch, None);
        }
    }

    #[test]
    fn overrides_replace_branch_and_prompt_unless_empty() {
        let options = StartOptions {
            branch: Some("hotfix".into()),
            prompt: Some("Just do it".into()),
            use_worktree: false,
            ..StartOptions::default()
        };
        let overridden = plan(None, &task(""), &["hotfix"], options);
        assert_eq!(
            (overridden.branch.as_str(), overridden.prompt.as_str()),
            ("hotfix", "Just do it")
        );
        assert!(!overridden.is_new_branch);
        assert!(!overridden.use_worktree);

        let empty = StartOptions {
            branch: Some(String::new()),
            prompt: Some(String::new()),
            ..StartOptions::default()
        };
        let defaults = plan(None, &task(""), &[], empty);
        assert_eq!(defaults.branch, "pla-12/fix-login-redirect");
        assert_eq!(defaults.prompt, "Implement task PLA-12: Fix Login Redirect");
    }

    #[test]
    fn a_plugin_provider_without_auto_approve_forces_it_off() {
        let chosen = |provider: &str, auto_approve| {
            let options = StartOptions {
                provider: Some(provider.into()),
                auto_approve,
                ..StartOptions::default()
            };
            let plan = plan(None, &task(""), &[], options);
            (plan.provider, plan.auto_approve)
        };
        assert_eq!(chosen("chat:claude", true), ("chat:claude".into(), false));
        assert_eq!(chosen("claude", true), ("claude".into(), true));
        assert_eq!(chosen("claude", false), ("claude".into(), false));
    }

    #[test]
    fn no_provider_at_all_is_refused() {
        let catalog = ProviderCatalog {
            default: String::new(),
            providers: Vec::new(),
        };
        let error = plan_task_session(None, &catalog, &task(""), &[], &StartOptions::default());
        assert_eq!(error, Err(NO_PROVIDER.to_string()));
    }

    #[test]
    fn a_task_starts_once_while_its_start_is_in_flight() {
        let claims = StartClaims::default();
        let Admission::Start(claim) = claims.admit("PLA-1", || Ok(false)).unwrap() else {
            panic!("a task without a session starts");
        };
        let retried = claims.admit("PLA-1", || {
            panic!("an in-flight start is not checked again")
        });
        assert!(matches!(retried, Ok(Admission::Starting)));
        assert!(matches!(
            claims.admit("PLA-2", || Ok(false)),
            Ok(Admission::Start(_))
        ));

        drop(claim);
        assert!(matches!(
            claims.admit("PLA-1", || Ok(true)),
            Ok(Admission::Exists)
        ));
        assert!(
            matches!(claims.admit("PLA-1", || Ok(false)), Ok(Admission::Start(_))),
            "finding a session releases the claim"
        );
        assert!(
            claims.admit("PLA-3", || Err("db down".into())).is_err()
                && matches!(claims.admit("PLA-3", || Ok(false)), Ok(Admission::Start(_))),
            "a failed check releases the claim"
        );
    }

    #[test]
    fn any_session_row_of_the_task_counts_as_its_session() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        let project = crate::db::create_project(&conn, "Demo", "/tmp/demo").unwrap();
        crate::db::create_session_with_id(
            &conn,
            "s1",
            &project.id,
            "PLA-1: Retro",
            None,
            "pla-1/retro",
            None,
            None,
            "local",
            true,
            Some("PLA-1"),
            None,
            None,
        )
        .unwrap();
        conn.execute("UPDATE sessions SET status = 'archived'", [])
            .unwrap();

        assert!(has_session(&conn, "PLA-1").unwrap());
        assert!(!has_session(&conn, "PLA-2").unwrap());
    }

    fn plugin(id: &str, state: PluginRuntimeState, providers: bool) -> PluginInventory {
        PluginInventory {
            id: id.into(),
            name: id.into(),
            version: "1.0.0".into(),
            host_api_version: "planeai.plugin-host.v3".into(),
            source_kind: PluginSourceKind::Local,
            backend_entrypoint: String::new(),
            ui_contributions: Vec::new(),
            capabilities: if providers {
                vec![PluginHostCapability::Providers]
            } else {
                Vec::new()
            },
            background_service: None,
            providers: vec![
                PluginProvider {
                    id: "claude".into(),
                    label: "Claude Chat".into(),
                    entrypoint: "ui/chat.js".into(),
                    supports: Vec::new(),
                },
                PluginProvider {
                    id: "echo".into(),
                    label: "Echo".into(),
                    entrypoint: "ui/echo.js".into(),
                    supports: vec![ProviderFeature::AutoApprove],
                },
            ],
            installed_hash: None,
            installed_path: None,
            original_display_path: None,
            enabled: true,
            state,
            last_error: None,
            log_path: None,
        }
    }

    #[test]
    fn the_catalog_lists_configured_then_running_plugin_providers() {
        let cfg = Config {
            default_provider: "kiro".into(),
            providers: ["kiro", "claude", "chat:reserved"]
                .into_iter()
                .map(|key| (key.to_string(), Default::default()))
                .collect(),
            ..Default::default()
        };
        let catalog = ProviderCatalog::new(
            &cfg,
            &[
                plugin("chat", PluginRuntimeState::Running, true),
                plugin("stopped", PluginRuntimeState::Disabled, true),
                plugin("plain", PluginRuntimeState::Running, false),
            ],
        );
        let option = |key: &str, label: &str, auto_approve| ProviderOption {
            key: key.into(),
            label: label.into(),
            auto_approve,
        };
        assert_eq!(
            catalog,
            ProviderCatalog {
                default: "kiro".into(),
                providers: vec![
                    option("claude", "claude", true),
                    option("kiro", "kiro", true),
                    option("chat:claude", "Claude Chat", false),
                    option("chat:echo", "Echo", true),
                ],
            }
        );
    }
}
