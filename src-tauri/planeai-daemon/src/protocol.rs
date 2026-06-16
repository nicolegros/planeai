use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

// Binary frame type constants
pub const FRAME_OUTPUT: u8 = 0x01;
pub const FRAME_INPUT: u8 = 0x02;

// Connection type discriminator (first byte on new connection)
pub const CONN_CONTROL: u8 = 0x00;
pub const CONN_DATA: u8 = 0x01;

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
