# ADR-0013: Plugin-provided session runtimes

## Status

Proposed.
The `planeai.plugin-host.v3` contract is unstable and may change freely until the first provider plugin ships end to end.
It partially supersedes ADR-0011's rejection of plugins reusing the session runtime.

## Context

Every session runs a provider command in a PTY, and PlaneAI renders it with xterm.js.
Agents with a headless mode, such as Claude Code through the Claude Agent SDK, can instead be driven programmatically: streamed messages, permission callbacks, interrupt and resume.
A chat UI built on that can be better than the TUI, but only if the session stays a PlaneAI session, with its worktree, branch, linked task, status and lifecycle.

ADR-0011 rejected plugins reusing the session runtime, because plugins are not PTYs and do not belong to worktrees.
A headless agent is not a PTY either, but it does belong to a worktree, so neither the PTY backends nor the existing plugin capabilities fit.

The plugin protocol was also request/response only.
The host read sidecar stdout only while a request was in flight, and an unsolicited notification was a fatal protocol error, so a sidecar could not stream anything.

## Decision

A trusted local plugin can provide **session runtimes**, which PlaneAI offers as providers.

- **Contract.** A manifest using `planeai.plugin-host.v3` may declare `providers: [{ id, label, entrypoint, supports }]` together with the `providers` capability.
  `supports` currently accepts `yolo` and `handoff`.
  Bundled plugins cannot declare providers.
- **Domain.** A runtime-backed provider has the key `<plugin id>:<provider id>`.
  It is listed beside configured command providers and is chosen when a session is created.
  Its sessions use the `plugin` session backend.
- **Ownership.** The host owns the session row, worktree, branch, task link, lifecycle, status display, notifications and prompt routing.
  The plugin owns the agent process, the conversation, its transcript and the chat UI.
- **Host to sidecar.** `provider.session.start` (with an optional `initial_prompt`), `provider.session.resume`, `provider.session.send`, `provider.session.interrupt` and `provider.session.stop` (`archive`, `destroy` or `exit`).
  Each one returns promptly; work happens asynchronously.
  Plugin UI cannot call `provider.*` methods directly.
- **Environment.** Start and resume carry the session's working directory and an `env` with `PLANEAI_SESSION_ID`, `PLANEAI_SOCKET` and the augmented `PATH`, so the agent can still spawn sub-sessions and manage tasks.
- **Sidecar to host.** A JSON-RPC notification `host.session.status { session_id, status }` with `busy`, `idle`, `needs_attention` or `exited`, and `host.session.event { session_id, seq, payload }`.
  The host forwards `payload` opaquely to the session's mounted UI.
  `seq` increases per session.
- **Transport.** The host owns sidecar stdout through a dedicated reader task for the process lifetime.
  Provider notifications are routed as they arrive; every other frame keeps the existing request path and its error semantics.
  A notification for a session the sidecar does not drive is dropped.
- **Status.** The provider is the only status source for its sessions.
  Hook and PTY status signals for those sessions are ignored, although the user's own agent hooks still run inside them.
  `needs_attention` shows as idle with an attention notification, as hook-driven terminal sessions do.
- **UI.** The provider's entrypoint replaces the terminal in the session's agent tab.
  Its bridge adds `host.session.send`, `host.session.interrupt` and `host.session.onEvent`.
  The host makes sure the current sidecar drives the session (resuming it if needed) before mounting the UI.
  The plugin rebuilds its view after a remount by subscribing first, then fetching its own snapshot in frame-sized pages and dropping events at or below the snapshot's `seq`.
- **Prompt routing.** Every host path that prompts a session (CLI, recipes and loops, `sessions.prompt`, the chat UI) reaches `provider.session.send`.
  Routing follows the session's stored backend.
  A prompt is limited to 48 KiB as JSON-escaped text, so its request always fits one frame.
- **Terminal handoff.** A provider that supports `handoff` can hand a session to its agent's own TUI.
  `provider.session.handoff` detaches the provider and returns the `argv` that continues the conversation, which the host runs in a shell tab of the same session.
  Closing that tab calls `provider.session.handback`, so only one side ever drives the conversation.
- **Lifecycle.** Runtimes still die with the app.
  Sessions resume lazily through the provider on next use.
  Ending a session through any path stops its provider session, including CLI archive, CLI delete and task completion, which the GUI reconciles when notified.
  Quitting the app while a provider session is busy asks for confirmation.

## Consequences

- A headless agent session behaves like any other session in the sidebar, task workflow and automation, while its plugin controls the conversation UI.
- PlaneAI stays agent-neutral: the host never parses provider payloads, and another headless agent can implement the same contract.
- Loops that target a provider session stop when the app quits; there is no detached provider runtime.
- The stdout reader changes the transport for every plugin, but frames other than provider notifications reach the request path in order, as before.
- The plugin author guide, the local fixture (an echo provider) and `planeai-cli plugin test` (provider conformance and `--provider-turn`) cover the contract.

## Deferred

- Launching runtime-backed sessions from the CLI, which has no plugin runtime.
- Launching runtime-backed sessions from recipes, loop candidates and symphony, which resolve providers from the config file only.
- Using a runtime-backed provider as `default_provider`.
- A distinct needs-attention state in the session list.
- Declaring v3 stable.

## Rejected alternatives

- **A plugin-owned chat outside sessions.** It loses worktrees, tasks and lifecycle, which are PlaneAI's value.
- **A sidecar-hosted local server the UI connects to.** It bypasses the typed, plugin-scoped bridge and adds ports, local authentication and CSP problems.
- **Hooks as the status source.** Two writers for one status flicker and race, and the SDK stream is exact.
- **Building an agent on the Messages API (Client SDK).** It reimplements Claude Code instead of reusing the user's configured harness.
