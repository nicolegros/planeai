//! The sidecar's JSON-RPC wire format: frames, requests, responses, callbacks, and the
//! typed errors a request or callback ends with.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const MAX_RPC_FRAME_BYTES: u64 = 64 * 1024;
pub(crate) const STDOUT_CLOSED_ERROR: &str = "plugin process closed stdout unexpectedly";

/// Why a request to a sidecar failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRpcError {
    /// The plugin answered with a JSON-RPC error; the connection stays usable.
    Rpc { code: i64, message: String },
    /// The request never reached the sidecar, which stays untouched.
    NotSent(String),
    /// The plugin is not running, so nothing was sent.
    NotRunning(String),
    /// Stdout or the request protocol can no longer be trusted, so the runtime is retired
    /// before another request can consume a stale or malformed frame.
    Broken(String),
}

impl PluginRpcError {
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::Broken(_))
    }

    pub fn code(&self) -> Option<i64> {
        match self {
            Self::Rpc { code, .. } => Some(*code),
            _ => None,
        }
    }
}

impl std::fmt::Display for PluginRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rpc { code, message } => write!(f, "plugin RPC error {code}: {message}"),
            Self::NotSent(message) | Self::NotRunning(message) | Self::Broken(message) => {
                f.write_str(message)
            }
        }
    }
}

impl From<PluginRpcError> for String {
    fn from(error: PluginRpcError) -> Self {
        error.to_string()
    }
}

/// Failures before the request reached the sidecar.
impl From<String> for PluginRpcError {
    fn from(message: String) -> Self {
        Self::NotSent(message)
    }
}

/// Why a host callback a sidecar made failed, which picks its JSON-RPC error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallbackError {
    NotFound,
    NotGranted,
    InvalidParams(String),
    /// The request is valid but cannot be served now; the plugin should retry it later.
    Unavailable(String),
    Internal(String),
}

impl CallbackError {
    pub(crate) fn code(&self) -> i64 {
        match self {
            Self::NotFound => -32601,
            Self::NotGranted => -32003,
            Self::InvalidParams(_) => -32602,
            Self::Unavailable(_) => -32004,
            Self::Internal(_) => -32603,
        }
    }
}

impl std::fmt::Display for CallbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("host method not found"),
            Self::NotGranted => f.write_str("plugin capability is not granted"),
            Self::InvalidParams(message) | Self::Unavailable(message) | Self::Internal(message) => {
                f.write_str(message)
            }
        }
    }
}

impl From<String> for CallbackError {
    fn from(message: String) -> Self {
        Self::Internal(message)
    }
}

