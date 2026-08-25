use serde_json::{json, Value};
use std::path::Path;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines, Stdin, Stdout};

const PLUGIN_ID: &str = "local-fixture";
const PLUGIN_NAME: &str = "Local Fixture";
const PLUGIN_VERSION: &str = "0.1.0";
const HOST_API_VERSION: &str = "planeai.plugin-host.v1";
const SETTINGS_GET_ID: &str = "fixture-settings-get";
const SETTINGS_REPLACE_ID: &str = "fixture-settings-replace";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("{PLUGIN_ID} starting");
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    let mut output = tokio::io::stdout();
    while let Some(line) = input.next_line().await? {
        let request: Value = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("{PLUGIN_ID} ignored malformed JSON-RPC frame: {error}");
                continue;
            }
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let response = if method == "fixture.persistSettings" {
            match persist_settings(&request, &mut input, &mut output).await {
                Ok(result) => success(id, result),
                Err(message) => error(id, -32000, &message),
            }
        } else if method == "fixture.awaitCancellation" {
            await_cancellation(&request, &mut input).await
        } else if method == "fixture.requireStateDirectories" {
            match required_state_directories() {
                Ok(result) => success(id, result),
                Err(message) => error(id, -32000, &message),
            }
        } else {
            dispatch(&request).0
        };
        if request.get("id").is_some() {
            write_frame(&mut output, &response).await?;
        }
        if method == "plugin.shutdown" {
            eprintln!("{PLUGIN_ID} shutting down");
            break;
        }
    }
    Ok(())
}

async fn await_cancellation(request: &Value, input: &mut Lines<BufReader<Stdin>>) -> Value {
    let request_id = request.get("id").cloned().unwrap_or(Value::Null);
    loop {
        let line = match input.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => return error(request_id, -32000, "host closed stdin before cancellation"),
            Err(read_error) => {
                return error(
                    request_id,
                    -32000,
                    &format!("failed to read cancellation: {read_error}"),
                )
            }
        };
        let notification: Value = match serde_json::from_str(&line) {
            Ok(notification) => notification,
            Err(_) => continue,
        };
        if cancellation_matches(&notification, &request_id) {
            return error(request_id, -32800, "request cancelled");
        }
    }
}

fn cancellation_matches(notification: &Value, request_id: &Value) -> bool {
    notification.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
        && notification.get("method").and_then(Value::as_str) == Some("$/cancelRequest")
        && notification
            .get("params")
            .and_then(|params| params.get("id"))
            == Some(request_id)
}

fn required_state_directories() -> Result<Value, String> {
    let data_dir = std::env::var("PLANEAI_PLUGIN_DATA_DIR")
        .map_err(|_| "PLANEAI_PLUGIN_DATA_DIR was not provided by the host".to_string())?;
    let secrets_dir = std::env::var("PLANEAI_PLUGIN_SECRETS_DIR")
        .map_err(|_| "PLANEAI_PLUGIN_SECRETS_DIR was not provided by the host".to_string())?;
    if !Path::new(&data_dir).is_dir() || !Path::new(&secrets_dir).is_dir() {
        return Err("host-provided plugin state directories must exist".to_string());
    }
    Ok(json!({ "state_directories_present": true }))
}

async fn persist_settings(
    request: &Value,
    input: &mut Lines<BufReader<Stdin>>,
    output: &mut Stdout,
) -> Result<Value, String> {
    let settings = settings_from(request)?;
    let previous = host_call(
        input,
        output,
        SETTINGS_GET_ID,
        "host.settings.get",
        Value::Null,
    )
    .await?;
    let saved = host_call(
        input,
        output,
        SETTINGS_REPLACE_ID,
        "host.settings.replace",
        json!({ "settings": settings }),
    )
    .await?;
    Ok(json!({
        "previous_settings": previous.get("settings").cloned().unwrap_or_else(|| json!({})),
        "settings": saved.get("settings").cloned().unwrap_or_else(|| json!({})),
    }))
}

async fn host_call(
    input: &mut Lines<BufReader<Stdin>>,
    output: &mut Stdout,
    id: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    write_frame(
        output,
        &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
    )
    .await
    .map_err(|error| format!("failed to write host callback: {error}"))?;
    loop {
        let line = input
            .next_line()
            .await
            .map_err(|error| format!("failed to read host callback response: {error}"))?
            .ok_or("host closed stdin while handling callback")?;
        let response: Value = serde_json::from_str(&line)
            .map_err(|error| format!("host callback response was not JSON: {error}"))?;
        if response.get("id").and_then(Value::as_str) != Some(id) {
            continue;
        }
        if let Some(error) = response.get("error") {
            return Err(format!("host callback failed: {error}"));
        }
        return response
            .get("result")
            .cloned()
            .ok_or_else(|| "host callback response had no result".to_string());
    }
}

