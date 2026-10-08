//! Naming rules mapping PlaneAI identities onto rmux session names.
//!
//! PlaneAI's SQLite rows are the source of truth for identity; rmux names are
//! derived labels. Everything PlaneAI creates is prefixed so a stray session is
//! always attributable, and so discovery can list only PlaneAI's own sessions
//! even on a shared daemon.

/// Prefix applied to every session PlaneAI creates.
pub const SESSION_PREFIX: &str = "planeai-";

/// Which rmux session hosts a set of PlaneAI terminal resources.
///
/// A task workspace is the unit of persistence (ADR-0012): every agent session,
/// shell, and helper belonging to one task shares a single rmux session, which
/// outlives any individual agent and is removed only when the task is deleted.
/// Sessions with no task fall back to their own workspace so they are still
/// isolated and still cleaned up.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WorkspaceKey {
    /// One workspace per `(project_id, task_key)`.
    Task {
        project_id: String,
        task_key: String,
    },
    /// A session not linked to a task owns its workspace alone.
    Session { session_id: String },
}

/// The rmux session name identifying a workspace.
///
/// Derived once from a [`WorkspaceKey`] and then carried verbatim. Reconstructing
/// a key from a name would be lossy — a project id is a UUID containing hyphens,
/// so the name cannot be split back into its parts — so the name itself is what
/// PlaneAI persists and passes around.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkspaceName(String);

impl WorkspaceName {
    /// Accept a name PlaneAI previously stored.
    pub fn from_stored(value: &str) -> Option<Self> {
        is_planeai_session(value).then(|| Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for WorkspaceName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl WorkspaceKey {
    /// Build the key for a session, preferring its task workspace.
    pub fn for_session(project_id: &str, task_key: Option<&str>, session_id: &str) -> Self {
        match task_key.map(str::trim).filter(|key| !key.is_empty()) {
            Some(task_key) => Self::Task {
                project_id: project_id.to_string(),
                task_key: task_key.to_string(),
            },
            None => Self::Session {
                session_id: session_id.to_string(),
            },
        }
    }

    /// The rmux session name for this workspace.
    ///
    /// Task and session workspaces use distinct infixes so a task named like a
    /// session id can never collide with it.
    pub fn name(&self) -> WorkspaceName {
        WorkspaceName(self.session_name())
    }

    /// The raw session name string.
    pub fn session_name(&self) -> String {
        match self {
            Self::Task {
                project_id,
                task_key,
            } => format!(
                "{SESSION_PREFIX}ws-{}-{}",
                sanitize(project_id),
                sanitize(task_key)
            ),
            Self::Session { session_id } => {
                format!("{SESSION_PREFIX}s-{}", sanitize(session_id))
            }
        }
    }
}

/// Whether this rmux session name belongs to PlaneAI.
pub fn is_planeai_session(name: &str) -> bool {
    name.starts_with(SESSION_PREFIX)
}

/// rmux rejects `:` and `.` in session names and rewrites them itself; doing it
/// here keeps the derived name predictable instead of daemon-dependent.
fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            ':' | '.' => '_',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_sessions_share_one_workspace() {
        let first = WorkspaceKey::for_session("proj-1", Some("PLA-42"), "session-a");
        let second = WorkspaceKey::for_session("proj-1", Some("PLA-42"), "session-b");

        // Two agents on the same task must land in the same rmux session.
        assert_eq!(first, second);
        assert_eq!(first.session_name(), second.session_name());
    }

    #[test]
    fn different_tasks_get_different_workspaces() {
        let first = WorkspaceKey::for_session("proj-1", Some("PLA-42"), "session-a");
        let second = WorkspaceKey::for_session("proj-1", Some("PLA-43"), "session-a");

        assert_ne!(first.session_name(), second.session_name());
    }

    #[test]
    fn the_same_task_key_in_another_project_is_a_different_workspace() {
        let first = WorkspaceKey::for_session("proj-1", Some("PLA-42"), "session-a");
        let second = WorkspaceKey::for_session("proj-2", Some("PLA-42"), "session-a");

        assert_ne!(first.session_name(), second.session_name());
    }

    #[test]
    fn a_session_without_a_task_owns_its_workspace() {
        let key = WorkspaceKey::for_session("proj-1", None, "session-a");

        assert_eq!(
            key,
            WorkspaceKey::Session {
                session_id: "session-a".to_string()
            }
        );
        assert_eq!(key.session_name(), "planeai-s-session-a");
    }

    #[test]
    fn a_blank_task_key_is_treated_as_absent() {
        // An empty string in the DB must not produce a shared "empty task" workspace
        // that unrelated sessions would join.
        let blank = WorkspaceKey::for_session("proj-1", Some("   "), "session-a");
        let absent = WorkspaceKey::for_session("proj-1", None, "session-a");

        assert_eq!(blank, absent);
    }

    #[test]
    fn task_and_session_workspaces_cannot_collide() {
        let task = WorkspaceKey::Task {
            project_id: "p".to_string(),
            task_key: "x".to_string(),
        };
        let session = WorkspaceKey::Session {
            session_id: "p-x".to_string(),
        };

        assert_ne!(task.session_name(), session.session_name());
    }

    #[test]
    fn a_stored_name_round_trips_without_being_reparsed() {
        // A project id is a UUID full of hyphens, so the name cannot be split
        // back into parts. Carrying the name verbatim avoids the problem.
        let key =
            WorkspaceKey::for_session("3f1c6a2e-9b44-4d21-8e7a-1c5d9f0b2a33", Some("PLA-42"), "s");
        let name = key.name();
        let restored = WorkspaceName::from_stored(name.as_str()).unwrap();

        assert_eq!(restored, name);
        assert_eq!(restored.as_str(), key.session_name());
    }

    #[test]
    fn a_foreign_name_is_not_accepted_as_a_workspace() {
        assert!(WorkspaceName::from_stored("someone-elses-session").is_none());
    }

    #[test]
    fn names_are_attributable_to_planeai() {
        let key = WorkspaceKey::for_session("proj-1", Some("PLA-42"), "session-a");

        assert!(is_planeai_session(&key.session_name()));
        assert!(!is_planeai_session("my-own-work"));
    }

    #[test]
    fn characters_rmux_would_rewrite_are_normalised_up_front() {
        let key = WorkspaceKey::for_session("proj.1", Some("PLA:42"), "s");

        assert_eq!(key.session_name(), "planeai-ws-proj_1-PLA_42");
    }
}
