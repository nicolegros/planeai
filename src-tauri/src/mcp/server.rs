//! The MCP server agents reach over streamable HTTP on loopback (ADR-0016).
//!
//! Every request carries its session's bearer token, resolved here, at the boundary, into
//! an `McpCaller` that tool handlers trust. rmcp rejects foreign `Host` headers and any
//! browser `Origin`, which blocks DNS rebinding.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use planeai_core::mcp::McpCaller;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData,
    Implementation, ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{RoleServer, ServerHandler};
use tokio_util::sync::CancellationToken;

use serde::Serialize;

use super::tools::{Registry, ToolContext};

#[derive(Clone)]
struct Handler {
    registry: Arc<Registry>,
    context: ToolContext,
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("planeai", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "PlaneAI tracks tasks and runs agent sessions. These tools act as the session \
                 you run in: task and session defaults are its own task and session.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.registry.tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let caller = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<McpCaller>())
            .cloned()
            .ok_or_else(|| {
                ErrorData::internal_error("request reached a tool unauthenticated", None)
            })?;
        let tool = self.registry.get(&request.name).ok_or_else(|| {
            ErrorData::invalid_params(format!("unknown tool: {}", request.name), None)
        })?;
        let tool_context = self.context.clone();
        let arguments = request.arguments.unwrap_or_default();
        tracing::info!(tool = %tool.name, session_id = %caller.session_id, "mcp tool call");
        // Handlers do SQLite, git and IPC work synchronously.
        let output =
            tokio::task::spawn_blocking(move || tool.call(&tool_context, &caller, arguments))
                .await
                .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
        let content = vec![ContentBlock::text(output.text)];
        Ok(if output.is_error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        }
        .into())
    }
}

/// Resolves bearer tokens over one connection kept for the server's lifetime.
struct TokenLookup {
    db_path: PathBuf,
    conn: std::sync::Mutex<Option<rusqlite::Connection>>,
}

impl TokenLookup {
    fn resolve(&self, token: &str) -> Result<Option<McpCaller>, String> {
        let mut conn = self.conn.lock().map_err(|error| error.to_string())?;
        if conn.is_none() {
            *conn = Some(rusqlite::Connection::open(&self.db_path).map_err(|e| e.to_string())?);
        }
        let conn = conn.as_ref().expect("opened above");
        planeai_core::mcp::resolve(conn, token).map_err(|error| error.to_string())
    }
}

/// The HTTP app: `/mcp` behind bearer-token authentication.
fn router(
    registry: Arc<Registry>,
    context: ToolContext,
    cancel: CancellationToken,
) -> axum::Router {
    let lookup = Arc::new(TokenLookup {
        db_path: context.db_path.clone(),
        conn: std::sync::Mutex::new(None),
    });
    let handler = Handler { registry, context };
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None)
        .enforce_origin_validation()
        .with_cancellation_token(cancel);
    let service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(NeverSessionManager::default()),
        config,
    );
    axum::Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(lookup, authenticate))
}

async fn authenticate(
    State(lookup): State<Arc<TokenLookup>>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_string);
    let caller = match token {
        Some(token) => tokio::task::spawn_blocking(move || lookup.resolve(&token))
            .await
            .ok()
            .and_then(|resolved| {
                resolved
                    .inspect_err(|error| tracing::warn!(%error, "mcp token lookup failed"))
                    .ok()
            })
            .flatten(),
        None => None,
    };
    match caller {
        Some(caller) => {
            request.extensions_mut().insert(caller);
            next.run(request).await
        }
        None => (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "A PlaneAI session token is required.",
        )
            .into_response(),
    }
}

/// A running server; dropping it does not stop it, `stop` does.
pub struct RunningServer {
    pub port: u16,
    cancel: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl RunningServer {
    pub async fn stop(self) {
        self.cancel.cancel();
        let _ = self.task.await;
    }
}

/// Bind `127.0.0.1:port`. The port is never changed silently: agents already running hold
/// its URL.
async fn bind(port: u16) -> Result<tokio::net::TcpListener, String> {
    tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .await
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::AddrInUse => format!("Port {port} is already in use."),
            _ => format!("Could not listen on port {port}: {error}"),
        })
}

fn serve(
    listener: tokio::net::TcpListener,
    port: u16,
    registry: Arc<Registry>,
    context: ToolContext,
) -> RunningServer {
    let cancel = CancellationToken::new();
    // Sessions a tool starts connect to the server that received the call.
    let context = ToolContext {
        mcp_url: Some(planeai_core::mcp::server_url(port)),
        ..context
    };
    let app = router(registry, context, cancel.child_token());
    let shutdown = cancel.clone();
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app)
            .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
            .await
        {
            tracing::error!(%error, "mcp server stopped");
        }
    });
    tracing::info!(port, "mcp server listening");
    RunningServer { port, cancel, task }
}

/// What the server is doing, as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum McpStatus {
    Off,
    Listening { port: u16, url: String },
    Failed { port: u16, reason: String },
}

enum ServerState {
    Off,
    Listening(RunningServer),
    Failed { port: u16, reason: String },
}

impl ServerState {
    fn status(&self) -> McpStatus {
        match self {
            ServerState::Off => McpStatus::Off,
            ServerState::Listening(running) => McpStatus::Listening {
                port: running.port,
                url: planeai_core::mcp::server_url(running.port),
            },
            ServerState::Failed { port, reason } => McpStatus::Failed {
                port: *port,
                reason: reason.clone(),
            },
        }
    }
}

/// The app's one MCP server, converging on the configured port.
pub struct McpServer {
    registry: Arc<Registry>,
    context: ToolContext,
    state: std::sync::Mutex<ServerState>,
    applying: tokio::sync::Mutex<()>,
}

impl McpServer {
    pub fn new(registry: Registry, context: ToolContext) -> Self {
        Self {
            registry: Arc::new(registry),
            context,
            state: std::sync::Mutex::new(ServerState::Off),
            applying: tokio::sync::Mutex::new(()),
        }
    }

    /// Serve on `port`, or stop for `None`. Re-applying the port already served is a no-op;
    /// a failed port is retried.
    pub async fn apply(&self, port: Option<u16>) -> McpStatus {
        let _applying = self.applying.lock().await;
        let previous = {
            let mut state = self.lock();
            if let (Some(port), ServerState::Listening(running)) = (port, &*state) {
                if running.port == port {
                    return state.status();
                }
            }
            std::mem::replace(&mut *state, ServerState::Off)
        };
        if let ServerState::Listening(running) = previous {
            running.stop().await;
        }
        let next = match port {
            None => ServerState::Off,
            Some(port) => match bind(port).await {
                Ok(listener) => {
                    let port = listener.local_addr().map_or(port, |address| address.port());
                    ServerState::Listening(serve(
                        listener,
                        port,
                        self.registry.clone(),
                        self.context.clone(),
                    ))
                }
                Err(reason) => {
                    tracing::warn!(port, %reason, "mcp server could not start");
                    ServerState::Failed { port, reason }
                }
            },
        };
        let mut state = self.lock();
        *state = next;
        state.status()
    }

    pub fn status(&self) -> McpStatus {
        self.lock().status()
    }

    /// The URL agents launched now connect to; `None` unless the server is listening.
    pub fn url(&self) -> Option<String> {
        match &*self.lock() {
            ServerState::Listening(running) => Some(planeai_core::mcp::server_url(running.port)),
            _ => None,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ServerState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
