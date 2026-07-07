use crate::types::{SpawnMode, SpawnOutcome};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

// Binary frame type constants
pub const FRAME_OUTPUT: u8 = 0x01;
pub const FRAME_INPUT: u8 = 0x02;
pub const FRAME_RESIZE: u8 = 0x03;
pub const FRAME_EOF: u8 = 0x04;
pub const FRAME_ERROR: u8 = 0x05;
pub const FRAME_HELLO: u8 = 0x06;
pub const FRAME_ATTACH: u8 = 0x07;
pub const FRAME_GAP: u8 = 0x08;

// Connection type discriminator (first byte on new connection)
pub const CONN_CONTROL: u8 = 0x00;
pub const CONN_DATA: u8 = 0x01;

/// Current protocol version.
pub const PROTOCOL_VERSION: u8 = 2;

/// Write a binary frame: [1-byte type][4-byte big-endian length][payload]
pub async fn write_frame(
    stream: &mut (impl AsyncWrite + Unpin),
    frame_type: u8,
    payload: &[u8],
) -> anyhow::Result<()> {
    let mut header = [0u8; 5];
    header[0] = frame_type;
    header[1..5].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    stream.write_all(&header).await?;
    stream.write_all(payload).await?;
    stream.flush().await?;
    Ok(())
}

/// Read a binary frame. Returns (frame_type, payload). Handles partial reads.
pub async fn read_frame(stream: &mut (impl AsyncRead + Unpin)) -> anyhow::Result<(u8, Vec<u8>)> {
    let mut header = [0u8; 5];
    stream.read_exact(&mut header).await?;
    let frame_type = header[0];
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).await?;
    Ok((frame_type, payload))
}

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
        #[serde(default)]
        mode: Option<SpawnMode>,
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
    ReadBuffer {
        session_id: String,
        #[serde(default = "default_read_lines")]
        lines: usize,
    },
    ReadBufferAfter {
        session_id: String,
        #[serde(default)]
        after: u64,
        #[serde(default)]
        max_bytes: usize,
    },
}

fn default_read_lines() -> usize {
    100
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Response {
    Ok {
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        outcome: Option<SpawnOutcome>,
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
    BufferText {
        ok: bool,
        session_id: String,
        text: String,
        line_count: usize,
    },
    BufferTextCursor {
        ok: bool,
        session_id: String,
        text: String,
        cursor: u64,
        truncated: bool,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfoDto {
    pub session_id: String,
    pub alive: bool,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
}

impl Response {
    pub fn ok(session_id: Option<String>) -> Self {
        Self::Ok {
            ok: true,
            session_id,
            outcome: None,
        }
    }

    pub fn ok_with_outcome(session_id: Option<String>, outcome: SpawnOutcome) -> Self {
        Self::Ok {
            ok: true,
            session_id,
            outcome: Some(outcome),
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
