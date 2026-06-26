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
