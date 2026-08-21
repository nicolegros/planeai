//! Headless local-plugin conformance harness for `planeai-cli plugin test`.
//!
//! Scenario JSONL is intentionally not interpreted yet: accepting an unspecified
//! scenario grammar would make the CLI contract unstable. `run` reports a clear
//! error for a nonempty `--scenario` rather than silently ignoring it.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};

const HOST_API_VERSION: &str = "planeai.plugin-host.v1";
const MAX_FRAME_BYTES: usize = 64 * 1024;
const RPC_TIMEOUT: Duration = Duration::from_secs(5);

pub fn run(package: &Path, scenario: Option<&Path>) -> Result<()> {
    if scenario.is_some_and(|path| !path.as_os_str().is_empty()) {
        bail!(
            "--scenario is not supported yet; scenario JSONL execution is intentionally disabled until its format is specified"
        );
    }

    let package = package
        .canonicalize()
        .with_context(|| format!("failed to resolve plugin package {}", package.display()))?;
    if !package.is_dir() {
        bail!("plugin package must be a directory: {}", package.display());
    }

    let manifest = read_manifest(&package)?;
    let entrypoint = validate_local_manifest(&manifest, current_platform_key())?;
    let executable = validate_entrypoint(&package, &entrypoint)?;

    let mut process = PluginProcess::spawn(&executable, &package)?;
    let handshake = process.call(
        1,
        "plugin.handshake",
        json!({ "host_api_version": HOST_API_VERSION }),
    )?;
    let subscriptions = validate_handshake(&handshake, &manifest)?;

    process.call(2, "fixture.status", Value::Null)?;
    if subscriptions
        .iter()
        .any(|subscription| subscription == "task.lifecycle")
    {
        process.call(
            3,
            "plugin.taskLifecycle",
            json!({
                "batch": {
                    "batch_id": "planeai-cli-plugin-test",
                    "origin": "cli-plugin-test",
                    "events": []
                }
            }),
        )?;
    }
    process.call(4, "plugin.shutdown", Value::Null)?;
    process.wait_for_exit()?;

    println!(
        "plugin test passed: {}",
        manifest["id"].as_str().unwrap_or("<unknown>")
    );
    Ok(())
}

fn read_manifest(package: &Path) -> Result<Value> {
    let path = package.join("planeai-plugin.json");
    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read plugin manifest {}", path.display()))?;
    let manifest = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse plugin manifest {}", path.display()))?;
    Ok(manifest)
}

fn validate_local_manifest(manifest: &Value, platform: &str) -> Result<String> {
    let object = manifest
        .as_object()
        .ok_or_else(|| anyhow!("plugin manifest must be a JSON object"))?;
    required_string(object, "schema")?;
    if required_string(object, "schema")? != "planeai.plugin.v1" {
        bail!("unsupported plugin manifest schema");
    }
    if required_string(object, "source_kind")? != "local" {
        bail!("plugin test only accepts local plugin packages (source_kind must be \"local\")");
    }
    for field in ["id", "name", "version", "host_api_version"] {
        required_string(object, field)?;
    }

    let entrypoints = object
        .get("backend_entrypoints")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("local plugin manifest backend_entrypoints is required"))?;
    let entrypoint = entrypoints
        .get(platform)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("plugin manifest has no backend entrypoint for {platform}"))?;
    validate_relative_path(entrypoint)?;
    Ok(entrypoint.to_owned())
}

fn required_string<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("plugin manifest {field} must be a nonempty string"))
}

fn validate_relative_path(entrypoint: &str) -> Result<()> {
    let path = Path::new(entrypoint);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || entrypoint.starts_with('\\')
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("plugin backend entrypoint must be a safe package-relative path: {entrypoint}");
    }
    Ok(())
}

fn validate_entrypoint(package: &Path, entrypoint: &str) -> Result<PathBuf> {
    let executable = package.join(entrypoint);
    let executable = executable.canonicalize().with_context(|| {
        format!(
            "plugin backend entrypoint for {} is missing: {entrypoint}",
            current_platform_key()
        )
    })?;
    if !executable.starts_with(package) {
        bail!("plugin backend entrypoint resolves outside the package: {entrypoint}");
    }
    let metadata = fs::metadata(&executable)
        .with_context(|| format!("failed to inspect plugin backend {}", executable.display()))?;
    if !metadata.is_file() {
        bail!("plugin backend entrypoint is not a file: {entrypoint}");
    }
    if !is_executable(&metadata) {
        bail!("plugin backend entrypoint is not executable: {entrypoint}");
    }
    Ok(executable)
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    true
}

fn current_platform_key() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "macos-arm64",
        ("macos", "x86_64") => "macos-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("windows", "x86_64") => "windows-x64",
        _ => "unsupported-platform",
    }
}

