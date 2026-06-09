use planeai_core::dispatch::TaskDispatcher;
use planeai_core::task::TaskManagerConfig;
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;

/// Helper: create a shell script that prints the given string to stdout.
fn write_script(dir: &std::path::Path, name: &str, output: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\nprintf '%s' '{output}'")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

#[tokio::test]
async fn filters_blocked_and_claimed_tasks_returns_sorted_eligible() {
    let dir = tempdir().unwrap();

    // list_tasks returns 4 tasks:
    // - KAN-1: priority 2, unblocked
    // - KAN-2: priority 1, blocked by KAN-5 (KAN-5 is not in list, will need get_task)
    // - KAN-3: priority 3, unblocked (but will be claimed)
    // - KAN-4: priority 1, blocked by KAN-1 (KAN-1 is in list with status "todo" = non-terminal)
    let list_json = r#"[
        {"key":"KAN-1","title":"First task","status":"todo","description":"","priority":2,"blocked_by":[]},
        {"key":"KAN-2","title":"Second task","status":"todo","description":"","priority":1,"blocked_by":["KAN-5"]},
        {"key":"KAN-3","title":"Third task","status":"todo","description":"","priority":3,"blocked_by":[]},
        {"key":"KAN-4","title":"Fourth task","status":"todo","description":"","priority":1,"blocked_by":["KAN-1"]}
    ]"#;

    // get_task for KAN-5 returns a terminal state (done) → blocker resolved
    let get_task_json = r#"{"key":"KAN-5","title":"Done task","status":"done","description":"","priority":1,"blocked_by":[]}"#;

    let list_script = write_script(dir.path(), "list.sh", list_json);
    let get_script = write_script(dir.path(), "get.sh", get_task_json);

    let config = TaskManagerConfig {
        list_tasks: format!("{list_script} --project {{project}}"),
        get_task: format!("{get_script} {{key}}"),
        move_task: String::new(),
        terminal_states: vec!["done".to_string(), "cancelled".to_string()],
        on_start: None,
    };

    let dispatcher = TaskDispatcher::new(&config, "myproject", dir.path());

    // KAN-3 is already claimed
    let claimed: HashSet<String> = HashSet::from(["KAN-3".to_string()]);

    let tasks = dispatcher.fetch_dispatchable_tasks(&claimed).await.unwrap();

    // Expected results:
    // - KAN-1: eligible (unblocked, not claimed, priority 2)
    // - KAN-2: eligible (blocked by KAN-5, but KAN-5 is "done" = terminal → resolved, priority 1)
    // - KAN-3: filtered out (claimed)
    // - KAN-4: filtered out (blocked by KAN-1 which is "todo" = non-terminal)
    //
    // Sorted by priority ascending: KAN-2 (1), KAN-1 (2)
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].key, "KAN-2");
    assert_eq!(tasks[1].key, "KAN-1");
}
