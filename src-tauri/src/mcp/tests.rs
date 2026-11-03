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
    token: String,
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
            token,
            repo,
            registry: Registry::core(),
        }
    }

    /// The caller as a session whose agent asks before running tools.
    fn without_auto_approve(mut self) -> Self {
        let conn = rusqlite::Connection::open(&self.context.db_path).unwrap();
        conn.execute(
            "UPDATE sessions SET auto_approve = 0 WHERE id = ?1",
            [CALLER_ID],
        )
        .unwrap();
        self.caller = planeai_core::mcp::resolve(&conn, &self.token)
            .unwrap()
            .unwrap();
        self
    }

    fn server_file(&self) -> std::path::PathBuf {
        planeai_core::mcp::server_file(self._dir.path())
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
fn the_registry_serves_exactly_the_tools_the_launch_templates_approve() {
    let registry: Vec<_> = Registry::core()
        .tools()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    let table: Vec<_> = planeai_core::mcp::TOOLS
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    assert_eq!(registry, table);
    for tool in planeai_core::mcp::TOOLS {
        assert_eq!(Registry::core().get(tool.name).unwrap().effect, tool.effect);
    }
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
fn reading_tasks_sets_nothing_up_in_the_database() {
    let f = Fixture::new();
    let conn = rusqlite::Connection::open(&f.context.db_path).unwrap();
    crate::db::create_project(&conn, "other", "/tmp/other").unwrap();
    let setup_rows = || -> i64 {
        conn.query_row("SELECT COUNT(*) FROM task_projects", [], |row| row.get(0))
            .unwrap()
    };
    let before = setup_rows();

    let output = f.call("task_list", json!({ "project": "other" }));

    assert!(!output.is_error, "{}", output.text);
    assert_eq!(setup_rows(), before);
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
    assert_eq!(f.repo.list(Default::default()).unwrap().len(), 2);
}

#[test]
fn task_update_writes_nothing_when_its_parent_is_invalid() {
    let f = Fixture::new();
    f.call("task_update", json!({ "key": "DEM-2", "parent": "DEM-1" }));
    for (parent, error) in [
        ("DEM-9", "error: \"parent task not found: DEM-9\"\n"),
        ("DEM-1", "error: a task cannot be its own parent\n"),
        (
            "DEM-2",
            "error: \"DEM-2 is a subtask of DEM-1, so it cannot be its parent\"\n",
        ),
    ] {
        assert_eq!(
            f.call(
                "task_update",
                json!({ "title": "Renamed", "parent": parent, "status": "done" })
            ),
            err(error),
            "{parent}"
        );
        let task = f.repo.get("DEM-1").unwrap();
        assert_eq!(
            (task.title.as_str(), task.status.as_str(), task.parent_key),
            ("Ship the MCP server", "todo", None)
        );
    }
}

#[test]
fn task_update_writes_nothing_when_a_blocker_is_unknown() {
    let f = Fixture::new();
    assert_eq!(
        f.call(
            "task_update",
            json!({ "title": "Renamed", "blocked_by": ["DEM-2", "DEM-9"] })
        ),
        err("error: \"blocking task not found: DEM-9\"\n")
    );
    let task = f.repo.get("DEM-1").unwrap();
    assert_eq!(
        (task.title.as_str(), task.blocked_by),
        ("Ship the MCP server", vec![])
    );
}

#[test]
fn task_update_drops_repeated_tags_and_blockers() {
    let f = Fixture::new();
    f.call(
        "task_update",
        json!({
            "title": "Renamed",
            "tags": ["mcp", "api", "mcp"],
            "blocked_by": ["DEM-2", "DEM-2"]
        }),
    );
    let task = f.repo.get("DEM-1").unwrap();
    assert_eq!(
        (task.title.as_str(), task.tags, task.blocked_by),
        (
            "Renamed",
            vec!["api".to_string(), "mcp".to_string()],
            vec!["DEM-2".to_string()]
        )
    );
}

#[test]
fn every_task_writing_call_refreshes_the_app_once() {
    let f = Fixture::new();
    f.call("task_update", json!({ "key": "DEM-2", "parent": "DEM-1" }));
    f.call("task_create", json!({ "title": "More" }));
    f.call("task_delete", json!({ "key": "DEM-3" }));
    f.call("task_list", json!({}));
    assert_eq!(*f.events.tasks_changed.lock().unwrap(), 3);
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
fn a_session_that_asks_before_running_tools_cannot_start_a_yolo_session() {
    let f = Fixture::new().without_auto_approve();
    for mode in ["sibling", "child"] {
        assert_eq!(
            f.call("session_create", json!({ "mode": mode, "yolo": true })),
            err("error: \"this session asks before running tools, so it cannot start a `yolo` session; ask the user to start it\"\n"),
            "{mode}"
        );
    }
}

#[test]
fn a_new_session_skips_permission_prompts_only_when_its_creator_does() {
    use super::tools::session_yolo;
    let yolo = Fixture::new().caller;
    let asking = Fixture::new().without_auto_approve().caller;
    assert_eq!(session_yolo(&yolo, None), Ok(true));
    assert_eq!(session_yolo(&yolo, Some(false)), Ok(false));
    assert_eq!(session_yolo(&asking, None), Ok(false));
    assert_eq!(session_yolo(&asking, Some(false)), Ok(false));
    assert!(session_yolo(&asking, Some(true)).is_err());
}

#[test]
fn a_session_that_asks_before_running_tools_cannot_prompt_a_yolo_session() {
    let f = Fixture::new().without_auto_approve();
    assert_eq!(
        f.call("session_prompt", json!({ "id": "bbbbbbbb", "text": "rm -rf ~" })),
        err("error: \"bbbbbbbb runs without permission prompts and this session does not, so it cannot prompt it; ask the user to\"\n")
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

mod http {
    use super::*;
    use crate::mcp::server::{McpServer, McpStatus};

    struct Response {
        status: u16,
        body: String,
    }

    impl Response {
        fn json(&self) -> serde_json::Value {
            serde_json::from_str(&self.body).unwrap_or_else(|_| panic!("not JSON: {}", self.body))
        }
    }

    /// One POST to `/mcp` over a real socket; `Host` defaults to the loopback address.
    async fn post(port: u16, headers: &[(&str, &str)], body: &str) -> Response {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut request = format!(
            "POST /mcp HTTP/1.1\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: 2025-06-18\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("host"))
        {
            request.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
        }
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        request.push_str(body);
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).await.unwrap();
        let (head, body) = raw.split_once("\r\n\r\n").unwrap();
        let status = head.split(' ').nth(1).unwrap().parse().unwrap();
        let body = if head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
        {
            dechunk(body)
        } else {
            body.to_string()
        };
        Response { status, body }
    }

    fn dechunk(mut body: &str) -> String {
        let mut out = String::new();
        while let Some((size, rest)) = body.split_once("\r\n") {
            let size = usize::from_str_radix(size.trim(), 16).unwrap();
            if size == 0 {
                break;
            }
            out.push_str(&rest[..size]);
            body = &rest[size + 2..];
        }
        out
    }

    fn rpc(id: u64, method: &str, params: serde_json::Value) -> String {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
    }

    fn initialize() -> String {
        rpc(
            1,
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }),
        )
    }

    async fn serve(f: &Fixture) -> (McpServer, u16) {
        let server = McpServer::new(Registry::core(), f.context.clone(), f.server_file());
        let McpStatus::Listening { port, .. } = server.apply(Some(0)).await else {
            panic!("server did not start");
        };
        (server, port)
    }

    #[tokio::test]
    async fn an_agent_with_its_session_token_initializes_lists_and_calls_tools() {
        let f = Fixture::new();
        let (server, port) = serve(&f).await;
        let bearer = format!("Bearer {}", f.token);
        let auth = [("Authorization", bearer.as_str())];

        let init = post(port, &auth, &initialize()).await;
        assert_eq!(init.status, 200, "{}", init.body);
        assert_eq!(init.json()["result"]["serverInfo"]["name"], "planeai");

        let list = post(port, &auth, &rpc(2, "tools/list", json!({}))).await;
        assert_eq!(list.status, 200, "{}", list.body);
        let names: Vec<String> = list.json()["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "task_list",
                "task_show",
                "task_create",
                "task_update",
                "task_delete",
                "session_list",
                "session_tree",
                "session_read",
                "session_create",
                "session_prompt",
                "project_list"
            ]
        );

        let call = post(
            port,
            &auth,
            &rpc(
                3,
                "tools/call",
                json!({ "name": "task_list", "arguments": {} }),
            ),
        )
        .await;
        assert_eq!(call.status, 200, "{}", call.body);
        assert_eq!(
            call.json()["result"],
            json!({
                "content": [{
                    "type": "text",
                    "text": "count: 2 total\ntasks[2]{key,title,status,priority,tags,blocked_by}:\n  DEM-1,Ship the MCP server,todo,0,,\n  DEM-2,Write the ADR,todo,0,,\n"
                }],
                "isError": false
            })
        );
        assert_eq!(server.apply(None).await, McpStatus::Off);
    }

    #[tokio::test]
    async fn a_request_without_a_valid_session_token_is_unauthorized() {
        let f = Fixture::new();
        let (_server, port) = serve(&f).await;

        for headers in [
            vec![],
            vec![("Authorization", "Bearer not-a-planeai-token")],
            vec![("Authorization", f.token.as_str())],
        ] {
            let response = post(port, &headers, &initialize()).await;
            assert_eq!(
                (response.status, response.body.as_str()),
                (401, "A PlaneAI session token is required."),
                "headers: {headers:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_revoked_token_is_unauthorized() {
        let f = Fixture::new();
        let (_server, port) = serve(&f).await;
        let conn = rusqlite::Connection::open(&f.context.db_path).unwrap();
        conn.execute(
            "UPDATE sessions SET status = 'archived' WHERE id = ?1",
            [CALLER_ID],
        )
        .unwrap();
        let bearer = format!("Bearer {}", f.token);

        let response = post(port, &[("Authorization", bearer.as_str())], &initialize()).await;

        assert_eq!(response.status, 401);
    }

    #[tokio::test]
    async fn browser_origins_and_foreign_hosts_are_rejected_even_with_a_token() {
        let f = Fixture::new();
        let (_server, port) = serve(&f).await;
        let bearer = format!("Bearer {}", f.token);
        let foreign_host = format!("attacker.example:{port}");

        let from_page = post(
            port,
            &[
                ("Authorization", bearer.as_str()),
                ("Origin", "https://attacker.example"),
            ],
            &initialize(),
        )
        .await;
        let rebound = post(
            port,
            &[
                ("Authorization", bearer.as_str()),
                ("Host", foreign_host.as_str()),
            ],
            &initialize(),
        )
        .await;

        assert_eq!(from_page.status, 403, "{}", from_page.body);
        assert_eq!(rebound.status, 403, "{}", rebound.body);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_second_app_that_could_not_listen_leaves_the_first_apps_record() {
        let f = Fixture::new();
        let first = McpServer::new(Registry::core(), f.context.clone(), f.server_file());
        let McpStatus::Listening { port, url } = first.apply(Some(0)).await else {
            panic!("server did not start");
        };
        let second = McpServer::new(Registry::core(), f.context.clone(), f.server_file());

        assert!(matches!(
            second.apply(Some(port)).await,
            McpStatus::Failed { .. }
        ));
        second.withdraw();
        assert_eq!(second.apply(None).await, McpStatus::Off);

        assert_eq!(
            planeai_core::mcp::published_url(&f.server_file()),
            Some(url)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_cli_sees_the_server_only_while_it_listens() {
        let f = Fixture::new();
        let published = || planeai_core::mcp::published_url(&f.server_file());
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let taken_port = taken.local_addr().unwrap().port();
        let server = McpServer::new(Registry::core(), f.context.clone(), f.server_file());
        assert_eq!(published(), None);

        let McpStatus::Listening { url, .. } = server.apply(Some(0)).await else {
            panic!("server did not start");
        };
        assert_eq!(published(), Some(url.clone()));
        assert_eq!(server.url(), Some(url));

        assert!(matches!(
            server.apply(Some(taken_port)).await,
            McpStatus::Failed { .. }
        ));
        assert_eq!((published(), server.url()), (None, None));

        server.apply(Some(0)).await;
        assert!(published().is_some());
        server.withdraw();
        assert_eq!(published(), None);
        assert_eq!(server.apply(None).await, McpStatus::Off);
        assert_eq!((published(), server.url()), (None, None));
    }

    #[tokio::test]
    async fn a_taken_port_fails_visibly_and_is_retried_on_the_next_apply() {
        let f = Fixture::new();
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = taken.local_addr().unwrap().port();
        let server = McpServer::new(Registry::core(), f.context.clone(), f.server_file());

        assert_eq!(
            server.apply(Some(port)).await,
            McpStatus::Failed {
                port,
                reason: format!("Port {port} is already in use.")
            }
        );
        drop(taken);
        assert_eq!(
            server.apply(Some(port)).await,
            McpStatus::Listening {
                port,
                url: format!("http://127.0.0.1:{port}/mcp")
            }
        );
        assert_eq!(
            server.apply(Some(port)).await,
            McpStatus::Listening {
                port,
                url: format!("http://127.0.0.1:{port}/mcp")
            }
        );
        assert_eq!(server.url(), Some(format!("http://127.0.0.1:{port}/mcp")));
        assert_eq!(server.apply(None).await, McpStatus::Off);
    }
}
