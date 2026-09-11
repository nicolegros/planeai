use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use crate::config::{Config, LanguageServerProfile};

/// Tauri-managed LSP process registry.
///
/// Language servers are shared by workspace and trusted profile. CodeMirror
/// clients have independent JSON-RPC request-ID spaces, so requests are
/// rewritten before reaching the shared server and rewritten back when their
/// responses are routed to the originating client.
pub struct LspState(pub Arc<Mutex<LspManager>>);

impl Default for LspState {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(LspManager::default())))
    }
}

#[derive(Default)]
pub struct LspManager {
    next_connection_id: u64,
    servers: HashMap<String, LspServer>,
    connections: HashMap<String, String>,
}

struct LspServer {
    stdin: Arc<Mutex<ChildStdin>>,
    child: Child,
    clients: HashMap<String, Channel<String>>,
    next_request_id: u64,
    pending_requests: HashMap<u64, PendingRequest>,
    initialize_request_id: Option<u64>,
    initialize_waiters: Vec<PendingRequest>,
    initialize_result: Option<Value>,
    initialized_sent: bool,
    document_clients: HashMap<String, HashSet<String>>,
}

#[derive(Clone)]
struct PendingRequest {
    connection_id: String,
    client_id: Value,
}

#[derive(Clone, Serialize)]
pub struct LspConnection {
    pub connection_id: String,
    pub language_id: String,
    pub profile_id: String,
}

#[derive(Clone)]
struct ResolvedProfile {
    id: String,
    language_id: String,
    command: String,
    args: Vec<String>,
}

impl LspManager {
    fn next_connection(&mut self) -> String {
        self.next_connection_id += 1;
        format!("lsp-{}", self.next_connection_id)
    }

    pub async fn connect(
        &mut self,
        repo_path: String,
        file_path: String,
        config: &Config,
        on_message: Channel<String>,
        manager: Arc<Mutex<LspManager>>,
    ) -> Result<LspConnection, String> {
        let profile = resolve_profile(&file_path, config)
            .ok_or_else(|| format!("No trusted language server is configured for {file_path}"))?;
        let server_key = server_key(&repo_path, &profile.id);
        let connection_id = self.next_connection();

        if !self.servers.contains_key(&server_key) {
            let mut command = Command::new(crate::command::resolve(&profile.command));
            command.args(&profile.args).current_dir(&repo_path);
            command.env(
                "PATH",
                planeai_core::command::augmented_path(&config.resolved_extra_path_dirs()),
            );
            #[cfg(windows)]
            planeai_core::command::no_window_tokio(&mut command);
            command.stdin(std::process::Stdio::piped());
            command.stdout(std::process::Stdio::piped());
            command.stderr(std::process::Stdio::piped());
            let mut child = command
                .spawn()
                .map_err(|error| format!("failed to start {}: {error}", profile.command))?;
            let stdin = child
                .stdin
                .take()
                .ok_or("language server stdin was not piped")?;
            let stdout = child
                .stdout
                .take()
                .ok_or("language server stdout was not piped")?;
            self.servers.insert(
                server_key.clone(),
                LspServer {
                    stdin: Arc::new(Mutex::new(stdin)),
                    child,
                    clients: HashMap::new(),
                    next_request_id: 0,
                    pending_requests: HashMap::new(),
                    initialize_request_id: None,
                    initialize_waiters: Vec::new(),
                    initialize_result: None,
                    initialized_sent: false,
                    document_clients: HashMap::new(),
                },
            );
            spawn_reader(server_key.clone(), stdout, manager);
        }

        self.servers
            .get_mut(&server_key)
            .expect("server was inserted above")
            .clients
            .insert(connection_id.clone(), on_message);
        self.connections.insert(connection_id.clone(), server_key);

        Ok(LspConnection {
            connection_id,
            language_id: profile.language_id,
            profile_id: profile.id,
        })
    }

