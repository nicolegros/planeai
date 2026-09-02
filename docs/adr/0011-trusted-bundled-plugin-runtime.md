# ADR-0011: Trusted local plugin subprocess runtime

## Status

Accepted — amended for local packages

## Context

PlaneAI needs an extensibility path for integrations that does not entangle their process lifetime, storage, or UI with the session runtime. The existing Jira integration is a legacy in-process OAuth/sync implementation. Its future migration needs a safe tracer bullet without changing current Jira behavior.

In-process plugins and dynamically loaded native libraries make a plugin crash, memory-unsafety defect, dependency conflict, or lifecycle leak part of PlaneAI's process. They also make versioning and unload behavior difficult to reason about.

PlaneAI users also need to install an integration that is not bundled with the application. This is intentionally a **trusted local-code** model: selecting a package is an explicit trust decision by the user. The host contract supplies compatibility, lifecycle, and composition boundaries; it is not an OS sandbox or permission system.

## Decision

PlaneAI runs bundled and user-selected local plugins as separately compiled subprocesses.

- A strict `planeai.plugin.v1` manifest declares a stable ID, display name, plugin version, exact supported host API version, source kind, platform-specific backend entrypoints, explicit capabilities, and optional UI contributions. Unknown manifest fields and legacy `ui_entrypoint` are rejected for newly installed local packages. A local package contains executable sidecars and self-contained browser ESM UI entrypoints.
- Users install a local directory through Plugins. PlaneAI validates the current platform executable, copies the directory into a hash-addressed immutable package snapshot, and records inventory/lifecycle state separately from sessions and integration state. Reload restarts that snapshot; a duplicate ID fails; removal is destructive and deletes package and plugin state.
- A local plugin is trusted native code. It receives its own data and secrets directories and can technically access the user's account according to OS permissions. Host capabilities are compatibility and lifecycle grants only. The sidecar owns its data/SQLite and secret files; PlaneAI owns the public JSON settings document and captured stderr logs. Plugins never access PlaneAI's database directly.
- Runtime states are `disabled`, `starting`, `running`, `stopping`, and `error`. Each record includes the latest diagnostic and stderr log location. State transitions are emitted as `plugin-runtime-changed` events.
- The host starts sidecars with `tokio::process::Command`, direct argv rather than a shell, and `no_window_tokio()` on Windows. It speaks UTF-8, newline-framed JSON-RPC 2.0 on stdin/stdout. Startup requires a versioned `plugin.handshake`. Handshake and ordinary calls have a five-second deadline. The host may send `$/cancelRequest`; a cancellable call must complete with `-32800` within three seconds. Shutdown gives the sidecar three seconds before kill fallback.
- v1 local capabilities are `settings`, `tasks.read`, and `task-events`; task write APIs are not public. Settings are a host-owned JSON object with atomic full-document replacement and last-writer-wins behavior. Task events are post-commit, best-effort live batches with no replay, retry, or cross-batch ordering guarantee; plugins reconcile with `tasks.read`.
- The frontend exposes only a typed, plugin-scoped host bridge. Plugin UI must not import PlaneAI Svelte stores, Tauri APIs, components, or internal frontend modules. A local UI entrypoint is a self-contained single-file browser ESM bundle mounted into a host-owned open Shadow Root and must return a disposer called before reload, disable, or unmount. Shadow DOM is a CSS/DOM composition boundary, not a security sandbox.
- PlaneAI ships `planeai-cli plugin test` as a headless author harness. It validates a package and exercises handshake, callback settings, task-event delivery, malformed response detection, and lifecycle shutdown without launching the desktop app.

## Consequences

- A plugin failure, malformed RPC message, unexpected process exit, or cancellation failure becomes a plugin-scoped diagnostic and leaves the Tauri process and legacy Jira integration running.
- Local packages can be developed and distributed directly by trusted users, but PlaneAI has no marketplace, remote URL installation, signing policy, automatic updates, or sandboxed/WASM plugin mode in v1.
- Plugin runtimes do not persist after application exit in v1. Persisted data is inventory, plugin-owned state, public settings, and failure/recovery information—not a detached background runtime.
- New APIs require deliberate additions to the manifest capability vocabulary, typed UI bridge, runtime protocol, guide, fixture, and contract tests. Arbitrary Tauri command access and undeclared host RPC methods remain prohibited.
- Release packaging must build and ship every trusted sidecar with Tauri's target-suffixed `externalBin` convention. Development requires the selected local package to contain a built current-platform executable; the host fails closed when it is absent.

## Rejected alternatives

- **In-process Rust trait plugins:** one plugin can crash or corrupt PlaneAI and cannot be safely unloaded.
- **Dynamic libraries:** platform ABI, signing, dependency, and unload semantics make the host less reliable than a supervised process.
- **Reusing session/daemon runtime:** plugins are not PTYs, do not belong to projects or worktrees, and should not inherit agent lifecycle semantics.
- **Remote packages, marketplace policy, signing, or automatic updates:** each is a distinct code-distribution and trust-policy problem, intentionally deferred from local trusted v1.
- **Remote or arbitrary multi-file UI module graphs:** v1 accepts only package-contained self-contained browser ESM bundles. Future module resolution or untrusted UI needs a separate security and compatibility design.
