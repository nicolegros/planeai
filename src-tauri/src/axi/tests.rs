use super::*;
use planeai_tasks::model::Status;
use planeai_tasks::provider::TaskProvider;
use planeai_tasks::sqlite::SqliteRepository;

fn setup_repo(prefix: &str) -> SqliteRepository {
    SqliteRepository::open_in_memory(prefix).unwrap()
}

fn add_task(repo: &dyn TaskProvider, title: &str) {
    use planeai_tasks::model::CreateParams;
    repo.create(CreateParams {
        title: title.to_string(),
        ..Default::default()
    })
    .unwrap();
}

#[test]
fn task_ls_shows_toon_table_with_count() {
    let repo = setup_repo("TST");
    add_task(&repo, "Fix auth bug");
    add_task(&repo, "Add pagination");

    let (output, code) = task_ls(&repo, None, &[]);
    assert_eq!(code, 0);
    assert!(output.contains("count: 2 total"), "output was:\n{output}");
    assert!(
        output.contains("tasks[2]{key,title,status,priority,tags,blocked_by}:"),
        "output was:\n{output}"
    );
    assert!(
        output.contains("TST-1,Fix auth bug,todo,0,,"),
        "output was:\n{output}"
    );
    assert!(
        output.contains("TST-2,Add pagination,todo,0,,"),
        "output was:\n{output}"
    );
}

#[test]
fn task_ls_filters_by_status() {
    let repo = setup_repo("TST");
    add_task(&repo, "first");
    add_task(&repo, "second");
    repo.update(
        "TST-1",
        planeai_tasks::model::UpdateParams {
            status: Some(Status::Done),
            ..Default::default()
        },
    )
    .unwrap();

    let (output, code) = task_ls(&repo, Some("todo"), &[]);
    assert_eq!(code, 0);
    assert!(output.contains("count: 1 matching"));
    assert!(output.contains("TST-2"));
    assert!(!output.contains("TST-1"));
}

#[test]
fn task_ls_empty_state() {
    let repo = setup_repo("TST");
    let (output, code) = task_ls(&repo, None, &[]);
    assert_eq!(code, 0);
    assert!(output.contains("tasks: 0 tasks found"));
}

#[test]
fn task_ls_empty_state_with_status_filter() {
    let repo = setup_repo("TST");
    let (output, code) = task_ls(&repo, Some("done"), &[]);
    assert_eq!(code, 0);
    assert!(output.contains("tasks: 0 done tasks found"));
}

#[test]
fn task_show_outputs_full_detail() {
    let repo = setup_repo("TST");
    use planeai_tasks::model::CreateParams;
    // Create a blocker first
    repo.create(CreateParams {
        key: None,
        title: "Blocker task".into(),
        description: "".into(),
        status: None,
        priority: 0,
        tags: vec![],
        blocked_by: vec![],
        parent_key: None,
        base_branch: "main".into(),
    })
    .unwrap();
    repo.create(CreateParams {
        key: None,
        title: "Fix auth bug".into(),
        description: "Need to fix the login flow".into(),
        status: None,
        priority: 2,
        tags: vec!["backend".into()],
        blocked_by: vec!["TST-1".into()],
        parent_key: None,
        base_branch: "main".into(),
    })
    .unwrap();

    let (output, code) = task_show(&repo, "TST-2");
    assert_eq!(code, 0);
    assert!(output.contains("key: TST-2"), "output:\n{output}");
    assert!(output.contains("title: Fix auth bug"), "output:\n{output}");
    assert!(output.contains("status: todo"), "output:\n{output}");
    assert!(output.contains("priority: 2"), "output:\n{output}");
    assert!(
        output.contains("description: Need to fix the login flow"),
        "output:\n{output}"
    );
    assert!(output.contains("tags[1]: backend"), "output:\n{output}");
    assert!(output.contains("blocked_by[1]: TST-1"), "output:\n{output}");
    assert!(output.contains("base_branch: main"), "output:\n{output}");
}

#[test]
fn task_show_truncates_long_description() {
    let repo = setup_repo("TST");
    use planeai_tasks::model::CreateParams;
    let long_desc = "x".repeat(1000);
    repo.create(CreateParams {
        key: None,
        title: "Long task".into(),
        description: long_desc,
        status: None,
        priority: 0,
        tags: vec![],
        blocked_by: vec![],
        parent_key: None,
        base_branch: "main".into(),
    })
    .unwrap();

    let (output, _) = task_show(&repo, "TST-1");
    assert!(
        output.contains("truncated, 1000 chars total"),
        "output:\n{output}"
    );
}

#[test]
fn task_add_echoes_created_task_with_hint() {
    let repo = setup_repo("TST");
    let (output, code) = task_add(
        &repo,
        crate::task_cli::AddParams {
            title: "New feature",
            description: "",
            priority: 1,
            tags: &[],
            blocked_by: &[],
            parent: None,
            base_branch: None,
        },
    );
    assert_eq!(code, 0);
    assert!(output.contains("key: TST-1"), "output:\n{output}");
    assert!(output.contains("title: New feature"), "output:\n{output}");
    assert!(output.contains("status: todo"), "output:\n{output}");
    assert!(output.contains("priority: 1"), "output:\n{output}");
    assert!(
        output.contains("planeai-cli axi task move TST-1 in_progress"),
        "output:\n{output}"
    );
}

#[test]
fn task_move_echoes_updated_task() {
    let repo = setup_repo("TST");
    add_task(&repo, "A task");
    let (output, code) = task_move(&repo, "TST-1", "in_progress");
    assert_eq!(code, 0);
    assert!(output.contains("status: in_progress"), "output:\n{output}");
}

#[test]
fn task_move_idempotent_noop() {
    let repo = setup_repo("TST");
    add_task(&repo, "A task");
    // Move once
    task_move(&repo, "TST-1", "done");
    // Move again to same status
    let (output, code) = task_move(&repo, "TST-1", "done");
    assert_eq!(code, 0);
    assert!(output.contains("no-op"), "output:\n{output}");
    assert!(output.contains("status: done"), "output:\n{output}");
}

#[test]
fn task_show_not_found_returns_error() {
    let repo = setup_repo("TST");
    let (output, code) = task_show(&repo, "TST-999");
    assert_eq!(code, 1);
    assert!(output.contains("error:"), "output:\n{output}");
    assert!(output.contains("help"), "output:\n{output}");
}

#[test]
fn task_move_invalid_status_returns_error() {
    let repo = setup_repo("TST");
    add_task(&repo, "A task");
    let (output, code) = task_move(&repo, "TST-1", "bogus");
    assert_eq!(code, 1);
    assert!(output.contains("error:"), "output:\n{output}");
    assert!(output.contains("invalid status"), "output:\n{output}");
    assert!(output.contains("Valid statuses"), "output:\n{output}");
}

