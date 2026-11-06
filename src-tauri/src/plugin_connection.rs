//! The JSON-RPC connection to one plugin sidecar. Host requests go one at a time over stdin
//! and read their responses from stdout. The sidecar's callbacks come out on their own
//! channel, to be answered as they arrive: a request that times out then never strands one.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio::time::{timeout, Duration};

use crate::plugin_providers::NotificationSink;
use crate::plugin_rpc::*;

const FRAME_QUEUE_CAPACITY: usize = 256;
/// How long a sidecar has to acknowledge the cancellation of a timed-out request.
const CANCELLATION_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) struct PluginConnection {
    stdin: AsyncMutex<ChildStdin>,
    /// Response frames from the stdout reader, in arrival order.
    responses: AsyncMutex<mpsc::Receiver<Result<String, String>>>,
    pub(crate) requests: RequestLane,
    next_request_id: AtomicU64,
    /// Why the stdout reader stopped, once it has: nothing the sidecar sends is read anymore.
    stdout_failure: Arc<OnceLock<String>>,
}

impl PluginConnection {
    /// Starts reading stdout. The returned callbacks must be answered with `answer_callback`.
    pub(crate) fn open(
        stdin: ChildStdin,
        stdout: ChildStdout,
        plugin_id: &str,
        notifications: Option<NotificationSink>,
    ) -> (Self, mpsc::Receiver<JsonRpcCallbackRequest>) {
        let (response_sender, responses) = mpsc::channel(FRAME_QUEUE_CAPACITY);
        let (callback_sender, callbacks) = mpsc::channel(FRAME_QUEUE_CAPACITY);
        let stdout_failure = Arc::new(OnceLock::new());
        tokio::spawn(read_stdout_frames(
            BufReader::new(stdout),
            response_sender,
            callback_sender,
            plugin_id.to_string(),
            notifications,
            stdout_failure.clone(),
        ));
        let connection = Self {
            stdin: AsyncMutex::new(stdin),
            responses: AsyncMutex::new(responses),
            requests: RequestLane::default(),
            next_request_id: AtomicU64::new(0),
            stdout_failure,
        };
        (connection, callbacks)
    }

    pub(crate) fn stdout_failure(&self) -> Option<&String> {
        self.stdout_failure.get()
    }

    pub(crate) async fn request(
        &self,
        method: &str,
        params: Value,
        request_timeout: Duration,
    ) -> Result<Value, PluginRpcError> {
        let deadline = Instant::now() + request_timeout;
        self.requests
            .run(method, request_timeout, || async {
                let request_id = self.next_request_id();
                let frame = encode_request(request_id, method, params)?;
                self.write_frame(&frame, deadline, "plugin RPC request")
                    .await
                    .map_err(PluginRpcError::Broken)?;

                let remaining = deadline.saturating_duration_since(Instant::now());
                match timeout(remaining, self.read_response()).await {
                    Ok(Ok(frame)) => decode_json_rpc_frame(&frame, request_id),
                    Ok(Err(error)) => Err(PluginRpcError::Broken(error)),
                    Err(_) => Err(self.cancel_timed_out_request(method, request_id).await),
                }
            })
            .await
    }