    pub async fn send(&mut self, connection_id: &str, message: &str) -> Result<(), String> {
        let server_key = self
            .connections
            .get(connection_id)
            .cloned()
            .ok_or("LSP connection not found")?;
        let value: Value = serde_json::from_str(message)
            .map_err(|error| format!("invalid LSP client message: {error}"))?;
        let method = value.get("method").and_then(Value::as_str);

        let mut immediate = None;
        let outbound = {
            let server = self
                .servers
                .get_mut(&server_key)
                .ok_or("LSP server not found")?;

            if method == Some("initialize") {
                let client_id = message_id(&value).ok_or("initialize request is missing an id")?;
                let request = PendingRequest {
                    connection_id: connection_id.to_string(),
                    client_id,
                };
                if let Some(result) = &server.initialize_result {
                    if let Some(client) = server.clients.get(connection_id) {
                        immediate = Some((
                            client.clone(),
                            json!({ "jsonrpc": "2.0", "id": request.client_id, "result": result })
                                .to_string(),
                        ));
                    }
                    None
                } else {
                    server.initialize_waiters.push(request);
                    if server.initialize_request_id.is_some() {
                        None
                    } else {
                        let server_id = server.next_server_request_id();
                        server.initialize_request_id = Some(server_id);
                        Some((
                            server.stdin.clone(),
                            replace_message_id(value, json!(server_id))?.to_string(),
                        ))
                    }
                }
            } else if method == Some("initialized") {
                if server.initialized_sent {
                    None
                } else {
                    server.initialized_sent = true;
                    Some((server.stdin.clone(), message.to_string()))
                }
            } else if method == Some("textDocument/didOpen") {
                let uri = document_uri(&value, "textDocument/didOpen")?;
                let clients = server.document_clients.entry(uri).or_default();
                if clients.insert(connection_id.to_string()) {
                    Some((server.stdin.clone(), message.to_string()))
                } else {
                    None
                }
            } else if method == Some("textDocument/didClose") {
                let uri = document_uri(&value, "textDocument/didClose")?;
                let should_close = server
                    .document_clients
                    .get_mut(&uri)
                    .map(|clients| {
                        clients.remove(connection_id);
                        clients.is_empty()
                    })
                    .unwrap_or(false);
                if should_close {
                    server.document_clients.remove(&uri);
                    Some((server.stdin.clone(), message.to_string()))
                } else {
                    None
                }
            } else if method.is_none() && value.get("id").is_some() {
                // Responses to server-initiated requests retain the server's ID.
                Some((server.stdin.clone(), message.to_string()))
            } else if let Some(client_id) = message_id(&value) {
                let server_id = server.next_server_request_id();
                server.pending_requests.insert(
                    server_id,
                    PendingRequest {
                        connection_id: connection_id.to_string(),
                        client_id,
                    },
                );
                Some((
                    server.stdin.clone(),
                    replace_message_id(value, json!(server_id))?.to_string(),
                ))
            } else {
                Some((server.stdin.clone(), message.to_string()))
            }
        };

        if let Some((client, response)) = immediate {
            let _ = client.send(response);
        }
        if let Some((stdin, outbound)) = outbound {
            let mut stdin = stdin.lock().await;
            write_message(&mut stdin, &outbound).await?;
        }
        Ok(())
    }