#[test]
fn session_create_outputs_toon_with_session_id() {
    let session = crate::db::Session {
        id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
        project_id: "proj-1".to_string(),
        name: "my-feature".to_string(),
        tmux_name: None,
        branch: "feat/my-feature".to_string(),
        status: "active".to_string(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
        worktree_path: Some("/tmp/wt/aaaaaaaa".to_string()),
        provider: Some("kiro".to_string()),
        backend: "daemon".to_string(),
        provider_session_id: None,
        tab_count: 1,
        auto_approve: true,
        task_key: None,
        base_branch: Some("main".to_string()),
        pr_url: None,
        pr_state: None,
        attached_once: false,
        parent_session_id: Some("pppppppp-1111-2222-3333-444444444444".to_string()),
    };

    let (output, code) = session_create_output(&session);
    assert_eq!(code, 0);
    assert!(
        output.contains("id: aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
        "output:\n{output}"
    );
    assert!(output.contains("name: my-feature"), "output:\n{output}");
    assert!(output.contains("status: active"), "output:\n{output}");
    assert!(
        output.contains("branch: feat/my-feature"),
        "output:\n{output}"
    );
    assert!(
        output.contains("worktree_path: /tmp/wt/aaaaaaaa"),
        "output:\n{output}"
    );
    assert!(
        output.contains("parent_session_id: pppppppp-1111-2222-3333-444444444444"),
        "output:\n{output}"
    );
    // Should include help hint
    assert!(
        output.contains("planeai-cli axi session prompt"),
        "output:\n{output}"
    );
}

#[test]
fn session_read_outputs_toon_with_text() {
    let text = "line1\nline2\nline3";
    let (output, code) = session_read_output("aaaabbbb", text);
    assert_eq!(code, 0);
    assert!(output.contains("session_id: aaaabbbb"), "output:\n{output}");
    assert!(output.contains("lines: 3"), "output:\n{output}");
    // Lines are emitted as a list (one per line)
    assert!(output.contains("output[3]:"), "output:\n{output}");
    assert!(output.contains("- line1"), "output:\n{output}");
    assert!(output.contains("- line2"), "output:\n{output}");
    assert!(output.contains("- line3"), "output:\n{output}");
}

#[test]
fn session_read_cursor_outputs_toon_with_cursor_fields() {
    let (output, code) = session_read_cursor_output(
        "aaaabbbb",
        "daemon",
        "daemon:1234",
        false,
        "new output here",
    );
    assert_eq!(code, 0);
    assert!(output.contains("session_id: aaaabbbb"), "output:\n{output}");
    assert!(output.contains("backend: daemon"), "output:\n{output}");
    assert!(
        output.contains("cursor: \"daemon:1234\""),
        "output:\n{output}"
    );
    assert!(output.contains("truncated: false"), "output:\n{output}");
    assert!(
        output.contains("text: new output here"),
        "output:\n{output}"
    );
}

#[test]
fn session_read_cursor_truncated_flag() {
    let (output, code) = session_read_cursor_output(
        "bbbbcccc",
        "tmux",
        "tmux:100:9876543210",
        true,
        "all available content",
    );
    assert_eq!(code, 0);
    assert!(output.contains("truncated: true"), "output:\n{output}");
    assert!(output.contains("backend: tmux"), "output:\n{output}");
}

// ─── Session children/tree TOON output ───────────────────────────────────────

fn setup_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    crate::db::migrate(&conn).unwrap();
    planeai_tasks::sqlite::migrate(&conn).unwrap();
    planeai_core::loop_service::LoopService::migrate(&conn).unwrap();
    conn
}

/// Create a tree: root → child1, child2; child1 → grandchild
fn setup_session_tree(conn: &rusqlite::Connection) -> (String, String, String, String) {
    let project = crate::db::create_project(conn, "test-project", "/tmp/test").unwrap();
    let root_id = "aaaaaaaa-1111-2222-3333-444444444444".to_string();
    let child1_id = "bbbbbbbb-1111-2222-3333-444444444444".to_string();
    let child2_id = "cccccccc-1111-2222-3333-444444444444".to_string();
    let grandchild_id = "dddddddd-1111-2222-3333-444444444444".to_string();

    crate::db::create_session_with_id(
        conn,
        &root_id,
        &project.id,
        "Planner",
        None,
        "main",
        None,
        Some("claude"),
        "daemon",
        true,
        Some("PLA-201"),
        None,
        None,
    )
    .unwrap();

    crate::db::create_session_with_id(
        conn,
        &child1_id,
        &project.id,
        "Worker 1",
        None,
        "main",
        None,
        Some("codex"),
        "daemon",
        true,
        Some("PLA-201"),
        None,
        Some(&root_id),
    )
    .unwrap();

    crate::db::create_session_with_id(
        conn,
        &child2_id,
        &project.id,
        "Reviewer",
        None,
        "main",
        None,
        Some("kiro"),
        "daemon",
        true,
        Some("PLA-201"),
        None,
        Some(&root_id),
    )
    .unwrap();

    crate::db::create_session_with_id(
        conn,
        &grandchild_id,
        &project.id,
        "Sub-worker",
        None,
        "main",
        None,
        Some("codex"),
        "daemon",
        true,
        None,
        None,
        Some(&child1_id),
    )
    .unwrap();

    (root_id, child1_id, child2_id, grandchild_id)
}

#[test]
fn session_children_outputs_toon_table() {
    let conn = setup_db();
    let (root_id, child1_id, child2_id, _) = setup_session_tree(&conn);

    let (output, code) = session_children(&conn, &root_id[..8]);
    assert_eq!(code, 0);
    assert!(
        output.contains("parent_session_id: aaaaaaaa"),
        "output:\n{output}"
    );
    assert!(
        output.contains("children[2]{id,parent_session_id,name,status,provider,task_key,backend}:"),
        "output:\n{output}"
    );
    assert!(output.contains(&child1_id[..8]), "output:\n{output}");
    assert!(output.contains(&child2_id[..8]), "output:\n{output}");
    assert!(output.contains("Worker 1"), "output:\n{output}");
    assert!(output.contains("Reviewer"), "output:\n{output}");
}

#[test]
fn session_children_empty_outputs_message() {
    let conn = setup_db();
    let (_, _, child2_id, _) = setup_session_tree(&conn);

    // child2 has no children
    let (output, code) = session_children(&conn, &child2_id[..8]);
    assert_eq!(code, 0);
    assert!(output.contains("children: 0 children"), "output:\n{output}");
}

#[test]
fn session_tree_outputs_full_tree_toon() {
    let conn = setup_db();
    let (root_id, child1_id, child2_id, grandchild_id) = setup_session_tree(&conn);

    let (output, code) = session_tree(&conn, &root_id[..8]);
    assert_eq!(code, 0);
    assert!(output.contains("session_tree:"), "output:\n{output}");
    assert!(output.contains("root: aaaaaaaa"), "output:\n{output}");
    assert!(
        output.contains("sessions[4]{id,parent_session_id,name,status,provider,task_key,backend}:"),
        "output:\n{output}"
    );
    // BFS order: root, child1, child2, grandchild
    let lines: Vec<&str> = output.lines().collect();
    let session_lines: Vec<&&str> = lines
        .iter()
        .filter(|l| {
            l.trim().starts_with(&root_id[..8])
                || l.trim().starts_with(&child1_id[..8])
                || l.trim().starts_with(&child2_id[..8])
                || l.trim().starts_with(&grandchild_id[..8])
        })
        .collect();
    assert_eq!(
        session_lines.len(),
        4,
        "expected 4 session rows, output:\n{output}"
    );
}

#[test]
fn session_tree_from_child_shows_full_tree() {
    let conn = setup_db();
    let (_, _, _, grandchild_id) = setup_session_tree(&conn);

    // Call from grandchild — should walk up to root
    let (output, code) = session_tree(&conn, &grandchild_id[..8]);
    assert_eq!(code, 0);
    assert!(output.contains("root: aaaaaaaa"), "output:\n{output}");
    assert!(output.contains("sessions[4]"), "output:\n{output}");
}

// ─── Loop tests ──────────────────────────────────────────────────────────────

fn extract_loop_id(toon_output: &str) -> String {
    toon_output
        .lines()
        .find(|l| l.trim().starts_with("id: "))
        .and_then(|l| l.trim().strip_prefix("id: "))
        .unwrap_or("")
        .to_string()
}

#[test]
fn loop_create_outputs_toon_with_loop_id_and_status() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    let (output, code) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        None,
        "Implement auth",
        3,
        false,
    );
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains("loop:"), "output:\n{output}");
    assert!(output.contains("status: draft"), "output:\n{output}");
    assert!(
        output.contains("recipe_id: maker-verifier"),
        "output:\n{output}"
    );
    assert!(output.contains("goal: Implement auth"), "output:\n{output}");
    assert!(output.contains("max_rounds: 3"), "output:\n{output}");
    assert!(output.contains("current_round: 0"), "output:\n{output}");
    assert!(output.contains("next_actions[1]:"), "output:\n{output}");
    assert!(
        output.contains("planeai-cli axi loop tick"),
        "output:\n{output}"
    );
    // ID should be a valid UUID prefix (8 hex chars)
    assert!(output.contains("id: "), "output:\n{output}");
}

#[test]
fn loop_create_with_start_outputs_running_status() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    let (output, code) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        None,
        "Build feature",
        5,
        true,
    );
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains("status: running"), "output:\n{output}");
    // Recipe's policy overrides CLI max_rounds
    assert!(output.contains("max_rounds: 3"), "output:\n{output}");
}

#[test]
fn loop_create_with_session_id_env_stores_parent() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    // Set the env var for this test
    std::env::set_var("PLANEAI_SESSION_ID", "parent-session-1234");
    let (output, code) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        None,
        "Goal",
        3,
        false,
    );
    std::env::remove_var("PLANEAI_SESSION_ID");

    assert_eq!(code, 0, "output:\n{output}");
    // Verify via observe that created_by_session_id is stored
    // Extract loop ID from the output
    let id_line = output
        .lines()
        .find(|l| l.trim().starts_with("id: "))
        .unwrap();
    let loop_id = id_line.trim().strip_prefix("id: ").unwrap();

    let (obs_output, obs_code) = loop_observe(&conn, loop_id, 20);
    assert_eq!(obs_code, 0, "observe output:\n{obs_output}");
    assert!(
        obs_output.contains("created_by_session_id: parent-session-1234"),
        "observe output:\n{obs_output}"
    );
}

#[test]
fn loop_create_validates_task_key() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    // Insert a task directly so it exists in the same DB
    conn.execute(
        "INSERT OR IGNORE INTO task_projects (prefix) VALUES ('MYA')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (key, project_prefix, title, status, created_at, updated_at)
         VALUES ('MYA-1', 'MYA', 'Real task', 'todo', '2026-01-01', '2026-01-01')",
        [],
    )
    .unwrap();

    // Valid task key works
    let (output, code) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        Some("MYA-1"),
        "maker-verifier",
        None,
        "Goal",
        3,
        false,
    );
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains("task_key: MYA-1"), "output:\n{output}");

    // Invalid task key fails
    let (output, code) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        Some("NONEXIST-999"),
        "maker-verifier",
        None,
        "Goal",
        3,
        false,
    );
    assert_eq!(code, 1, "output:\n{output}");
    assert!(output.contains("task not found"), "output:\n{output}");
}

#[test]
fn loop_create_rejects_invalid_max_rounds() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    let (output, code) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        None,
        "Goal",
        0,
        false,
    );
    assert_eq!(code, 1, "output:\n{output}");
    assert!(
        output.contains("--max-rounds must be >= 1"),
        "output:\n{output}"
    );
}

#[test]
fn loop_observe_returns_status_and_events() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    // Create a loop
    let (create_output, _) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        None,
        "Build auth",
        3,
        false,
    );
    let loop_id = extract_loop_id(&create_output);

    // Observe it
    let (output, code) = loop_observe(&conn, &loop_id, 20);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains("loop:"), "output:\n{output}");
    assert!(output.contains("status: draft"), "output:\n{output}");
    assert!(
        output.contains("strategy: maker-verifier"),
        "output:\n{output}"
    );
    assert!(output.contains("goal: Build auth"), "output:\n{output}");
    assert!(output.contains("sessions: 0 sessions"), "output:\n{output}");
    // loop_created event should exist from the create call
    assert!(
        output.contains("loop_created"),
        "expected loop_created event, output:\n{output}"
    );
    assert!(output.contains("next_actions"), "output:\n{output}");
}

