//! Activity on the task an agent works on: a finished turn, a request for attention, an exit.

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Manager};

use planeai_tasks::model::Snooze;
use planeai_tasks::provider::Error as TaskError;
use planeai_tasks::sqlite::Visit;

use crate::db;

/// Emitted with a `TaskActivity` once a task's activity is recorded.
pub const EVENT: &str = "task-activity";

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TaskActivity {
    pub project_id: String,
    pub task_key: String,
    #[serde(flatten)]
    pub visit: Visit,
    #[serde(flatten)]
    pub snooze: Snooze,
}

/// What an agent did; only the ones that need the user wake a snoozed task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// A turn ended on its own: hook Stop, a silence timeout, provider Idle.
    Finished,
    /// The agent waits on the user: hook Notification, provider NeedsAttention.
    NeedsAttention,
    Exited,
}

impl Activity {
    fn wakes(self) -> bool {
        !matches!(self, Self::Finished)
    }
}

/// The project's task prefix, which scopes every visit write to that project.
pub fn project_prefix(conn: &Connection, project_id: &str) -> Result<String, String> {
    db::get_project(conn, project_id)
        .map_err(|e| e.to_string())?
        .map(|project| project.prefix)
        .ok_or_else(|| format!("project not found: {project_id}"))
}

/// Records activity on the session's task, waking it early if snoozed and the agent needs the user;
/// `None` when the session has no task that still exists.
pub fn stamp(
    conn: &Connection,
    session_id: &str,
    activity: Activity,
    at: DateTime<Utc>,
) -> Result<Option<TaskActivity>, String> {
    let Some(session) = db::get_session(conn, session_id).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    // Done archives the task's sessions while their notify state lives on, so a turn cut short by the
    // archive still settles afterwards; it is not news on a finished task.
    if session.status == "archived" {
        return Ok(None);
    }
    let Some(task_key) = session.task_key.clone() else {
        return Ok(None);
    };
    let project_id = session.task_project_id().to_string();
    let Ok(prefix) = project_prefix(conn, &project_id) else {
        return Ok(None);
    };
    let recorded =
        planeai_tasks::sqlite::record_activity(conn, &prefix, &task_key, at).and_then(|visit| {
            if activity.wakes() {
                planeai_tasks::sqlite::wake_early(conn, &prefix, &task_key, at)?;
            }
            Ok((
                visit,
                planeai_tasks::sqlite::read_snooze(conn, &prefix, &task_key)?,
            ))
        });
    match recorded {
        Ok((visit, snooze)) => Ok(Some(TaskActivity {
            project_id,
            task_key,
            visit,
            snooze,
        })),
        Err(TaskError::NotFound) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// Records activity on the session's task off the calling thread and tells the frontend.
pub fn record(app: &AppHandle, session_id: &str, activity: Activity) {
    let app = app.clone();
    let session_id = session_id.to_string();
    tauri::async_runtime::spawn(async move {
        let db = app.state::<crate::state::DbState>().0.clone();
        let id = session_id.clone();
        let stamped = crate::commands::blocking(move || {
            let conn = db.lock().map_err(|e| e.to_string())?;
            stamp(&conn, &id, activity, Utc::now())
        })
        .await;
        match stamped {
            Ok(Some(activity)) => {
                let _ = app.emit(EVENT, activity);
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(%session_id, %error, "failed to record task activity"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use planeai_tasks::model::CreateParams;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        conn
    }

    fn add_task(conn: &mut Connection, prefix: &str) -> String {
        let tx = conn.transaction().unwrap();
        let task = planeai_tasks::sqlite::insert_task(
            &tx,
            prefix,
            &CreateParams {
                title: "watched".into(),
                ..Default::default()
            },
        )
        .unwrap();
        tx.commit().unwrap();
        task.key
    }

    fn add_session(
        conn: &Connection,
        id: &str,
        project_id: &str,
        task_key: Option<&str>,
        task_project_id: Option<&str>,
    ) {
        db::create_session_with_params(
            conn,
            &planeai_core::services::CreateSessionParams {
                id: id.to_string(),
                project_id: project_id.to_string(),
                name: "agent".to_string(),
                branch: "main".to_string(),
                backend: "daemon".to_string(),
                task_key: task_key.map(str::to_string),
                task_project_id: task_project_id.map(str::to_string),
                ..Default::default()
            },
        )
        .unwrap();
    }

    fn activity_of(conn: &Connection, key: &str) -> Option<String> {
        conn.query_row(
            "SELECT last_activity_at FROM tasks WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn stamps_the_task_a_session_works_on() {
        let mut conn = setup();
        let project = db::create_project(&conn, "planeai", "/tmp/planeai").unwrap();
        let key = add_task(&mut conn, &project.prefix);
        add_session(&conn, "s1", &project.id, Some(&key), None);

        let activity = stamp(&conn, "s1", Activity::Finished, at("2026-11-03T10:00:00Z")).unwrap();

        assert_eq!(
            activity,
            Some(TaskActivity {
                project_id: project.id,
                task_key: key.clone(),
                visit: Visit {
                    last_visited_at: None,
                    last_activity_at: Some(at("2026-11-03T10:00:00Z")),
                },
                snooze: Snooze::default(),
            })
        );
        assert_eq!(
            activity_of(&conn, &key).as_deref(),
            Some("2026-11-03T10:00:00.000+00:00")
        );
    }

    #[test]
    fn stamps_the_owning_projects_task_for_a_cross_project_session() {
        let mut conn = setup();
        let owner = db::create_project(&conn, "owner", "/tmp/owner").unwrap();
        let repo = db::create_project(&conn, "repo", "/tmp/repo").unwrap();
        let key = add_task(&mut conn, &owner.prefix);
        add_session(&conn, "s1", &repo.id, Some(&key), Some(&owner.id));

        let activity = stamp(&conn, "s1", Activity::Finished, at("2026-11-03T10:00:00Z")).unwrap();

        assert_eq!(activity.map(|a| a.project_id), Some(owner.id));
        assert_eq!(
            activity_of(&conn, &key).as_deref(),
            Some("2026-11-03T10:00:00.000+00:00")
        );
    }

    #[test]
    fn leaves_everything_alone_without_a_linked_task() {
        let mut conn = setup();
        let project = db::create_project(&conn, "planeai", "/tmp/planeai").unwrap();
        let key = add_task(&mut conn, &project.prefix);
        add_session(&conn, "unlinked", &project.id, None, None);
        add_session(&conn, "dangling", &project.id, Some("PLA-404"), None);

        let unlinked = stamp(
            &conn,
            "unlinked",
            Activity::Finished,
            at("2026-11-03T10:00:00Z"),
        )
        .unwrap();
        let dangling = stamp(
            &conn,
            "dangling",
            Activity::Finished,
            at("2026-11-03T10:00:00Z"),
        )
        .unwrap();
        let unknown = stamp(
            &conn,
            "missing",
            Activity::Finished,
            at("2026-11-03T10:00:00Z"),
        )
        .unwrap();

        assert_eq!((unlinked, dangling, unknown), (None, None, None));
        assert_eq!(activity_of(&conn, &key), None);
    }

    #[test]
    fn ignores_an_archived_session_settling_after_its_task_is_done() {
        let mut conn = setup();
        let project = db::create_project(&conn, "planeai", "/tmp/planeai").unwrap();
        let key = add_task(&mut conn, &project.prefix);
        add_session(&conn, "s1", &project.id, Some(&key), None);
        db::archive_session(&conn, "s1").unwrap();

        let activity = stamp(&conn, "s1", Activity::Finished, at("2026-11-03T10:00:00Z")).unwrap();

        assert_eq!(activity, None);
        assert_eq!(activity_of(&conn, &key), None);
    }

    #[test]
    fn the_event_carries_the_task_and_its_visit() {
        let activity = TaskActivity {
            project_id: "p1".into(),
            task_key: "PLA-1".into(),
            visit: Visit {
                last_visited_at: None,
                last_activity_at: Some(at("2026-11-03T10:00:00.250Z")),
            },
            snooze: Snooze {
                snoozed_until: Some(at("2026-11-04T09:00:00Z")),
                snoozed_at: Some(at("2026-11-03T09:00:00Z")),
                woke_at: Some(at("2026-11-03T10:00:00.250Z")),
            },
        };
        assert_eq!(
            serde_json::to_value(activity).unwrap(),
            serde_json::json!({
                "project_id": "p1",
                "task_key": "PLA-1",
                "last_visited_at": null,
                "last_activity_at": "2026-11-03T10:00:00.250Z",
                "snoozed_until": "2026-11-04T09:00:00Z",
                "snoozed_at": "2026-11-03T09:00:00Z",
                "woke_at": "2026-11-03T10:00:00.250Z",
            })
        );
    }

    fn snoozed_task(conn: &mut Connection) -> (db::Project, String) {
        let project = db::create_project(conn, "planeai", "/tmp/planeai").unwrap();
        let key = add_task(conn, &project.prefix);
        add_session(conn, "s1", &project.id, Some(&key), None);
        planeai_tasks::sqlite::snooze(
            conn,
            &project.prefix,
            &key,
            at("2026-11-04T09:00:00Z"),
            at("2026-11-03T10:00:00Z"),
        )
        .unwrap();
        (project, key)
    }

    fn woke_at(conn: &Connection, key: &str) -> Option<String> {
        conn.query_row("SELECT woke_at FROM tasks WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .unwrap()
    }

    #[test]
    fn an_agent_asking_for_attention_or_exiting_wakes_a_snoozed_task() {
        for activity in [Activity::NeedsAttention, Activity::Exited] {
            let mut conn = setup();
            let (_, key) = snoozed_task(&mut conn);

            let stamped = stamp(&conn, "s1", activity, at("2026-11-03T12:00:00Z")).unwrap();

            assert_eq!(
                stamped.map(|a| a.snooze.woke_at),
                Some(Some(at("2026-11-03T12:00:00Z"))),
                "{activity:?}"
            );
            assert_eq!(
                woke_at(&conn, &key).as_deref(),
                Some("2026-11-03T12:00:00.000+00:00"),
                "{activity:?}"
            );
        }
    }

    #[test]
    fn a_finished_turn_leaves_a_snoozed_task_asleep() {
        let mut conn = setup();
        let (_, key) = snoozed_task(&mut conn);

        let stamped = stamp(&conn, "s1", Activity::Finished, at("2026-11-03T12:00:00Z")).unwrap();

        assert_eq!(
            stamped.map(|a| a.snooze),
            Some(Snooze {
                snoozed_until: Some(at("2026-11-04T09:00:00Z")),
                snoozed_at: Some(at("2026-11-03T10:00:00Z")),
                woke_at: None,
            })
        );
        assert_eq!(woke_at(&conn, &key), None);
    }

    #[test]
    fn attention_from_before_the_snooze_does_not_wake_it() {
        let mut conn = setup();
        let (_, key) = snoozed_task(&mut conn);

        stamp(
            &conn,
            "s1",
            Activity::NeedsAttention,
            at("2026-11-03T09:59:59Z"),
        )
        .unwrap();

        assert_eq!(woke_at(&conn, &key), None);
    }

    #[test]
    fn an_archived_session_does_not_wake_a_snoozed_task() {
        let mut conn = setup();
        let (_, key) = snoozed_task(&mut conn);
        db::archive_session(&conn, "s1").unwrap();

        let stamped = stamp(&conn, "s1", Activity::Exited, at("2026-11-03T12:00:00Z")).unwrap();

        assert_eq!(stamped, None);
        assert_eq!(woke_at(&conn, &key), None);
    }
}
