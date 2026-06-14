use planeai_core::dispatch::TaskDispatcher;
use planeai_core::task::{Task, TaskSource};
use std::collections::HashSet;
use std::sync::Arc;

struct MockTaskSource {
    tasks: Vec<Task>,
    terminal_states: Vec<String>,
}

impl TaskSource for MockTaskSource {
    fn list_tasks(&self) -> Result<Vec<Task>, String> {
        Ok(self.tasks.clone())
    }
    fn get_task(&self, key: &str) -> Result<Task, String> {
        self.tasks
            .iter()
            .find(|t| t.key == key)
            .cloned()
            .ok_or_else(|| "not found".to_string())
    }
    fn move_task(&self, _key: &str, _status: &str) -> Result<(), String> {
        Ok(())
    }
    fn is_terminal(&self, status: &str) -> bool {
        self.terminal_states
            .iter()
            .any(|s| s.eq_ignore_ascii_case(status))
    }
}

#[tokio::test]
async fn filters_blocked_and_claimed_tasks_returns_sorted_eligible() {
    // KAN-5 is terminal (done) → blocker resolved for KAN-2
    let source = Arc::new(MockTaskSource {
        tasks: vec![
            Task {
                key: "KAN-1".into(),
                title: "First task".into(),
                status: "todo".into(),
                priority: 2,
                blocked_by: vec![],
                subtasks: vec![],
                ..Default::default()
            },
            Task {
                key: "KAN-2".into(),
                title: "Second task".into(),
                status: "todo".into(),
                priority: 1,
                blocked_by: vec!["KAN-5".into()],
                subtasks: vec![],
                ..Default::default()
            },
            Task {
                key: "KAN-3".into(),
                title: "Third task".into(),
                status: "todo".into(),
                priority: 3,
                blocked_by: vec![],
                subtasks: vec![],
                ..Default::default()
            },
            Task {
                key: "KAN-4".into(),
                title: "Fourth task".into(),
                status: "todo".into(),
                priority: 1,
                blocked_by: vec!["KAN-1".into()],
                subtasks: vec![],
                ..Default::default()
            },
            Task {
                key: "KAN-5".into(),
                title: "Done task".into(),
                status: "done".into(),
                priority: 1,
                blocked_by: vec![],
                subtasks: vec![],
                ..Default::default()
            },
        ],
        terminal_states: vec!["done".into(), "cancelled".into()],
    });

    let dispatcher = TaskDispatcher::new(source);
    // KAN-3 is already claimed
    let claimed: HashSet<String> = HashSet::from(["KAN-3".to_string()]);

    let tasks = dispatcher.fetch_dispatchable_tasks(&claimed).await.unwrap();

    // KAN-1: eligible (unblocked, not claimed, priority 2)
    // KAN-2: eligible (blocked by KAN-5, but KAN-5 is "done" = terminal → resolved, priority 1)
    // KAN-3: filtered out (claimed)
    // KAN-4: filtered out (blocked by KAN-1 which is "todo" = non-terminal)
    // KAN-5: filtered out (terminal)
    // Sorted by priority ascending: KAN-2 (1), KAN-1 (2)
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].key, "KAN-2");
    assert_eq!(tasks[1].key, "KAN-1");
}

#[tokio::test]
async fn skips_parent_tasks_with_subtasks() {
    let source = Arc::new(MockTaskSource {
        tasks: vec![
            Task {
                key: "KAN-1".into(),
                title: "Parent task".into(),
                status: "todo".into(),
                priority: 1,
                blocked_by: vec![],
                subtasks: vec!["KAN-2".into(), "KAN-3".into()],
                ..Default::default()
            },
            Task {
                key: "KAN-2".into(),
                title: "Child one".into(),
                status: "todo".into(),
                priority: 2,
                blocked_by: vec![],
                subtasks: vec![],
                ..Default::default()
            },
            Task {
                key: "KAN-3".into(),
                title: "Child two".into(),
                status: "todo".into(),
                priority: 3,
                blocked_by: vec![],
                subtasks: vec![],
                ..Default::default()
            },
        ],
        terminal_states: vec!["done".into()],
    });

    let dispatcher = TaskDispatcher::new(source);
    let tasks = dispatcher
        .fetch_dispatchable_tasks(&HashSet::new())
        .await
        .unwrap();

    // KAN-1 is a parent (has subtasks) → should be skipped
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].key, "KAN-2");
    assert_eq!(tasks[1].key, "KAN-3");
}