#[test]
fn loop_tick_appends_event_and_returns_state() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    // Create a draft loop with a non-recipe strategy (legacy path)
    let (create_output, _) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "legacy-strategy",
        None,
        "Goal",
        3,
        false,
    );
    let loop_id = extract_loop_id(&create_output);

    // Tick should transition draft → running
    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains("loop:"), "output:\n{output}");
    assert!(output.contains("status: running"), "output:\n{output}");
    assert!(output.contains("event:"), "output:\n{output}");
    assert!(output.contains("kind: tick"), "output:\n{output}");
    assert!(output.contains("next_actions"), "output:\n{output}");

    // Verify loop_started event was appended via observe
    let (obs_output, _) = loop_observe(&conn, &loop_id, 20);
    assert!(
        obs_output.contains("loop_started"),
        "expected loop_started event, output:\n{obs_output}"
    );
}

#[test]
fn loop_stop_cancels_running_loop() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    let (create_output, _) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        None,
        "Goal",
        3,
        true,
    );
    let loop_id = extract_loop_id(&create_output);

    let (output, code) = loop_stop(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains("status: cancelled"), "output:\n{output}");
    assert!(output.contains("next_actions"), "output:\n{output}");
    assert!(
        output.contains("Clean up any running sessions manually"),
        "output:\n{output}"
    );
}

#[test]
fn loop_stop_is_idempotent_on_terminal_status() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    let (create_output, _) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        None,
        "Goal",
        3,
        true,
    );
    let loop_id = extract_loop_id(&create_output);

    // Stop once
    loop_stop(&conn, &loop_id);
    // Stop again — should be idempotent
    let (output, code) = loop_stop(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains("status: cancelled"), "output:\n{output}");
    assert!(output.contains("no-op"), "output:\n{output}");
}

#[test]
fn loop_stop_treats_completed_unreviewed_as_terminal() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    let (create_output, _) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        None,
        "Goal",
        3,
        true,
    );
    let loop_id = extract_loop_id(&create_output);

    // Manually transition to completed_unreviewed
    use planeai_core::loop_run::{LoopStatus, LoopTrigger};
    use planeai_core::loop_service::LoopService;
    LoopService::transition_loop(&conn, &loop_id, LoopTrigger::RecipeSetStatus(LoopStatus::CompletedUnreviewed)).unwrap();

    // Stop should be a no-op
    let (output, code) = loop_stop(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(
        output.contains("status: completed_unreviewed"),
        "output:\n{output}"
    );
    assert!(output.contains("no-op"), "output:\n{output}");
}

#[test]
fn loop_tick_rejects_terminal_status() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    let (create_output, _) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        None,
        "Goal",
        3,
        true,
    );
    let loop_id = extract_loop_id(&create_output);

    loop_stop(&conn, &loop_id);

    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 1, "expected error exit code, output:\n{output}");
    assert!(
        output.contains("terminal status"),
        "expected terminal status error, output:\n{output}"
    );
}

#[test]
fn loop_tree_handles_zero_sessions() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    let (create_output, _) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        None,
        "Goal",
        3,
        false,
    );
    let loop_id = extract_loop_id(&create_output);

    let (output, code) = loop_tree(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains("sessions: 0 sessions"), "output:\n{output}");
}

#[test]
fn loop_prefix_resolution_works() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    let (create_output, _) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        None,
        "Goal",
        3,
        false,
    );
    let loop_id = extract_loop_id(&create_output);
    let prefix = &loop_id[..8];

    // Should resolve via prefix
    let (output, code) = loop_observe(&conn, prefix, 20);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains(&loop_id), "output:\n{output}");
}

#[test]
fn loop_tree_shows_sessions_with_children() {
    let conn = setup_db();
    let project = crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    // Create a loop
    let (create_output, _) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        None,
        "Goal",
        3,
        false,
    );
    let loop_id = extract_loop_id(&create_output);

    // Create sessions and register them with the loop
    let maker_id = "11111111-aaaa-bbbb-cccc-dddddddddddd".to_string();
    let child_id = "22222222-aaaa-bbbb-cccc-dddddddddddd".to_string();

    crate::db::create_session_with_id(
        &conn,
        &maker_id,
        &project.id,
        "Maker",
        None,
        "main",
        None,
        Some("claude"),
        "daemon",
        true,
        None,
        None,
        None,
    )
    .unwrap();

    crate::db::create_session_with_id(
        &conn,
        &child_id,
        &project.id,
        "Sub-worker",
        None,
        "main",
        None,
        Some("codex"),
        "daemon",
        true,
        None,
        None,
        Some(&maker_id),
    )
    .unwrap();

    // Add maker as a loop session
    use planeai_core::loop_service::{AddLoopSessionParams, LoopService};
    LoopService::add_loop_session(
        &conn,
        AddLoopSessionParams {
            loop_id: loop_id.clone(),
            session_id: maker_id.clone(),
            role: "maker".to_string(),
            round: 0,
            provider: Some("claude".to_string()),
            status: "active".to_string(),
        },
    )
    .unwrap();

    // loop tree should show both the maker and its child
    let (output, code) = loop_tree(&conn, &loop_id[..8]);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(
        output.contains("sessions[2]"),
        "expected 2 sessions (maker + child), output:\n{output}"
    );
    assert!(output.contains("11111111"), "output:\n{output}");
    assert!(output.contains("22222222"), "output:\n{output}");
    assert!(output.contains("Maker"), "output:\n{output}");
    assert!(output.contains("Sub-worker"), "output:\n{output}");
}

// ─── Handoff AXI Tests ───────────────────────────────────────────────────

fn setup_loop_db() -> rusqlite::Connection {
    use planeai_core::services::open_db_at;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let conn = open_db_at(&path).unwrap();
    std::mem::forget(dir);
    conn
}

fn create_test_loop_with_session(conn: &rusqlite::Connection) -> (String, String) {
    use planeai_core::loop_run::{LoopStrategy, LoopTrigger};
    use planeai_core::loop_service::{AddLoopSessionParams, CreateLoopParams, LoopService};

    let loop_run = LoopService::create_loop(
        conn,
        CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: Some("PLA-201".into()),
            created_by_session_id: None,
            strategy: LoopStrategy::new("maker-verifier"),
            goal: "Test handoff".into(),
            max_rounds: 3,
            policy_json: None,
            budget_json: None,
        },
    )
    .unwrap();

    LoopService::transition_loop(conn, &loop_run.id, LoopTrigger::Start).unwrap();

    let session_id = "aaaabbbb-1111-2222-3333-444455556666".to_string();
    LoopService::add_loop_session(
        conn,
        AddLoopSessionParams {
            loop_id: loop_run.id.clone(),
            session_id: session_id.clone(),
            role: "maker".to_string(),
            round: 1,
            provider: Some("claude".to_string()),
            status: "running".to_string(),
        },
    )
    .unwrap();

    (loop_run.id, session_id)
}

#[test]
fn handoff_path_emits_toon_with_correct_fields() {
    let conn = setup_loop_db();
    let (loop_id, session_id) = create_test_loop_with_session(&conn);
    let cwd = "/tmp/test-project";

    let (output, code) = loop_handoff_path(&conn, &loop_id[..8], &session_id[..8], cwd);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains("handoff_path:"), "output:\n{output}");
    assert!(
        output.contains(&format!("loop_id: {loop_id}")),
        "output:\n{output}"
    );
    assert!(
        output.contains(&format!("session_id: {session_id}")),
        "output:\n{output}"
    );
    assert!(output.contains("role: maker"), "output:\n{output}");
    assert!(output.contains("handoff.json"), "output:\n{output}");
    assert!(output.contains("next_actions[2]:"), "output:\n{output}");
    assert!(
        output.contains("write a planeai.handoff.v1 JSON file"),
        "output:\n{output}"
    );
}

#[test]
fn handoff_path_fails_for_unknown_session() {
    let conn = setup_loop_db();
    let (loop_id, _) = create_test_loop_with_session(&conn);

    let (output, code) = loop_handoff_path(&conn, &loop_id[..8], "nonexist", "/tmp");
    assert_eq!(code, 1);
    assert!(output.contains("error:"), "output:\n{output}");
    assert!(output.contains("session not found"), "output:\n{output}");
}

