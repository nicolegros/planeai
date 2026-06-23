use crate::protocol::{
    read_frame, write_frame, FRAME_ATTACH, FRAME_EOF, FRAME_ERROR, FRAME_GAP, FRAME_HELLO,
    FRAME_INPUT, FRAME_OUTPUT, FRAME_RESIZE,
};
use crate::registry::SessionRegistry;
use crate::transport::DaemonStream;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;

const CHUNK_SIZE: usize = 65536;

/// Handle a data connection: handshake, replay, live stream + input.
pub async fn handle_data_connection(stream: DaemonStream, registry: Arc<Mutex<SessionRegistry>>) {
    if let Err(e) = handle_data_inner(stream, registry).await {
        tracing::debug!("data connection ended: {e}");
    }
}

async fn handle_data_inner(
    mut stream: DaemonStream,
    registry: Arc<Mutex<SessionRegistry>>,
) -> anyhow::Result<()> {
    // Handshake: accept both legacy (FRAME_OUTPUT with session_id) and new protocol
    let (frame_type, payload) = read_frame(&mut stream).await?;
    let session_id = match frame_type {
        // New protocol: FRAME_HELLO with version, then FRAME_ATTACH with session_id
        FRAME_HELLO => {
            let _version = payload.first().copied().unwrap_or(1);
            // Read FRAME_ATTACH
            let (attach_type, attach_payload) = read_frame(&mut stream).await?;
            if attach_type != FRAME_ATTACH {
                let msg = format!("expected FRAME_ATTACH, got 0x{attach_type:02x}");
                let _ = write_frame(&mut stream, FRAME_ERROR, msg.as_bytes()).await;
                anyhow::bail!("{msg}");
            }
            String::from_utf8(attach_payload)?
        }
        // Legacy protocol: FRAME_OUTPUT containing session_id as handshake
        FRAME_OUTPUT => String::from_utf8(payload)?,
        other => {
            let msg = format!("invalid handshake frame type: 0x{other:02x}");
            let _ = write_frame(&mut stream, FRAME_ERROR, msg.as_bytes()).await;
            anyhow::bail!("{msg}");
        }
    };

    tracing::info!(session_id = %session_id, "data attach");

    // Look up session (works for both running and exited sessions for replay)
    let reg = registry.lock().await;
    let session = reg
        .get(&session_id)
        .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;

    // Get snapshot and subscriber while holding lock
    let snapshot = session.buffer_snapshot();
    let mut output_rx = session.subscribe_output();
    let alive = session.is_alive();
    drop(reg);

    let (mut reader, mut writer) = tokio::io::split(stream);

    // Step 1: Replay buffer snapshot in chunks
    for chunk in snapshot.chunks(CHUNK_SIZE) {
        write_frame(&mut writer, FRAME_OUTPUT, chunk).await?;
    }

    if !alive {
        // Send EOF for exited sessions after replay
        write_frame(&mut writer, FRAME_EOF, b"").await?;
        return Ok(());
    }

    // Steps 2+3: Live stream output + read input concurrently
    let registry_input = Arc::clone(&registry);
    let sid = session_id.clone();

    tokio::select! {
        result = forward_output(&mut writer, &mut output_rx) => {
            result?;
        }
        result = forward_input(&mut reader, &registry_input, &sid) => {
            result?;
        }
    }

    tracing::info!(session_id = %session_id, "data detach");
    Ok(())
}

async fn forward_output(
    writer: &mut (impl AsyncWrite + Unpin),
    rx: &mut tokio::sync::broadcast::Receiver<Vec<u8>>,
) -> anyhow::Result<()> {
    loop {
        match rx.recv().await {
            Ok(data) => write_frame(writer, FRAME_OUTPUT, &data).await?,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                // Session exited — send EOF
                write_frame(writer, FRAME_EOF, b"").await?;
                return Ok(());
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(lagged = n, "broadcast lagged, sending FRAME_GAP");
                let gap_msg = format!("{{\"lagged\":{n}}}");
                write_frame(writer, FRAME_GAP, gap_msg.as_bytes()).await?;
            }
        }
    }
}

async fn forward_input(
    reader: &mut (impl AsyncRead + Unpin),
    registry: &Arc<Mutex<SessionRegistry>>,
    session_id: &str,
) -> anyhow::Result<()> {
    loop {
        let (frame_type, payload) = read_frame(reader).await?;
        if frame_type == FRAME_INPUT {
            let reg = registry.lock().await;
            if let Some(session) = reg.get(session_id) {
                session.write(&payload)?;
            }
        } else if frame_type == FRAME_RESIZE && payload.len() >= 4 {
            let cols = u16::from_be_bytes([payload[0], payload[1]]);
            let rows = u16::from_be_bytes([payload[2], payload[3]]);
            let reg = registry.lock().await;
            if let Some(session) = reg.get(session_id) {
                let _ = session.resize(cols, rows);
            }
        }
    }
}
