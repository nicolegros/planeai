use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PLUGIN_ID: &str = "jira";
const PLUGIN_NAME: &str = "Jira";
const PLUGIN_VERSION: &str = "0.1.0";
const HOST_API_VERSION: &str = "planeai.plugin-host.v1";

#[derive(Deserialize)]
struct Request {
    jsonrpc: String,
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i64,
    message: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        let request: Request = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("malformed JSON-RPC request: {error}");
                continue;
            }
        };
        let should_shutdown = request.method == "plugin.shutdown";
        let response = dispatch(request);
        let frame = serde_json::to_string(&response)?;
        stdout.write_all(frame.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
        if should_shutdown {
            break;
        }
    }
    Ok(())
}

fn dispatch(request: Request) -> Response {
    if request.jsonrpc != "2.0" {
        return error(request.id, -32600, "expected jsonrpc 2.0");
    }
    match request.method.as_str() {
        "plugin.handshake" => {
            let host_api_version = request
                .params
                .get("host_api_version")
                .and_then(Value::as_str);
            if host_api_version != Some(HOST_API_VERSION) {
                return error(request.id, -32001, "unsupported plugin host API version");
            }
            success(
                request.id,
                serde_json::json!({
                    "plugin_id": PLUGIN_ID,
                    "plugin_name": PLUGIN_NAME,
                    "plugin_version": PLUGIN_VERSION,
                    "host_api_version": HOST_API_VERSION,
                }),
            )
        }
        "jira.status" => success(
            request.id,
            serde_json::json!({
                "plugin_id": PLUGIN_ID,
                "plugin_name": PLUGIN_NAME,
                "plugin_version": PLUGIN_VERSION,
                "host_api_version": HOST_API_VERSION,
                "runtime_state": "running",
                "last_error": null,
            }),
        ),
        "plugin.shutdown" => success(request.id, serde_json::json!({ "stopping": true })),
        _ => error(request.id, -32601, "method not found"),
    }
}

fn success(id: Value, result: Value) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn error(id: Value, code: i64, message: &str) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(RpcError {
            code,
            message: message.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_is_versioned() {
        let response = dispatch(Request {
            jsonrpc: "2.0".into(),
            id: Value::from(1),
            method: "plugin.handshake".into(),
            params: serde_json::json!({ "host_api_version": HOST_API_VERSION }),
        });
        assert_eq!(response.result.unwrap()["plugin_id"], PLUGIN_ID);
    }

    #[test]
    fn status_is_read_only_identity_data() {
        let response = dispatch(Request {
            jsonrpc: "2.0".into(),
            id: Value::from(2),
            method: "jira.status".into(),
            params: Value::Null,
        });
        let status = response.result.unwrap();
        assert_eq!(status["runtime_state"], "running");
        assert_eq!(status["plugin_version"], PLUGIN_VERSION);
    }
}