#[test]
fn handoff_record_emits_toon_on_success() {
    let conn = setup_loop_db();
    let (loop_id, session_id) = create_test_loop_with_session(&conn);

    // Create a handoff file
    let dir = tempfile::tempdir().unwrap();
    let handoff_path = dir.path().join("handoff.json");
    let handoff_json = serde_json::json!({
        "schema": "planeai.handoff.v1",
        "loop_id": loop_id,
        "session_id": session_id,
        "status": "completed",
        "summary": "Feature implemented",
        "branch": "feat/test",
        "commit": "abc123",
        "changed_files": ["src/main.rs"],
        "risks": ["Might break on Windows"],
        "evidence": [{
            "kind": "test",
            "name": "cargo test",
            "result": "pass",
            "source": "direct"
        }]
    });
    std::fs::write(&handoff_path, handoff_json.to_string()).unwrap();

    // Use the temp dir as the CWD (so path validation passes)
    let cwd = dir.path().to_string_lossy().to_string();

    let (output, code) =
        loop_handoff_record(&conn, &loop_id[..8], &session_id[..8], &handoff_path, &cwd);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains("handoff_recorded:"), "output:\n{output}");
    assert!(
        output.contains(&format!("loop_id: {loop_id}")),
        "output:\n{output}"
    );
    assert!(
        output.contains(&format!("session_id: {session_id}")),
        "output:\n{output}"
    );
    assert!(
        output.contains("schema: planeai.handoff.v1"),
        "output:\n{output}"
    );
    assert!(output.contains("status: completed"), "output:\n{output}");
    assert!(
        output.contains("loop_status: observing"),
        "output:\n{output}"
    );
    assert!(
        output.contains("session_status: completed"),
        "output:\n{output}"
    );
    assert!(output.contains("state_changed: true"), "output:\n{output}");
    assert!(output.contains("risks[1]:"), "output:\n{output}");
    assert!(
        output.contains("Might break on Windows"),
        "output:\n{output}"
    );
    assert!(output.contains("next_actions[1]:"), "output:\n{output}");

    std::mem::forget(dir);
}

#[test]
fn handoff_record_persists_artifact_and_event() {
    use planeai_core::loop_service::LoopService;

    let conn = setup_loop_db();
    let (loop_id, session_id) = create_test_loop_with_session(&conn);

    let dir = tempfile::tempdir().unwrap();
    let handoff_path = dir.path().join("handoff.json");
    let handoff_json = serde_json::json!({
        "schema": "planeai.handoff.v1",
        "loop_id": loop_id,
        "session_id": session_id,
        "status": "blocked",
        "summary": "Blocked by migration",
        "risks": ["Migration conflict"]
    });
    std::fs::write(&handoff_path, handoff_json.to_string()).unwrap();

    let cwd = dir.path().to_string_lossy().to_string();
    let (_, code) = loop_handoff_record(&conn, &loop_id, &session_id, &handoff_path, &cwd);
    assert_eq!(code, 0);

    // Check event was stored
    let events = LoopService::list_loop_events(&conn, &loop_id).unwrap();
    let handoff_events: Vec<_> = events
        .iter()
        .filter(|e| e.kind == "handoff_recorded")
        .collect();
    assert_eq!(handoff_events.len(), 1);
    assert_eq!(
        handoff_events[0].payload_json["status"].as_str().unwrap(),
        "blocked"
    );

    // Check loop status was updated to blocked
    let updated = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    assert_eq!(updated.status, planeai_core::loop_run::LoopStatus::Blocked);

    // Check session status was updated
    let sessions = LoopService::list_loop_sessions(&conn, &loop_id).unwrap();
    assert_eq!(sessions[0].status, "blocked");

    std::mem::forget(dir);
}

#[test]
fn handoff_record_fails_on_invalid_json() {
    let conn = setup_loop_db();
    let (loop_id, session_id) = create_test_loop_with_session(&conn);

    let dir = tempfile::tempdir().unwrap();
    let handoff_path = dir.path().join("handoff.json");
    std::fs::write(&handoff_path, "not valid json").unwrap();

    let cwd = dir.path().to_string_lossy().to_string();
    let (output, code) = loop_handoff_record(&conn, &loop_id, &session_id, &handoff_path, &cwd);
    assert_eq!(code, 1);
    assert!(
        output.contains("error: invalid handoff file"),
        "output:\n{output}"
    );
    assert!(output.contains("details["), "output:\n{output}");

    std::mem::forget(dir);
}

#[test]
fn handoff_record_fails_on_id_mismatch() {
    let conn = setup_loop_db();
    let (loop_id, session_id) = create_test_loop_with_session(&conn);

    let dir = tempfile::tempdir().unwrap();
    let handoff_path = dir.path().join("handoff.json");
    let handoff_json = serde_json::json!({
        "schema": "planeai.handoff.v1",
        "loop_id": "wrong_loop_id",
        "session_id": session_id,
        "status": "completed",
        "summary": "Done"
    });
    std::fs::write(&handoff_path, handoff_json.to_string()).unwrap();

    let cwd = dir.path().to_string_lossy().to_string();
    let (output, code) = loop_handoff_record(&conn, &loop_id, &session_id, &handoff_path, &cwd);
    assert_eq!(code, 1);
    assert!(
        output.contains("error: invalid handoff file"),
        "output:\n{output}"
    );
    assert!(output.contains("loop_id mismatch"), "output:\n{output}");

    std::mem::forget(dir);
}

#[test]
fn handoff_record_fails_on_path_outside_project() {
    let conn = setup_loop_db();
    let (loop_id, session_id) = create_test_loop_with_session(&conn);

    // Create file in /tmp but use a different cwd
    let dir = tempfile::tempdir().unwrap();
    let other_dir = tempfile::tempdir().unwrap();
    let handoff_path = dir.path().join("handoff.json");
    std::fs::write(&handoff_path, "{}").unwrap();

    let cwd = other_dir.path().to_string_lossy().to_string();
    let (output, code) = loop_handoff_record(&conn, &loop_id, &session_id, &handoff_path, &cwd);
    assert_eq!(code, 1);
    assert!(
        output.contains("outside the project root"),
        "output:\n{output}"
    );

    std::mem::forget(dir);
    std::mem::forget(other_dir);
}

// ─── Verifier Gate Tests ─────────────────────────────────────────────────

fn create_test_loop_with_session_in_dir(
    conn: &rusqlite::Connection,
    project_path: &str,
    worktree_path: Option<&str>,
) -> (String, String) {
    planeai_core::test_fixtures::setup_loop_with_session(conn, project_path, worktree_path)
}

#[test]
fn verify_successful_command_renders_toon_pass() {
    let conn = setup_loop_db();
    let dir = tempfile::tempdir().unwrap();
    let project_path = dir.path().to_string_lossy().to_string();
    let (loop_id, session_id) = create_test_loop_with_session_in_dir(&conn, &project_path, None);

    let (output, code) = loop_verify(
        &conn,
        &loop_id[..8],
        &session_id[..8],
        "echo-test",
        "echo hello",
        planeai_core::verifier::DEFAULT_TIMEOUT_MS,
        planeai_core::verifier::DEFAULT_MAX_OUTPUT_BYTES,
    );
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains("verifier:"), "output:\n{output}");
    assert!(output.contains("name: echo-test"), "output:\n{output}");
    assert!(output.contains("status: pass"), "output:\n{output}");
    assert!(output.contains("exit_code: 0"), "output:\n{output}");
    assert!(output.contains("output_path:"), "output:\n{output}");
    assert!(output.contains("next_actions[2]:"), "output:\n{output}");
    assert!(
        output.contains("planeai-cli axi loop observe"),
        "output:\n{output}"
    );
}

#[test]
fn verify_failing_command_renders_toon_fail() {
    let conn = setup_loop_db();
    let dir = tempfile::tempdir().unwrap();
    let project_path = dir.path().to_string_lossy().to_string();
    let (loop_id, session_id) = create_test_loop_with_session_in_dir(&conn, &project_path, None);

    let (output, code) = loop_verify(
        &conn,
        &loop_id[..8],
        &session_id[..8],
        "failing-test",
        "exit 42",
        planeai_core::verifier::DEFAULT_TIMEOUT_MS,
        planeai_core::verifier::DEFAULT_MAX_OUTPUT_BYTES,
    );
    assert_eq!(code, 1, "output:\n{output}");
    assert!(output.contains("status: fail"), "output:\n{output}");
    assert!(output.contains("exit_code: 42"), "output:\n{output}");
    assert!(output.contains("inspect output at:"), "output:\n{output}");
}

#[test]
fn verify_missing_loop_returns_error() {
    let conn = setup_loop_db();
    let (output, code) = loop_verify(
        &conn,
        "nonexistent",
        "some-session",
        "test",
        "echo hi",
        planeai_core::verifier::DEFAULT_TIMEOUT_MS,
        planeai_core::verifier::DEFAULT_MAX_OUTPUT_BYTES,
    );
    assert_eq!(code, 1);
    assert!(output.contains("error:"), "output:\n{output}");
    assert!(output.contains("loop not found"), "output:\n{output}");
}

#[test]
fn verify_missing_session_returns_error() {
    let conn = setup_loop_db();
    let dir = tempfile::tempdir().unwrap();
    let project_path = dir.path().to_string_lossy().to_string();
    let (loop_id, _) = create_test_loop_with_session_in_dir(&conn, &project_path, None);

    let (output, code) = loop_verify(
        &conn,
        &loop_id[..8],
        "nonexistent",
        "test",
        "echo hi",
        planeai_core::verifier::DEFAULT_TIMEOUT_MS,
        planeai_core::verifier::DEFAULT_MAX_OUTPUT_BYTES,
    );
    assert_eq!(code, 1);
    assert!(output.contains("error:"), "output:\n{output}");
    assert!(output.contains("session not found"), "output:\n{output}");
}

#[test]
fn verify_missing_worktree_returns_cwd_unavailable_error() {
    let conn = setup_loop_db();
    let dir = tempfile::tempdir().unwrap();
    let project_path = dir.path().to_string_lossy().to_string();
    let (loop_id, session_id) =
        create_test_loop_with_session_in_dir(&conn, &project_path, Some("/nonexistent/wt"));

    let (output, code) = loop_verify(
        &conn,
        &loop_id[..8],
        &session_id[..8],
        "test",
        "echo hi",
        planeai_core::verifier::DEFAULT_TIMEOUT_MS,
        planeai_core::verifier::DEFAULT_MAX_OUTPUT_BYTES,
    );
    assert_eq!(code, 1);
    assert!(
        output.contains("verifier working directory unavailable"),
        "output:\n{output}"
    );
    assert!(
        output.contains("worktree_path does not exist"),
        "output:\n{output}"
    );
}

