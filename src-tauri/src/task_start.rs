//! Starting a session for a task, the one owner of what "Start session immediately" means:
//! the desktop task form and plugin-created routine tasks both go through here.

use std::collections::HashMap;

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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
    let render = |template: Option<&String>| {
        template.map(|template| planeai_core::template::render(template, &vars))
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

    #[test]
    fn an_empty_description_leaves_the_prompt_at_its_first_line() {
        let plan = plan(None, &task(""), &[], StartOptions::default());
        assert_eq!(plan.prompt, "Implement task PLA-12: Fix Login Redirect");
    }

    #[test]
    fn the_default_branch_keeps_only_lowercase_ascii_digits_hyphens_and_slashes() {
        let mut odd = task("");
        odd.title = "  Été: v2/API_call --  ".into();
        assert_eq!(
            plan(None, &odd, &[], StartOptions::default()).branch,
            "pla-12/-t-v2/apicall"
        );
    }

    #[test]
    fn templates_render_with_every_task_field() {
        let templates = templates(
            "{key:lower}/{title:slug}",
            "{key:upper} {status} p{priority}",
            "{title} [{tags}] after {blocked_by} under {parent_key} from {base_branch}",
        );
        let plan = plan(Some(&templates), &task(""), &[], StartOptions::default());
        assert_eq!(plan.branch, "pla-12/fix-login-redirect");
        assert_eq!(plan.name, "PLA-12 todo p2");
        assert_eq!(
            plan.prompt,
            "Fix Login Redirect [web,auth] after PLA-1, PLA-2 under PLA-12 from develop"
        );
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
