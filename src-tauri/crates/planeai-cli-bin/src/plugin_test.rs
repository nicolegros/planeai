//! Headless local-plugin conformance harness for `planeai-cli plugin test`.
//!
//! Optional scenarios are stable JSONL request sequences executed after the
//! handshake, before the standard lifecycle and shutdown checks.

use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};

const HOST_API_VERSION: &str = "planeai.plugin-host.v1";
const MAX_FRAME_BYTES: usize = 64 * 1024;
const RPC_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const CANCELLATION_ACK_TIMEOUT: Duration = Duration::from_secs(3);

pub fn run(package: &Path, scenario: Option<&Path>) -> Result<()> {
    let scenario = scenario.map(read_scenario).transpose()?;

    let package = package
        .canonicalize()
        .with_context(|| format!("failed to resolve plugin package {}", package.display()))?;
    if !package.is_dir() {
        bail!("plugin package must be a directory: {}", package.display());
    }

    let manifest = read_manifest(&package)?;
    let entrypoint = validate_local_manifest(&manifest, current_platform_key())?;
    let capabilities = manifest_capabilities(&manifest);
    validate_ui_entrypoints(&package, &manifest)?;
    let executable = validate_entrypoint(&package, &entrypoint)?;

    let mut process = PluginProcess::spawn(&executable, &package, &capabilities)?;
    let handshake = process.call(1, "plugin.handshake", handshake_params(&capabilities))?;
    let subscriptions = validate_handshake(&handshake, &manifest)?;
    let mut request_id = 2;

    for request in scenario.unwrap_or_default() {
        match request.timeout {
            Some(timeout) => process.call_until_cancelled(
                request_id,
                &request.method,
                request.params,
                timeout,
            )?,
            None => {
                process.call(request_id, &request.method, request.params)?;
            }
        }
        request_id += 1;
    }
    if lifecycle_delivery_is_granted(&capabilities, &subscriptions) {
        process.call(
            request_id,
            "plugin.taskLifecycle",
            json!({
                "batch": {
                    "batch_id": "planeai-cli-plugin-test",
                    "origin": "cli-plugin-test",
                    "events": []
                }
            }),
        )?;
        request_id += 1;
    }
    let shutdown_deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    process.call_before_deadline(
        request_id,
        "plugin.shutdown",
        Value::Null,
        shutdown_deadline,
    )?;
    process.wait_for_exit_before(shutdown_deadline)?;

    println!(
        "plugin test passed: {}",
        manifest["id"].as_str().unwrap_or("<unknown>")
    );
    Ok(())
}

#[derive(Debug, PartialEq)]
struct ScenarioRequest {
    method: String,
    params: Value,
    timeout: Option<Duration>,
}

fn read_scenario(path: &Path) -> Result<Vec<ScenarioRequest>> {
    let file = fs::File::open(path)
        .with_context(|| format!("failed to read scenario {}", path.display()))?;
    BufReader::new(file)
        .lines()
        .enumerate()
        .filter_map(|(index, line)| match line {
            Ok(line) if line.trim().is_empty() => None,
            Ok(line) => Some(parse_scenario_line(&line).with_context(|| {
                format!("invalid scenario {} line {}", path.display(), index + 1)
            })),
            Err(error) => Some(Err(anyhow!(
                "failed to read scenario {} line {}: {error}",
                path.display(),
                index + 1
            ))),
        })
        .collect()
}

