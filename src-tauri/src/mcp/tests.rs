use std::sync::{Arc, Mutex};

use planeai_core::mcp::McpCaller;
use planeai_core::task_lifecycle::{TaskLifecycleBatch, TaskLifecycleEvent};
use planeai_tasks::model::CreateParams;
use planeai_tasks::provider::TaskProvider;
use planeai_tasks::sqlite::SqliteRepository;
use serde_json::json;

use super::tools::{AppEvents, Registry, ToolContext, ToolOutput};

const CALLER_ID: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const CHILD_ID: &str = "bbbbbbbb-1111-2222-3333-444444444444";

#[derive(Default)]
struct RecordedEvents {
    tasks_changed: Mutex<usize>,
    batches: Mutex<Vec<TaskLifecycleBatch>>,
}

impl AppEvents for RecordedEvents {
    fn tasks_changed(&self) {
        *self.tasks_changed.lock().unwrap() += 1;
    }
    fn task_lifecycle(&self, batch: TaskLifecycleBatch) {
        self.batches.lock().unwrap().push(batch);
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    context: ToolContext,
    events: Arc<RecordedEvents>,
    caller: McpCaller,
    repo: SqliteRepository,
    registry: Registry,
}

impl Fixture {
    /// A `demo` project with tasks DEM-1 and DEM-2, and a caller session working on DEM-1.
    fn new() -> Self {
        let dir = tempfile::TempDir::new().unwrap();
        let db_path = dir.path().join("planeai.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        crate::db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        let project = crate::db::create_project(&conn, "demo", "/tmp/demo").unwrap();
        let repo = SqliteRepository::open(db_path.to_str().unwrap(), &project.prefix).unwrap();
        for title in ["Ship the MCP server", "Write the ADR"] {
            repo.create(CreateParams {
                title: title.into(),
                ..Default::default()
            })
            .unwrap();
        }
        let task_key = format!("{}-1", project.prefix);
        for (id, name, parent) in [
            (CALLER_ID, "Planner", None),
            (CHILD_ID, "Worker", Some(CALLER_ID)),
        ] {
            crate::db::create_session_with_id(
                &conn,
                id,
                &project.id,
                name,
                None,
                "feat/mcp",
                None,
                Some("claude"),
                "local",
                true,
                Some(&task_key),
                None,
                parent,
            )
            .unwrap();
        }
        let token = planeai_core::mcp::issue(&conn, CALLER_ID, "http://127.0.0.1:1/mcp").unwrap();
        let token = token.env()[1].1.to_string();
        let caller = planeai_core::mcp::resolve(&conn, &token).unwrap().unwrap();
        let events = Arc::new(RecordedEvents::default());
        Self {
            _dir: dir,
            context: ToolContext {
                db_path,
                events: events.clone(),
            },
            events,
            caller,
            repo,
            registry: Registry::core(),
        }
    }

    fn call(&self, tool: &str, arguments: serde_json::Value) -> ToolOutput {
        let serde_json::Value::Object(arguments) = arguments else {
            panic!("arguments must be an object");
        };
        self.registry
            .get(tool)
            .unwrap_or_else(|| panic!("no tool {tool}"))
            .call(&self.context, &self.caller, arguments)
    }
}

fn ok(text: &str) -> ToolOutput {
    ToolOutput {
        text: text.into(),
        is_error: false,
    }
}

fn err(text: &str) -> ToolOutput {
    ToolOutput {
        text: text.into(),
        is_error: true,
    }
}

fn without_created_at(output: ToolOutput) -> ToolOutput {
    ToolOutput {
        text: output
            .text
            .lines()
            .filter(|line| !line.trim_start().starts_with("created_at:"))
            .map(|line| format!("{line}\n"))
            .collect(),
        ..output
    }
}

#[test]
fn the_core_registry_annotates_every_tool_by_its_effect() {
    let registry = Registry::core();
    let annotated: Vec<(String, bool, bool, bool)> = registry
        .tools()
        .into_iter()
        .map(|tool| {
            let hints = tool.annotations.expect("every tool is annotated");
            (
                tool.name.to_string(),
                hints.read_only_hint.unwrap(),
                hints.destructive_hint.unwrap(),
                hints.open_world_hint.unwrap(),
            )
        })
        .collect();
    let expected = [
        ("task_list", true, false),
        ("task_show", true, false),
        ("task_create", false, false),
        ("task_update", false, false),
        ("task_delete", false, true),
        ("session_list", true, false),
        ("session_tree", true, false),
        ("session_read", true, false),
        ("session_create", false, false),
        ("session_prompt", false, false),
        ("project_list", true, false),
    ]
    .map(|(name, read_only, destructive)| (name.to_string(), read_only, destructive, false));
    assert_eq!(annotated, expected);
}

#[test]
fn task_list_defaults_to_the_project_of_the_callers_task() {
    let f = Fixture::new();
    assert_eq!(
        f.call("task_list", json!({})),
        ok("count: 2 total\ntasks[2]{key,title,status,priority,tags,blocked_by}:\n  DEM-1,Ship the MCP server,todo,0,,\n  DEM-2,Write the ADR,todo,0,,\n")
    );
    assert_eq!(
        f.call("task_list", json!({ "project": "nope" })),
        err("error: \"unknown project: nope\"\n")
    );
}

#[test]
fn task_show_defaults_to_the_callers_task() {
    let f = Fixture::new();
    assert_eq!(
        without_created_at(f.call("task_show", json!({}))),
        ok("task:\n  key: DEM-1\n  title: Ship the MCP server\n  status: todo\n  priority: 0\n  base_branch: main\n")
    );
}

#[test]
fn task_create_makes_a_subtask_and_tells_the_app() {
    let f = Fixture::new();

    let output = f.call(
        "task_create",
        json!({ "title": "Add plugin tools", "parent": "DEM-1" }),
    );

    assert_eq!(
        without_created_at(output),
        ok("task:\n  key: DEM-3\n  title: Add plugin tools\n  status: todo\n  priority: 0\n  parent_key: DEM-1\n  base_branch: main\n")
    );
    assert_eq!(
        f.repo.get("DEM-3").unwrap().parent_key.as_deref(),
        Some("DEM-1")
    );
    assert_eq!(*f.events.tasks_changed.lock().unwrap(), 1);
    let batches = f.events.batches.lock().unwrap();
    assert_eq!(
        batches
            .iter()
            .map(|batch| &batch.events)
            .collect::<Vec<_>>(),
        [&vec![TaskLifecycleEvent::ChildAssigned {
            child_key: "DEM-3".into(),
            parent_key: "DEM-1".into(),
            is_first_child_assignment: true,
        }]]
    );
}

#[test]
fn task_update_edits_and_moves_the_callers_task() {
    let f = Fixture::new();

    let output = f.call(
        "task_update",
        json!({ "title": "Ship it", "status": "in_review" }),
    );

    assert_eq!(
        without_created_at(output),
        ok("task:\n  key: DEM-1\n  title: Ship it\n  status: in_review\n  priority: 0\n  base_branch: main\n")
    );
    assert_eq!(*f.events.tasks_changed.lock().unwrap(), 1);
    assert_eq!(f.events.batches.lock().unwrap().len(), 1);
    assert_eq!(
        f.call("task_update", json!({})),
        err("error: nothing to update\n")
    );
}

#[test]
fn task_update_with_an_empty_parent_detaches_the_subtask() {
    let f = Fixture::new();
    f.call("task_update", json!({ "key": "DEM-2", "parent": "DEM-1" }));
    assert_eq!(
        f.repo.get("DEM-2").unwrap().parent_key.as_deref(),
        Some("DEM-1")
    );

    f.call("task_update", json!({ "key": "DEM-2", "parent": "" }));

    assert_eq!(f.repo.get("DEM-2").unwrap().parent_key, None);
}

#[test]
fn task_delete_of_an_unknown_task_changes_nothing() {
    let f = Fixture::new();
    assert_eq!(
        f.call("task_delete", json!({ "key": "DEM-9" })),
        err("error: task not found\n")
    );
    assert_eq!(*f.events.tasks_changed.lock().unwrap(), 0);
}

#[test]
fn session_tools_default_to_the_calling_session() {
    let f = Fixture::new();
    assert_eq!(
        f.call("session_tree", json!({})),
        ok("session_tree:\n  root: aaaaaaaa\nsessions[2]{id,parent_session_id,name,status,provider,task_key,backend}:\n  aaaaaaaa,,Planner,active,claude,DEM-1,local\n  bbbbbbbb,aaaaaaaa,Worker,active,claude,DEM-1,local\n")
    );
    let list = f.call("session_list", json!({}));
    assert!(!list.is_error);
    assert!(
        list.text.starts_with("count: 2 total\nsessions[2]{id,name,status,provider,branch,task_key,parent_session_id,worktree_path,base_branch,yolo,created_at}:\n  aaaaaaaa,Planner,active,claude,feat/mcp,DEM-1,,,,true,"),
        "{}",
        list.text
    );
}

#[test]
fn session_read_takes_lines_or_a_cursor_but_not_both() {
    let f = Fixture::new();
    assert_eq!(
        f.call(
            "session_read",
            json!({ "id": "bbbbbbbb", "lines": 3, "after": "x" })
        ),
        err("error: \"pass either `lines` or `after`, not both\"\n")
    );
    assert_eq!(
        f.call("session_read", json!({ "id": "bbbbbbbb" })),
        err("error: local backend does not support remote read\n")
    );
}

#[test]
fn session_create_refuses_child_only_fields_for_a_sibling() {
    let f = Fixture::new();
    assert_eq!(
        f.call(
            "session_create",
            json!({ "mode": "sibling", "task_key": "DEM-2" })
        ),
        err("error: `task_key` applies to child sessions only\n")
    );
}

#[test]
fn session_prompt_reports_an_unknown_session() {
    let f = Fixture::new();
    assert_eq!(
        f.call("session_prompt", json!({ "id": "zzzzzzzz", "text": "hi" })),
        err("error: \"no session matching prefix: zzzzzzzz\"\n")
    );
}

#[test]
fn project_list_lists_every_project() {
    let f = Fixture::new();
    assert_eq!(
        f.call("project_list", json!({})),
        ok("projects[1]{prefix,name,path,status}:\n  DEM,demo,/tmp/demo,active\n")
    );
}

#[test]
fn unknown_arguments_are_reported_to_the_agent() {
    let f = Fixture::new();
    assert_eq!(
        f.call("task_list", json!({ "bogus": 1 })),
        err("error: \"invalid arguments: unknown field `bogus`, expected one of `project`, `status`, `tags`\"\n")
    );
}
