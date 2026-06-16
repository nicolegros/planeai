use crate::protocol::{read_frame, write_frame, FRAME_INPUT, FRAME_OUTPUT};
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
    // Handshake: first frame is FRAME_OUTPUT containing session_id
    let (frame_type, payload) = read_frame(&mut stream).await?;
    if frame_type != FRAME_OUTPUT {
        anyhow::bail!("invalid handshake frame type: {frame_type}");
    }
    let session_id = String::from_utf8(payload)?;
    tracing::info!("data attach: {session_id}");

    // Look up session
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

    tracing::info!("data detach: {session_id}");
    Ok(())
}

async fn forward_output(
    writer: &mut (impl AsyncWrite + Unpin),
    rx: &mut tokio::sync::broadcast::Receiver<Vec<u8>>,
) -> anyhow::Result<()> {
    loop {
        match rx.recv().await {
            Ok(data) => write_frame(writer, FRAME_OUTPUT, &data).await?,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("broadcast lagged {n} messages");
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
        if frame_type != FRAME_INPUT {
            continue;
        }
        let reg = registry.lock().await;
        if let Some(session) = reg.get(session_id) {
            session.write(&payload)?;
        }
    }
}