fn parse_scenario_line(line: &str) -> Result<ScenarioRequest> {
    let value: Value = serde_json::from_str(line)
        .map_err(|error| anyhow!("scenario line is not valid JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("scenario line must be a JSON object"))?;
    reject_unknown_fields(object, &["method", "params", "timeout_ms"], "scenario line")?;
    let method = required_string(object, "method")?.to_owned();
    if is_host_controlled_method(&method) {
        bail!("scenario may not send host-controlled method: {method}");
    }
    let timeout = match object.get("timeout_ms") {
        None => None,
        Some(value) => {
            let milliseconds = value
                .as_u64()
                .filter(|milliseconds| {
                    *milliseconds > 0 && *milliseconds <= RPC_TIMEOUT.as_millis() as u64
                })
                .ok_or_else(|| {
                    anyhow!(
                        "scenario timeout_ms must be an integer between 1 and {}",
                        RPC_TIMEOUT.as_millis()
                    )
                })?;
            Some(Duration::from_millis(milliseconds))
        }
    };
    Ok(ScenarioRequest {
        method,
        params: object.get("params").cloned().unwrap_or(Value::Null),
        timeout,
    })
}

fn is_host_controlled_method(method: &str) -> bool {
    matches!(
        method,
        "plugin.handshake" | "plugin.shutdown" | "plugin.taskLifecycle" | "$/cancelRequest"
    )
}

fn manifest_capabilities(manifest: &Value) -> Vec<String> {
    manifest
        .get("capabilities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|capability| {
            capability
                .as_str()
                .expect("manifest capabilities were validated")
                .to_owned()
        })
        .collect()
}

fn handshake_params(capabilities: &[String]) -> Value {
    json!({
        "host_api_version": HOST_API_VERSION,
        "host_capabilities": capabilities,
    })
}

fn lifecycle_delivery_is_granted(capabilities: &[String], subscriptions: &[String]) -> bool {
    capabilities
        .iter()
        .any(|capability| capability == "task-events")
        && subscriptions
            .iter()
            .any(|subscription| subscription == "task.lifecycle")
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
    planeai_plugin_contract::validate_local_manifest(manifest, platform)
}

fn reject_unknown_fields(
    object: &Map<String, Value>,
    allowed: &[&str],
    subject: &str,
) -> Result<()> {
    if let Some(field) = object
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        bail!("{subject} contains undocumented field: {field}");
    }
    Ok(())
}

fn required_string<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("plugin manifest {field} must be a nonempty string"))
}

