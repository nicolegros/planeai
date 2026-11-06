//! The JSON-RPC connection to one plugin sidecar. Host requests go one at a time over stdin
//! and read their responses from stdout. The sidecar's callbacks come out on their own
//! channel, answered by `answer_callbacks` as they arrive: a request that times out then never
//! strands one.

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Instant;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{mpsc, Mutex as AsyncMutex, MutexGuard};
use tokio::time::{timeout, Duration};

use crate::plugin_providers::NotificationSink;
use crate::plugin_rpc::*;

const FRAME_QUEUE_CAPACITY: usize = 256;
/// How long a sidecar has to acknowledge the cancellation of a timed-out request.
const CANCELLATION_TIMEOUT: Duration = Duration::from_secs(3);
const CALLBACKS_UNANSWERED_ERROR: &str = "plugin callbacks are no longer answered";

pub(crate) struct PluginConnection {
    plugin_id: String,
    stdin: AsyncMutex<ChildStdin>,
    /// Response frames from the stdout reader, in arrival order.
    responses: AsyncMutex<mpsc::Receiver<Result<String, String>>>,
    requests: RequestLane,
    next_request_id: AtomicU64,
    /// Why the stdout reader stopped, once it has: nothing the sidecar sends is read anymore.
    stdout_failure: Arc<OnceLock<String>>,
}

/// Owns a connection and serves the callbacks its sidecar makes.
pub(crate) trait CallbackServer: Send + Sync + 'static {
    fn connection(&self) -> &PluginConnection;

    fn serve(
        &self,
        method: &str,
        params: Value,
    ) -> impl Future<Output = Result<Value, CallbackError>> + Send;
}

impl PluginConnection {
    /// Starts reading stdout. The returned callbacks must go to `answer_callbacks`.
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
            plugin_id: plugin_id.to_string(),
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

    /// Whether a request left stdout untrustworthy, so the runtime must be retired.
    pub(crate) fn is_broken(&self) -> bool {
        self.requests.is_broken()
    }

