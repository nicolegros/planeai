//! Per-session credentials for PlaneAI's MCP server (ADR-0016).
//!
//! Every agent launch mints a bearer token for its session. Only the token's SHA-256
//! hash is stored, so the database never holds a usable credential; the agent gets the
//! token through its environment. Tokens outlive app restarts and end with the session.

use std::fmt;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngExt;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

pub const URL_ENV: &str = "PLANEAI_MCP_URL";
pub const TOKEN_ENV: &str = "PLANEAI_MCP_TOKEN";

/// What an agent process needs to reach the MCP server as its session.
#[derive(Clone, PartialEq, Eq)]
pub struct McpEndpoint {
    pub url: String,
    token: String,
}

impl McpEndpoint {
    /// The variables an agent's environment carries; the token never goes on a command line.
    pub fn env(&self) -> [(&'static str, &str); 2] {
        [(URL_ENV, &self.url), (TOKEN_ENV, &self.token)]
    }
}

impl fmt::Debug for McpEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpEndpoint")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

/// The session a request was made from, resolved from its bearer token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCaller {
    pub session_id: String,
    /// The project the session runs in.
    pub project_id: String,
    pub task: Option<CallerTask>,
}

/// The task a session works on, with the project that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerTask {
    pub key: String,
    pub project_id: String,
}

impl McpCaller {
    /// The project whose tasks the caller works in: its task's project, else its own.
    pub fn workspace_project_id(&self) -> &str {
        self.task
            .as_ref()
            .map_or(&self.project_id, |task| &task.project_id)
    }
}

/// Create the token table and the triggers that revoke a token when its session ends.
/// Idempotent; needs the sessions table.
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mcp_tokens (
            session_id TEXT PRIMARY KEY,
            token_hash TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL
        );
        CREATE TRIGGER IF NOT EXISTS mcp_tokens_revoke_on_session_end
        AFTER UPDATE OF status ON sessions
        WHEN NEW.status IN ('archived', 'destroyed')
        BEGIN DELETE FROM mcp_tokens WHERE session_id = NEW.id; END;
        CREATE TRIGGER IF NOT EXISTS mcp_tokens_revoke_on_session_delete
        AFTER DELETE ON sessions
        BEGIN DELETE FROM mcp_tokens WHERE session_id = OLD.id; END;",
    )
}

/// Mint a token for the session's next agent process, replacing any earlier one.
pub fn issue(conn: &Connection, session_id: &str, url: &str) -> rusqlite::Result<McpEndpoint> {
    // The CLI can launch against a database the app has not migrated yet.
    migrate(conn)?;
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    conn.execute(
        "INSERT INTO mcp_tokens (session_id, token_hash, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(session_id) DO UPDATE SET token_hash = excluded.token_hash, created_at = excluded.created_at",
        params![session_id, hash(&token), chrono::Utc::now().to_rfc3339()],
    )?;
    Ok(McpEndpoint {
        url: url.to_string(),
        token,
    })
}

/// The live session a presented token belongs to, or `None` for an unknown or revoked token.
pub fn resolve(conn: &Connection, token: &str) -> rusqlite::Result<Option<McpCaller>> {
    conn.query_row(
        "SELECT s.id, s.project_id, s.task_key, COALESCE(s.task_project_id, s.project_id)
         FROM mcp_tokens t JOIN sessions s ON s.id = t.session_id
         WHERE t.token_hash = ?1",
        params![hash(token)],
        |row| {
            let task_key: Option<String> = row.get(2)?;
            Ok(McpCaller {
                session_id: row.get(0)?,
                project_id: row.get(1)?,
                task: match task_key {
                    Some(key) => Some(CallerTask {
                        key,
                        project_id: row.get(3)?,
                    }),
                    None => None,
                },
            })
        },
    )
    .optional()
}

