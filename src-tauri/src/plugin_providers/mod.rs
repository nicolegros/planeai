//! Plugin-provided session runtimes (ADR-0014).
//!
//! The host owns the session row, worktree, lifecycle and status display; the
//! provider plugin owns the agent process and its conversation. Provider
//! notifications are routed here by the sidecar stdout reader.

mod errors;
mod registry;
mod routing;
mod runtime;
mod sink;

#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
mod tests;

pub use errors::ProviderError;
pub use registry::ProviderSessions;
pub use routing::{
    ended_sessions, ensure, handback, handoff, interrupt, launch, reconcile, resolve, send, stop,
    stop_reason, SessionRuntimeContext, StopTarget,
};
pub use runtime::{AppRuntime, ProviderRuntime};
pub use sink::{runtime_stopped, NotificationSink};

pub use planeai_plugin_contract::provider::{parse_provider_key, ProviderSessionStatus};
