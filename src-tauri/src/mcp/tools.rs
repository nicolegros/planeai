//! The tools agents call, kept as a registry: PlaneAI's own, one per verb, each a thin
//! adapter over the AXI domain functions the CLI uses (ADR-0016), and those of running
//! plugins (ADR-0017).

use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, RwLock, RwLockReadGuard};

use planeai_core::mcp::McpCaller;
use planeai_core::task_lifecycle::{TaskLifecycleBatch, TaskLifecycleOrigin};
use rmcp::model::{JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::axi::{self, ReadRange, TaskScope, TaskUpdate};
use crate::task_cli::LifecycleSink;

/// What the app learns from tools that change its state.
pub trait AppEvents: Send + Sync {
    fn tasks_changed(&self);
    fn task_lifecycle(&self, batch: TaskLifecycleBatch);
}

/// What tool handlers run against.
#[derive(Clone)]
pub struct ToolContext {
    pub db_path: PathBuf,
    pub events: Arc<dyn AppEvents>,
    /// The URL of the server running the tools; sessions they start connect to it.
    pub(super) mcp_url: Option<String>,
}

impl ToolContext {
    pub fn new(db_path: PathBuf, events: Arc<dyn AppEvents>) -> Self {
        Self {
            db_path,
            events,
            mcp_url: None,
        }
    }
}

use planeai_core::mcp::Effect;

fn annotations(effect: Effect, open_world: bool) -> ToolAnnotations {
    let (read_only, destructive, idempotent) = match effect {
        Effect::Read => (true, false, true),
        Effect::Write { idempotent } => (false, false, idempotent),
        Effect::Delete => (false, true, true),
    };
    ToolAnnotations::new()
        .read_only(read_only)
        .destructive(destructive)
        .idempotent(idempotent)
        .open_world(open_world)
}

/// A tool's TOON reply; errors are reported to the agent, not as protocol failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub text: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn error(message: &str) -> Self {
        Self {
            text: planeai_toon::render(&[planeai_toon::field(
                "error",
                planeai_toon::str_val(message),
            )]),
            is_error: true,
        }
    }
}

impl From<(String, i32)> for ToolOutput {
    fn from((text, code): (String, i32)) -> Self {
        Self {
            text: without_cli_help(&text),
            is_error: code != 0,
        }
    }
}

pub type ToolFuture = Pin<Box<dyn Future<Output = ToolOutput> + Send>>;
type Handler = Box<dyn Fn(ToolContext, McpCaller, JsonObject) -> ToolFuture + Send + Sync>;

pub struct ToolDef {
    pub name: String,
    description: String,
    effect: Effect,
    open_world: bool,
    /// The app reloads its tasks after every call, even a failed one, which may have
    /// written part of its change.
    refreshes_tasks: bool,
    input_schema: Arc<JsonObject>,
    handler: Handler,
}

impl ToolDef {
    /// One of PlaneAI's own tools; its effect comes from `planeai_core::mcp::TOOLS`.
    fn core<I>(
        name: &'static str,
        description: &'static str,
        run: fn(&ToolContext, &McpCaller, I) -> Result<ToolOutput, String>,
    ) -> Self
    where
        I: DeserializeOwned + JsonSchema + 'static,
    {
        let effect = planeai_core::mcp::TOOLS
            .iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("{name} is missing from planeai_core::mcp::TOOLS"))
            .effect;
        Self {
            name: name.to_string(),
            description: description.to_string(),
            effect,
            open_world: false,
            refreshes_tasks: false,
            input_schema: rmcp::handler::server::common::schema_for_type::<I>(),
            handler: Box::new(move |context, caller, arguments| {
                // Handlers do SQLite, git and IPC work synchronously.
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        match serde_json::from_value::<I>(serde_json::Value::Object(arguments)) {
                            Ok(input) => run(&context, &caller, input)
                                .unwrap_or_else(|message| ToolOutput::error(&message)),
                            Err(error) => ToolOutput::error(&format!("invalid arguments: {error}")),
                        }
                    })
                    .await
                    .unwrap_or_else(|error| ToolOutput::error(&error.to_string()))
                })
            }),
        }
    }

    /// A tool a plugin serves (ADR-0017); `run` answers every call, failures included.
    pub fn plugin(
        name: String,
        description: String,
        input_schema: JsonObject,
        effect: Effect,
        open_world: bool,
        run: impl Fn(McpCaller, JsonObject) -> ToolFuture + Send + Sync + 'static,
    ) -> Self {
        let served = name.clone();
        Self {
            name,
            description,
            effect,
            open_world,
            refreshes_tasks: false,
            input_schema: Arc::new(input_schema),
            handler: Box::new(move |_context, caller, arguments| {
                // A tool that asks must not run under an approval its launch granted.
                if !effect.pre_approved() && caller.approved_plugin_tools.contains(&served) {
                    let message = format!("{served} now asks before running, but this session's launch approved it; restart the session to use it");
                    return Box::pin(async move { ToolOutput::error(&message) });
                }
                run(caller, arguments)
            }),
        }
    }

    fn refreshing_tasks(self) -> Self {
        Self {
            refreshes_tasks: true,
            ..self
        }
    }

    pub fn tool(&self) -> Tool {
        Tool::new(
            self.name.clone(),
            self.description.clone(),
            self.input_schema.clone(),
        )
        .with_annotations(annotations(self.effect, self.open_world))
    }

    pub async fn call(
        &self,
        context: &ToolContext,
        caller: &McpCaller,
        arguments: JsonObject,
    ) -> ToolOutput {
        let output = (self.handler)(context.clone(), caller.clone(), arguments).await;
        if self.refreshes_tasks {
            context.events.tasks_changed();
        }
        output
    }
}