fn validate_ui_entrypoints(package: &Path, manifest: &Value) -> Result<()> {
    let Some(contributions) = manifest.get("ui_contributions").and_then(Value::as_array) else {
        return Ok(());
    };
    for contribution in contributions {
        let entrypoint = contribution["entrypoint"]
            .as_str()
            .expect("UI contribution entrypoint was validated");
        if !package.join(entrypoint).is_file() {
            bail!("plugin UI contribution entrypoint is missing: {entrypoint}");
        }
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

struct TemporaryPluginState {
    root: PathBuf,
    data_dir: PathBuf,
    secrets_dir: PathBuf,
}

impl TemporaryPluginState {
    fn new() -> Result<Self> {
        for attempt in 0..10 {
            let entropy = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "planeai-plugin-test-{}-{entropy}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&root) {
                Ok(()) => {
                    let data_dir = root.join("data");
                    let secrets_dir = root.join("secrets");
                    if let Err(error) = fs::create_dir_all(&data_dir)
                        .and_then(|()| fs::create_dir_all(&secrets_dir))
                    {
                        let _ = fs::remove_dir_all(&root);
                        return Err(error)
                            .context("failed to create plugin test state directories");
                    }
                    return Ok(Self {
                        root,
                        data_dir,
                        secrets_dir,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error).context("failed to create plugin test state root"),
            }
        }
        bail!("failed to create a unique plugin test state root")
    }
}

impl Drop for TemporaryPluginState {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct PluginProcess {
    child: Child,
    writer: Sender<WriteFrame>,
    frames: Receiver<Result<String>>,
    settings: Value,
    capabilities: HashSet<String>,
    _state: TemporaryPluginState,
}

struct WriteFrame {
    frame: String,
    completed: Sender<Result<()>>,
}

impl PluginProcess {
    fn spawn(executable: &Path, package: &Path, capabilities: &[String]) -> Result<Self> {
        let state = TemporaryPluginState::new()?;
        let mut command = Command::new(executable);
        command
            .current_dir(package)
            .env("PLANEAI_PLUGIN_DATA_DIR", &state.data_dir)
            .env("PLANEAI_PLUGIN_SECRETS_DIR", &state.secrets_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        planeai_core::command::no_window(&mut command);
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to start plugin sidecar {}", executable.display()))?;
        let stdin = child.stdin.take().expect("piped child stdin exists");
        let stdout = child.stdout.take().expect("piped child stdout exists");
        let (writer, write_frames) = mpsc::channel();
        std::thread::spawn(move || write_frames_to_plugin(stdin, write_frames));
        let (sender, frames) = mpsc::channel();
        std::thread::spawn(move || read_frames(stdout, sender));
        Ok(Self {
            child,
            writer,
            frames,
            settings: json!({}),
            capabilities: capabilities.iter().cloned().collect(),
            _state: state,
        })
    }

    fn call(&mut self, id: u64, method: &str, params: Value) -> Result<Value> {
        let deadline = Instant::now() + RPC_TIMEOUT;
        self.send_before(
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
            deadline,
        )?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        match self.await_response(id, remaining)? {
            Some(Ok(result)) => Ok(result),
            Some(Err(error)) => bail!("plugin RPC error {}: {}", error.code, error.message),
            None => {
                self.verify_cancellation(id)?;
                bail!("timed out waiting for plugin JSON-RPC output")
            }
        }
    }

    fn call_before_deadline(
        &mut self,
        id: u64,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value> {
        self.send_before(
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
            deadline,
        )?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        self.await_response(id, remaining)?
            .ok_or_else(|| anyhow!("timed out waiting for plugin JSON-RPC output"))?
            .map_err(|error| anyhow!("plugin RPC error {}: {}", error.code, error.message))
    }

    fn call_until_cancelled(
        &mut self,
        id: u64,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<()> {
        let deadline = Instant::now() + timeout;
        self.send_before(
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
            deadline,
        )?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if let Some(response) = self.await_response(id, remaining)? {
            match response {
                Ok(_) => bail!("plugin responded before the cancellation deadline"),
                Err(error) => bail!(
                    "plugin returned error {} before the cancellation deadline: {}",
                    error.code,
                    error.message
                ),
            }
        }
        self.verify_cancellation(id)
    }

    fn verify_cancellation(&mut self, id: u64) -> Result<()> {
        let deadline = Instant::now() + CANCELLATION_ACK_TIMEOUT;
        self.send_before(
            json!({
                "jsonrpc": "2.0",
                "method": "$/cancelRequest",
                "params": { "id": id },
            }),
            deadline,
        )?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        match self
            .await_response(id, remaining)?
            .ok_or_else(|| anyhow!("plugin did not acknowledge cancellation"))?
        {
            Err(error) if error.code == -32800 => Ok(()),
            Err(error) => bail!(
                "plugin cancellation response used error code {}; expected -32800",
                error.code
            ),
            Ok(_) => bail!("plugin cancellation response must use error code -32800"),
        }
    }

    fn await_response(
        &mut self,
        expected_id: u64,
        timeout: Duration,
    ) -> Result<Option<std::result::Result<Value, RpcError>>> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            let line = match self.frames.recv_timeout(remaining) {
                Ok(frame) => frame?,
                Err(mpsc::RecvTimeoutError::Timeout) => return Ok(None),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    bail!("plugin closed stdout before responding")
                }
            };
            match parse_frame(&line)? {
                Frame::Response {
                    id: response_id,
                    result,
                } => {
                    if response_id != json!(expected_id) {
                        bail!("mismatched JSON-RPC response id: expected {expected_id}, got {response_id}");
                    }
                    return Ok(Some(result));
                }
                Frame::Request { id, method, params } => {
                    self.handle_host_request(id, &method, params)?
                }
            }
        }
    }

    fn send(&self, frame: Value) -> Result<()> {
        self.send_before(frame, Instant::now() + RPC_TIMEOUT)
    }

    fn send_before(&self, frame: Value, deadline: Instant) -> Result<()> {
        let frame = serde_json::to_string(&frame).expect("JSON value serializes");
        validate_outbound_frame_size(&frame)?;
        let (completed, result) = mpsc::channel();
        self.writer
            .send(WriteFrame { frame, completed })
            .map_err(|_| anyhow!("plugin JSON-RPC writer stopped unexpectedly"))?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        match result.recv_timeout(remaining) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                bail!("timed out writing plugin JSON-RPC request")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("plugin JSON-RPC writer stopped before completing a request")
            }
        }
    }

    fn handle_host_request(
        &mut self,
        id: Value,
        method: &str,
        params: Option<Value>,
    ) -> Result<()> {
        match self.handle_host_callback(method, params) {
            Ok(Some(result)) => self.send(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            })),
            Ok(None) => self.send(host_method_not_found_response(id)),
            Err(error) => self.send(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": error.to_string() },
            })),
        }
    }

    fn handle_host_callback(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Option<Value>> {
        host_callback(&mut self.settings, &self.capabilities, method, params)
    }

    fn wait_for_exit_before(&mut self, deadline: Instant) -> Result<()> {
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

fn validate_outbound_frame_size(frame: &str) -> Result<()> {
    if frame.len() + 1 > MAX_FRAME_BYTES {
        bail!("plugin JSON-RPC request exceeded the frame limit");
    }
    Ok(())
}

fn host_method_not_found_response(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": -32601, "message": "host method not found" },
    })
}