    pub async fn disconnect(&mut self, connection_id: &str) {
        let Some(server_key) = self.connections.remove(connection_id) else {
            return;
        };
        let Some(server) = self.servers.get_mut(&server_key) else {
            return;
        };

        server.clients.remove(connection_id);
        server
            .pending_requests
            .retain(|_, request| request.connection_id != connection_id);
        server
            .initialize_waiters
            .retain(|request| request.connection_id != connection_id);

        let closed_uris: Vec<String> = server
            .document_clients
            .iter_mut()
            .filter_map(|(uri, clients)| {
                clients.remove(connection_id);
                clients.is_empty().then(|| uri.clone())
            })
            .collect();
        for uri in &closed_uris {
            server.document_clients.remove(uri);
        }

        if server.clients.is_empty() {
            if let Some(mut server) = self.servers.remove(&server_key) {
                let _ = server.child.start_kill();
            }
            return;
        }

        let stdin = server.stdin.clone();
        for uri in closed_uris {
            let message = json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didClose",
                "params": { "textDocument": { "uri": uri } },
            })
            .to_string();
            let mut stdin = stdin.lock().await;
            let _ = write_message(&mut stdin, &message).await;
        }
    }

    fn handle_server_message(
        &mut self,
        server_key: &str,
        message: &str,
    ) -> Vec<(Channel<String>, String)> {
        let Ok(value) = serde_json::from_str::<Value>(message) else {
            return self.broadcast(server_key, message);
        };
        let Some(server) = self.servers.get_mut(server_key) else {
            return Vec::new();
        };

        if value.get("method").is_none() && value.get("id").is_some() {
            let Some(server_id) = value.get("id").and_then(Value::as_u64) else {
                return Vec::new();
            };
            if server.initialize_request_id == Some(server_id) {
                server.initialize_request_id = None;
                if let Some(result) = value.get("result") {
                    server.initialize_result = Some(result.clone());
                }
                return server
                    .initialize_waiters
                    .drain(..)
                    .filter_map(|request| {
                        server
                            .clients
                            .get(&request.connection_id)
                            .cloned()
                            .map(|client| {
                                (
                                    client,
                                    replace_message_id(value.clone(), request.client_id)
                                        .expect("response has an id")
                                        .to_string(),
                                )
                            })
                    })
                    .collect();
            }
            return server
                .pending_requests
                .remove(&server_id)
                .and_then(|request| {
                    server
                        .clients
                        .get(&request.connection_id)
                        .cloned()
                        .map(|client| {
                            (
                                client,
                                replace_message_id(value, request.client_id)
                                    .expect("response has an id")
                                    .to_string(),
                            )
                        })
                })
                .into_iter()
                .collect();
        }

        if value.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics") {
            let Some(uri) = value.pointer("/params/uri").and_then(Value::as_str) else {
                return Vec::new();
            };
            return server
                .document_clients
                .get(uri)
                .into_iter()
                .flatten()
                .filter_map(|connection_id| server.clients.get(connection_id).cloned())
                .map(|client| (client, message.to_string()))
                .collect();
        }

        server
            .clients
            .values()
            .cloned()
            .map(|client| (client, message.to_string()))
            .collect()
    }

    fn broadcast(&self, server_key: &str, message: &str) -> Vec<(Channel<String>, String)> {
        self.servers
            .get(server_key)
            .into_iter()
            .flat_map(|server| server.clients.values().cloned())
            .map(|client| (client, message.to_string()))
            .collect()
    }
}

impl LspServer {
    fn next_server_request_id(&mut self) -> u64 {
        self.next_request_id += 1;
        self.next_request_id
    }
}

fn spawn_reader(server_key: String, stdout: ChildStdout, manager: Arc<Mutex<LspManager>>) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        loop {
            let message = match read_message(&mut reader).await {
                Ok(Some(message)) => message,
                Ok(None) => break,
                Err(error) => {
                    tracing::warn!(%server_key, "language server protocol error: {error}");
                    break;
                }
            };
            let clients = manager
                .lock()
                .await
                .handle_server_message(&server_key, &message);
            for (client, message) in clients {
                let _ = client.send(message);
            }
        }
        let mut state = manager.lock().await;
        state.servers.remove(&server_key);
        state.connections.retain(|_, key| key != &server_key);
    });
}

fn server_key(repo_path: &str, profile_id: &str) -> String {
    format!("{repo_path}\u{0}{profile_id}")
}

fn message_id(value: &Value) -> Option<Value> {
    value.get("id").cloned()
}

fn replace_message_id(mut value: Value, id: Value) -> Result<Value, String> {
    let object = value
        .as_object_mut()
        .ok_or("LSP message must be a JSON object")?;
    object.insert("id".to_string(), id);
    Ok(value)
}

fn document_uri(value: &Value, method: &str) -> Result<String, String> {
    value
        .pointer("/params/textDocument/uri")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("{method} notification is missing textDocument.uri"))
}

async fn write_message(stdin: &mut ChildStdin, message: &str) -> Result<(), String> {
    let header = format!("Content-Length: {}\r\n\r\n", message.len());
    stdin
        .write_all(header.as_bytes())
        .await
        .map_err(|error| format!("failed to write LSP header: {error}"))?;
    stdin
        .write_all(message.as_bytes())
        .await
        .map_err(|error| format!("failed to write LSP message: {error}"))?;
    stdin
        .flush()
        .await
        .map_err(|error| format!("failed to flush LSP message: {error}"))?;
    Ok(())
}