    /// How a request that timed out ends, once cancelled.
    async fn cancel_timed_out_request(&self, method: &str, request_id: u64) -> PluginRpcError {
        let deadline = Instant::now() + CANCELLATION_TIMEOUT;
        let frame = match encode_json_rpc_cancel_request_line(request_id) {
            Ok(frame) => frame,
            Err(error) => return PluginRpcError::Broken(error),
        };
        if self
            .write_frame(&frame, deadline, "plugin cancellation notification")
            .await
            .is_err()
        {
            return cancellation_acknowledgement_error(method, request_id, None);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        match timeout(remaining, self.read_response()).await {
            Ok(Ok(frame)) => cancellation_acknowledgement_error(method, request_id, Some(&frame)),
            // The original timeout remains the externally meaningful error when
            // the plugin cannot acknowledge cancellation correctly or in time.
            Ok(Err(_)) | Err(_) => cancellation_acknowledgement_error(method, request_id, None),
        }
    }

    /// Waits for the request turn even on a broken lane, since a shutdown answer is never trusted.
    pub(crate) async fn request_shutdown(&self, deadline: Instant) -> Result<Value, String> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let _turn = self
            .requests
            .shutdown_turn(remaining)
            .await
            .ok_or_else(|| "plugin shutdown request timed out".to_string())?;
        let request_id = self.next_request_id();
        let frame = encode_json_rpc_line(request_id, "plugin.shutdown", Value::Null)?;
        self.write_frame(&frame, deadline, "plugin shutdown request")
            .await?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let frame = timeout(remaining, self.read_response())
            .await
            .map_err(|_| "plugin shutdown request timed out".to_string())??;
        decode_json_rpc_frame(&frame, request_id).map_err(String::from)
    }

    /// Answers one sidecar callback in a single write, so no other frame can split it.
    pub(crate) async fn answer_callback(
        &self,
        id: Value,
        result: Result<Value, CallbackError>,
    ) -> Result<(), String> {
        let mut frame = encode_host_callback_response(id, result)?;
        frame.push('\n');
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(frame.as_bytes())
            .await
            .map_err(|error| format!("failed to write host callback response: {error}"))?;
        stdin
            .flush()
            .await
            .map_err(|error| format!("failed to flush host callback response: {error}"))
    }

    fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    async fn write_frame(&self, frame: &str, deadline: Instant, kind: &str) -> Result<(), String> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let mut stdin = timeout(remaining, self.stdin.lock())
            .await
            .map_err(|_| format!("{kind} timed out"))?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        timeout(remaining, async {
            stdin
                .write_all(frame.as_bytes())
                .await
                .map_err(|error| format!("failed to write {kind}: {error}"))?;
            stdin
                .flush()
                .await
                .map_err(|error| format!("failed to flush {kind}: {error}"))
        })
        .await
        .map_err(|_| format!("{kind} timed out"))?
    }

    /// The next response frame. Fails only when stdout can no longer be read.
    async fn read_response(&self) -> Result<String, String> {
        self.responses
            .lock()
            .await
            .recv()
            .await
            .unwrap_or_else(|| Err(STDOUT_CLOSED_ERROR.to_string()))
    }
}

/// Owns sidecar stdout for the process lifetime, so nothing waits on a host request being in
/// flight: provider notifications are routed, any other `host.*` notification, as one from a
/// newer host's contract, is dropped, and callbacks go to their own channel. Responses are
/// queued for the request path in arrival order. The first transport or protocol error ends
/// the stream and is recorded in `failure`, so the runtime is retired without waiting for a
/// request to see it.
async fn read_stdout_frames(
    mut stdout: BufReader<ChildStdout>,
    responses: mpsc::Sender<Result<String, String>>,
    callbacks: mpsc::Sender<JsonRpcCallbackRequest>,
    plugin_id: String,
    notifications: Option<NotificationSink>,
    failure: Arc<OnceLock<String>>,
) {
    loop {
        let frame = read_stdout_frame(&mut stdout).await;
        let handles = |method: &str| {
            notifications
                .as_ref()
                .is_some_and(|sink| sink.handles(method))
        };
        let frame = match frame.as_deref().map_or(StdoutFrame::Response, |frame| {
            classify_stdout_frame(frame, handles)
        }) {
            StdoutFrame::Route(method, params) => {
                if let Some(sink) = &notifications {
                    sink.route(&method, params);
                }
                continue;
            }
            StdoutFrame::Drop(method) => {
                tracing::warn!(%plugin_id, %method, "ignored a host notification this host does not know");
                continue;
            }
            StdoutFrame::Callback(Ok(callback)) => {
                if callbacks.send(callback).await.is_err() {
                    return;
                }
                continue;
            }
            StdoutFrame::Callback(Err(error)) => Err(error),
            StdoutFrame::Response => frame,
        };
        if let Err(error) = &frame {
            let _ = failure.set(error.clone());
        }
        let failed = frame.is_err();
        if responses.send(frame).await.is_err() || failed {
            return;
        }
    }
}