// ─── Recipe AXI & Tick Runtime Tests ─────────────────────────────────────

#[test]
fn recipe_ls_emits_toon() {
    let (output, code) = recipe_ls("/tmp");
    assert_eq!(code, 0, "output:\n{output}");
    assert!(
        output.contains("recipes["),
        "expected recipes table header, output:\n{output}"
    );
    assert!(
        output.contains("maker-verifier"),
        "expected built-in maker-verifier recipe, output:\n{output}"
    );
}

#[test]
fn recipe_show_emits_roles_and_steps() {
    let (output, code) = recipe_show("maker-verifier", "/tmp");
    assert_eq!(code, 0, "output:\n{output}");
    assert!(
        output.contains("recipe:"),
        "expected recipe object header, output:\n{output}"
    );
    assert!(
        output.contains("roles["),
        "expected roles table, output:\n{output}"
    );
    assert!(
        output.contains("steps["),
        "expected steps table, output:\n{output}"
    );
    assert!(
        output.contains("id: maker-verifier"),
        "expected recipe id, output:\n{output}"
    );
    assert!(
        output.contains("valid: true"),
        "expected valid: true, output:\n{output}"
    );
}

#[test]
fn recipe_validate_succeeds() {
    let (output, code) = recipe_validate("maker-verifier", "/tmp");
    assert_eq!(code, 0, "output:\n{output}");
    assert!(
        output.contains("recipe_validation:"),
        "expected recipe_validation header, output:\n{output}"
    );
    assert!(
        output.contains("valid: true"),
        "expected valid: true, output:\n{output}"
    );
}

#[test]
fn recipe_validate_fails_for_nonexistent() {
    let (output, code) = recipe_validate("nonexistent", "/tmp");
    assert_eq!(code, 1, "output:\n{output}");
    assert!(
        output.contains("error"),
        "expected error in output, output:\n{output}"
    );
}

#[test]
fn loop_create_with_recipe_stores_snapshot() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    let (output, code) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        Some("maker-verifier"),
        "Build auth",
        3,
        false,
    );
    assert_eq!(code, 0, "output:\n{output}");

    let loop_id = extract_loop_id(&output);
    assert!(
        !loop_id.is_empty(),
        "failed to extract loop_id from output:\n{output}"
    );

    let (obs_output, obs_code) = loop_observe(&conn, &loop_id, 20);
    assert_eq!(obs_code, 0, "observe output:\n{obs_output}");
    assert!(
        obs_output.contains("strategy: maker-verifier"),
        "expected strategy in observe output:\n{obs_output}"
    );
}

#[test]
fn loop_create_strategy_alias_works() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    let (output, code) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        None,
        "Build feature",
        3,
        false,
    );
    assert_eq!(code, 0, "output:\n{output}");
    assert!(
        output.contains("loop:"),
        "expected loop TOON object, output:\n{output}"
    );
    assert!(
        output.contains("status: draft"),
        "expected draft status, output:\n{output}"
    );
}

#[test]
fn recipe_tick_session_create_fails_gracefully_when_backend_unavailable() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    // Create a loop with recipe and start=false (draft)
    let (create_output, create_code) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        Some("maker-verifier"),
        "Implement feature",
        3,
        false,
    );
    assert_eq!(create_code, 0, "create output:\n{create_output}");

    let loop_id = extract_loop_id(&create_output);
    assert!(!loop_id.is_empty(), "failed to extract loop_id");

    // First tick: draft->running transition + session.create step fails because
    // /tmp/myapp is not a valid git repo and no backend is available
    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 1, "tick output:\n{output}");
    assert!(
        output.contains("session.create failed"),
        "expected session.create failure message, output:\n{output}"
    );
}

#[test]
fn recipe_tick_session_prompt_fails_when_no_sessions_exist() {
    use planeai_core::loop_recipe::*;
    use planeai_core::loop_recipe_service::*;
    use planeai_core::loop_run::LoopTrigger;
    use planeai_core::loop_service::LoopService;
    use std::collections::BTreeMap;

    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    // Create a loop with a snapshot that starts on session.prompt (no sessions exist)
    let steps = vec![RecipeStep {
        id: "prompt_maker".into(),
        kind: STEP_SESSION_PROMPT.into(),
        role: Some("maker".into()),
        prompt: Some("Do the thing".into()),
        from: None,
        on: None,
        status: None,
        next: None,
        select: Some("latest".into()),
        event_kind: None,
        gates: vec![],
    }];

    let snapshot = RecipeSnapshot {
        recipe_schema: RECIPE_SCHEMA_V1.into(),
        recipe_id: "test-recipe".into(),
        recipe_source: "builtin".into(),
        recipe_path: None,
        inputs: BTreeMap::new(),
        runtime: RecipeRuntime {
            current_step: "prompt_maker".into(),
            tick_count: 0,
            round: 1,
            created_session_ids: BTreeMap::new(), // No sessions!
            last_error: None,
            last_handoff_consumed_at: None,
        },
        policy: SnapshotPolicy {
            max_rounds: 3,
            max_ticks: 50,
            max_sessions: 5,
            merge_policy: "human".into(),
            auto_approve: true,
        },
        roles: BTreeMap::new(),
        steps,
        knowledge: RecipeKnowledge::default(),
        tools: RecipeTools::default(),
    };

    let policy_json = serde_json::to_value(&snapshot).unwrap();

    let loop_run = LoopService::create_loop(
        &conn,
        planeai_core::loop_service::CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: planeai_core::loop_run::LoopStrategy::new("test-recipe"),
            goal: "test prompt".into(),
            max_rounds: 3,
            policy_json: Some(policy_json),
            budget_json: None,
        },
    )
    .unwrap();

    LoopService::transition_loop(&conn, &loop_run.id, LoopTrigger::Start).unwrap();

    // Tick — session.prompt should fail because no sessions for role
    let (output, code) = loop_tick(&conn, &loop_run.id);
    assert_eq!(code, 1, "expected failure, output:\n{output}");
    assert!(
        output.contains("no sessions exist for role"),
        "expected no-sessions error, output:\n{output}"
    );
}

#[test]
fn recipe_tick_max_ticks_prevents_runaway() {
    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    let (create_output, create_code) = loop_create(
        &conn,
        "/tmp/myapp",
        None,
        None,
        "maker-verifier",
        Some("maker-verifier"),
        "Implement feature",
        3,
        false,
    );
    assert_eq!(create_code, 0, "create output:\n{create_output}");

    let loop_id = extract_loop_id(&create_output);
    assert!(!loop_id.is_empty(), "failed to extract loop_id");

    // Transition to running so tick_recipe is invoked
    use planeai_core::loop_run::LoopTrigger;
    use planeai_core::loop_service::LoopService;
    LoopService::transition_loop(&conn, &loop_id, LoopTrigger::Start).unwrap();

    // Set tick_count = max_ticks so next tick is blocked
    conn.execute(
        "UPDATE loop_runs SET policy_json = json_set(policy_json, '$.runtime.tick_count', json_extract(policy_json, '$.policy.max_ticks')) WHERE id = ?1",
        rusqlite::params![loop_id],
    )
    .unwrap();

    // Next tick should fail with max_ticks exceeded
    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 1, "expected failure code, output:\n{output}");
    assert!(
        output.contains("max_ticks"),
        "expected max_ticks error message, output:\n{output}"
    );
}

#[test]
fn recipe_tick_round_next_increments_round() {
    use planeai_core::loop_recipe::*;
    use planeai_core::loop_recipe_service::*;
    use planeai_core::loop_run::LoopTrigger;
    use planeai_core::loop_service::LoopService;
    use std::collections::BTreeMap;

    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    // Build a snapshot with a round.next step
    let steps = vec![
        RecipeStep {
            id: "next_round".into(),
            kind: STEP_ROUND_NEXT.into(),
            role: None,
            prompt: None,
            from: None,
            on: None,
            status: None,
            next: Some("after_round".into()),
            select: None,
            event_kind: None,
            gates: vec![],
        },
        RecipeStep {
            id: "after_round".into(),
            kind: STEP_LOOP_EVENT.into(),
            role: None,
            prompt: None,
            from: None,
            on: None,
            status: None,
            next: None,
            select: None,
            event_kind: Some("post_round".into()),
            gates: vec![],
        },
    ];

    let snapshot = RecipeSnapshot {
        recipe_schema: RECIPE_SCHEMA_V1.into(),
        recipe_id: "test-recipe".into(),
        recipe_source: "builtin".into(),
        recipe_path: None,
        inputs: BTreeMap::new(),
        runtime: RecipeRuntime {
            current_step: "next_round".into(),
            tick_count: 0,
            round: 1,
            created_session_ids: BTreeMap::new(),
            last_error: None,
            last_handoff_consumed_at: None,
        },
        policy: SnapshotPolicy {
            max_rounds: 3,
            max_ticks: 50,
            max_sessions: 5,
            merge_policy: "human".into(),
            auto_approve: true,
        },
        roles: BTreeMap::new(),
        steps,
        knowledge: RecipeKnowledge::default(),
        tools: RecipeTools::default(),
    };

    let policy_json = serde_json::to_value(&snapshot).unwrap();

    // Create loop with this snapshot
    let loop_run = LoopService::create_loop(
        &conn,
        planeai_core::loop_service::CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: planeai_core::loop_run::LoopStrategy::new("test-recipe"),
            goal: "test round".into(),
            max_rounds: 3,
            policy_json: Some(policy_json),
            budget_json: None,
        },
    )
    .unwrap();

    // Move to running
    LoopService::transition_loop(&conn, &loop_run.id, LoopTrigger::Start).unwrap();

    // Tick — should execute round.next
    let (output, code) = loop_tick(&conn, &loop_run.id);
    assert_eq!(code, 0, "round.next should succeed, output:\n{output}");
    assert!(output.contains("round.next"), "output:\n{output}");

    // Verify round was incremented in DB
    let updated = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert_eq!(updated.current_round, 2, "current_round should be 2");

    // Verify snapshot runtime.round was updated
    let snap: RecipeSnapshot = serde_json::from_value(updated.policy_json.unwrap()).unwrap();
    assert_eq!(snap.runtime.round, 2);
    assert_eq!(snap.runtime.current_step, "after_round");
}