async fn read_message(reader: &mut BufReader<ChildStdout>) -> Result<Option<String>, String> {
    let mut line = Vec::new();
    let mut content_length = None;
    loop {
        line.clear();
        let bytes = reader
            .read_until(b'\n', &mut line)
            .await
            .map_err(|error| format!("failed to read LSP header: {error}"))?;
        if bytes == 0 {
            return Ok(None);
        }
        let header = String::from_utf8_lossy(&line);
        let header = header.trim();
        if header.is_empty() {
            break;
        }
        if let Some(value) = header.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "invalid LSP Content-Length header".to_string())?,
            );
        }
    }
    let content_length = content_length.ok_or("LSP message is missing Content-Length")?;
    let mut body = vec![0; content_length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|error| format!("failed to read LSP body: {error}"))?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|_| "LSP message was not valid UTF-8".to_string())
}

fn resolve_profile(file_path: &str, config: &Config) -> Option<ResolvedProfile> {
    let extension = file_path.rsplit('.').next()?.to_ascii_lowercase();
    if let Some(settings) = &config.language_servers {
        if !settings.enabled.unwrap_or(true) {
            return None;
        }
        if let Some(profile) = settings.profiles.iter().find(|profile| {
            profile.enabled.unwrap_or(true)
                && profile
                    .extensions
                    .iter()
                    .any(|item| item.eq_ignore_ascii_case(&extension))
        }) {
            return Some(from_config_profile(profile));
        }
    }

    let (id, language_id, command, args) = match extension.as_str() {
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "json" | "jsonc" => (
            "typescript-language-server",
            if extension == "json" || extension == "jsonc" {
                "json"
            } else if extension == "js"
                || extension == "jsx"
                || extension == "mjs"
                || extension == "cjs"
            {
                "javascript"
            } else {
                "typescript"
            },
            "typescript-language-server",
            vec!["--stdio".to_string()],
        ),
        "rs" => ("rust-analyzer", "rust", "rust-analyzer", vec![]),
        "py" => (
            "pyright",
            "python",
            "pyright-langserver",
            vec!["--stdio".to_string()],
        ),
        "go" => ("gopls", "go", "gopls", vec![]),
        "c" | "cc" | "cpp" | "cxx" | "h" | "hh" | "hpp" | "hxx" => {
            ("clangd", "cpp", "clangd", vec![])
        }
        _ => return None,
    };
    Some(ResolvedProfile {
        id: id.to_string(),
        language_id: language_id.to_string(),
        command: command.to_string(),
        args,
    })
}

fn from_config_profile(profile: &LanguageServerProfile) -> ResolvedProfile {
    ResolvedProfile {
        id: profile.id.clone(),
        language_id: profile.language_id.clone(),
        command: profile.command.clone(),
        args: profile.args.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_first_class_typescript_profile() {
        let profile = resolve_profile("src/app.ts", &Config::default()).unwrap();
        assert_eq!(profile.id, "typescript-language-server");
        assert_eq!(profile.language_id, "typescript");
        assert_eq!(profile.args, ["--stdio"]);
    }

    #[test]
    fn workspace_connections_share_a_server_key() {
        let mut manager = LspManager::default();
        let first_connection = manager.next_connection();
        let second_connection = manager.next_connection();

        assert_ne!(first_connection, second_connection);
        assert_eq!(
            server_key("/repo", "rust-analyzer"),
            server_key("/repo", "rust-analyzer")
        );
        assert_ne!(
            server_key("/repo", "rust-analyzer"),
            server_key("/other-repo", "rust-analyzer")
        );
    }

    #[test]
    fn response_ids_are_restored_before_client_delivery() {
        let server_response = json!({ "jsonrpc": "2.0", "id": 42, "result": {} });
        let restored = replace_message_id(server_response, json!("client-request-1")).unwrap();
        assert_eq!(restored["id"], "client-request-1");
        assert_eq!(restored["result"], json!({}));
    }

    #[test]
    fn custom_profiles_override_builtins() {
        let mut config = Config::default();
        config.language_servers = Some(crate::config::LanguageServerSettings {
            enabled: Some(true),
            profiles: vec![LanguageServerProfile {
                id: "custom-python".to_string(),
                language_id: "python".to_string(),
                extensions: vec!["py".to_string()],
                command: "my-python-lsp".to_string(),
                args: vec!["--stdio".to_string()],
                enabled: Some(true),
            }],
            max_servers: None,
            format_on_save: None,
        });
        let profile = resolve_profile("main.py", &config).unwrap();
        assert_eq!(profile.id, "custom-python");
    }
}
