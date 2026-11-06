use super::*;
use crate::model::*;

fn repo() -> SqliteRepository {
    SqliteRepository::open_in_memory("TEST").unwrap()
}

#[test]
fn create_and_get() {
    let r = repo();
    let task = r
        .create(CreateParams {
            title: "First task".into(),
            description: "desc".into(),
            priority: 1,
            tags: vec!["backend".into()],
            ..Default::default()
        })
        .unwrap();

    assert_eq!(task.key, "TEST-1");
    assert_eq!(task.status, Status::Todo);
    assert_eq!(task.tags, vec!["backend"]);

    let fetched = r.get("TEST-1").unwrap();
    assert_eq!(fetched.title, "First task");
    assert_eq!(fetched.priority, 1);
}

#[test]
fn sequential_keys() {
    let r = repo();
    let t1 = r
        .create(CreateParams {
            title: "a".into(),
            ..Default::default()
        })
        .unwrap();
    let t2 = r
        .create(CreateParams {
            title: "b".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(t1.key, "TEST-1");
    assert_eq!(t2.key, "TEST-2");
}

#[test]
fn get_not_found() {
    let r = repo();
    assert!(matches!(r.get("NOPE-1"), Err(Error::NotFound)));
}

#[test]
fn update_partial() {
    let r = repo();
    r.create(CreateParams {
        title: "original".into(),
        priority: 1,
        ..Default::default()
    })
    .unwrap();
    let updated = r
        .update(
            "TEST-1",
            UpdateParams {
                title: Some("renamed".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(updated.title, "renamed");
    assert_eq!(updated.priority, 1);
}

#[test]
fn update_status() {
    let r = repo();
    r.create(CreateParams {
        title: "task".into(),
        ..Default::default()
    })
    .unwrap();
    let updated = r
        .update(
            "TEST-1",
            UpdateParams {
                status: Some(Status::InProgress),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(updated.status, Status::InProgress);
}

#[test]
fn update_not_found() {
    let r = repo();
    assert!(matches!(
        r.update("NOPE-1", UpdateParams::default()),
        Err(Error::NotFound)
    ));
}

#[test]
fn delete_task() {
    let r = repo();
    r.create(CreateParams {
        title: "doomed".into(),
        ..Default::default()
    })
    .unwrap();
    r.delete("TEST-1").unwrap();
    assert!(matches!(r.get("TEST-1"), Err(Error::NotFound)));
}

#[test]
fn delete_not_found() {
    let r = repo();
    assert!(matches!(r.delete("NOPE-1"), Err(Error::NotFound)));
}

#[test]
fn list_all() {
    let r = repo();
    r.create(CreateParams {
        title: "a".into(),
        ..Default::default()
    })
    .unwrap();
    r.create(CreateParams {
        title: "b".into(),
        ..Default::default()
    })
    .unwrap();
    let tasks = r.list(ListFilter::default()).unwrap();
    assert_eq!(tasks.len(), 2);
}

#[test]
fn list_filter_by_status() {
    let r = repo();
    r.create(CreateParams {
        title: "a".into(),
        ..Default::default()
    })
    .unwrap();
    r.create(CreateParams {
        title: "b".into(),
        ..Default::default()
    })
    .unwrap();
    r.update(
        "TEST-1",
        UpdateParams {
            status: Some(Status::Done),
            ..Default::default()
        },
    )
    .unwrap();

    let todo = r
        .list(ListFilter {
            status: Some(Status::Todo),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(todo.len(), 1);
    assert_eq!(todo[0].key, "TEST-2");

    let not_done = r
        .list(ListFilter {
            exclude_status: Some(Status::Done),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(not_done.len(), 1);
}

#[test]
fn list_filter_by_tags() {
    let r = repo();
    r.create(CreateParams {
        title: "a".into(),
        tags: vec!["ui".into()],
        ..Default::default()
    })
    .unwrap();
    r.create(CreateParams {
        title: "b".into(),
        tags: vec!["backend".into()],
        ..Default::default()
    })
    .unwrap();

    let ui = r
        .list(ListFilter {
            tags: vec!["ui".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(ui.len(), 1);
    assert_eq!(ui[0].key, "TEST-1");
}

#[test]
fn list_priority_ordering() {
    let r = repo();
    r.create(CreateParams {
        title: "low".into(),
        priority: 0,
        ..Default::default()
    })
    .unwrap();
    r.create(CreateParams {
        title: "high".into(),
        priority: 1,
        ..Default::default()
    })
    .unwrap();
    r.create(CreateParams {
        title: "medium".into(),
        priority: 2,
        ..Default::default()
    })
    .unwrap();

    let tasks = r.list(ListFilter::default()).unwrap();
    assert_eq!(tasks[0].title, "high");
    assert_eq!(tasks[1].title, "medium");
    assert_eq!(tasks[2].title, "low");
}

#[test]
fn blockers_roundtrip() {
    let r = repo();
    r.create(CreateParams {
        title: "first".into(),
        ..Default::default()
    })
    .unwrap();
    r.create(CreateParams {
        title: "second".into(),
        blocked_by: vec!["TEST-1".into()],
        ..Default::default()
    })
    .unwrap();

    let t = r.get("TEST-2").unwrap();
    assert_eq!(t.blocked_by, vec!["TEST-1"]);

    r.update(
        "TEST-2",
        UpdateParams {
            blocked_by: Some(vec![]),
            ..Default::default()
        },
    )
    .unwrap();
    let t = r.get("TEST-2").unwrap();
    assert!(t.blocked_by.is_empty());
}

#[test]
fn parent_key() {
    let r = repo();
    r.create(CreateParams {
        title: "parent".into(),
        ..Default::default()
    })
    .unwrap();
    r.create(CreateParams {
        title: "child".into(),
        parent_key: Some("TEST-1".into()),
        ..Default::default()
    })
    .unwrap();

    let child = r.get("TEST-2").unwrap();
    assert_eq!(child.parent_key, Some("TEST-1".to_string()));

    let roots = r
        .list(ListFilter {
            parent_key: Some(None),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].key, "TEST-1");
}

#[test]
fn derive_prefix_from_name() {
    assert_eq!(derive_prefix("planeai"), "PLA");
    assert_eq!(derive_prefix("nomi"), "NOM");
    assert_eq!(derive_prefix("budget-buddy"), "BB");
    assert_eq!(derive_prefix("AB"), "AB");
}

#[test]
fn derive_prefix_unique_for_similar_names() {
    // These must produce different prefixes
    let a = derive_prefix("deployment-pipeline");
    let b = derive_prefix("deployment-pipeline-api");
    assert_ne!(
        a, b,
        "deployment-pipeline and deployment-pipeline-api must have different prefixes"
    );
    assert_eq!(a, "DP");
    assert_eq!(b, "DPA");
}

#[test]
fn new_with_existing_connection() {
    let conn = Connection::open_in_memory().unwrap();
    let repo = SqliteRepository::new(conn, "FOO").unwrap();
    let task = repo
        .create(CreateParams {
            title: "works".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(task.key, "FOO-1");
}

#[test]
fn base_branch_defaults_to_main() {
    let r = repo();
    let task = r
        .create(CreateParams {
            title: "task".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(task.base_branch, "main");
    let fetched = r.get("TEST-1").unwrap();
    assert_eq!(fetched.base_branch, "main");
}

#[test]
fn base_branch_custom_on_create() {
    let r = repo();
    let task = r
        .create(CreateParams {
            title: "task".into(),
            base_branch: "develop".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(task.base_branch, "develop");
    let fetched = r.get("TEST-1").unwrap();
    assert_eq!(fetched.base_branch, "develop");
}

#[test]
fn base_branch_update() {
    let r = repo();
    r.create(CreateParams {
        title: "task".into(),
        ..Default::default()
    })
    .unwrap();
    let updated = r
        .update(
            "TEST-1",
            UpdateParams {
                base_branch: Some("release/v2".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(updated.base_branch, "release/v2");
}

#[test]
fn base_branch_preserved_on_unrelated_update() {
    let r = repo();
    r.create(CreateParams {
        title: "task".into(),
        base_branch: "develop".into(),
        ..Default::default()
    })
    .unwrap();
    let updated = r
        .update(
            "TEST-1",
            UpdateParams {
                title: Some("renamed".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(updated.base_branch, "develop");
}

#[test]
fn base_branch_in_list() {
    let r = repo();
    r.create(CreateParams {
        title: "a".into(),
        base_branch: "develop".into(),
        ..Default::default()
    })
    .unwrap();
    r.create(CreateParams {
        title: "b".into(),
        ..Default::default()
    })
    .unwrap();
    let tasks = r.list(ListFilter::default()).unwrap();
    assert_eq!(tasks[0].base_branch, "develop");
    assert_eq!(tasks[1].base_branch, "main");
}

#[test]
fn create_with_custom_key() {
    let r = repo();
    let task = r
        .create(CreateParams {
            key: Some("PES-3206".into()),
            title: "Jira task".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(task.key, "PES-3206");
    let fetched = r.get("PES-3206").unwrap();
    assert_eq!(fetched.title, "Jira task");
}

#[test]
fn create_with_duplicate_key_is_idempotent() {
    let r = repo();
    let t1 = r
        .create(CreateParams {
            key: Some("PES-1".into()),
            title: "Original".into(),
            ..Default::default()
        })
        .unwrap();
    let t2 = r
        .create(CreateParams {
            key: Some("PES-1".into()),
            title: "Duplicate".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(t1.key, t2.key);
    assert_eq!(t2.title, "Original"); // returns existing, not new
}

#[test]
fn a_generated_key_skips_one_an_explicit_key_already_holds() {
    let r = repo();
    r.create(CreateParams {
        key: Some("TEST-2".into()),
        title: "Synced from a tracker".into(),
        ..Default::default()
    })
    .unwrap();
    let keys: Vec<String> = (0..2)
        .map(|_| {
            r.create(CreateParams {
                title: "auto".into(),
                ..Default::default()
            })
            .unwrap()
            .key
        })
        .collect();
    assert_eq!(keys, ["TEST-1", "TEST-3"]);
    assert_eq!(r.get("TEST-2").unwrap().title, "Synced from a tracker");
}

#[test]
fn create_without_key_still_auto_generates() {
    let r = repo();
    let t1 = r
        .create(CreateParams {
            title: "auto".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(t1.key, "TEST-1");
}

#[test]
fn parent_auto_complete_when_all_children_done() {
    let r = repo();
    r.create(CreateParams {
        title: "parent".into(),
        ..Default::default()
    })
    .unwrap();
    r.create(CreateParams {
        title: "child1".into(),
        parent_key: Some("TEST-1".into()),
        ..Default::default()
    })
    .unwrap();
    r.create(CreateParams {
        title: "child2".into(),
        parent_key: Some("TEST-1".into()),
        ..Default::default()
    })
    .unwrap();

    // Only one child done — should not auto-complete
    r.update(
        "TEST-2",
        UpdateParams {
            status: Some(Status::Done),
            ..Default::default()
        },
    )
    .unwrap();
    let child1 = r.get("TEST-2").unwrap();
    assert_eq!(crate::try_auto_complete_parent(&r, &child1), None);
    assert_ne!(r.get("TEST-1").unwrap().status, Status::Done);

    // All children done — should auto-complete parent
    let child2 = r
        .update(
            "TEST-3",
            UpdateParams {
                status: Some(Status::Done),
                ..Default::default()
            },
        )
        .unwrap();
    let result = crate::try_auto_complete_parent(&r, &child2);
    assert_eq!(result, Some("TEST-1".to_string()));
    assert_eq!(r.get("TEST-1").unwrap().status, Status::Done);
}

#[test]
fn create_with_first_child_assignment_reports_only_the_first_child() {
    let r = repo();
    let parent = r
        .create(CreateParams {
            title: "parent".into(),
            ..Default::default()
        })
        .unwrap();
    let (_, first) = r
        .create_with_first_child_assignment(CreateParams {
            title: "first".into(),
            parent_key: Some(parent.key.clone()),
            ..Default::default()
        })
        .unwrap();
    let (_, second) = r
        .create_with_first_child_assignment(CreateParams {
            title: "second".into(),
            parent_key: Some(parent.key),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(first, Some(true));
    assert_eq!(second, Some(false));
}

#[test]
fn set_parent_with_first_child_assignment_reports_only_the_first_assignment() {
    let r = repo();
    let parent = r
        .create(CreateParams {
            title: "parent".into(),
            ..Default::default()
        })
        .unwrap();
    let first = r
        .create(CreateParams {
            title: "first".into(),
            ..Default::default()
        })
        .unwrap();
    let second = r
        .create(CreateParams {
            title: "second".into(),
            ..Default::default()
        })
        .unwrap();
    let (_, first_assignment) = r
        .set_parent_with_first_child_assignment(&first.key, Some(parent.key.clone()))
        .unwrap();
    let (_, second_assignment) = r
        .set_parent_with_first_child_assignment(&second.key, Some(parent.key))
        .unwrap();
    assert_eq!(first_assignment, Some(true));
    assert_eq!(second_assignment, Some(false));
}

#[test]
fn insert_task_commits_and_rolls_back_with_the_callers_transaction() {
    let mut conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    let params = CreateParams {
        title: "Retro".into(),
        tags: vec!["weekly".into()],
        ..Default::default()
    };
    let count = |conn: &Connection| -> i64 {
        conn.query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
            .unwrap()
    };

    let tx = conn.transaction().unwrap();
    let rolled_back = insert_task(&tx, "OPS", &params).unwrap();
    drop(tx);
    assert_eq!(rolled_back.key, "OPS-1");
    assert_eq!(count(&conn), 0, "a dropped transaction leaves no task");

    let tx = conn.transaction().unwrap();
    let created = insert_task(&tx, "OPS", &params).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        created,
        InsertedTask {
            key: "OPS-1".into(),
            inserted: true,
            first_child: None,
        }
    );
    let row: (String, String) = conn
        .query_row(
            "SELECT base_branch, (SELECT tag FROM task_tags WHERE task_key = 'OPS-1') FROM tasks WHERE key = 'OPS-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, ("main".to_string(), "weekly".to_string()));
}

fn at(rfc3339: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .unwrap()
        .with_timezone(&Utc)
}

/// A repository and a second connection to the same file, as the app shares its DB.
fn shared_db() -> (tempfile::TempDir, SqliteRepository, Connection) {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("tasks.db");
    let repo = SqliteRepository::open(path.to_str().unwrap(), "TEST").unwrap();
    repo.create(CreateParams {
        title: "watched".into(),
        ..Default::default()
    })
    .unwrap();
    let conn = Connection::open(path).unwrap();
    (directory, repo, conn)
}

#[test]
fn migration_adds_visit_columns_to_an_existing_database() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE task_schema_version (version INTEGER NOT NULL);
         INSERT INTO task_schema_version (version) VALUES (2);
         CREATE TABLE task_projects (prefix TEXT PRIMARY KEY, next_seq INTEGER NOT NULL DEFAULT 1);
         INSERT INTO task_projects (prefix, next_seq) VALUES ('OLD', 2);
         CREATE TABLE tasks (
            key TEXT PRIMARY KEY,
            project_prefix TEXT NOT NULL REFERENCES task_projects(prefix),
            title TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'todo',
            priority INTEGER NOT NULL DEFAULT 0,
            parent_key TEXT REFERENCES tasks(key) ON DELETE CASCADE,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            base_branch TEXT
         );
         CREATE TABLE task_blockers (task_key TEXT NOT NULL, blocked_by_key TEXT NOT NULL);
         CREATE TABLE task_tags (task_key TEXT NOT NULL, tag TEXT NOT NULL);
         INSERT INTO tasks (key, project_prefix, title, created_at, updated_at)
            VALUES ('OLD-1', 'OLD', 'from before', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00');",
    )
    .unwrap();

    let repo = SqliteRepository::new(conn, "OLD").unwrap();
    let task = repo.get("OLD-1").unwrap();

    assert_eq!(task.title, "from before");
    assert_eq!(task.last_visited_at, None);
    assert_eq!(task.last_activity_at, None);
}

#[test]
fn a_new_task_has_neither_visit_nor_activity() {
    let task = repo()
        .create(CreateParams {
            title: "fresh".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(task.last_visited_at, None);
    assert_eq!(task.last_activity_at, None);
}

#[test]
fn recorded_activity_is_read_back_to_the_millisecond() {
    let (_directory, repo, conn) = shared_db();

    let visit =
        record_activity(&conn, "TEST", "TEST-1", at("2026-11-03T10:00:00.123456Z")).unwrap();

    let expected = Visit {
        last_visited_at: None,
        last_activity_at: Some(at("2026-11-03T10:00:00.123Z")),
    };
    assert_eq!(visit, expected);
    let task = repo.get("TEST-1").unwrap();
    assert_eq!(
        (task.last_visited_at, task.last_activity_at),
        (None, Some(at("2026-11-03T10:00:00.123Z")))
    );
}

#[test]
fn marking_visited_stamps_the_visit() {
    let (_directory, repo, conn) = shared_db();
    record_activity(&conn, "TEST", "TEST-1", at("2026-11-03T10:00:00Z")).unwrap();

    let visit = mark_visited(&conn, "TEST", "TEST-1", at("2026-11-03T10:05:00Z")).unwrap();

    assert_eq!(
        visit,
        Visit {
            last_visited_at: Some(at("2026-11-03T10:05:00Z")),
            last_activity_at: Some(at("2026-11-03T10:00:00Z")),
        }
    );
    assert_eq!(
        repo.get("TEST-1").unwrap().last_visited_at,
        Some(at("2026-11-03T10:05:00Z"))
    );
}

#[test]
fn marking_unread_puts_the_visit_just_before_the_activity() {
    let (_directory, repo, conn) = shared_db();
    record_activity(&conn, "TEST", "TEST-1", at("2026-11-03T10:00:00.500Z")).unwrap();
    mark_visited(&conn, "TEST", "TEST-1", at("2026-11-03T10:05:00Z")).unwrap();

    let visit = mark_unread(&conn, "TEST", "TEST-1").unwrap();

    assert_eq!(
        visit,
        Visit {
            last_visited_at: Some(at("2026-11-03T10:00:00.499Z")),
            last_activity_at: Some(at("2026-11-03T10:00:00.500Z")),
        }
    );
    assert_eq!(
        repo.get("TEST-1").unwrap().last_visited_at,
        Some(at("2026-11-03T10:00:00.499Z"))
    );
}

#[test]
fn marking_unread_without_activity_changes_nothing() {
    let (_directory, _repo, conn) = shared_db();
    mark_visited(&conn, "TEST", "TEST-1", at("2026-11-03T10:05:00Z")).unwrap();

    let visit = mark_unread(&conn, "TEST", "TEST-1").unwrap();

    assert_eq!(
        visit,
        Visit {
            last_visited_at: Some(at("2026-11-03T10:05:00Z")),
            last_activity_at: None,
        }
    );
}

#[test]
fn visit_writes_only_reach_a_task_of_the_given_project() {
    let (_directory, repo, conn) = shared_db();

    let stamped = record_activity(&conn, "OTHER", "TEST-1", at("2026-11-03T10:00:00Z"));

    assert!(matches!(stamped, Err(Error::NotFound)), "{stamped:?}");
    assert!(matches!(
        mark_visited(&conn, "TEST", "TEST-404", at("2026-11-03T10:00:00Z")),
        Err(Error::NotFound)
    ));
    assert_eq!(repo.get("TEST-1").unwrap().last_activity_at, None);
}

#[test]
fn editing_a_task_keeps_its_visit_and_activity() {
    let (_directory, repo, conn) = shared_db();
    record_activity(&conn, "TEST", "TEST-1", at("2026-11-03T10:00:00Z")).unwrap();
    mark_visited(&conn, "TEST", "TEST-1", at("2026-11-03T10:05:00Z")).unwrap();

    let task = repo
        .update(
            "TEST-1",
            UpdateParams {
                title: Some("renamed".into()),
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(task.title, "renamed");
    assert_eq!(
        (task.last_visited_at, task.last_activity_at),
        (
            Some(at("2026-11-03T10:05:00Z")),
            Some(at("2026-11-03T10:00:00Z"))
        )
    );
}

#[test]
fn migration_adds_snooze_columns_to_a_database_with_visits() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE task_schema_version (version INTEGER NOT NULL);
         INSERT INTO task_schema_version (version) VALUES (3);
         CREATE TABLE task_projects (prefix TEXT PRIMARY KEY, next_seq INTEGER NOT NULL DEFAULT 1);
         INSERT INTO task_projects (prefix, next_seq) VALUES ('OLD', 2);
         CREATE TABLE tasks (
            key TEXT PRIMARY KEY,
            project_prefix TEXT NOT NULL REFERENCES task_projects(prefix),
            title TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'todo',
            priority INTEGER NOT NULL DEFAULT 0,
            parent_key TEXT REFERENCES tasks(key) ON DELETE CASCADE,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            base_branch TEXT,
            last_visited_at TEXT,
            last_activity_at TEXT
         );
         CREATE TABLE task_blockers (task_key TEXT NOT NULL, blocked_by_key TEXT NOT NULL);
         CREATE TABLE task_tags (task_key TEXT NOT NULL, tag TEXT NOT NULL);
         INSERT INTO tasks (key, project_prefix, title, created_at, updated_at, last_visited_at)
            VALUES ('OLD-1', 'OLD', 'from before', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '2026-11-03T10:00:00.000+00:00');",
    )
    .unwrap();

    let repo = SqliteRepository::new(conn, "OLD").unwrap();
    let task = repo.get("OLD-1").unwrap();

    assert_eq!(task.last_visited_at, Some(at("2026-11-03T10:00:00Z")));
    assert_eq!(task.snooze.times, Snooze::default());
}

#[test]
fn migration_starts_the_snooze_revision_of_an_existing_snooze_at_zero() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE task_schema_version (version INTEGER NOT NULL);
         INSERT INTO task_schema_version (version) VALUES (4);
         CREATE TABLE task_projects (prefix TEXT PRIMARY KEY, next_seq INTEGER NOT NULL DEFAULT 1);
         INSERT INTO task_projects (prefix, next_seq) VALUES ('OLD', 2);
         CREATE TABLE tasks (
            key TEXT PRIMARY KEY,
            project_prefix TEXT NOT NULL REFERENCES task_projects(prefix),
            title TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'todo',
            priority INTEGER NOT NULL DEFAULT 0,
            parent_key TEXT REFERENCES tasks(key) ON DELETE CASCADE,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            base_branch TEXT,
            last_visited_at TEXT,
            last_activity_at TEXT,
            snoozed_until TEXT,
            snoozed_at TEXT,
            woke_at TEXT
         );
         CREATE TABLE task_blockers (task_key TEXT NOT NULL, blocked_by_key TEXT NOT NULL);
         CREATE TABLE task_tags (task_key TEXT NOT NULL, tag TEXT NOT NULL);
         INSERT INTO tasks (key, project_prefix, title, created_at, updated_at, snoozed_until, snoozed_at)
            VALUES ('OLD-1', 'OLD', 'from before', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '2026-11-04T09:00:00.000+00:00', '2026-11-03T10:00:00.000+00:00');",
    )
    .unwrap();

    let repo = SqliteRepository::new(conn, "OLD").unwrap();
    let task = repo.get("OLD-1").unwrap();

    assert_eq!(
        (task.snooze.times, task.snooze.snooze_rev),
        (
            Snooze {
                snoozed_until: Some(at("2026-11-04T09:00:00Z")),
                snoozed_at: Some(at("2026-11-03T10:00:00Z")),
                woke_at: None,
            },
            0
        )
    );
}

fn stored_snooze_of(repo: &SqliteRepository) -> StoredSnooze {
    let task = repo.get("TEST-1").unwrap();
    task.snooze
}

fn snooze_tomorrow(conn: &Connection) {
    snooze(
        conn,
        "TEST",
        "TEST-1",
        at("2026-11-04T09:00:00Z"),
        at("2026-11-03T10:00:00Z"),
    )
    .unwrap();
}

fn tomorrow() -> Snooze {
    Snooze {
        snoozed_until: Some(at("2026-11-04T09:00:00Z")),
        snoozed_at: Some(at("2026-11-03T10:00:00Z")),
        woke_at: None,
    }
}

#[test]
fn update_that_fails_midway_leaves_the_task_unchanged() {
    let r = repo();
    for title in ["blocker", "other"] {
        r.create(CreateParams {
            title: title.into(),
            ..Default::default()
        })
        .unwrap();
    }
    let task = r
        .create(CreateParams {
            title: "Original".into(),
            tags: vec!["keep".into()],
            blocked_by: vec!["TEST-1".into()],
            ..Default::default()
        })
        .unwrap();
    r.conn
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER refuse_tag BEFORE INSERT ON task_tags WHEN NEW.tag = 'refused'
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();

    let failed = r.update(
        &task.key,
        UpdateParams {
            title: Some("Renamed".into()),
            blocked_by: Some(vec!["TEST-2".into()]),
            tags: Some(vec!["new".into(), "refused".into()]),
            ..Default::default()
        },
    );

    assert!(matches!(failed, Err(Error::Storage(_))));
    let after = r.get(&task.key).unwrap();
    assert_eq!(
        (after.title.as_str(), after.tags, after.blocked_by),
        (
            "Original",
            vec!["keep".to_string()],
            vec!["TEST-1".to_string()]
        )
    );
}

#[test]
fn update_drops_repeated_tags_and_blockers() {
    let r = repo();
    for title in ["first", "second", "task"] {
        r.create(CreateParams {
            title: title.into(),
            ..Default::default()
        })
        .unwrap();
    }

    let task = r
        .update(
            "TEST-3",
            UpdateParams {
                tags: Some(vec!["b".into(), "a".into(), "b".into()]),
                blocked_by: Some(vec!["TEST-1".into(), "TEST-2".into(), "TEST-1".into()]),
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(
        (task.tags, task.blocked_by),
        (
            vec!["a".to_string(), "b".to_string()],
            vec!["TEST-1".to_string(), "TEST-2".to_string()]
        )
    );
}

#[test]
fn a_new_task_starts_at_snooze_revision_zero() {
    let (_directory, repo, _conn) = shared_db();

    assert_eq!(stored_snooze_of(&repo), StoredSnooze::default());
}

#[test]
fn snoozing_records_when_and_until_and_bumps_the_revision() {
    let (_directory, repo, conn) = shared_db();

    let snooze = snooze(
        &conn,
        "TEST",
        "TEST-1",
        at("2026-11-04T09:00:00Z"),
        at("2026-11-03T10:00:00.123456Z"),
    )
    .unwrap();

    let expected = StoredSnooze {
        times: Snooze {
            snoozed_until: Some(at("2026-11-04T09:00:00Z")),
            snoozed_at: Some(at("2026-11-03T10:00:00.123Z")),
            woke_at: None,
        },
        snooze_rev: 1,
    };
    assert_eq!(snooze, expected);
    assert_eq!(stored_snooze_of(&repo), expected);
    assert_eq!(repo.get("TEST-1").unwrap().status, Status::Todo);
}

#[test]
fn snoozing_into_the_past_or_a_done_task_is_rejected() {
    let (_directory, repo, conn) = shared_db();

    let past = snooze(
        &conn,
        "TEST",
        "TEST-1",
        at("2026-11-03T10:00:00Z"),
        at("2026-11-03T10:00:00Z"),
    );
    repo.update(
        "TEST-1",
        UpdateParams {
            status: Some(Status::Done),
            ..Default::default()
        },
    )
    .unwrap();
    let done = snooze(
        &conn,
        "TEST",
        "TEST-1",
        at("2026-11-04T09:00:00Z"),
        at("2026-11-03T10:00:00Z"),
    );

    assert_eq!(past.unwrap_err().to_string(), "the snooze time has passed");
    assert_eq!(
        done.unwrap_err().to_string(),
        "a done task cannot be snoozed"
    );
    assert_eq!(repo.get("TEST-1").unwrap().snooze.times, Snooze::default());
}

#[test]
fn a_snooze_past_the_year_9999_is_rejected() {
    let (_directory, repo, conn) = shared_db();
    let far = at("9999-12-31T23:59:59Z") + chrono::Duration::seconds(1);

    let snoozed = snooze(&conn, "TEST", "TEST-1", far, at("2026-11-03T10:00:00Z"));
    let restored = restore_snooze(
        &conn,
        "TEST",
        "TEST-1",
        Snooze {
            snoozed_until: Some(far),
            ..tomorrow()
        },
    );

    assert_eq!(
        snoozed.unwrap_err().to_string(),
        "the snooze time is too far ahead"
    );
    assert_eq!(
        restored.unwrap_err().to_string(),
        "the snooze time is too far ahead"
    );
    assert_eq!(stored_snooze_of(&repo), StoredSnooze::default());
}

#[test]
fn a_snooze_in_the_year_9999_is_kept() {
    let (_directory, repo, conn) = shared_db();

    snooze(
        &conn,
        "TEST",
        "TEST-1",
        at("9999-12-31T23:59:59Z"),
        at("2026-11-03T10:00:00Z"),
    )
    .unwrap();

    assert_eq!(
        repo.get("TEST-1").unwrap().snooze.times.snoozed_until,
        Some(at("9999-12-31T23:59:59Z"))
    );
}

#[test]
fn waking_by_hand_clears_the_snooze_and_bumps_the_revision() {
    let (_directory, repo, conn) = shared_db();
    snooze_tomorrow(&conn);

    let expected = StoredSnooze {
        times: Snooze::default(),
        snooze_rev: 2,
    };
    assert_eq!(wake(&conn, "TEST", "TEST-1").unwrap(), expected);
    assert_eq!(stored_snooze_of(&repo), expected);
}

#[test]
fn restoring_puts_back_the_exact_snooze_and_bumps_the_revision() {
    let (_directory, repo, conn) = shared_db();
    snooze_tomorrow(&conn);
    let previous = Snooze {
        woke_at: Some(at("2026-11-03T11:00:00Z")),
        ..tomorrow()
    };

    let expected = StoredSnooze {
        times: previous,
        snooze_rev: 2,
    };
    assert_eq!(
        restore_snooze(&conn, "TEST", "TEST-1", previous).unwrap(),
        expected
    );
    assert_eq!(stored_snooze_of(&repo), expected);
}

#[test]
fn moving_to_done_ends_the_snooze_and_bumps_the_revision_and_other_moves_keep_both() {
    let (_directory, repo, conn) = shared_db();
    snooze_tomorrow(&conn);
    let move_to = |status| {
        let task = repo
            .update(
                "TEST-1",
                UpdateParams {
                    status: Some(status),
                    ..Default::default()
                },
            )
            .unwrap();
        (task.snooze.times, task.snooze.snooze_rev)
    };

    assert_eq!(move_to(Status::InReview), (tomorrow(), 1));
    assert_eq!(move_to(Status::Done), (Snooze::default(), 2));
}

#[test]
fn an_agent_wakes_a_snoozed_task_early_only_while_it_sleeps() {
    let (_directory, repo, conn) = shared_db();
    snooze_tomorrow(&conn);

    let before_snooze = wake_early(&conn, "TEST", "TEST-1", at("2026-11-03T09:59:59Z")).unwrap();
    let after_wake_time = wake_early(&conn, "TEST", "TEST-1", at("2026-11-04T09:00:00Z")).unwrap();
    let woke = wake_early(&conn, "TEST", "TEST-1", at("2026-11-03T12:00:00Z")).unwrap();
    let again = wake_early(&conn, "TEST", "TEST-1", at("2026-11-03T13:00:00Z")).unwrap();

    assert_eq!((before_snooze, after_wake_time), (None, None));
    let expected = StoredSnooze {
        times: Snooze {
            woke_at: Some(at("2026-11-03T12:00:00Z")),
            ..tomorrow()
        },
        snooze_rev: 2,
    };
    assert_eq!(woke, Some(expected));
    assert_eq!(again, None);
    assert_eq!(stored_snooze_of(&repo), expected);
}

#[test]
fn a_task_never_snoozed_does_not_wake() {
    let (_directory, repo, conn) = shared_db();

    let woke = wake_early(&conn, "TEST", "TEST-1", at("2026-11-03T12:00:00Z")).unwrap();

    assert_eq!(woke, None);
    assert_eq!(stored_snooze_of(&repo), StoredSnooze::default());
}

#[test]
fn a_task_is_snoozed_until_the_instant_it_wakes() {
    let (_directory, repo, conn) = shared_db();
    snooze(
        &conn,
        "TEST",
        "TEST-1",
        at("2026-11-04T09:00:00Z"),
        at("2026-11-03T10:00:00Z"),
    )
    .unwrap();
    let task = repo.get("TEST-1").unwrap();

    assert!(task.is_snoozed(at("2026-11-04T08:59:59.999Z")));
    assert!(!task.is_snoozed(at("2026-11-04T09:00:00Z")));
}

#[test]
fn create_drops_repeated_tags_and_blockers() {
    let r = repo();
    for title in ["first", "second"] {
        r.create(CreateParams {
            title: title.into(),
            ..Default::default()
        })
        .unwrap();
    }

    let params = || CreateParams {
        title: "task".into(),
        tags: vec!["b".into(), "a".into(), "b".into()],
        blocked_by: vec!["TEST-1".into(), "TEST-2".into(), "TEST-1".into()],
        ..Default::default()
    };
    let created = [
        r.create(params()).unwrap(),
        r.create_with_first_child_assignment(params()).unwrap().0,
    ];

    for task in created {
        assert_eq!(
            (task.tags, task.blocked_by),
            (
                vec!["a".to_string(), "b".to_string()],
                vec!["TEST-1".to_string(), "TEST-2".to_string()]
            )
        );
    }
}

#[test]
fn a_task_cannot_become_its_own_ancestor() {
    let r = repo();
    for title in ["root", "child", "grandchild"] {
        r.create(CreateParams {
            title: title.into(),
            ..Default::default()
        })
        .unwrap();
    }
    r.set_parent_with_first_child_assignment("TEST-2", Some("TEST-1".into()))
        .unwrap();
    r.set_parent_with_first_child_assignment("TEST-3", Some("TEST-2".into()))
        .unwrap();

    let refusals = [
        r.set_parent_with_first_child_assignment("TEST-1", Some("TEST-1".into()))
            .unwrap_err()
            .to_string(),
        r.set_parent_with_first_child_assignment("TEST-1", Some("TEST-3".into()))
            .unwrap_err()
            .to_string(),
        r.set_parent_with_first_child_assignment("TEST-1", Some("TEST-9".into()))
            .unwrap_err()
            .to_string(),
        r.update(
            "TEST-1",
            UpdateParams {
                parent_key: Some(Some("TEST-2".into())),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string(),
    ];

    assert_eq!(
        refusals,
        [
            "a task cannot be its own parent",
            "TEST-3 is a subtask of TEST-1, so it cannot be its parent",
            "parent task not found: TEST-9",
            "TEST-2 is a subtask of TEST-1, so it cannot be its parent",
        ]
    );
    assert_eq!(r.get("TEST-1").unwrap().parent_key, None);
}
