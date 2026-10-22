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
        auto_approve: true,
        task_key: None,
        base_branch: Some("main".to_string()),
        pr_url: None,
        pr_state: None,
        attached_once: false,
        parent_session_id: Some("pppppppp-1111-2222-3333-444444444444".to_string()),
        task_project_id: None,
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