fn hash(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{CreateSessionParams, ProjectService, SessionService};

    const URL: &str = "http://127.0.0.1:47437/mcp";

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::services::migrate(&conn).unwrap();
        conn
    }

    fn session(conn: &Connection, task: Option<(&str, Option<&str>)>) -> (String, String) {
        let dir = tempfile::tempdir().unwrap();
        let project = ProjectService::create(
            conn,
            &format!("p{}", uuid::Uuid::new_v4()),
            dir.path().to_str().unwrap(),
        )
        .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        SessionService::create(
            conn,
            &CreateSessionParams {
                id: id.clone(),
                project_id: project.id.clone(),
                name: "agent".into(),
                branch: "main".into(),
                backend: "daemon".into(),
                task_key: task.map(|(key, _)| key.to_string()),
                task_project_id: task.and_then(|(_, owner)| owner.map(str::to_string)),
                ..Default::default()
            },
        )
        .unwrap();
        (id, project.id)
    }

    fn token(endpoint: &McpEndpoint) -> String {
        endpoint.env()[1].1.to_string()
    }

    #[test]
    fn an_issued_token_resolves_to_its_session_and_project() {
        let conn = db();
        let (id, project_id) = session(&conn, None);
        let endpoint = issue(&conn, &id, URL).unwrap();

        assert_eq!(
            resolve(&conn, &token(&endpoint)).unwrap(),
            Some(McpCaller {
                session_id: id,
                project_id,
                task: None,
            })
        );
    }

    #[test]
    fn a_task_session_resolves_its_task_and_the_project_owning_it() {
        let conn = db();
        let (_, owner_id) = session(&conn, None);
        let (id, project_id) = session(&conn, Some(("PLA-7", Some(&owner_id))));
        let caller = resolve(&conn, &token(&issue(&conn, &id, URL).unwrap()))
            .unwrap()
            .unwrap();

        assert_eq!(
            caller.task,
            Some(CallerTask {
                key: "PLA-7".into(),
                project_id: owner_id.clone(),
            })
        );
        assert_eq!(caller.project_id, project_id);
        assert_eq!(caller.workspace_project_id(), owner_id);
    }

    #[test]
    fn the_token_is_a_32_byte_base64url_value_and_only_its_hash_is_stored() {
        let conn = db();
        let (id, _) = session(&conn, None);
        let endpoint = issue(&conn, &id, URL).unwrap();
        let token = token(&endpoint);

        assert_eq!(URL_SAFE_NO_PAD.decode(&token).unwrap().len(), 32);
        let stored: String = conn
            .query_row("SELECT token_hash FROM mcp_tokens", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored, hash(&token));
        assert_ne!(stored, token);
        assert_eq!(
            hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn the_endpoint_hands_both_variables_to_the_agent_environment() {
        let conn = db();
        let (id, _) = session(&conn, None);
        let endpoint = issue(&conn, &id, URL).unwrap();
        let env = endpoint.env();

        assert_eq!(env[0], ("PLANEAI_MCP_URL", URL));
        assert_eq!(env[1].0, "PLANEAI_MCP_TOKEN");
        assert!(!format!("{endpoint:?}").contains(env[1].1));
    }

    #[test]
    fn reissuing_replaces_the_previous_token() {
        let conn = db();
        let (id, _) = session(&conn, None);
        let first = token(&issue(&conn, &id, URL).unwrap());
        let second = token(&issue(&conn, &id, URL).unwrap());

        assert_eq!(resolve(&conn, &first).unwrap(), None);
        assert_eq!(resolve(&conn, &second).unwrap().unwrap().session_id, id);
    }

    #[test]
    fn unknown_tokens_resolve_to_nobody() {
        let conn = db();
        let (id, _) = session(&conn, None);
        issue(&conn, &id, URL).unwrap();
        assert_eq!(resolve(&conn, "not-a-token").unwrap(), None);
        assert_eq!(resolve(&conn, "").unwrap(), None);
    }

    #[test]
    fn archiving_destroying_or_deleting_a_session_revokes_its_token() {
        let conn = db();
        for end in [
            SessionService::archive as fn(&Connection, &str) -> rusqlite::Result<()>,
            SessionService::destroy,
            SessionService::delete,
        ] {
            let (id, _) = session(&conn, None);
            let token = token(&issue(&conn, &id, URL).unwrap());
            end(&conn, &id).unwrap();
            assert_eq!(resolve(&conn, &token).unwrap(), None);
            let rows: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM mcp_tokens WHERE session_id = ?1",
                    [&id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(rows, 0);
        }
    }

    #[test]
    fn an_exited_session_keeps_its_token_for_the_agent_that_may_reconnect() {
        let conn = db();
        let (id, _) = session(&conn, None);
        let token = token(&issue(&conn, &id, URL).unwrap());
        SessionService::mark_exited(&conn, &id).unwrap();
        assert_eq!(resolve(&conn, &token).unwrap().unwrap().session_id, id);
    }

    #[test]
    fn tokens_survive_reopening_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("planeai.db");
        let (id, token) = {
            let conn = crate::services::open_db_at(&path).unwrap();
            let (id, _) = session(&conn, None);
            (id.clone(), token(&issue(&conn, &id, URL).unwrap()))
        };
        let conn = crate::services::open_db_at(&path).unwrap();
        assert_eq!(resolve(&conn, &token).unwrap().unwrap().session_id, id);
    }
}
