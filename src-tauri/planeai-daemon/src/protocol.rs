use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Spawn {
        session_id: String,
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        env: Option<HashMap<String, String>>,
    },
    Kill {
        session_id: String,
    },
    Resize {
        session_id: String,
        cols: u16,
        rows: u16,
    },
    List,
    Attach {
        session_id: String,
    },
    Detach {
        session_id: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Response {
    Ok {
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    Error {
        error: String,
    },
    Sessions {
        sessions: Vec<SessionInfoDto>,
    },
    Event {
        event: String,
        session_id: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfoDto {
    pub session_id: String,
    pub alive: bool,
}

impl Response {
    pub fn ok(session_id: Option<String>) -> Self {
        Self::Ok {
            ok: true,
            session_id,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self::Error { error: msg.into() }
    }

    pub fn event(event: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self::Event {
            event: event.into(),
            session_id: session_id.into(),
        }
    }
}