struct PluginProcess {
    child: Child,
    stdin: ChildStdin,
    frames: Receiver<Result<String>>,
    settings: Value,
}

impl PluginProcess {
    fn spawn(executable: &Path, package: &Path) -> Result<Self> {
        let mut child = Command::new(executable)
            .current_dir(package)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("failed to start plugin sidecar {}", executable.display()))?;
        let stdin = child.stdin.take().expect("piped child stdin exists");
        let stdout = child.stdout.take().expect("piped child stdout exists");
        let (sender, frames) = mpsc::channel();
        std::thread::spawn(move || read_frames(stdout, sender));
        Ok(Self {
            child,
            stdin,
            frames,
            settings: json!({}),
        })
    }

    fn call(&mut self, id: u64, method: &str, params: Value) -> Result<Value> {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        loop {
            match parse_frame(&self.next_line()?)? {
                Frame::Response {
                    id: response_id,
                    result,
                } => {
                    if response_id != json!(id) {
                        bail!("mismatched JSON-RPC response id: expected {id}, got {response_id}");
                    }
                    return result.map_err(|message| anyhow!("plugin RPC error: {message}"));
                }
                Frame::Request { id, method, params } => {
                    self.handle_host_request(id, &method, params)?
                }
            }
        }
    }

    fn send(&mut self, frame: Value) -> Result<()> {
        let frame = serde_json::to_string(&frame).expect("JSON value serializes");
        if frame.len() > MAX_FRAME_BYTES {
            bail!("plugin JSON-RPC request exceeded the frame limit");
        }
        self.stdin
            .write_all(frame.as_bytes())
            .context("failed to write plugin JSON-RPC request")?;
        self.stdin
            .write_all(b"\n")
            .context("failed to terminate plugin JSON-RPC request")?;
        self.stdin.flush().context("failed to flush plugin request")
    }

    fn next_line(&self) -> Result<String> {
        match self.frames.recv_timeout(RPC_TIMEOUT) {
            Ok(frame) => frame,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                bail!("timed out waiting for plugin JSON-RPC output")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("plugin closed stdout before responding")
            }
        }
    }

    fn handle_host_request(
        &mut self,
        id: Value,
        method: &str,
        params: Option<Value>,
    ) -> Result<()> {
        let result = match method {
            "host.settings.get" => json!({ "settings": self.settings.clone() }),
            "host.settings.replace" => {
                let params = params.unwrap_or(Value::Null);
                let settings = params.get("settings").cloned().unwrap_or(params);
                if !settings.is_object() {
                    bail!("malformed host.settings.replace callback: settings must be an object");
                }
                self.settings = settings.clone();
                json!({ "settings": settings })
            }
            _ => bail!("unexpected JSON-RPC request from plugin: {method}"),
        };
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    fn wait_for_exit(&mut self) -> Result<()> {
        let deadline = std::time::Instant::now() + RPC_TIMEOUT;
        loop {
            if let Some(status) = self
                .child
                .try_wait()
                .context("failed to inspect plugin sidecar")?
            {
                if !status.success() {
                    bail!("plugin sidecar exited with {status}");
                }
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                bail!("timed out waiting for plugin sidecar to exit after shutdown");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_frames(stdout: impl std::io::Read, sender: mpsc::Sender<Result<String>>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut bytes = Vec::new();
        let read = reader
            .by_ref()
            .take((MAX_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut bytes);
        match read {
            Ok(0) => break,
            Ok(_) if bytes.len() > MAX_FRAME_BYTES || !bytes.ends_with(b"\n") => {
                let _ = sender.send(Err(anyhow!(
                    "plugin JSON-RPC output exceeded the frame limit"
                )));
                break;
            }
            Ok(_) => {
                bytes.pop();
                let frame = String::from_utf8(bytes)
                    .map_err(|error| anyhow!("plugin JSON-RPC output was not UTF-8: {error}"));
                if sender.send(frame).is_err() {
                    break;
                }
            }
            Err(error) => {
                let _ = sender.send(Err(anyhow!(
                    "failed to read plugin JSON-RPC output: {error}"
                )));
                break;
            }
        }
    }
}

enum Frame {
    Request {
        id: Value,
        method: String,
        params: Option<Value>,
    },
    Response {
        id: Value,
        result: std::result::Result<Value, String>,
    },
}

fn parse_frame(line: &str) -> Result<Frame> {
    let value: Value = serde_json::from_str(line)
        .map_err(|error| anyhow!("malformed JSON-RPC output: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("malformed JSON-RPC output: frame must be an object"))?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        bail!("malformed JSON-RPC output: expected jsonrpc 2.0");
    }
    let id = valid_id(object.get("id"))?;
    if let Some(method) = object.get("method") {
        if object.contains_key("result") || object.contains_key("error") {
            bail!("malformed JSON-RPC output: request cannot contain result or error");
        }
        return Ok(Frame::Request {
            id,
            method: method
                .as_str()
                .filter(|method| !method.is_empty())
                .ok_or_else(|| {
                    anyhow!("malformed JSON-RPC output: method must be a nonempty string")
                })?
                .to_owned(),
            params: object.get("params").cloned(),
        });
    }
    let has_result = object.contains_key("result");
    let has_error = object.contains_key("error");
    if has_result == has_error {
        bail!("malformed JSON-RPC output: response must contain exactly one of result or error");
    }
    if has_error {
        let error = object["error"]
            .as_object()
            .filter(|error| {
                error.get("code").and_then(Value::as_i64).is_some()
                    && error.get("message").and_then(Value::as_str).is_some()
            })
            .ok_or_else(|| anyhow!("malformed JSON-RPC output: invalid error object"))?;
        return Ok(Frame::Response {
            id,
            result: Err(format!(
                "{}: {}",
                error["code"],
                error["message"].as_str().expect("validated string")
            )),
        });
    }
    Ok(Frame::Response {
        id,
        result: Ok(object["result"].clone()),
    })
}

fn valid_id(id: Option<&Value>) -> Result<Value> {
    match id {
        Some(Value::String(_)) | Some(Value::Number(_)) => Ok(id.expect("matched some").clone()),
        _ => bail!("malformed JSON-RPC output: id must be a string or number"),
    }
}

fn validate_handshake(result: &Value, manifest: &Value) -> Result<Vec<String>> {
    let result = result
        .as_object()
        .ok_or_else(|| anyhow!("malformed plugin.handshake result: expected object"))?;
    for (field, manifest_field) in [
        ("plugin_id", "id"),
        ("plugin_name", "name"),
        ("plugin_version", "version"),
        ("host_api_version", "host_api_version"),
    ] {
        if result.get(field) != manifest.get(manifest_field) {
            bail!("mismatched plugin.handshake result: {field} does not match manifest");
        }
    }
    if result.get("host_api_version").and_then(Value::as_str) != Some(HOST_API_VERSION) {
        bail!("mismatched plugin.handshake result: unsupported host API version");
    }
    result
        .get("lifecycle_event_subscriptions")
        .cloned()
        .unwrap_or_else(|| json!([]))
        .as_array()
        .ok_or_else(|| {
            anyhow!("malformed plugin.handshake result: subscriptions must be an array")
        })?
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                anyhow!("malformed plugin.handshake result: subscriptions must be strings")
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Value {
        json!({
            "schema": "planeai.plugin.v1",
            "id": "fixture",
            "name": "Fixture",
            "version": "1.0.0",
            "host_api_version": HOST_API_VERSION,
            "source_kind": "local",
            "backend_entrypoints": { "test-platform": "bin/plugin" }
        })
    }

    #[test]
    fn manifest_validation_requires_local_schema_and_platform_entrypoint() {
        assert_eq!(
            validate_local_manifest(&manifest(), "test-platform").unwrap(),
            "bin/plugin"
        );
        let mut unsupported_schema = manifest();
        unsupported_schema["schema"] = json!("other");
        assert!(validate_local_manifest(&unsupported_schema, "test-platform").is_err());
        assert!(validate_local_manifest(&manifest(), "missing-platform").is_err());
    }

    #[test]
    fn manifest_validation_rejects_non_local_and_unsafe_entrypoints() {
        let mut non_local = manifest();
        non_local["source_kind"] = json!("builtin");
        assert!(validate_local_manifest(&non_local, "test-platform").is_err());
        let mut traversal = manifest();
        traversal["backend_entrypoints"]["test-platform"] = json!("../plugin");
        assert!(validate_local_manifest(&traversal, "test-platform").is_err());
    }

    #[test]
    fn frame_validation_accepts_a_well_formed_response_and_callback() {
        let response = parse_frame(r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#).unwrap();
        match response {
            Frame::Response { id, result } => {
                assert_eq!(id, json!(1));
                assert_eq!(result.unwrap(), json!({ "ok": true }));
            }
            Frame::Request { .. } => panic!("expected response"),
        }
        match parse_frame(
            r#"{"jsonrpc":"2.0","id":"get","method":"host.settings.get","params":null}"#,
        )
        .unwrap()
        {
            Frame::Request { method, .. } => assert_eq!(method, "host.settings.get"),
            Frame::Response { .. } => panic!("expected request"),
        }
    }

    #[test]
    fn frame_validation_rejects_invalid_or_ambiguous_output() {
        assert!(parse_frame(r#"{"jsonrpc":"1.0","id":1,"result":{}}"#).is_err());
        assert!(parse_frame(r#"{"jsonrpc":"2.0","id":1,"result":{},"error":{}}"#).is_err());
        assert!(parse_frame(r#"{"jsonrpc":"2.0","method":"host.settings.get"}"#).is_err());
    }
}