impl From<&str> for CallbackError {
    fn from(message: &str) -> Self {
        Self::Internal(message.to_string())
    }
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcNotification<'a> {
    jsonrpc: &'static str,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Debug)]
pub(crate) struct JsonRpcCallbackRequest {
    pub(crate) id: Value,
    pub(crate) method: String,
    pub(crate) params: Value,
}

/// Encode one complete JSON-RPC message frame. The process transport is JSONL,
/// so every request and response must have exactly one trailing newline.
pub fn encode_json_rpc_line(id: u64, method: &str, params: Value) -> Result<String, String> {
    let value = serde_json::to_string(&JsonRpcRequest {
        jsonrpc: "2.0",
        id,
        method,
        params,
    })
    .map_err(|e| format!("failed to encode JSON-RPC request: {e}"))?;
    encode_json_rpc_frame(value, "request")
}

pub(crate) fn encode_json_rpc_cancel_request_line(request_id: u64) -> Result<String, String> {
    let value = serde_json::to_string(&JsonRpcNotification {
        jsonrpc: "2.0",
        method: "$/cancelRequest",
        params: serde_json::json!({ "id": request_id }),
    })
    .map_err(|e| format!("failed to encode JSON-RPC cancellation notification: {e}"))?;
    encode_json_rpc_frame(value, "cancellation notification")
}

fn encode_json_rpc_frame(value: String, kind: &str) -> Result<String, String> {
    let frame = format!("{value}\n");
    if frame.len() > MAX_RPC_FRAME_BYTES as usize {
        return Err(format!("plugin JSON-RPC {kind} exceeded the frame limit"));
    }
    Ok(frame)
}

/// A request too large or malformed to send fails before reaching the sidecar.
pub(crate) fn encode_request(
    id: u64,
    method: &str,
    params: Value,
) -> Result<String, PluginRpcError> {
    encode_json_rpc_line(id, method, params)
        .map_err(|error| PluginRpcError::NotSent(format!("plugin RPC request not sent: {error}")))
}

fn parse_json_rpc_response(
    line: &str,
    expected_id: u64,
) -> Result<(JsonRpcResponse, Option<Value>), String> {
    let value: Value =
        serde_json::from_str(line).map_err(|e| format!("malformed JSON-RPC response: {e}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "malformed JSON-RPC response: expected an object".to_string())?;
    let has_result = object.contains_key("result");
    if has_result == object.contains_key("error") {
        return Err(
            "malformed JSON-RPC response: expected exactly one of result or error".to_string(),
        );
    }
    // Preserve member presence separately from its JSON value: `result: null`
    // is a valid JSON-RPC success response and must not look like a missing field.
    let result = has_result.then(|| object["result"].clone());
    let response: JsonRpcResponse =
        serde_json::from_value(value).map_err(|e| format!("malformed JSON-RPC response: {e}"))?;
    if response.jsonrpc != "2.0" {
        return Err("malformed JSON-RPC response: expected jsonrpc 2.0".to_string());
    }
    if response.id.as_u64() != Some(expected_id) {
        return Err("malformed JSON-RPC response: response id did not match request".to_string());
    }
    Ok((response, result))
}

pub(crate) fn decode_json_rpc_response(
    line: &str,
    expected_id: u64,
) -> Result<Value, PluginRpcError> {
    let (response, result) =
        parse_json_rpc_response(line, expected_id).map_err(PluginRpcError::Broken)?;
    if let Some(error) = response.error {
        return Err(PluginRpcError::Rpc {
            code: error.code,
            message: error.message,
        });
    }
    result.ok_or_else(|| {
        PluginRpcError::Broken("malformed JSON-RPC response: missing result".to_string())
    })
}

pub(crate) fn decode_json_rpc_frame(frame: &str, request_id: u64) -> Result<Value, PluginRpcError> {
    let line = frame.strip_suffix('\n').ok_or_else(|| {
        PluginRpcError::Broken("plugin JSON-RPC response was not newline terminated".to_string())
    })?;
    decode_json_rpc_response(line, request_id)
}

fn decode_json_rpc_cancellation_response(
    line: &str,
    expected_id: u64,
) -> Result<PluginRpcError, String> {
    let (response, _) = parse_json_rpc_response(line, expected_id)?;
    match response.error {
        Some(error) if error.code == -32800 => Ok(PluginRpcError::Rpc {
            code: error.code,
            message: error.message,
        }),
        Some(error) => Err(format!(
            "plugin cancellation response used error code {}; expected -32800",
            error.code
        )),
        None => Err("plugin cancellation response must use error code -32800".to_string()),
    }
}

pub(crate) fn timed_out_request_error(method: &str) -> PluginRpcError {
    PluginRpcError::Broken(format!("plugin RPC {method} timed out"))
}

/// How a timed-out request ends: cancelled when the plugin acknowledged the cancellation
/// correctly, otherwise timed out, which retires the runtime.
pub(crate) fn cancellation_acknowledgement_error(
    method: &str,
    request_id: u64,
    frame: Option<&str>,
) -> PluginRpcError {
    frame
        .and_then(|frame| decode_json_rpc_cancellation_response(frame, request_id).ok())
        .unwrap_or_else(|| timed_out_request_error(method))
}

pub(crate) fn request_queue_timeout_error(method: &str) -> PluginRpcError {
    PluginRpcError::NotSent(format!("plugin RPC {method} request queue timed out"))
}

pub(crate) fn host_callback_response_fits(frame: &str) -> bool {
    frame.len() < MAX_RPC_FRAME_BYTES as usize
}

pub(crate) fn encode_host_callback_response(
    id: Value,
    result: Result<Value, CallbackError>,
) -> Result<String, String> {
    let response = match result {
        Ok(result) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": error.code(), "message": error.to_string() },
        }),
    };
    let mut frame = serde_json::to_string(&response)
        .map_err(|error| format!("failed to encode host callback response: {error}"))?;
    if !host_callback_response_fits(&frame) {
        frame = serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": response["id"],
            "error": {
                "code": -32000,
                "message": "host callback response exceeded the frame limit",
            },
        }))
        .map_err(|error| format!("failed to encode bounded host callback error: {error}"))?;
        if !host_callback_response_fits(&frame) {
            return Err(
                "plugin callback id leaves no room for a bounded host response".to_string(),
            );
        }
    }
    Ok(frame)
}