    /// Waits up to `request_timeout` for its turn, then gives the sidecar `request_timeout`
    /// from then: a request queued behind a long one keeps its whole budget.
    pub(crate) async fn request(
        &self,
        method: &str,
        params: Value,
        request_timeout: Duration,
    ) -> Result<Value, PluginRpcError> {
        self.requests
            .run(method, request_timeout, || async {
                let deadline = Instant::now() + request_timeout;
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
    async fn answer_callback(
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

/// Answers the sidecar's callbacks in order for as long as their server lives. Holds it weakly:
/// a strong reference would keep the sidecar's stdin open, and with it the sidecar. Stops at
/// the first answer it cannot write, after which the stdout reader fails the connection.
pub(crate) fn answer_callbacks<S: CallbackServer>(
    server: Weak<S>,
    mut callbacks: mpsc::Receiver<JsonRpcCallbackRequest>,
) {
    tokio::spawn(async move {
        while let Some(JsonRpcCallbackRequest { id, method, params }) = callbacks.recv().await {
            let Some(server) = server.upgrade() else {
                return;
            };
            let result = server.serve(&method, params).await;
            let connection = server.connection();
            if let Err(error) = connection.answer_callback(id, result).await {
                tracing::warn!(plugin_id = %connection.plugin_id, %error, "failed to answer plugin callback");
                return;
            }
        }
    });
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
    let handles = |method: &str| {
        notifications
            .as_ref()
            .is_some_and(|sink| sink.handles(method))
    };
    let error = loop {
        let frame = match read_stdout_frame(&mut stdout).await {
            Ok(frame) => frame,
            Err(error) => break error,
        };
        match classify_stdout_frame(&frame, handles) {
            StdoutFrame::Route(method, params) => {
                if let Some(sink) = &notifications {
                    sink.route(&method, params);
                }
            }
            StdoutFrame::Drop(method) => {
                tracing::warn!(%plugin_id, %method, "ignored a host notification this host does not know");
            }
            StdoutFrame::Callback(Ok(callback)) => {
                if callbacks.send(callback).await.is_err() {
                    break CALLBACKS_UNANSWERED_ERROR.to_string();
                }
            }
            StdoutFrame::Callback(Err(error)) => break error,
            StdoutFrame::Response => {
                if responses.send(Ok(frame)).await.is_err() {
                    return;
                }
            }
        }
    };
    let _ = failure.set(error.clone());
    let _ = responses.send(Err(error)).await;
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

/// Runs a sidecar's requests one at a time over its shared stdout. Once one leaves stdout
/// untrustworthy, such as holding the late answer of a timed-out request, every request
/// queued behind it fails without writing, so none reads that answer before the runtime is
/// retired.
#[derive(Default)]
struct RequestLane {
    turn: AsyncMutex<()>,
    broken: AtomicBool,
}

impl RequestLane {
    async fn run<F>(
        &self,
        method: &str,
        wait: Duration,
        request: impl FnOnce() -> F,
    ) -> Result<Value, PluginRpcError>
    where
        F: Future<Output = Result<Value, PluginRpcError>>,
    {
        let _turn = timeout(wait, self.turn.lock())
            .await
            .map_err(|_| request_queue_timeout_error(method))?;
        if self.is_broken() {
            return Err(PluginRpcError::NotRunning(format!(
                "plugin RPC {method} not sent: the plugin is being stopped after a broken request"
            )));
        }
        let result = request().await;
        if result.as_ref().is_err_and(PluginRpcError::is_fatal) {
            self.broken.store(true, Ordering::SeqCst);
        }
        result
    }

    /// Waits for the turn even on a broken lane: a shutdown request is safe there, since its
    /// answer is never trusted.
    async fn shutdown_turn(&self, wait: Duration) -> Option<MutexGuard<'_, ()>> {
        timeout(wait, self.turn.lock()).await.ok()
    }

    fn is_broken(&self) -> bool {
        self.broken.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod lane_tests {
    use super::*;

    #[tokio::test]
    async fn a_broken_request_stops_the_requests_queued_behind_it() {
        let lane = Arc::new(RequestLane::default());
        let (release, released) = tokio::sync::oneshot::channel::<()>();
        let first = tokio::spawn({
            let lane = lane.clone();
            async move {
                lane.run("github.reconcile", Duration::from_secs(5), || async {
                    released.await.unwrap();
                    Err(timed_out_request_error("github.reconcile"))
                })
                .await
            }
        });
        tokio::task::yield_now().await;
        let sent = Arc::new(AtomicBool::new(false));
        let queued = tokio::spawn({
            let (lane, sent) = (lane.clone(), sent.clone());
            async move {
                lane.run("tool.call", Duration::from_secs(5), || async move {
                    sent.store(true, Ordering::SeqCst);
                    Ok(Value::Null)
                })
                .await
            }
        });
        tokio::task::yield_now().await;
        release.send(()).unwrap();

        assert!(first.await.unwrap().unwrap_err().is_fatal());
        assert!(matches!(
            queued.await.unwrap(),
            Err(PluginRpcError::NotRunning(_))
        ));
        assert!(!sent.load(Ordering::SeqCst), "nothing was written");
        assert!(lane.is_broken());
        assert!(
            lane.shutdown_turn(Duration::from_secs(1)).await.is_some(),
            "a broken runtime is still asked to shut down"
        );
    }

    #[tokio::test]
    async fn an_answered_or_rejected_request_keeps_the_lane_open() {
        let lane = RequestLane::default();
        let wait = Duration::from_secs(1);
        let rejected = lane
            .run("github.link", wait, || async {
                Err(PluginRpcError::Rpc {
                    code: -32000,
                    message: "no".into(),
                })
            })
            .await;
        assert!(rejected.is_err());
        assert_eq!(
            lane.run("github.status", wait, || async { Ok(Value::Bool(true)) })
                .await,
            Ok(Value::Bool(true))
        );
        assert!(!lane.is_broken());
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Stdio;
    use tokio::process::{Child, Command};

    struct Sidecar {
        connection: PluginConnection,
        answer_after: Duration,
        answer: Value,
        _child: Child,
    }

    impl CallbackServer for Sidecar {
        fn connection(&self) -> &PluginConnection {
            &self.connection
        }

        async fn serve(&self, _method: &str, _params: Value) -> Result<Value, CallbackError> {
            tokio::time::sleep(self.answer_after).await;
            Ok(self.answer.clone())
        }
    }

    /// A sidecar running `script`, its callbacks answered with `answer` after `answer_after`.
    fn sidecar(script: &str, answer_after: Duration, answer: Value) -> Arc<Sidecar> {
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
        let sidecar = Arc::new(Sidecar {
            connection,
            answer_after,
            answer,
            _child: child,
        });
        answer_callbacks(Arc::downgrade(&sidecar), callbacks);
        sidecar
    }

    async fn stdout_failure(connection: &PluginConnection) -> String {
        for _ in 0..500 {
            if let Some(failure) = connection.stdout_failure() {
                return failure.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the stdout reader never failed");
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
        // Answers after the first request has timed out.
        let sidecar = sidecar(
            ACKNOWLEDGES_ONCE_ITS_CALLBACK_IS_ANSWERED,
            Duration::from_millis(300),
            serde_json::json!({ "ok": true }),
        );
        let connection = &sidecar.connection;

        let timed_out = connection
            .request("github.reconcile", Value::Null, Duration::from_millis(100))
            .await
            .unwrap_err();
        assert_eq!(timed_out.code(), Some(-32800), "{timed_out}");
        assert!(!connection.is_broken());

        let next = connection
            .request("github.reconcile", Value::Null, Duration::from_secs(5))
            .await;
        assert_eq!(next, Ok(serde_json::json!({ "checked": 1 })));
    }

    #[tokio::test]
    async fn a_request_queued_behind_a_long_one_keeps_its_whole_timeout() {
        let sidecar = sidecar(
            r#"
            read -r request
            sleep 0.3
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":"slow"}'
            read -r request
            sleep 0.3
            printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":"queued"}'
            read -r line
        "#,
            Duration::ZERO,
            Value::Null,
        );
        let connection = &sidecar.connection;

        let (slow, queued) = tokio::join!(
            connection.request("github.reconcile", Value::Null, Duration::from_secs(5)),
            async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                connection
                    .request("github.status", Value::Null, Duration::from_millis(500))
                    .await
            }
        );
        assert_eq!(slow, Ok(Value::from("slow")));
        assert_eq!(
            queued,
            Ok(Value::from("queued")),
            "300 ms queued + 300 ms answer"
        );
    }

    #[tokio::test]
    async fn an_unanswerable_callback_fails_the_connection() {
        // An answer too large for a frame, to a callback id so long that even the bounded
        // error that replaces it exceeds the frame limit.
        let sidecar = sidecar(
            r#"
            id=$(head -c 65460 /dev/zero | tr '\0' x)
            printf '{"jsonrpc":"2.0","id":"%s","method":"h"}\n' "$id"
            sleep 0.2
            printf '%s\n' '{"jsonrpc":"2.0","id":"c2","method":"host.state.get"}'
            read -r line
        "#,
            Duration::ZERO,
            Value::String("x".repeat(MAX_RPC_FRAME_BYTES as usize)),
        );

        assert_eq!(
            stdout_failure(&sidecar.connection).await,
            CALLBACKS_UNANSWERED_ERROR
        );
    }

    #[tokio::test]
    async fn a_malformed_callback_fails_the_waiting_request_and_ends_the_stream() {
        let sidecar = sidecar(
            r#"
            read -r request
            printf '%s\n' '{"jsonrpc":"2.0","method":"host.settings.get","id":[1]}'
            read -r line
        "#,
            Duration::ZERO,
            Value::Null,
        );

        let error = sidecar
            .connection
            .request("jira.status", Value::Null, Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(error.is_fatal(), "{error}");
        assert!(stdout_failure(&sidecar.connection)
            .await
            .contains("id must be"));
    }
}
