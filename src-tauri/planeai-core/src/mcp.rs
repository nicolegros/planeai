//! Per-session credentials for PlaneAI's MCP server (ADR-0016).
//!
//! Every agent launch mints a bearer token for its session. Only the token's SHA-256
//! hash is stored, so the database never holds a usable credential; the agent gets the
//! token through its environment. Tokens outlive app restarts and end with the session.
//! An app launch connects its agent only while the server is listening; a CLI launch
//! uses the configured URL.

use std::fmt;
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngExt;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

pub const URL_ENV: &str = "PLANEAI_MCP_URL";
pub const TOKEN_ENV: &str = "PLANEAI_MCP_TOKEN";
/// The name agents know the server by, which prefixes its tools in their settings.
pub const SERVER_NAME: &str = "planeai";

/// What a tool call does to PlaneAI state, which decides its MCP annotations and whether
/// agents may run it without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Read,
    Write { idempotent: bool },
    Delete,
}

impl Effect {
    /// Launch templates pre-approve every tool but deletes.
    pub fn pre_approved(self) -> bool {
        self != Effect::Delete
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: &'static str,
    pub effect: Effect,
}

/// PlaneAI's own tools, in the order the server lists them. The app's registry serves
/// exactly these, and launch templates derive tool approval from them.
pub const TOOLS: [ToolSpec; 11] = [
    ToolSpec {
        name: "task_list",
        effect: Effect::Read,
    },
    ToolSpec {
        name: "task_show",
        effect: Effect::Read,
    },
    ToolSpec {
        name: "task_create",
        effect: Effect::Write { idempotent: false },
    },
    ToolSpec {
        name: "task_update",
        effect: Effect::Write { idempotent: true },
    },
    ToolSpec {
        name: "task_delete",
        effect: Effect::Delete,
    },
    ToolSpec {
        name: "session_list",
        effect: Effect::Read,
    },
    ToolSpec {
        name: "session_tree",
        effect: Effect::Read,
    },
    ToolSpec {
        name: "session_read",
        effect: Effect::Read,
    },
    ToolSpec {
        name: "session_create",
        effect: Effect::Write { idempotent: false },
    },
    ToolSpec {
        name: "session_prompt",
        effect: Effect::Write { idempotent: false },
    },
    ToolSpec {
        name: "project_list",
        effect: Effect::Read,
    },
];

/// Agents call a tool as `mcp__planeai__<name>`, which Claude Code and Codex cap at 64
/// characters.
pub const MAX_TOOL_NAME_CHARS: usize = 64 - "mcp__".len() - SERVER_NAME.len() - "__".len();

/// The name a plugin's tool is served under. Plugin ids have no `_`, so the names of two
/// plugins' tools never meet; one a PlaneAI tool uses is refused.
pub fn plugin_tool_name(plugin_id: &str, tool: &str) -> Result<String, String> {
    let name = format!("{plugin_id}_{tool}");
    if TOOLS.iter().any(|core| core.name == name) {
        return Err(format!(
            "plugin {plugin_id} tool {tool} would be served as {name}, a PlaneAI tool"
        ));
    }
    if name.len() > MAX_TOOL_NAME_CHARS {
        return Err(format!(
            "plugin {plugin_id} tool {tool} would be served as {name}, longer than {MAX_TOOL_NAME_CHARS} characters"
        ));
    }
    Ok(name)
}

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

    /// Hand the token over in a new file only the user can read, for a spawner whose
    /// environment variables would show on its command line; the agent's shell deletes it.
    pub fn write_token_file(&self, dir: &Path) -> std::io::Result<PathBuf> {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(dir)?;
        let file = dir.join(format!("{}.token", uuid::Uuid::new_v4().simple()));
        write_private(&file, self.token.as_bytes())?;
        Ok(file)
    }

    pub fn owned_env(&self) -> impl Iterator<Item = (String, String)> + '_ {
        self.env()
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
    }
}

/// The URL agents reach the server at.
pub fn server_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
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
    /// The session's agent runs without permission prompts (yolo).
    pub auto_approve: bool,
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
/// The token table exists: the app and the CLI both migrate it when they open the database.
pub fn issue(conn: &Connection, session_id: &str, url: &str) -> rusqlite::Result<McpEndpoint> {
    let token = random_secret();
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

/// Mint the token the session's next agent process carries; `None` while the server is
/// not listening.
pub fn endpoint(
    conn: &Connection,
    url: Option<&str>,
    session_id: &str,
) -> Result<Option<McpEndpoint>, String> {
    url.map(|url| issue(conn, session_id, url))
        .transpose()
        .map_err(|error| format!("failed to mint the session's MCP token: {error}"))
}

/// `endpoint` for an agent a launch spawns now on `backend`. A `local` agent spawns on its
/// first attach instead, which mints its own token.
pub fn launch_endpoint(
    conn: &Connection,
    url: Option<&str>,
    session_id: &str,
    backend: &str,
) -> Result<Option<McpEndpoint>, String> {
    endpoint(conn, url.filter(|_| backend != "local"), session_id)
}

/// A fresh random token, URL-safe.
fn random_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(contents)
}

/// The live session a presented token belongs to, or `None` for an unknown or revoked token.
pub fn resolve(conn: &Connection, token: &str) -> rusqlite::Result<Option<McpCaller>> {
    conn.query_row(
        "SELECT s.id, s.project_id, s.task_key, COALESCE(s.task_project_id, s.project_id), s.auto_approve
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
                auto_approve: row.get(4)?,
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
    fn a_plugin_tool_is_served_under_its_plugin_id() {
        assert_eq!(
            plugin_tool_name("github", "pr_show"),
            Ok("github_pr_show".to_string())
        );
        assert_eq!(
            plugin_tool_name("task", "list"),
            Err("plugin task tool list would be served as task_list, a PlaneAI tool".into())
        );
        let longest = "x".repeat(MAX_TOOL_NAME_CHARS - "github_".len());
        assert!(plugin_tool_name("github", &longest).is_ok());
        assert_eq!(
            plugin_tool_name("github", &format!("{longest}x")),
            Err(format!(
                "plugin github tool {longest}x would be served as github_{longest}x, longer than {MAX_TOOL_NAME_CHARS} characters"
            ))
        );
        assert_eq!(MAX_TOOL_NAME_CHARS, 50);
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
                auto_approve: false,
            })
        );
    }

    #[test]
    fn a_yolo_session_resolves_as_auto_approving() {
        let conn = db();
        let (id, _) = session(&conn, None);
        conn.execute("UPDATE sessions SET auto_approve = 1 WHERE id = ?1", [&id])
            .unwrap();
        let caller = resolve(&conn, &token(&issue(&conn, &id, URL).unwrap()))
            .unwrap()
            .unwrap();
        assert!(caller.auto_approve);
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
    fn a_local_launch_mints_nothing_and_other_backends_mint_while_listening() {
        let conn = db();
        let (id, _) = session(&conn, None);
        assert_eq!(launch_endpoint(&conn, Some(URL), &id, "local"), Ok(None));
        assert_eq!(launch_endpoint(&conn, None, &id, "daemon"), Ok(None));
        let endpoint = launch_endpoint(&conn, Some(URL), &id, "daemon")
            .unwrap()
            .unwrap();
        assert_eq!(
            resolve(&conn, &token(&endpoint))
                .unwrap()
                .unwrap()
                .session_id,
            id
        );
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