pub(crate) fn validate_plugin_response_frame(frame: &[u8]) -> Result<(), String> {
    if frame.len() > MAX_RPC_FRAME_BYTES as usize {
        return Err("plugin JSON-RPC response exceeded the frame limit".to_string());
    }
    if !frame.ends_with(b"\n") {
        return Err("plugin JSON-RPC response was not newline terminated".to_string());
    }
    Ok(())
}

pub(crate) fn parse_json_rpc_callback_request(
    value: Value,
) -> Result<JsonRpcCallbackRequest, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "malformed plugin JSON-RPC callback: expected an object".to_string())?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err("malformed plugin JSON-RPC callback: expected jsonrpc 2.0".to_string());
    }
    if object.contains_key("result") || object.contains_key("error") {
        return Err(
            "malformed plugin JSON-RPC callback: request cannot contain result or error"
                .to_string(),
        );
    }
    let id = object
        .get("id")
        .filter(|id| matches!(id, Value::String(_) | Value::Number(_)))
        .cloned()
        .ok_or_else(|| {
            "malformed plugin JSON-RPC callback: id must be a string or number".to_string()
        })?;
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .filter(|method| !method.is_empty())
        .ok_or_else(|| {
            "malformed plugin JSON-RPC callback: method must be a nonempty string".to_string()
        })?
        .to_owned();
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    if !params.is_null() && !params.is_array() && !params.is_object() {
        return Err(
            "malformed plugin JSON-RPC callback: params must be null, an array, or an object"
                .to_string(),
        );
    }
    Ok(JsonRpcCallbackRequest { id, method, params })
}

/// What the stdout reader does with a frame.
#[derive(Debug, PartialEq)]
pub(crate) enum StdoutFrame {
    /// A notification the reader routes itself.
    Route(String, Value),
    /// A `host.*` notification nothing here handles, as one from a newer host's contract.
    Drop(String),
    /// Anything else, for the request path.
    Queue,
}

pub(crate) fn classify_stdout_frame(frame: &str, routes: impl Fn(&str) -> bool) -> StdoutFrame {
    match host_notification(frame) {
        Some((method, params)) if routes(&method) => StdoutFrame::Route(method, params),
        Some((method, _)) => StdoutFrame::Drop(method),
        None => StdoutFrame::Queue,
    }
}

