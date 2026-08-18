# ADR-0011: Trusted bundled plugin subprocess runtime

## Status

Accepted

## Context

PlaneAI needs an extensibility path for integrations that does not entangle their process lifetime, storage, or UI with the session runtime. The legacy in-process Jira OAuth/sync implementation is inert while the bundled Jira plugin owns the connection flow. The migration safely imports legacy public Jira settings once into the plugin namespace but does not import legacy OAuth tokens.

In-process plugins and dynamically loaded native libraries were considered. They make a plugin crash, memory-unsafety defect, dependency conflict, or lifecycle leak part of PlaneAI's process. They also make versioning and unload behavior difficult to reason about. User-installed executable paths and remote UI modules are out of scope for the first iteration because they would introduce a distribution and trust model before the host contract is proven.

## Decision

PlaneAI runs reviewed, bundled plugins as separately compiled subprocesses.

- A `planeai.plugin.v1` manifest is embedded in the signed application. It declares a stable ID, display name, plugin version, supported host API version, `builtin` or `local` source kind, a backend entrypoint, and optional UI entrypoint. The initial registry has one `builtin` Jira plugin. The host accepts only its known binary name; it does not use PATH fallback for missing packaged plugin binaries.
- Plugin inventory and lifecycle state live in dedicated `plugin_inventory` storage, not in `sessions`, legacy Jira tables, or Jira runtime state. A clean launch discovers Jira and records it as disabled. On a later startup, an active persisted state is reconciled to an actionable `error` diagnostic rather than silently claiming a process survived.
- Runtime states are `disabled`, `starting`, `running`, `stopping`, and `error`. Each record includes the latest diagnostic and stderr log location. State transitions are emitted as `plugin-runtime-changed` events.
- The host starts the trusted sidecar with `tokio::process::Command`, uses direct argv rather than a shell, and applies `no_window_tokio()` on Windows. It speaks one-request-at-a-time newline-framed JSON-RPC through stdin/stdout. Startup requires a versioned `plugin.handshake`; the Jira plugin exposes connection status and its scoped authorization lifecycle (`jira.connect.start`, `jira.connect.complete`, `jira.connect.cancel`, and `jira.disconnect`). A bounded shutdown asks for `plugin.shutdown`, then kills as a fallback.
- The frontend has a small plugin SDK. A plugin UI entrypoint receives only a typed, plugin-scoped host capability object, not PlaneAI's Svelte stores or direct `invoke()` access. The App dynamically selects from a build-time registry of bundled entrypoints, gives the entrypoint a host-owned open Shadow Root, and always calls its disposer before reload, disable, or unmount.
- Plugin workspace pages mount directly in the main workspace, alongside loop dashboards, rather than as session/split-tree tabs. Plugins management appears in Preferences; a plugin may also contribute a typed sidebar connection-status item. This avoids changing session serialization or terminal pooling.

## Consequences

- A plugin failure, malformed RPC message, or unexpected process exit becomes a plugin-scoped diagnostic and leaves the Tauri process running; legacy Jira runtime behavior remains inert while connection ownership belongs to the bundled plugin.
- Shadow DOM is a CSS and DOM composition boundary, not a security sandbox. The security boundary is the reviewed, signed, build-time plugin registry plus an allowlisted native sidecar. Future untrusted plugins require a distinct sandbox and permission model.
- Plugin runtimes do not persist after the application exits in v1. Persisted data is inventory and failure/recovery information, not a detached background runtime.
- New plugin APIs must be added deliberately to the SDK and typed API facade. Arbitrary Tauri command names and arbitrary JSON-RPC method forwarding remain prohibited.
- Release packaging must build and ship every trusted sidecar with Tauri's target-suffixed `externalBin` convention. Development requires the sibling sidecar binary to be built explicitly; the host fails closed when it is absent.

## Rejected alternatives

- **In-process Rust trait plugins:** one plugin can crash or corrupt PlaneAI and cannot be safely unloaded.
- **Dynamic libraries:** platform ABI, signing, dependency, and unload semantics make the host less reliable than a supervised process.
- **Reusing session/daemon runtime:** plugins are not PTYs, do not belong to projects or worktrees, and should not inherit agent lifecycle semantics.
- **Remote or user-selected UI modules:** these turn a trusted bundled platform into a code-distribution problem before permissions and sandboxing exist.