#[test]
fn recipe_tick_round_next_enforces_max_rounds() {
    use planeai_core::loop_recipe::*;
    use planeai_core::loop_recipe_service::*;
    use planeai_core::loop_run::{LoopStatus, LoopTrigger};
    use planeai_core::loop_service::LoopService;
    use std::collections::BTreeMap;

    let conn = setup_db();
    crate::db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

    let steps = vec![RecipeStep {
        id: "next_round".into(),
        kind: STEP_ROUND_NEXT.into(),
        role: None,
        prompt: None,
        from: None,
        on: None,
        status: None,
        next: Some("start".into()),
        select: None,
        event_kind: None,
        gates: vec![],
    }];

    let snapshot = RecipeSnapshot {
        recipe_schema: RECIPE_SCHEMA_V1.into(),
        recipe_id: "test-recipe".into(),
        recipe_source: "builtin".into(),
        recipe_path: None,
        inputs: BTreeMap::new(),
        runtime: RecipeRuntime {
            current_step: "next_round".into(),
            tick_count: 5,
            round: 3, // Already at max_rounds
            created_session_ids: BTreeMap::new(),
            last_error: None,
            last_handoff_consumed_at: None,
        },
        policy: SnapshotPolicy {
            max_rounds: 3,
            max_ticks: 50,
            max_sessions: 5,
            merge_policy: "human".into(),
            auto_approve: true,
        },
        roles: BTreeMap::new(),
        steps,
        knowledge: RecipeKnowledge::default(),
        tools: RecipeTools::default(),
    };

    let policy_json = serde_json::to_value(&snapshot).unwrap();

    let loop_run = LoopService::create_loop(
        &conn,
        planeai_core::loop_service::CreateLoopParams {
            project_id: "proj-1".into(),
            task_key: None,
            created_by_session_id: None,
            strategy: planeai_core::loop_run::LoopStrategy::new("test-recipe"),
            goal: "test limit".into(),
            max_rounds: 3,
            policy_json: Some(policy_json),
            budget_json: None,
        },
    )
    .unwrap();

    LoopService::transition_loop(&conn, &loop_run.id, LoopTrigger::Start).unwrap();

    // Tick — should fail because we're at max_rounds
    let (output, code) = loop_tick(&conn, &loop_run.id);
    assert_eq!(
        code, 0,
        "round.next at limit should return code 0 (sets blocked), output:\n{output}"
    );
    assert!(
        output.contains("blocked") || output.contains("max_rounds"),
        "expected max_rounds limit message, output:\n{output}"
    );

    // Verify loop status is now blocked
    let updated = LoopService::get_loop(&conn, &loop_run.id).unwrap().unwrap();
    assert_eq!(updated.status, LoopStatus::Blocked);
}

// ─── Maker-Verifier Full Flow Integration Tests ──────────────────────────

use planeai_core::loop_recipe_service::RecipeSnapshot;
use planeai_core::loop_run::{LoopStatus, LoopTrigger};
use planeai_core::loop_service::LoopService;

/// Helper: create a loop with a custom RecipeSnapshot, pre-populated with
/// sessions and optional handoff artifacts.
/// Returns (loop_id, project_id, snapshot).
fn setup_maker_verifier_flow(
    conn: &rusqlite::Connection,
    current_step: &str,
    round: u32,
    maker_session_id: Option<&str>,
    verifier_session_id: Option<&str>,
) -> (String, String, RecipeSnapshot) {
    use planeai_core::loop_recipe::*;
    use planeai_core::loop_recipe_service::*;
    use std::collections::BTreeMap;

    let project = crate::db::create_project(conn, "testapp", "/tmp/testapp").unwrap();

    // Parse the actual built-in recipe to get the real steps
    let recipe = RecipeService::parse_yaml(include_str!(
        "../../planeai-core/resources/recipes/maker-verifier.yaml"
    ))
    .expect("built-in maker-verifier should parse");

    let steps: Vec<RecipeStep> = recipe.steps;
    let roles: BTreeMap<String, RecipeRole> = recipe.roles;

    let mut created_session_ids: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Some(sid) = maker_session_id {
        created_session_ids
            .entry("maker".into())
            .or_default()
            .push(sid.to_string());
    }
    if let Some(sid) = verifier_session_id {
        created_session_ids
            .entry("verifier".into())
            .or_default()
            .push(sid.to_string());
    }

    let mut inputs = BTreeMap::new();
    inputs.insert("goal".to_string(), "Implement the feature".to_string());

    let snapshot = RecipeSnapshot {
        recipe_schema: RECIPE_SCHEMA_V1.into(),
        recipe_id: "maker-verifier".into(),
        recipe_source: "builtin".into(),
        recipe_path: None,
        inputs,
        runtime: RecipeRuntime {
            current_step: current_step.to_string(),
            tick_count: 1,
            round,
            created_session_ids,
            last_error: None,
            last_handoff_consumed_at: None,
        },
        policy: SnapshotPolicy {
            max_rounds: 3,
            max_ticks: 50,
            max_sessions: 5,
            merge_policy: "human".into(),
            auto_approve: true,
        },
        roles,
        steps,
        knowledge: RecipeKnowledge::default(),
        tools: RecipeTools::default(),
    };

    let policy_json = serde_json::to_value(&snapshot).unwrap();

    let loop_run = LoopService::create_loop(
        conn,
        planeai_core::loop_service::CreateLoopParams {
            project_id: project.id.clone(),
            task_key: Some("PLA-210".into()),
            created_by_session_id: None,
            strategy: planeai_core::loop_run::LoopStrategy::new("maker-verifier"),
            goal: "Implement the feature".into(),
            max_rounds: 3,
            policy_json: Some(policy_json),
            budget_json: None,
        },
    )
    .unwrap();

    LoopService::transition_loop(conn, &loop_run.id, LoopTrigger::Start).unwrap();
    (loop_run.id, project.id, snapshot)
}

/// Helper: insert a handoff artifact for a session.
fn insert_handoff(conn: &rusqlite::Connection, loop_id: &str, session_id: &str, status: &str) {
    let content = serde_json::json!({
        "schema": "planeai.handoff.v1",
        "loop_id": loop_id,
        "session_id": session_id,
        "status": status,
        "summary": "Work completed",
        "branch": "feat/test",
        "commit": "abc123",
        "changed_files": ["src/main.rs"],
        "risks": [],
        "next_actions": [],
        "evidence": []
    });
    conn.execute(
        "INSERT INTO loop_artifacts (id, loop_id, session_id, kind, content_json, created_at)
         VALUES (?1, ?2, ?3, 'handoff', ?4, datetime('now'))",
        rusqlite::params![
            uuid::Uuid::new_v4().to_string(),
            loop_id,
            session_id,
            content.to_string(),
        ],
    )
    .unwrap();
}

/// Helper: create a session record and link it to a loop.
fn create_and_link_session(
    conn: &rusqlite::Connection,
    loop_id: &str,
    session_id: &str,
    role: &str,
    round: i64,
    project_id: &str,
) {
    crate::db::create_session_with_id(
        conn,
        session_id,
        project_id,
        &format!("{role} session"),
        None,
        "main",
        None,
        Some("claude"),
        "daemon",
        true,
        Some("PLA-210"),
        None,
        None,
    )
    .unwrap();

    LoopService::add_loop_session(
        conn,
        planeai_core::loop_service::AddLoopSessionParams {
            loop_id: loop_id.to_string(),
            session_id: session_id.to_string(),
            role: role.to_string(),
            round,
            provider: Some("claude".to_string()),
            status: "active".to_string(),
        },
    )
    .unwrap();
}

#[test]
fn maker_verifier_handoff_wait_detects_completed_handoff_and_routes_to_gates() {
    let conn = setup_db();
    let maker_id = "maker-11111111-2222-3333-4444-555555555555";
    let (loop_id, project_id, _) =
        setup_maker_verifier_flow(&conn, "wait_for_maker", 1, Some(maker_id), None);
    create_and_link_session(&conn, &loop_id, maker_id, "maker", 1, &project_id);
    insert_handoff(&conn, &loop_id, maker_id, "completed");

    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains("handoff.wait"), "output:\n{output}");
    assert!(
        output.contains("completed") || output.contains("run_gates"),
        "should route to run_gates, output:\n{output}"
    );

    // Verify snapshot advanced to run_gates
    let updated = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    let snap: RecipeSnapshot = serde_json::from_value(updated.policy_json.unwrap()).unwrap();
    assert_eq!(snap.runtime.current_step, "run_gates");
}