async fn read_stdout_frame(stdout: &mut BufReader<ChildStdout>) -> Result<String, String> {
    let mut bytes = Vec::new();
    let bytes_read = stdout
        .take(MAX_RPC_FRAME_BYTES + 1)
        .read_until(b'\n', &mut bytes)
        .await
        .map_err(|e| format!("failed to read plugin JSON-RPC response: {e}"))?;
    if bytes_read == 0 {
        return Err(STDOUT_CLOSED_ERROR.to_string());
    }
    validate_plugin_response_frame(&bytes)?;
    String::from_utf8(bytes)
        .map_err(|e| format!("plugin JSON-RPC response was not valid UTF-8: {e}"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Stdio;
    use tokio::process::{Child, Command};

    fn sidecar(
        script: &str,
    ) -> (
        Child,
        PluginConnection,
        mpsc::Receiver<JsonRpcCallbackRequest>,
    ) {
        let mut child = Command::new("/bin/sh")
            .args(["-c", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (connection, callbacks) = PluginConnection::open(stdin, stdout, "fake", None);
        (child, connection, callbacks)
    }

    /// Models the GitHub plugin's reconcile: once cancelled, it can only acknowledge after
    /// the host answers the callback it is waiting on.
    const ACKNOWLEDGES_ONCE_ITS_CALLBACK_IS_ANSWERED: &str = r#"
        read -r request
        printf '%s\n' '{"jsonrpc":"2.0","id":"c1","method":"host.state.patch","params":{}}'
        answered=no; cancelled=no
        while [ $answered = no ] || [ $cancelled = no ]; do
            read -r line || exit 1
            case "$line" in
                *'"id":"c1"'*) answered=yes ;;
                *cancelRequest*) cancelled=yes ;;
            esac
        done
        printf '%s\n' '{"jsonrpc":"2.0","id":1,"error":{"code":-32800,"message":"request cancelled"}}'
        read -r request
        printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"checked":1}}'
        read -r line
    "#;

    #[tokio::test]
    async fn a_callback_outliving_its_request_is_answered_and_the_next_request_succeeds() {
        let (_child, connection, mut callbacks) =
            sidecar(ACKNOWLEDGES_ONCE_ITS_CALLBACK_IS_ANSWERED);
        let connection = Arc::new(connection);
        let server = connection.clone();
        tokio::spawn(async move {
            while let Some(callback) = callbacks.recv().await {
                // Answers after the request below has timed out.
                tokio::time::sleep(Duration::from_millis(300)).await;
                server
                    .answer_callback(callback.id, Ok(serde_json::json!({ "ok": true })))
                    .await
                    .unwrap();
            }
        });

        let timed_out = connection
            .request("github.reconcile", Value::Null, Duration::from_millis(100))
            .await
            .unwrap_err();
        assert_eq!(timed_out.code(), Some(-32800), "{timed_out}");
        assert!(!connection.requests.is_broken());

        let next = connection
            .request("github.reconcile", Value::Null, Duration::from_secs(5))
            .await;
        assert_eq!(next, Ok(serde_json::json!({ "checked": 1 })));
    }

    #[tokio::test]
    async fn a_malformed_callback_fails_the_waiting_request_and_ends_the_stream() {
        let (_child, connection, _callbacks) = sidecar(
            r#"
            read -r request
            printf '%s\n' '{"jsonrpc":"2.0","method":"host.settings.get","id":[1]}'
            read -r line
        "#,
        );

        let error = connection
            .request("jira.status", Value::Null, Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(error.is_fatal(), "{error}");
        assert!(connection
            .stdout_failure()
            .is_some_and(|failure| failure.contains("id must be")));
    }
}
