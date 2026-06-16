use serde::{Deserialize, Serialize};

/// Client → Daemon requests (JSON lines on control connection).
#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    CreateSession {
        session_id: String,
        command: String,
        args: Vec<String>,
        cwd: String,
        #[serde(default)]
        env: Vec<(String, String)>,
    },
    Attach {
        session_id: String,
    },
    Detach,
    Write {
        session_id: String,
        /// Base64-encoded bytes.
        data: String,
    },
    Resize {
        session_id: String,
        rows: u16,
        cols: u16,
    },
    Kill {
        session_id: String,
    },
    List,
    Ping,
}

/// Daemon → Client responses.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Ok {
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    Error {
        message: String,
    },
    SessionList {
        sessions: Vec<SessionInfo>,
    },
    SessionExited {
        session_id: String,
    },
    Pong,
}

#[derive(Debug, Serialize, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub alive: bool,
}