fn host_callback(
    settings: &mut Value,
    capabilities: &HashSet<String>,
    method: &str,
    params: Option<Value>,
) -> Result<Option<Value>> {
    match method {
        "host.settings.get" | "host.settings.replace" => {
            if !capabilities.contains("settings") {
                bail!("plugin capability is not granted");
            }
            settings_callback(settings, method, params)
        }
        "host.tasks.read" | "host.task.get" => {
            if !capabilities.contains("tasks.read") {
                bail!("plugin capability is not granted");
            }
            let key = params
                .as_ref()
                .and_then(|params| params.get("key"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("task read requires key"))?;
            let _ = key;
            Ok(Some(json!({ "task": null })))
        }
        _ => Ok(None),
    }
}

fn settings_callback(
    settings: &mut Value,
    method: &str,
    params: Option<Value>,
) -> Result<Option<Value>> {
    let settings = match method {
        "host.settings.get" => settings.clone(),
        "host.settings.replace" => {
            let params = params.unwrap_or(Value::Null);
            let replacement = params.get("settings").cloned().unwrap_or(params);
            if !replacement.is_object() {
                bail!("malformed host.settings.replace callback: settings must be an object");
            }
            *settings = replacement;
            settings.clone()
        }
        _ => return Ok(None),
    };
    Ok(Some(json!({ "settings": settings })))
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn write_frames_to_plugin(mut stdin: ChildStdin, frames: Receiver<WriteFrame>) {
    for WriteFrame { frame, completed } in frames {
        let result = (|| {
            stdin
                .write_all(frame.as_bytes())
                .context("failed to write plugin JSON-RPC request")?;
            stdin
                .write_all(b"\n")
                .context("failed to terminate plugin JSON-RPC request")?;
            stdin.flush().context("failed to flush plugin request")
        })();
        let failed = result.is_err();
        let _ = completed.send(result);
        if failed {
            break;
        }
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

#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
}

enum Frame {
    Request {
        id: Value,
        method: String,
        params: Option<Value>,
    },
    Response {
        id: Value,
        result: std::result::Result<Value, RpcError>,
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
        let params = object.get("params").cloned();
        if params
            .as_ref()
            .is_some_and(|params| !params.is_null() && !params.is_array() && !params.is_object())
        {
            bail!("malformed JSON-RPC output: params must be null, an array, or an object");
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
            params,
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
            result: Err(RpcError {
                code: error["code"].as_i64().expect("validated integer"),
                message: error["message"]
                    .as_str()
                    .expect("validated string")
                    .to_owned(),
            }),
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
            "backend_entrypoints": { "test-platform": "bin/plugin" },
            "capabilities": ["settings", "tasks.read", "task-events"],
            "ui_contributions": [{
                "id": "fixture-pane",
                "label": "Fixture",
                "placement": "main-pane",
                "entrypoint": "ui/entry.js",
                "shortcut": "Mod+G"
            }]
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
        let mut inactive_platform_traversal = manifest();
        inactive_platform_traversal["backend_entrypoints"]["windows-x64"] = json!("../plugin.exe");
        assert!(validate_local_manifest(&inactive_platform_traversal, "test-platform").is_err());
    }

    #[test]
    fn manifest_validation_fails_closed_and_enforces_public_v1_essentials() {
        let mut undocumented_root = manifest();
        undocumented_root["experimental"] = json!(true);
        assert!(validate_local_manifest(&undocumented_root, "test-platform").is_err());

        let mut invalid_id = manifest();
        invalid_id["id"] = json!("Fixture");
        assert!(validate_local_manifest(&invalid_id, "test-platform").is_err());

        let mut incompatible_api = manifest();
        incompatible_api["host_api_version"] = json!("planeai.plugin-host.v2");
        assert!(validate_local_manifest(&incompatible_api, "test-platform").is_err());

        let mut unsupported_capability = manifest();
        unsupported_capability["capabilities"] = json!(["storage"]);
        assert!(validate_local_manifest(&unsupported_capability, "test-platform").is_err());

        let mut undocumented_contribution = manifest();
        undocumented_contribution["ui_contributions"][0]["experimental"] = json!(true);
        assert!(validate_local_manifest(&undocumented_contribution, "test-platform").is_err());

        let mut invalid_contribution = manifest();
        invalid_contribution["ui_contributions"][0]["placement"] = json!("overlay");
        assert!(validate_local_manifest(&invalid_contribution, "test-platform").is_err());
        invalid_contribution["ui_contributions"][0]["placement"] = json!("preferences");
        invalid_contribution["ui_contributions"][0]["order"] = json!(0);
        assert!(validate_local_manifest(&invalid_contribution, "test-platform").is_err());
        invalid_contribution["ui_contributions"][0]
            .as_object_mut()
            .unwrap()
            .remove("order");
        invalid_contribution["ui_contributions"][0]["shortcut"] = json!("Mod+L");
        assert!(validate_local_manifest(&invalid_contribution, "test-platform").is_err());
        invalid_contribution["ui_contributions"][0]
            .as_object_mut()
            .unwrap()
            .remove("shortcut");
        invalid_contribution["ui_contributions"][0]["placement"] = json!("main-pane");
        invalid_contribution["ui_contributions"][0]["entrypoint"] = json!("../entry.js");
        assert!(validate_local_manifest(&invalid_contribution, "test-platform").is_err());
    }

    #[test]
    fn host_callbacks_support_granted_settings_and_task_reads() {
        let capabilities = HashSet::from(["settings".to_string(), "tasks.read".to_string()]);
        let mut settings = json!({});
        assert_eq!(
            host_callback(
                &mut settings,
                &capabilities,
                "host.settings.replace",
                Some(json!({ "settings": { "greeting": "Hello" } })),
            )
            .unwrap(),
            Some(json!({ "settings": { "greeting": "Hello" } }))
        );
        assert_eq!(
            host_callback(&mut settings, &capabilities, "host.settings.get", None).unwrap(),
            Some(json!({ "settings": { "greeting": "Hello" } }))
        );
        for method in ["host.tasks.read", "host.task.get"] {
            assert_eq!(
                host_callback(
                    &mut settings,
                    &capabilities,
                    method,
                    Some(json!({ "key": "PLN-1" })),
                )
                .unwrap(),
                Some(json!({ "task": null }))
            );
            assert!(host_callback(&mut settings, &capabilities, method, None).is_err());
        }
        assert!(host_callback(
            &mut settings,
            &capabilities,
            "host.settings.replace",
            Some(json!(false))
        )
        .is_err());
    }

    #[test]
    fn denied_callbacks_match_runtime_capability_errors_and_unknown_callbacks_are_not_found() {
        let mut settings = json!({});
        let capabilities = HashSet::new();
        for method in ["host.settings.get", "host.tasks.read", "host.task.get"] {
            assert!(
                host_callback(&mut settings, &capabilities, method, None)
                    .unwrap_err()
                    .to_string()
                    .contains("plugin capability is not granted"),
                "{method}"
            );
        }
        assert_eq!(
            host_callback(&mut settings, &capabilities, "host.unknown", None).unwrap(),
            None,
        );
        assert_eq!(
            host_method_not_found_response(json!(42)),
            json!({
                "jsonrpc": "2.0",
                "id": 42,
                "error": { "code": -32601, "message": "host method not found" },
            })
        );
    }

    #[test]
    fn outbound_frame_limit_includes_the_newline_terminator() {
        assert!(validate_outbound_frame_size(&"x".repeat(MAX_FRAME_BYTES - 1)).is_ok());
        assert!(validate_outbound_frame_size(&"x".repeat(MAX_FRAME_BYTES)).is_err());
    }

    #[test]
    fn handshake_advertises_manifest_granted_capabilities() {
        assert_eq!(
            handshake_params(&manifest_capabilities(&manifest())),
            json!({
                "host_api_version": HOST_API_VERSION,
                "host_capabilities": ["settings", "tasks.read", "task-events"],
            })
        );
    }

    #[test]
    fn task_lifecycle_requires_the_capability_and_subscription() {
        let capabilities = vec!["task-events".to_string()];
        let subscriptions = vec!["task.lifecycle".to_string()];
        assert!(lifecycle_delivery_is_granted(&capabilities, &subscriptions));
        assert!(!lifecycle_delivery_is_granted(&[], &subscriptions));
        assert!(!lifecycle_delivery_is_granted(&capabilities, &[]));
    }

    #[test]
    fn scenario_parser_accepts_stable_request_lines_and_rejects_invalid_shapes() {
        assert_eq!(
            parse_scenario_line(r#"{ "method": "fixture.persistSettings", "params": { "settings": { "nested": true } } }"#)
                .unwrap(),
            ScenarioRequest {
                method: "fixture.persistSettings".to_string(),
                params: json!({ "settings": { "nested": true } }),
                timeout: None,
            }
        );
        assert_eq!(
            parse_scenario_line(r#"{ "method": "fixture.status" }"#)
                .unwrap()
                .params,
            Value::Null
        );
        for line in [
            "[]",
            r#"{ "method": 1 }"#,
            r#"{ "method": "" }"#,
            r#"{ "method": "x", "id": 1 }"#,
        ] {
            assert!(parse_scenario_line(line).is_err(), "{line}");
        }
        for method in [
            "plugin.handshake",
            "plugin.shutdown",
            "plugin.taskLifecycle",
            "$/cancelRequest",
        ] {
            assert!(
                parse_scenario_line(&format!(r#"{{ "method": "{method}" }}"#)).is_err(),
                "{method}"
            );
        }
    }

    #[test]
    fn cancellation_and_shutdown_windows_match_the_runtime_contract() {
        assert_eq!(CANCELLATION_ACK_TIMEOUT, Duration::from_secs(3));
        assert_eq!(SHUTDOWN_TIMEOUT, Duration::from_secs(3));
    }

    #[test]
    fn scenario_timeout_requests_and_verifies_cancellation() {
        let request =
            parse_scenario_line(r#"{ "method": "fixture.awaitCancellation", "timeout_ms": 25 }"#)
                .unwrap();
        assert_eq!(request.timeout, Some(Duration::from_millis(25)));
        for line in [
            r#"{ "method": "fixture.awaitCancellation", "timeout_ms": 0 }"#,
            r#"{ "method": "fixture.awaitCancellation", "timeout_ms": 5001 }"#,
            r#"{ "method": "fixture.awaitCancellation", "timeout_ms": "25" }"#,
        ] {
            assert!(parse_scenario_line(line).is_err(), "{line}");
        }
    }

    #[test]
    fn temporary_plugin_state_creates_and_removes_host_owned_directories() {
        let state = TemporaryPluginState::new().unwrap();
        let root = state.root.clone();
        assert!(state.data_dir.is_dir());
        assert!(state.secrets_dir.is_dir());
        drop(state);
        assert!(!root.exists());
    }

    #[cfg(unix)]
    #[test]
    fn backend_entrypoint_may_be_a_safe_relative_symlink_to_an_external_target() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root = std::env::temp_dir().join(format!(
            "planeai-plugin-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let package = root.join("package");
        let external = root.join("external-plugin");
        fs::create_dir_all(package.join("bin")).unwrap();
        fs::write(&external, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = fs::metadata(&external).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&external, permissions).unwrap();
        symlink(&external, package.join("bin/plugin")).unwrap();

        assert_eq!(
            validate_entrypoint(&package, "bin/plugin").unwrap(),
            external.canonicalize().unwrap()
        );
        fs::remove_dir_all(root).unwrap();
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
        assert!(parse_frame(
            r#"{"jsonrpc":"2.0","id":1,"method":"host.tasks.read","params":true}"#,
        )
        .is_err());
    }
}