async fn write_frame(output: &mut Stdout, frame: &Value) -> Result<(), std::io::Error> {
    output
        .write_all(
            serde_json::to_string(frame)
                .expect("JSON value serializes")
                .as_bytes(),
        )
        .await?;
    output.write_all(b"\n").await?;
    output.flush().await
}

fn settings_from(request: &Value) -> Result<Value, String> {
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let settings = params.get("settings").cloned().unwrap_or(params);
    if !settings.is_object() {
        return Err("fixture.persistSettings requires a JSON object settings value".to_string());
    }
    Ok(settings)
}

fn dispatch(request: &Value) -> (Value, bool) {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let response = match method {
        "plugin.handshake" => {
            if request
                .get("params")
                .and_then(|params| params.get("host_api_version"))
                .and_then(Value::as_str)
                != Some(HOST_API_VERSION)
            {
                error(id, -32001, "unsupported plugin host API version")
            } else {
                success(
                    id,
                    json!({
                        "plugin_id": PLUGIN_ID,
                        "plugin_name": PLUGIN_NAME,
                        "plugin_version": PLUGIN_VERSION,
                        "host_api_version": HOST_API_VERSION,
                        "lifecycle_event_subscriptions": ["task.lifecycle"],
                    }),
                )
            }
        }
        "fixture.status" => success(
            id,
            json!({
                "plugin_id": PLUGIN_ID,
                "runtime_state": "running",
                "data_dir": std::env::var("PLANEAI_PLUGIN_DATA_DIR").ok(),
                "secrets_dir": std::env::var("PLANEAI_PLUGIN_SECRETS_DIR").ok(),
            }),
        ),
        "plugin.taskLifecycle" => {
            eprintln!(
                "{PLUGIN_ID} received task lifecycle event: {}",
                request["params"]
            );
            success(id, json!({ "received": true }))
        }
        "plugin.shutdown" => success(id, json!({ "stopping": true })),
        _ => error(id, -32601, "method not found"),
    };
    (response, method == "plugin.shutdown")
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_returns_fixture_identity_and_lifecycle_subscription() {
        let (response, should_shutdown) = dispatch(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "plugin.handshake",
            "params": { "host_api_version": HOST_API_VERSION },
        }));

        assert!(!should_shutdown);
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 1);
        assert_eq!(response["result"]["plugin_id"], PLUGIN_ID);
        assert_eq!(response["result"]["plugin_name"], PLUGIN_NAME);
        assert_eq!(response["result"]["plugin_version"], PLUGIN_VERSION);
        assert_eq!(response["result"]["host_api_version"], HOST_API_VERSION);
        assert_eq!(
            response["result"]["lifecycle_event_subscriptions"],
            json!(["task.lifecycle"])
        );
    }

    #[test]
    fn status_reports_running_fixture_state() {
        let (response, should_shutdown) = dispatch(&json!({
            "jsonrpc": "2.0",
            "id": "status",
            "method": "fixture.status",
        }));

        assert!(!should_shutdown);
        assert_eq!(response["id"], "status");
        assert_eq!(response["result"]["plugin_id"], PLUGIN_ID);
        assert_eq!(response["result"]["runtime_state"], "running");
        assert!(response["result"].get("data_dir").is_some());
        assert!(response["result"].get("secrets_dir").is_some());
    }

    #[test]
    fn settings_payload_accepts_wrapped_or_direct_objects_only() {
        assert_eq!(
            settings_from(&json!({ "params": { "settings": { "greeting": "hello" } } })).unwrap(),
            json!({ "greeting": "hello" })
        );
        assert_eq!(
            settings_from(&json!({ "params": { "greeting": "hello" } })).unwrap(),
            json!({ "greeting": "hello" })
        );
        assert!(settings_from(&json!({ "params": ["not", "an", "object"] })).is_err());
    }

    #[test]
    fn cancellation_notifications_match_only_the_active_request_id() {
        let request_id = json!(7);
        assert!(cancellation_matches(
            &json!({
                "jsonrpc": "2.0",
                "method": "$/cancelRequest",
                "params": { "id": 7 },
            }),
            &request_id,
        ));
        assert!(!cancellation_matches(
            &json!({
                "jsonrpc": "2.0",
                "method": "$/cancelRequest",
                "params": { "id": 8 },
            }),
            &request_id,
        ));
    }

    #[test]
    fn lifecycle_delivery_is_acknowledged() {
        let (response, should_shutdown) = dispatch(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "plugin.taskLifecycle",
            "params": { "batch": { "batch_id": "test" } },
        }));

        assert!(!should_shutdown);
        assert_eq!(response["result"], json!({ "received": true }));
    }

    #[test]
    fn shutdown_acknowledges_and_signals_the_jsonl_loop_to_exit() {
        let (response, should_shutdown) = dispatch(&json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "plugin.shutdown",
        }));

        assert!(should_shutdown);
        assert_eq!(response["id"], 3);
        assert_eq!(response["result"], json!({ "stopping": true }));
    }
}
