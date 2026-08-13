use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PLUGIN_ID: &str = "local-fixture";
const PLUGIN_NAME: &str = "Local Fixture";
const PLUGIN_VERSION: &str = "0.1.0";
const HOST_API_VERSION: &str = "planeai.plugin-host.v1";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    let mut output = tokio::io::stdout();
    while let Some(line) = input.next_line().await? {
        let request: Value = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(_) => continue,
        };
        let (response, should_shutdown) = dispatch(&request);
        output
            .write_all(serde_json::to_string(&response)?.as_bytes())
            .await?;
        output.write_all(b"\n").await?;
        output.flush().await?;
        if should_shutdown {
            break;
        }
    }
    Ok(())
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
    fn handshake_returns_fixture_identity_for_the_supported_host_api() {
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