type PluginTools = BTreeMap<String, Vec<Arc<ToolDef>>>;

/// The tools the server lists: PlaneAI's own, then those of each running plugin by id.
pub struct Registry {
    core: Vec<Arc<ToolDef>>,
    plugins: RwLock<PluginTools>,
}

impl Registry {
    pub fn get(&self, name: &str) -> Option<Arc<ToolDef>> {
        let plugins = self.plugins();
        self.core
            .iter()
            .chain(plugins.values().flatten())
            .find(|tool| tool.name == name)
            .cloned()
    }

    pub fn tools(&self) -> Vec<Tool> {
        let plugins = self.plugins();
        self.core
            .iter()
            .chain(plugins.values().flatten())
            .map(|tool| tool.tool())
            .collect()
    }

    /// Serve `tools` for the plugin, replacing its earlier set. A set holding a name that
    /// another tool is served under is refused whole.
    pub fn publish_plugin(&self, plugin_id: &str, tools: Vec<ToolDef>) -> Result<(), String> {
        let mut plugins = self
            .plugins
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut names = HashSet::new();
        for tool in &tools {
            let served_elsewhere = self
                .core
                .iter()
                .chain(
                    plugins
                        .iter()
                        .filter(|(id, _)| id.as_str() != plugin_id)
                        .flat_map(|(_, tools)| tools),
                )
                .any(|other| other.name == tool.name);
            if served_elsewhere || !names.insert(tool.name.as_str()) {
                return Err(format!(
                    "plugin {plugin_id} tool {} is already served",
                    tool.name
                ));
            }
        }
        plugins.insert(
            plugin_id.to_string(),
            tools.into_iter().map(Arc::new).collect(),
        );
        Ok(())
    }

    /// Served plugin tools agents may run without asking: every effect but deletes.
    pub fn approved_plugin_tools(&self) -> Vec<String> {
        self.plugins()
            .values()
            .flatten()
            .filter(|tool| tool.effect.pre_approved())
            .map(|tool| tool.name.clone())
            .collect()
    }

    pub fn retire_plugin(&self, plugin_id: &str) {
        self.plugins
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(plugin_id);
    }

