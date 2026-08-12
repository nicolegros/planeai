use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

const MAX_RPC_FRAME_BYTES: u64 = 64 * 1024;

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
    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();

    while let Some(line) = read_json_rpc_frame(&mut stdin).await? {
        let request: Request = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("malformed JSON-RPC request: {error}");
                continue;
            }
        };
        let should_shutdown = is_valid_shutdown_request(&request);
        let response = dispatch(request);
        let frame = encode_json_rpc_response(&response)?;
        stdout.write_all(frame.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
        if should_shutdown {
            break;
        }
    }
    Ok(())
}

fn encode_json_rpc_response(response: &Response) -> Result<String, serde_json::Error> {
    let frame = serde_json::to_string(response)?;
    if frame.len() < MAX_RPC_FRAME_BYTES as usize {
        return Ok(frame);
    }
    serde_json::to_string(&error(
        Value::Null,
        -32600,
        "JSON-RPC response exceeded the frame limit",
    ))
}

async fn read_json_rpc_frame<R>(reader: &mut R) -> Result<Option<String>, std::io::Error>
where
    R: AsyncBufRead + Unpin,
{
    let mut frame = Vec::new();
    let bytes_read = reader
        .take(MAX_RPC_FRAME_BYTES)
        .read_until(b'\n', &mut frame)
        .await?;
    if bytes_read == 0 {
        return Ok(None);
    }
    if !frame.ends_with(b"\n") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "JSON-RPC request exceeded the frame limit or was not newline terminated",
        ));
    }
    let frame = String::from_utf8(frame).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("JSON-RPC request was not valid UTF-8: {error}"),
        )
    })?;
    Ok(Some(frame))
}

fn is_valid_json_rpc_id(id: &Value) -> bool {
    id.is_null() || id.is_string() || id.is_number()
}

fn is_valid_shutdown_request(request: &Request) -> bool {
    request.jsonrpc == "2.0"
        && request.method == "plugin.shutdown"
        && is_valid_json_rpc_id(&request.id)
}

fn dispatch(request: Request) -> Response {
    if !is_valid_json_rpc_id(&request.id) {
        return error(Value::Null, -32600, "expected a scalar JSON-RPC id");
    }
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
    fn oversized_responses_fall_back_to_a_bounded_error_frame() {
        let response = error(
            Value::String("x".repeat(MAX_RPC_FRAME_BYTES as usize)),
            -32601,
            "method not found",
        );
        let frame = encode_json_rpc_response(&response).unwrap();
        assert!(frame.len() < MAX_RPC_FRAME_BYTES as usize);
        let value: Value = serde_json::from_str(&frame).unwrap();
        assert!(value["id"].is_null());
        assert_eq!(value["error"]["code"], -32600);
    }

    #[tokio::test]
    async fn json_rpc_requests_require_bounded_newline_frames() {
        let mut valid = BufReader::new(std::io::Cursor::new(b"{}\n".to_vec()));
        assert_eq!(
            read_json_rpc_frame(&mut valid).await.unwrap(),
            Some("{}\n".into())
        );

        let mut unterminated = BufReader::new(std::io::Cursor::new(b"{}".to_vec()));
        assert_eq!(
            read_json_rpc_frame(&mut unterminated)
                .await
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );

        let mut oversized = BufReader::new(std::io::Cursor::new(vec![
            b'x';
            MAX_RPC_FRAME_BYTES as usize
        ]));
        assert_eq!(
            read_json_rpc_frame(&mut oversized)
                .await
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn invalid_shutdown_requests_do_not_terminate_the_runtime() {
        let invalid_version_shutdown = Request {
            jsonrpc: "1.0".into(),
            id: Value::from(3),
            method: "plugin.shutdown".into(),
            params: Value::Null,
        };
        assert!(!is_valid_shutdown_request(&invalid_version_shutdown));
        assert_eq!(
            dispatch(invalid_version_shutdown).error.unwrap().code,
            -32600
        );

        let invalid_id_shutdown = Request {
            jsonrpc: "2.0".into(),
            id: serde_json::json!({ "invalid": true }),
            method: "plugin.shutdown".into(),
            params: Value::Null,
        };
        assert!(!is_valid_shutdown_request(&invalid_id_shutdown));
        let invalid_id_response = dispatch(invalid_id_shutdown);
        assert!(invalid_id_response.id.is_null());
        assert_eq!(invalid_id_response.error.unwrap().code, -32600);

        let handshake = dispatch(Request {
            jsonrpc: "2.0".into(),
            id: Value::from(4),
            method: "plugin.handshake".into(),
            params: serde_json::json!({ "host_api_version": HOST_API_VERSION }),
        });
        assert_eq!(handshake.result.unwrap()["plugin_id"], PLUGIN_ID);
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