/// A notification to a host method: no `id`, JSON-RPC 2.0, and a `host.*` method.
/// Anything else with a method is a callback, which must carry an id.
pub(crate) fn host_notification(frame: &str) -> Option<(String, Value)> {
    let Value::Object(mut object) = serde_json::from_str::<Value>(frame.trim_end()).ok()? else {
        return None;
    };
    if object.contains_key("id") || object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return None;
    }
    let method = object
        .get("method")?
        .as_str()
        .filter(|method| method.starts_with("host."))?
        .to_string();
    Some((method, object.remove("params").unwrap_or(Value::Null)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_rpc_errors_and_unsent_requests_leave_the_runtime_running() {
        let rpc = PluginRpcError::Rpc {
            code: -32000,
            message: "rejected".into(),
        };
        assert!(!rpc.is_fatal());
        assert_eq!(rpc.to_string(), "plugin RPC error -32000: rejected");
        assert!(!request_queue_timeout_error("jira.status").is_fatal());
        let oversized = encode_request(1, "x", serde_json::json!({ "t": "x".repeat(70_000) }));
        assert!(!oversized.unwrap_err().is_fatal());
        assert!(PluginRpcError::Broken(STDOUT_CLOSED_ERROR.into()).is_fatal());
        assert!(timed_out_request_error("jira.status").is_fatal());
    }

    #[test]
    fn rpc_errors_keep_their_code() {
        let error = decode_json_rpc_response(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32010,"message":"unknown session"}}"#,
            1,
        )
        .unwrap_err();
        assert_eq!(error.code(), Some(-32010));
        assert_eq!(
            decode_json_rpc_response("not json", 1).unwrap_err().code(),
            None
        );
    }

    #[test]
    fn host_callback_errors_carry_json_rpc_codes() {
        let code = |error: CallbackError| {
            let frame = encode_host_callback_response(serde_json::json!(1), Err(error)).unwrap();
            serde_json::from_str::<Value>(&frame).unwrap()["error"]["code"].clone()
        };
        assert_eq!(code(CallbackError::NotFound), -32601);
        assert_eq!(code(CallbackError::NotGranted), -32003);
        assert_eq!(code(CallbackError::InvalidParams("bad".into())), -32602);
        assert_eq!(code(CallbackError::Unavailable("later".into())), -32004);
        assert_eq!(code(CallbackError::Internal("locked".into())), -32603);
        assert_eq!(
            CallbackError::NotGranted.to_string(),
            "plugin capability is not granted"
        );
    }

    #[test]
    fn the_stdout_reader_routes_known_notifications_and_drops_other_host_ones() {
        let routes = |method: &str| method == "host.providerSession.status";
        let status = r#"{"jsonrpc":"2.0","method":"host.providerSession.status","params":{"s":1}}"#;
        assert_eq!(
            classify_stdout_frame(status, routes),
            StdoutFrame::Route(
                "host.providerSession.status".into(),
                serde_json::json!({ "s": 1 })
            )
        );
        // Without a sink, as for a plugin without the providers capability, it is dropped.
        assert_eq!(
            classify_stdout_frame(status, |_| false),
            StdoutFrame::Drop("host.providerSession.status".into())
        );
        let future = r#"{"jsonrpc":"2.0","method":"host.future.ping"}"#;
        assert_eq!(
            classify_stdout_frame(future, routes),
            StdoutFrame::Drop("host.future.ping".into())
        );
        for queued in [
            r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"host.settings.get"}"#,
            r#"{"jsonrpc":"2.0","method":"jira.ping"}"#,
            "not json",
        ] {
            assert_eq!(
                classify_stdout_frame(queued, routes),
                StdoutFrame::Queue,
                "{queued}"
            );
        }
    }

    #[test]
    fn only_host_notifications_are_host_notifications() {
        let notification = r#"{"jsonrpc":"2.0","method":"host.future.ping","params":{"a":1}}"#;
        assert_eq!(
            host_notification(notification),
            Some(("host.future.ping".into(), serde_json::json!({ "a": 1 })))
        );
        let callback = r#"{"jsonrpc":"2.0","id":1,"method":"host.future.ping"}"#;
        assert_eq!(host_notification(callback), None);
        let not_host = r#"{"jsonrpc":"2.0","method":"jira.ping"}"#;
        assert_eq!(host_notification(not_host), None);
        let response = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
        assert_eq!(host_notification(response), None);
    }

    #[test]
    fn json_rpc_uses_newline_frames_and_validates_responses() {
        let frame = encode_json_rpc_line(7, "jira.status", Value::Null).unwrap();
        assert!(frame.ends_with('\n'));
        assert!(
            encode_json_rpc_line(7, &"x".repeat(MAX_RPC_FRAME_BYTES as usize), Value::Null)
                .unwrap_err()
                .contains("exceeded")
        );
        let value = decode_json_rpc_frame(
            r#"{"jsonrpc":"2.0","id":7,"result":{"ok":true}}
"#,
            7,
        )
        .unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(
            decode_json_rpc_response(r#"{"jsonrpc":"2.0","id":7,"result":null}"#, 7).unwrap(),
            Value::Null
        );
        let exact_limit = [vec![b'x'; MAX_RPC_FRAME_BYTES as usize - 1], vec![b'\n']].concat();
        assert!(validate_plugin_response_frame(&exact_limit).is_ok());
        let over_limit = [vec![b'x'; MAX_RPC_FRAME_BYTES as usize], vec![b'\n']].concat();
        assert!(validate_plugin_response_frame(&over_limit).is_err());
        assert!(validate_plugin_response_frame(&vec![b'x'; MAX_RPC_FRAME_BYTES as usize]).is_err());
        assert!(decode_json_rpc_response("not json", 7)
            .unwrap_err()
            .to_string()
            .contains("malformed"));
        assert!(
            decode_json_rpc_frame(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#, 7)
                .unwrap_err()
                .to_string()
                .contains("newline terminated")
        );
        assert!(
            decode_json_rpc_response(r#"{"jsonrpc":"2.0","id":8,"result":{}}"#, 7)
                .unwrap_err()
                .to_string()
                .contains("did not match")
        );
        let cancellation = encode_json_rpc_cancel_request_line(7).unwrap();
        let cancellation: Value = serde_json::from_str(cancellation.trim_end()).unwrap();
        assert_eq!(cancellation["method"], "$/cancelRequest");
        assert_eq!(cancellation["params"]["id"], 7);
        assert!(
            decode_json_rpc_cancellation_response(
                r#"{"jsonrpc":"2.0","id":7,"error":{"code":-32800,"message":"cancelled"}}"#,
                7,
            )
            .unwrap()
            .code()
                == Some(-32800)
        );
        assert!(decode_json_rpc_cancellation_response(
            r#"{"jsonrpc":"2.0","id":7,"error":{"code":-32603,"message":"failed"}}"#,
            7,
        )
        .unwrap_err()
        .contains("expected -32800"));
        for response in [
            r#"{"jsonrpc":"2.0","id":7,"result":{},"error":{"code":-32800,"message":"cancelled"}}"#,
            r#"{"jsonrpc":"2.0","id":7}"#,
        ] {
            assert!(decode_json_rpc_response(response, 7)
                .unwrap_err()
                .to_string()
                .contains("exactly one"));
        }

        let timeout_error = timed_out_request_error("jira.status");
        for acknowledgement in [
            Some("not json"),
            Some(r#"{"jsonrpc":"2.0","id":8,"error":{"code":-32800,"message":"cancelled"}}"#),
            Some(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#),
            None,
        ] {
            let error = cancellation_acknowledgement_error("jira.status", 7, acknowledgement);
            assert_eq!(error, timeout_error);
            assert!(error.is_fatal());
        }
        assert_eq!(
            cancellation_acknowledgement_error(
                "jira.status",
                7,
                Some(r#"{"jsonrpc":"2.0","id":7,"error":{"code":-32800,"message":"cancelled"}}"#),
            )
            .to_string(),
            "plugin RPC error -32800: cancelled"
        );
    }
}