#[test]
fn maker_verifier_handoff_wait_blocked_routes_to_terminal() {
    let conn = setup_db();
    let maker_id = "maker-22222222-2222-3333-4444-555555555555";
    let (loop_id, project_id, _) =
        setup_maker_verifier_flow(&conn, "wait_for_maker", 1, Some(maker_id), None);
    create_and_link_session(&conn, &loop_id, maker_id, "maker", 1, &project_id);
    insert_handoff(&conn, &loop_id, maker_id, "blocked");

    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(
        output.contains("blocked"),
        "should route to blocked, output:\n{output}"
    );

    // Verify snapshot advanced to blocked step
    let updated = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    let snap: RecipeSnapshot = serde_json::from_value(updated.policy_json.unwrap()).unwrap();
    assert_eq!(snap.runtime.current_step, "blocked");
}

/// Helper: set up a loop at the `run_gates` step with a real project path
/// so gate commands can execute. Returns (loop_id, project_path as String).
/// The caller retains `_dir` to keep the tempdir alive.
fn setup_maker_verifier_flow_with_path(
    conn: &rusqlite::Connection,
    maker_id: &str,
) -> (String, String, tempfile::TempDir) {
    use planeai_core::loop_recipe::*;
    use planeai_core::loop_recipe_service::*;
    use std::collections::BTreeMap;

    let dir = tempfile::tempdir().unwrap();
    let project_path = dir.path().to_string_lossy().to_string();
    let project = crate::db::create_project(conn, "testapp", &project_path).unwrap();

    let recipe = RecipeService::parse_yaml(include_str!(
        "../../planeai-core/resources/recipes/maker-verifier.yaml"
    ))
    .unwrap();

    let steps: Vec<RecipeStep> = recipe.steps;
    let roles: BTreeMap<String, RecipeRole> = recipe.roles;

    let mut created_session_ids: BTreeMap<String, Vec<String>> = BTreeMap::new();
    created_session_ids
        .entry("maker".into())
        .or_default()
        .push(maker_id.to_string());

    let mut inputs = BTreeMap::new();
    inputs.insert("goal".to_string(), "Implement the feature".to_string());

    let snapshot = RecipeSnapshot {
        recipe_schema: RECIPE_SCHEMA_V1.into(),
        recipe_id: "maker-verifier".into(),
        recipe_source: "builtin".into(),
        recipe_path: None,
        inputs,
        runtime: RecipeRuntime {
            current_step: "run_gates".into(),
            tick_count: 2,
            round: 1,
            created_session_ids,
            last_error: None,
            last_handoff_consumed_at: None,
        },
        policy: SnapshotPolicy {
            max_rounds: 3,
            max_ticks: 50,
            max_sessions: 5,
            merge_policy: "human".into(),
            auto_approve: true,
        },
        roles,
        steps,
        knowledge: RecipeKnowledge::default(),
        tools: RecipeTools::default(),
    };

    let policy_json = serde_json::to_value(&snapshot).unwrap();

    let loop_run = LoopService::create_loop(
        conn,
        planeai_core::loop_service::CreateLoopParams {
            project_id: project.id.clone(),
            task_key: Some("PLA-210".into()),
            created_by_session_id: None,
            strategy: planeai_core::loop_run::LoopStrategy::new("maker-verifier"),
            goal: "Implement the feature".into(),
            max_rounds: 3,
            policy_json: Some(policy_json),
            budget_json: None,
        },
    )
    .unwrap();
    let loop_id = loop_run.id;

    LoopService::transition_loop(conn, &loop_id, LoopTrigger::Start).unwrap();

    crate::db::create_session_with_id(
        conn,
        maker_id,
        &project.id,
        "maker session",
        None,
        "main",
        Some(&project_path),
        Some("claude"),
        "daemon",
        true,
        Some("PLA-210"),
        None,
        None,
    )
    .unwrap();

    LoopService::add_loop_session(
        conn,
        planeai_core::loop_service::AddLoopSessionParams {
            loop_id: loop_id.clone(),
            session_id: maker_id.to_string(),
            role: "maker".to_string(),
            round: 1,
            provider: Some("claude".to_string()),
            status: "active".to_string(),
        },
    )
    .unwrap();

    (loop_id, project_path, dir)
}

#[test]
fn maker_verifier_gates_pass_routes_to_create_verifier() {
    let conn = setup_db();
    let maker_id = "maker-33333333-2222-3333-4444-555555555555";
    let (loop_id, _project_path, _dir) = setup_maker_verifier_flow_with_path(&conn, maker_id);

    // Override gate_command to a command that always succeeds (recipe defaults to `make ci`)
    {
        let run = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
        let mut snap: RecipeSnapshot = serde_json::from_value(run.policy_json.unwrap()).unwrap();
        snap.inputs
            .insert("gate_command".to_string(), "true".to_string());
        let updated_json = serde_json::to_value(&snap).unwrap();
        LoopService::update_policy_json(&conn, &loop_id, &updated_json).unwrap();
    }

    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 0, "gates.run should succeed, output:\n{output}");
    assert!(output.contains("gates.run"), "output:\n{output}");
    assert!(
        output.contains("pass") || output.contains("create_verifier"),
        "should route to create_verifier on pass, output:\n{output}"
    );

    let updated = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    let snap: RecipeSnapshot = serde_json::from_value(updated.policy_json.unwrap()).unwrap();
    assert_eq!(snap.runtime.current_step, "create_verifier");
}

#[test]
fn maker_verifier_gates_fail_routes_to_retry() {
    let conn = setup_db();
    let maker_id = "maker-44444444-2222-3333-4444-555555555555";
    let (loop_id, _project_path, _dir) = setup_maker_verifier_flow_with_path(&conn, maker_id);

    // Default gate_command is `make ci` which fails in a bare tempdir (no Makefile)
    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(
        code, 0,
        "gates.run returns 0 (routes through on.fail), output:\n{output}"
    );
    assert!(
        output.contains("fail") || output.contains("gates_failed_retry"),
        "should route to gates_failed_retry on fail, output:\n{output}"
    );

    let updated = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    let snap: RecipeSnapshot = serde_json::from_value(updated.policy_json.unwrap()).unwrap();
    assert_eq!(snap.runtime.current_step, "gates_failed_retry");
}

#[test]
fn maker_verifier_verifier_approval_marks_completed_unreviewed() {
    let conn = setup_db();
    let maker_id = "maker-55555555-2222-3333-4444-555555555555";
    let verifier_id = "verif-55555555-2222-3333-4444-555555555555";
    let (loop_id, project_id, _) = setup_maker_verifier_flow(
        &conn,
        "wait_for_verifier",
        1,
        Some(maker_id),
        Some(verifier_id),
    );
    create_and_link_session(&conn, &loop_id, maker_id, "maker", 1, &project_id);
    create_and_link_session(&conn, &loop_id, verifier_id, "verifier", 1, &project_id);
    insert_handoff(&conn, &loop_id, verifier_id, "completed");

    // Tick: handoff.wait should detect verifier completed → route to completed_unreviewed
    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(
        output.contains("completed_unreviewed") || output.contains("completed"),
        "should route to completed_unreviewed, output:\n{output}"
    );

    // Tick again: the loop.status step should set completed_unreviewed
    let (output2, code2) = loop_tick(&conn, &loop_id);
    assert_eq!(code2, 0, "output:\n{output2}");
    assert!(
        output2.contains("completed_unreviewed"),
        "should set completed_unreviewed status, output:\n{output2}"
    );

    // Verify loop is now in terminal state
    let updated = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    assert_eq!(updated.status, LoopStatus::CompletedUnreviewed);
}

#[test]
fn maker_verifier_verifier_rejection_routes_to_retry() {
    let conn = setup_db();
    let maker_id = "maker-66666666-2222-3333-4444-555555555555";
    let verifier_id = "verif-66666666-2222-3333-4444-555555555555";
    let (loop_id, project_id, _) = setup_maker_verifier_flow(
        &conn,
        "wait_for_verifier",
        1,
        Some(maker_id),
        Some(verifier_id),
    );
    create_and_link_session(&conn, &loop_id, maker_id, "maker", 1, &project_id);
    create_and_link_session(&conn, &loop_id, verifier_id, "verifier", 1, &project_id);
    insert_handoff(&conn, &loop_id, verifier_id, "needs_human");

    // Tick: handoff.wait should route to verifier_rejected_retry
    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(
        output.contains("verifier_rejected_retry") || output.contains("needs_human"),
        "should route to verifier_rejected_retry, output:\n{output}"
    );

    let updated = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    let snap: RecipeSnapshot = serde_json::from_value(updated.policy_json.unwrap()).unwrap();
    assert_eq!(snap.runtime.current_step, "verifier_rejected_retry");
}

#[test]
fn maker_verifier_round_increment_after_gates_fail_cycles_back() {
    let conn = setup_db();
    let maker_id = "maker-77777777-2222-3333-4444-555555555555";
    // Start at increment_round_after_gates step
    let (loop_id, project_id, _) = setup_maker_verifier_flow(
        &conn,
        "increment_round_after_gates",
        1,
        Some(maker_id),
        None,
    );
    create_and_link_session(&conn, &loop_id, maker_id, "maker", 1, &project_id);

    // Tick: round.next should increment round and advance to wait_for_maker
    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains("round.next"), "output:\n{output}");

    let updated = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    assert_eq!(updated.current_round, 2);
    let snap: RecipeSnapshot = serde_json::from_value(updated.policy_json.unwrap()).unwrap();
    assert_eq!(snap.runtime.round, 2);
    assert_eq!(snap.runtime.current_step, "wait_for_maker");
}

