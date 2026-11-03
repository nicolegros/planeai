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
        let name = request.name.to_string();
        if self.registry.get(&name).is_none() {
            return Err(ErrorData::invalid_params(
                format!("unknown tool: {name}"),
                None,
            ));
        }
        let (registry, tool_context) = (self.registry.clone(), self.context.clone());
        let arguments = request.arguments.unwrap_or_default();
        tracing::info!(tool = %name, session_id = %caller.session_id, "mcp tool call");
        // Handlers do SQLite, git and IPC work synchronously.
        let output = tokio::task::spawn_blocking(move || {
            registry
                .get(&name)
                .map(|tool| tool.call(&tool_context, &caller, arguments))
        })
        .await
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))?
        .ok_or_else(|| ErrorData::internal_error("tool vanished", None))?;
        let content = vec![ContentBlock::text(output.text)];
        Ok(if output.is_error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        }
        .into())
    }
}

/// The HTTP app: `/mcp` behind bearer-token authentication.
pub fn router(
    registry: Arc<Registry>,
    context: ToolContext,
    cancel: CancellationToken,
) -> axum::Router {
    let db_path = Arc::new(context.db_path.clone());
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
        .layer(middleware::from_fn_with_state(db_path, authenticate))
}

async fn authenticate(
    State(db_path): State<Arc<PathBuf>>,
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
        Some(token) => tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(db_path.as_path())?;
            planeai_core::mcp::resolve(&conn, &token)
        })
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

/// Bind `127.0.0.1:port` and serve until stopped. The port is never changed silently:
/// agents already running hold its URL.
pub async fn start(
    port: u16,
    registry: Arc<Registry>,
    context: ToolContext,
) -> Result<RunningServer, String> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::AddrInUse => format!("Port {port} is already in use."),
            _ => format!("Could not listen on port {port}: {error}"),
        })?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let cancel = CancellationToken::new();
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
    Ok(RunningServer { port, cancel, task })
}

/// What the server is doing, as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum McpStatus {
    Off,
    Listening { port: u16 },
    Failed { port: u16, reason: String },
}

/// The app's one MCP server, converging on the configured port.
pub struct McpServer {
    registry: Arc<Registry>,
    context: ToolContext,
    current: tokio::sync::Mutex<(McpStatus, Option<RunningServer>)>,
}

impl McpServer {
    pub fn new(registry: Registry, context: ToolContext) -> Self {
        Self {
            registry: Arc::new(registry),
            context,
            current: tokio::sync::Mutex::new((McpStatus::Off, None)),
        }
    }

    /// Serve on `port`, or stop for `None`. Re-applying the port already served is a no-op;
    /// a failed port is retried.
    pub async fn apply(&self, port: Option<u16>) -> McpStatus {
        let mut current = self.current.lock().await;
        if let (Some(port), McpStatus::Listening { port: serving }) = (port, &current.0) {
            if port == *serving {
                return current.0.clone();
            }
        }
        if let Some(running) = current.1.take() {
            running.stop().await;
        }
        current.0 = match port {
            None => McpStatus::Off,
            Some(port) => match start(port, self.registry.clone(), self.context.clone()).await {
                Ok(running) => {
                    let status = McpStatus::Listening { port: running.port };
                    current.1 = Some(running);
                    status
                }
                Err(reason) => {
                    tracing::warn!(port, %reason, "mcp server could not start");
                    McpStatus::Failed { port, reason }
                }
            },
        };
        current.0.clone()
    }

    pub async fn status(&self) -> McpStatus {
        self.current.lock().await.0.clone()
    }
}