    fn plugins(&self) -> RwLockReadGuard<'_, PluginTools> {
        self.plugins
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// PlaneAI's own tools.
    pub fn core() -> Self {
        Self {
            core: [
            ToolDef::core(
                "task_list",
                "List tasks of a project (default: the project of this session's task).",
                task_list,
            ),
            ToolDef::core(
                "task_show",
                "Show one task in full (default: this session's task).",
                task_show,
            ),
            ToolDef::core(
                "task_create",
                "Create a task, optionally as a subtask of `parent`.",
                task_create,
            )
            .refreshing_tasks(),
            ToolDef::core(
                "task_update",
                "Edit a task's fields and/or move it to another status (default: this session's task). An empty `parent` clears it. A refused update changes nothing.",
                task_update,
            )
            .refreshing_tasks(),
            ToolDef::core(
                "task_delete",
                "Delete a task permanently, with its task workspace.",
                task_delete,
            )
            .refreshing_tasks(),
            ToolDef::core(
                "session_list",
                "List agent sessions with their status, task and branch.",
                session_list,
            ),
            ToolDef::core(
                "session_tree",
                "Show the parent/child tree a session belongs to (default: this session).",
                session_tree,
            ),
            ToolDef::core(
                "session_read",
                "Read a session's terminal output: the last `lines`, or only what came after `after`, a cursor from a previous read.",
                session_read,
            ),
            ToolDef::core(
                "session_create",
                "Start an agent session. `child` starts one under this session in any project; `sibling` starts one on this session's task in an isolated worktree. It runs without permission prompts (`yolo`) only if this session does.",
                session_create,
            ),
            ToolDef::core(
                "session_prompt",
                "Send a prompt to a running session. A session that asks before running tools cannot prompt one that does not.",
                session_prompt,
            ),
            ToolDef::core(
                "project_list",
                "List the projects PlaneAI knows.",
                project_list,
            ),
        ]
            .into_iter()
            .map(Arc::new)
            .collect(),
            plugins: RwLock::default(),
        }
    }
}

/// `help` hints name CLI commands; an MCP client has the tools instead.
fn without_cli_help(toon: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in toon.split_inclusive('\n') {
        if !line.starts_with(' ') {
            skipping = line.starts_with("help:") || line.starts_with("help[");
        }
        if !skipping {
            out.push_str(line);
        }
    }
    out
}

fn open(context: &ToolContext) -> Result<rusqlite::Connection, String> {
    rusqlite::Connection::open(&context.db_path).map_err(|error| error.to_string())
}

/// The tasks of the named project, or of the caller's task workspace, over one connection.
fn scope(
    context: &ToolContext,
    caller: &McpCaller,
    project: Option<&str>,
) -> Result<TaskScope, String> {
    let conn = open(context)?;
    let projects = crate::db::list_projects(&conn).map_err(|error| error.to_string())?;
    let project = match project {
        Some(name) => projects
            .iter()
            .find(|candidate| candidate.name == name)
            .ok_or_else(|| format!("unknown project: {name}"))?,
        None => projects
            .iter()
            .find(|candidate| candidate.id == caller.workspace_project_id())
            .ok_or("the project of this session no longer exists")?,
    };
    let events = context.events.clone();
    // `Axi`, not a new origin: plugins built before MCP know only the existing ones.
    Ok(TaskScope::on_migrated(
        conn,
        project,
        TaskLifecycleOrigin::Axi,
        LifecycleSink::InProcess(Arc::new(move |batch| events.task_lifecycle(batch))),
    ))
}