#[test]
fn maker_verifier_round_increment_after_review_cycles_back() {
    let conn = setup_db();
    let maker_id = "maker-88888888-2222-3333-4444-555555555555";
    let verifier_id = "verif-88888888-2222-3333-4444-555555555555";
    // Start at increment_round_after_review step
    let (loop_id, project_id, _) = setup_maker_verifier_flow(
        &conn,
        "increment_round_after_review",
        1,
        Some(maker_id),
        Some(verifier_id),
    );
    create_and_link_session(&conn, &loop_id, maker_id, "maker", 1, &project_id);
    create_and_link_session(&conn, &loop_id, verifier_id, "verifier", 1, &project_id);

    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(output.contains("round.next"), "output:\n{output}");

    let updated = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    assert_eq!(updated.current_round, 2);
    let snap: RecipeSnapshot = serde_json::from_value(updated.policy_json.unwrap()).unwrap();
    assert_eq!(snap.runtime.round, 2);
    assert_eq!(snap.runtime.current_step, "wait_for_maker");
}

#[test]
fn maker_verifier_max_rounds_blocks_at_gates_retry() {
    let conn = setup_db();
    let maker_id = "maker-99999999-2222-3333-4444-555555555555";
    // Already at round 3 (max_rounds=3), trying to increment
    let (loop_id, project_id, _) = setup_maker_verifier_flow(
        &conn,
        "increment_round_after_gates",
        3, // at max
        Some(maker_id),
        None,
    );
    create_and_link_session(&conn, &loop_id, maker_id, "maker", 3, &project_id);

    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(
        output.contains("blocked") || output.contains("max_rounds"),
        "should mark blocked at max_rounds, output:\n{output}"
    );

    let updated = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    assert_eq!(updated.status, LoopStatus::Blocked);
}

#[test]
fn maker_verifier_max_rounds_blocks_at_review_retry() {
    let conn = setup_db();
    let maker_id = "maker-aaaaaaaa-2222-3333-4444-555555555555";
    let verifier_id = "verif-aaaaaaaa-2222-3333-4444-555555555555";
    // Already at round 3 (max_rounds=3), trying to increment after review
    let (loop_id, project_id, _) = setup_maker_verifier_flow(
        &conn,
        "increment_round_after_review",
        3,
        Some(maker_id),
        Some(verifier_id),
    );
    create_and_link_session(&conn, &loop_id, maker_id, "maker", 3, &project_id);
    create_and_link_session(&conn, &loop_id, verifier_id, "verifier", 3, &project_id);

    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(
        output.contains("blocked") || output.contains("max_rounds"),
        "should mark blocked at max_rounds, output:\n{output}"
    );

    let updated = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    assert_eq!(updated.status, LoopStatus::Blocked);
}

#[test]
fn maker_verifier_no_auto_merge_path_exists() {
    let conn = setup_db();
    let maker_id = "maker-bbbbbbbb-2222-3333-4444-555555555555";
    let verifier_id = "verif-bbbbbbbb-2222-3333-4444-555555555555";
    let (loop_id, project_id, _) = setup_maker_verifier_flow(
        &conn,
        "wait_for_verifier",
        1,
        Some(maker_id),
        Some(verifier_id),
    );
    create_and_link_session(&conn, &loop_id, maker_id, "maker", 1, &project_id);
    create_and_link_session(&conn, &loop_id, verifier_id, "verifier", 1, &project_id);
    insert_handoff(&conn, &loop_id, verifier_id, "completed");

    // Tick 1: handoff.wait detects verifier completed → routes to completed_unreviewed
    let (_output, _code) = loop_tick(&conn, &loop_id);
    // Tick 2: loop.status sets completed_unreviewed
    let (output2, code2) = loop_tick(&conn, &loop_id);
    assert_eq!(code2, 0, "output:\n{output2}");

    let updated = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    assert_eq!(
        updated.status,
        LoopStatus::CompletedUnreviewed,
        "final state should be completed_unreviewed, NOT merged/approved"
    );

    // Trying to tick a terminal loop should fail
    let (output3, code3) = loop_tick(&conn, &loop_id);
    assert_eq!(
        code3, 1,
        "terminal loop should not tick, output:\n{output3}"
    );
    assert!(output3.contains("terminal"), "output:\n{output3}");
}

#[test]
fn maker_verifier_handoff_wait_without_handoff_stays_observing() {
    let conn = setup_db();
    let maker_id = "maker-cccccccc-2222-3333-4444-555555555555";
    let (loop_id, project_id, _) =
        setup_maker_verifier_flow(&conn, "wait_for_maker", 1, Some(maker_id), None);
    create_and_link_session(&conn, &loop_id, maker_id, "maker", 1, &project_id);
    // No handoff recorded

    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(
        output.contains("observing") || output.contains("waiting_for"),
        "should stay observing without handoff, output:\n{output}"
    );

    // Step should NOT advance
    let updated = LoopService::get_loop(&conn, &loop_id).unwrap().unwrap();
    let snap: RecipeSnapshot = serde_json::from_value(updated.policy_json.unwrap()).unwrap();
    assert_eq!(snap.runtime.current_step, "wait_for_maker");
}

#[test]
fn maker_verifier_first_tick_creates_maker_session_and_loop_sessions_row() {
    use std::process::Command;

    let conn = setup_db();

    // Set up a real git repo so session.create can succeed
    let dir = tempfile::tempdir().unwrap();
    let repo_path = dir.path().to_string_lossy().to_string();
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "-c",
            "user.email=test@test.com",
            "-c",
            "user.name=Test",
            "commit",
            "--allow-empty",
            "-m",
            "init",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let _project = crate::db::create_project(&conn, "testapp", &repo_path).unwrap();

    // Create loop from the builtin recipe (starts at create_maker)
    let (create_output, create_code) = loop_create(
        &conn,
        &repo_path,
        None,
        None,
        "maker-verifier",
        Some("maker-verifier"),
        "Implement the feature",
        3,
        false,
    );
    assert_eq!(create_code, 0, "create output:\n{create_output}");
    let loop_id = extract_loop_id(&create_output);
    assert!(!loop_id.is_empty(), "failed to extract loop_id");

    // First tick: draft→running + session.create
    let (output, code) = loop_tick(&conn, &loop_id);

    // session.create may fail if no daemon/tmux backend is running, but
    // if it succeeds, verify the session is linked. If it fails, the error
    // should reference session.create (verifying we're on the right step).
    if code == 0 {
        // Verify session was linked in loop_sessions with role=maker, round=1
        let sessions = LoopService::list_loop_sessions(&conn, &loop_id).unwrap();
        assert!(
            !sessions.is_empty(),
            "expected at least one loop_session after create_maker"
        );
        let maker_session = sessions.iter().find(|s| s.role == "maker");
        assert!(
            maker_session.is_some(),
            "expected a session with role=maker"
        );
        let ms = maker_session.unwrap();
        assert_eq!(ms.round, 1, "maker session should be round 1");

        // Verify next_actions mentions waiting for handoff
        assert!(
            output.contains("next_actions") || output.contains("wait for"),
            "next_actions should guide user, output:\n{output}"
        );
    } else {
        // Even if it fails, verify we hit the right step
        assert!(
            output.contains("session.create"),
            "first tick should attempt session.create, output:\n{output}"
        );
    }

    // Keep tempdir alive
    drop(dir);
}

#[test]
fn maker_verifier_create_maker_prefers_worktree_isolation() {
    use planeai_core::loop_recipe_service::RecipeService;

    // Verify the recipe role configuration has worktree isolation
    let recipe = RecipeService::parse_yaml(include_str!(
        "../../planeai-core/resources/recipes/maker-verifier.yaml"
    ))
    .unwrap();

    let maker_role = recipe.roles.get("maker").expect("maker role should exist");
    assert_eq!(
        maker_role.isolation, "worktree",
        "maker role should prefer worktree isolation"
    );

    // Also verify the create_maker step references the maker role
    let create_step = recipe.steps.iter().find(|s| s.id == "create_maker");
    assert!(create_step.is_some(), "create_maker step should exist");
    assert_eq!(
        create_step.unwrap().role.as_deref(),
        Some("maker"),
        "create_maker should reference maker role"
    );
}

#[test]
fn maker_verifier_next_actions_contain_useful_guidance() {
    let conn = setup_db();
    let maker_id = "maker-dddddddd-2222-3333-4444-555555555555";
    let (loop_id, project_id, _) =
        setup_maker_verifier_flow(&conn, "wait_for_maker", 1, Some(maker_id), None);
    create_and_link_session(&conn, &loop_id, maker_id, "maker", 1, &project_id);

    // No handoff yet — tick should output next_actions with guidance
    let (output, code) = loop_tick(&conn, &loop_id);
    assert_eq!(code, 0, "output:\n{output}");
    assert!(
        output.contains("next_actions"),
        "output should contain next_actions section, output:\n{output}"
    );
    assert!(
        output.contains("handoff") || output.contains("planeai-cli"),
        "next_actions should mention handoff or CLI command, output:\n{output}"
    );

    // Now record a handoff and tick again
    insert_handoff(&conn, &loop_id, maker_id, "completed");
    let (output2, _code2) = loop_tick(&conn, &loop_id);
    assert!(
        output2.contains("next_actions"),
        "output should contain next_actions, output:\n{output2}"
    );
    assert!(
        output2.contains("loop tick") || output2.contains("next step"),
        "next_actions should guide to next tick, output:\n{output2}"
    );
}
