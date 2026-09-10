use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use crate::config::{Config, LanguageServerProfile};

/// Tauri-managed LSP process registry. Processes are keyed by workspace and
/// trusted server profile so multiple editor buffers share a server.
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
    #[allow(dead_code)]
    child: Child,
    clients: HashMap<String, Channel<String>>,
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
        let server_key = format!("{repo_path}\u{0}{}", profile.id);

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
                },
            );
            spawn_reader(server_key.clone(), stdout, manager);
        }

        self.next_connection_id += 1;
        let connection_id = format!("lsp-{}", self.next_connection_id);
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

    pub async fn send(&self, connection_id: &str, message: &str) -> Result<(), String> {
        let server_key = self
            .connections
            .get(connection_id)
            .ok_or("LSP connection not found")?;
        let stdin = self
            .servers
            .get(server_key)
            .ok_or("LSP server not found")?
            .stdin
            .clone();
        let mut stdin = stdin.lock().await;
        write_message(&mut stdin, message).await
    }

    pub fn disconnect(&mut self, connection_id: &str) {
        let Some(server_key) = self.connections.remove(connection_id) else {
            return;
        };
        if let Some(server) = self.servers.get_mut(&server_key) {
            server.clients.remove(connection_id);
        }
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
            let clients: Vec<Channel<String>> = {
                let state = manager.lock().await;
                state
                    .servers
                    .get(&server_key)
                    .map(|server| server.clients.values().cloned().collect())
                    .unwrap_or_default()
            };
            for client in clients {
                let _ = client.send(message.clone());
            }
        }
        let mut state = manager.lock().await;
        state.servers.remove(&server_key);
        state.connections.retain(|_, key| key != &server_key);
    });
}

async fn write_message(stdin: &mut ChildStdin, message: &str) -> Result<(), String> {
    let header = format!("Content-Length: {}\r\n\r\n", message.len());
    stdin
        .write_all(header.as_bytes())
        .await
        .and_then(|_| stdin.write_all(message.as_bytes()))
        .await
        .and_then(|_| stdin.flush())
        .await
        .map_err(|error| format!("failed to write LSP message: {error}"))
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