/// An explicit key, else the caller's own task.
fn task_key(caller: &McpCaller, key: Option<String>) -> Result<String, String> {
    key.or_else(|| caller.task.as_ref().map(|task| task.key.clone()))
        .ok_or_else(|| "this session has no task; pass `key`".to_string())
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum TaskStatus {
    Todo,
    InProgress,
    InReview,
    Done,
}

impl TaskStatus {
    fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Todo => "todo",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::InReview => "in_review",
            TaskStatus::Done => "done",
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaskListInput {
    /// Project name; defaults to the project of this session's task.
    project: Option<String>,
    status: Option<TaskStatus>,
    /// Only tasks carrying all of these tags.
    #[serde(default)]
    tags: Vec<String>,
}

fn task_list(
    context: &ToolContext,
    caller: &McpCaller,
    input: TaskListInput,
) -> Result<ToolOutput, String> {
    let scope = scope(context, caller, input.project.as_deref())?;
    Ok(axi::task_ls(
        &scope.repo,
        input.status.map(TaskStatus::as_str),
        &input.tags,
    )
    .into())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaskShowInput {
    /// Task key, e.g. PLA-12; defaults to this session's task.
    key: Option<String>,
    /// Project name owning `key`; defaults to the project of this session's task.
    project: Option<String>,
}

fn task_show(
    context: &ToolContext,
    caller: &McpCaller,
    input: TaskShowInput,
) -> Result<ToolOutput, String> {
    let key = task_key(caller, input.key)?;
    let scope = scope(context, caller, input.project.as_deref())?;
    Ok(axi::task_show(&scope.repo, &key).into())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaskCreateInput {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    priority: i32,
    #[serde(default)]
    tags: Vec<String>,
    /// Keys of tasks that must be done first.
    #[serde(default)]
    blocked_by: Vec<String>,
    /// Key of the parent task, making this a subtask.
    parent: Option<String>,
    /// Branch new sessions on this task start from; defaults to main.
    base_branch: Option<String>,
    /// Project name; defaults to the project of this session's task.
    project: Option<String>,
}

fn task_create(
    context: &ToolContext,
    caller: &McpCaller,
    input: TaskCreateInput,
) -> Result<ToolOutput, String> {
    let scope = scope(context, caller, input.project.as_deref())?;
    Ok(axi::task_add_with_lifecycle(
        &scope.repo,
        crate::task_cli::AddParams {
            title: &input.title,
            description: &input.description,
            priority: input.priority,
            tags: &input.tags,
            blocked_by: &input.blocked_by,
            parent: input.parent.as_deref(),
            base_branch: input.base_branch.as_deref(),
        },
        &scope.lifecycle,
    )
    .into())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaskUpdateInput {
    /// Task key; defaults to this session's task.
    key: Option<String>,
    /// Project name owning `key`; defaults to the project of this session's task.
    project: Option<String>,
    title: Option<String>,
    description: Option<String>,
    priority: Option<i32>,
    /// Replaces the task's tags.
    tags: Option<Vec<String>>,
    /// Replaces the task's blockers.
    blocked_by: Option<Vec<String>>,
    /// Parent task key; an empty string clears it.
    parent: Option<String>,
    base_branch: Option<String>,
    status: Option<TaskStatus>,
}

fn task_update(
    context: &ToolContext,
    caller: &McpCaller,
    input: TaskUpdateInput,
) -> Result<ToolOutput, String> {
    let key = task_key(caller, input.key)?;
    let scope = scope(context, caller, input.project.as_deref())?;
    Ok(axi::task_update(
        &scope,
        TaskUpdate {
            edit: crate::task_cli::EditParams {
                key: &key,
                title: input.title.as_deref(),
                description: input.description.as_deref(),
                priority: input.priority,
                tags: input.tags.as_deref(),
                blocked_by: input.blocked_by.as_deref(),
                parent: input
                    .parent
                    .as_deref()
                    .map(|parent| (!parent.is_empty()).then_some(parent)),
                base_branch: input.base_branch.as_deref(),
            },
            status: input.status.map(TaskStatus::as_str),
        },
    )
    .into())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaskDeleteInput {
    key: String,
    /// Project name owning `key`; defaults to the project of this session's task.
    project: Option<String>,
}

fn task_delete(
    context: &ToolContext,
    caller: &McpCaller,
    input: TaskDeleteInput,
) -> Result<ToolOutput, String> {
    let scope = scope(context, caller, input.project.as_deref())?;
    Ok(axi::task_delete(&scope, &input.key).into())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SessionListInput {
    /// List archived sessions instead of active ones.
    #[serde(default)]
    archived: bool,
}

fn session_list(
    context: &ToolContext,
    _caller: &McpCaller,
    input: SessionListInput,
) -> Result<ToolOutput, String> {
    Ok(axi::session_ls(&open(context)?, input.archived).into())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SessionTreeInput {
    /// Session id or unique id prefix; defaults to this session.
    id: Option<String>,
}

fn session_tree(
    context: &ToolContext,
    caller: &McpCaller,
    input: SessionTreeInput,
) -> Result<ToolOutput, String> {
    let id = input.id.unwrap_or_else(|| caller.session_id.clone());
    Ok(axi::session_tree(&open(context)?, &id).into())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SessionReadInput {
    /// Session id or unique id prefix.
    id: String,
    /// How many trailing lines to return when `after` is not given (default 100).
    lines: Option<usize>,
    /// Cursor from a previous read; returns only output produced since.
    after: Option<String>,
    /// With `after`, the most bytes to return (0 or absent: unlimited).
    max_bytes: Option<usize>,
}

fn session_read(
    context: &ToolContext,
    _caller: &McpCaller,
    input: SessionReadInput,
) -> Result<ToolOutput, String> {
    let range = match (input.after, input.lines) {
        (Some(_), Some(_)) => return Err("pass either `lines` or `after`, not both".into()),
        (Some(cursor), None) => ReadRange::After {
            cursor,
            max_bytes: input.max_bytes.unwrap_or(0),
        },
        (None, lines) => ReadRange::Tail {
            lines: lines.unwrap_or(100),
        },
    };
    Ok(axi::session_read(&open(context)?, &input.id, range).into())
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum SessionMode {
    /// A new session under this one, in any project.
    Child,
    /// Another agent on this session's task, in its own worktree.
    Sibling,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SessionCreateInput {
    mode: SessionMode,
    /// Initial prompt for the new agent.
    prompt: Option<String>,
    name: Option<String>,
    /// Branch to work on; defaults to this session's branch with a unique suffix.
    branch: Option<String>,
    base_branch: Option<String>,
    /// Agent to run; defaults to this session's agent for siblings, the default agent otherwise.
    provider: Option<String>,
    /// Skip permission prompts; defaults to this session's own setting, and only a session
    /// that skips them can start one that does.
    yolo: Option<bool>,
    /// Child only: project name; defaults to this session's project.
    project: Option<String>,
    /// Child only: run in an isolated worktree (default true).
    worktree: Option<bool>,
    /// Child only: task the new session works on.
    task_key: Option<String>,
    /// Child only: project owning `task_key` when it differs from `project`.
    task_project: Option<String>,
}

/// Whether a session the caller starts skips permission prompts: never more than the caller.
pub(crate) fn session_yolo(caller: &McpCaller, requested: Option<bool>) -> Result<bool, String> {
    match requested {
        Some(true) if !caller.auto_approve => Err("this session asks before running tools, so it cannot start a `yolo` session; ask the user to start it".into()),
        requested => Ok(requested.unwrap_or(caller.auto_approve)),
    }
}

fn session_create(
    context: &ToolContext,
    caller: &McpCaller,
    input: SessionCreateInput,
) -> Result<ToolOutput, String> {
    let yolo = session_yolo(caller, input.yolo)?;
    let conn = open(context)?;
    let session = match input.mode {
        SessionMode::Sibling => {
            let child_only = [
                ("project", input.project.is_some()),
                ("worktree", input.worktree.is_some()),
                ("task_key", input.task_key.is_some()),
                ("task_project", input.task_project.is_some()),
            ];
            if let Some((field, _)) = child_only.iter().find(|(_, set)| *set) {
                return Err(format!("`{field}` applies to child sessions only"));
            }
            crate::cli::create_workspace_sibling(
                &conn,
                &caller.session_id,
                crate::cli::WorkspaceSiblingOpts {
                    name: input.name,
                    branch: input.branch,
                    base_branch: input.base_branch,
                    yolo,
                    provider: input.provider,
                    prompt: input.prompt,
                },
                crate::cli::McpUrl::Known(context.mcp_url.clone()),
            )?
        }
        SessionMode::Child => crate::cli::create_child_session(
            &conn,
            &caller.session_id,
            crate::cli::ChildSessionOpts {
                project: input.project,
                name: input.name,
                branch: input.branch,
                base_branch: input.base_branch,
                worktree: input.worktree,
                yolo,
                provider: input.provider,
                task_key: input.task_key,
                task_project: input.task_project,
                prompt: input.prompt,
            },
            crate::cli::McpUrl::Known(context.mcp_url.clone()),
        )?,
    };
    Ok(axi::session_create_output(&session).into())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SessionPromptInput {
    /// Session id or unique id prefix.
    id: String,
    text: String,
}

fn session_prompt(
    context: &ToolContext,
    caller: &McpCaller,
    input: SessionPromptInput,
) -> Result<ToolOutput, String> {
    let conn = open(context)?;
    let target = crate::session_ops::resolve_session_by_prefix(&conn, &input.id)
        .map_err(|error| error.to_string())?;
    if target.auto_approve && !caller.auto_approve {
        return Err(format!(
            "{} runs without permission prompts and this session does not, so it cannot prompt it; ask the user to",
            &target.id[..8]
        ));
    }
    let ops = crate::session_ops::real_prompt_ops(planeai_paths::notify_socket_path());
    Ok(axi::session_prompt(&conn, &target.id, &input.text, &ops).into())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectListInput {}

fn project_list(
    context: &ToolContext,
    _caller: &McpCaller,
    _input: ProjectListInput,
) -> Result<ToolOutput, String> {
    Ok(axi::project_ls(&open(context)?).into())
}
