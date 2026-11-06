---
title: Plugin author guide
description: Build, package, install, and operate trusted PlaneAI local plugins.
---

PlaneAI local plugins are **trusted native extensions**, not web applications or sandboxed scripts. A package contains a native sidecar and optional browser ESM UI. Installing one gives its executable the same operating-system permissions as the signed-in user. Only install packages from authors you trust, review source and binaries before installation, and never treat a manifest capability as a security sandbox.

The checked-in [`src-tauri/plugins/local-fixture`](https://github.com/nicolegros/planeai/tree/main/src-tauri/plugins/local-fixture) package is the exact v1 example used throughout this guide. It pairs [`planeai-plugin-fixture`](https://github.com/nicolegros/planeai/tree/main/src-tauri/crates/planeai-plugin-fixture) with `ui/entry.js`.

## Package contract

A local package is a directory. PlaneAI requires this layout before copying it into its immutable, content-addressed package storage:

```text
my-plugin/
├── planeai-plugin.json
├── bin/
│   ├── my-plugin-macos-arm64
│   └── my-plugin-windows-x64.exe
└── ui/
    └── entry.js
```

`planeai-plugin.json` is strict: unknown fields are rejected. `id` may contain only lowercase ASCII letters, digits, and hyphens. The schema must be `planeai.plugin.v1`. Use `planeai.plugin-host.v1` for legacy host APIs, `planeai.plugin-host.v2` for plugins that require the dynamically focused recipient API, the unstable `planeai.plugin-host.v3` for plugins that declare [providers](#providers), `planeai.plugin-host.v4` for plugins that use the `sessions.start` capability or the `dialog` placement, `planeai.plugin-host.v5` for a `session.meta` slot with a tooltip that opens the plugin's session panel, `planeai.plugin-host.v6` for plugins that contribute [tools](#tools) to agents, `planeai.plugin-host.v7` for plugins that mirror an external tracker's issues as PlaneAI tasks with the `tasks.sync` capability and the [interaction bridge](#interaction-bridge-host-api-v7), or `planeai.plugin-host.v8` for a [background service](#background-service) that declares its own `timeout_ms` (each version includes the ones before it); local packages must use `source_kind: "local"`, `backend_entrypoints`, and `ui_contributions` (the legacy `ui_entrypoint` is rejected).

Every backend and UI path must be a package-relative file path: no absolute paths and no `..`. The active platform's backend must exist and be executable. On Unix, its executable mode is preserved in the imported copy.

### Background service

An optional `background_service` object makes PlaneAI call one sidecar method on a timer: `{ "method": "my-plugin.refresh", "interval_setting": "my_refresh_interval_ms", "default_interval_ms": 120000 }`.
PlaneAI calls `method` with no params, waits for it to answer, then waits the interval before the next call.
A run may take 40 minutes, or the optional `timeout_ms`, from 1 to 3600000, before PlaneAI cancels it; a UI call of `method` gets the same budget.
`timeout_ms` requires host API `planeai.plugin-host.v8`, since older hosts reject it as an unknown field.
PlaneAI sends a plugin one request at a time, so a request made during a run waits for its turn up to its own timeout, then fails without being sent.
A request that gets its turn has its full timeout from then on.
The interval is the positive integer stored under `interval_setting` in the plugin's settings, or `default_interval_ms` when that key is missing.
After each successful call, PlaneAI tells the plugin's UI its data changed, unless the method's JSON-RPC result is an object with `"changed": false`.

```json title="planeai-plugin.json" showLineNumbers
{
  "schema": "planeai.plugin.v1",
  "id": "local-fixture",
  "name": "Local Fixture",
  "version": "0.1.0",
  "host_api_version": "planeai.plugin-host.v6",
  "source_kind": "local",
  "backend_entrypoints": {
    "macos-arm64": "bin/macos-arm64/planeai-plugin-fixture",
    "macos-x64": "bin/macos-x64/planeai-plugin-fixture",
    "linux-x64": "bin/linux-x64/planeai-plugin-fixture",
    "linux-arm64": "bin/linux-arm64/planeai-plugin-fixture",
    "windows-x64": "bin/windows-x64/planeai-plugin-fixture.exe",
    "windows-arm64": "bin/windows-arm64/planeai-plugin-fixture.exe"
  },
  "capabilities": [
    "settings",
    "projects.read",
    "sessions.read",
    "tasks.read",
    "tasks.create",
    "task-events",
    "session-events",
    "providers",
    "tools"
  ],
  "ui_contributions": [
    {
      "id": "fixture",
      "label": "Fixture",
      "placement": "main-pane",
      "entrypoint": "ui/entry.js"
    }
  ],
  "providers": [
    {
      "id": "echo",
      "label": "Echo (fixture)",
      "entrypoint": "ui/chat.js",
      "supports": ["auto_approve"]
    }
  ],
  "tools": [
    {
      "name": "echo",
      "description": "Echo a text back with the session and project that called the tool.",
      "inputSchema": {
        "type": "object",
        "properties": { "text": { "type": "string" } },
        "required": ["text"]
      },
      "annotations": { "readOnlyHint": true },
      "timeoutMs": 5000
    }
  ]
}
```

The platform key is the current OS and architecture: `macos-arm64`, `macos-x64`, `linux-arm64`, `linux-x64`, `windows-arm64`, or `windows-x64`. Ship each declared binary at its declared path; a package may declare only the platforms it actually supports, but it cannot install on a platform without a matching binary. Windows backend filenames conventionally end in `.exe`.

### Downloads

PlaneAI can install a plugin straight from a Discovery card (see [Install from Discovery](#install-from-discovery)).
It looks for the package archive in two places, in this order.

1. The optional `downloads` object in the `planeai-plugin.json` on your repository's default branch, keyed by platform: `"downloads": { "macos-arm64": "https://github.com/acme/planeai-plugin-acme/releases/latest/download/planeai-plugin-acme-macos-arm64.tar.gz" }`.
   Every URL must be `https`, and every key must be one of the platform keys above.
   The installed copy of the manifest is validated the same way and the field is otherwise ignored.
2. The latest non-draft, non-prerelease GitHub release, when the default-branch manifest is missing, unparseable, or has no entry for the host platform.
   The asset must be named `<repository name>-<platform>.tar.gz` or `<repository name>-<platform>.zip`, for example `planeai-plugin-acme-macos-arm64.tar.gz` in `acme/planeai-plugin-acme`.

An archive must contain exactly one top-level directory, and that directory must hold `planeai-plugin.json` and the package laid out as above.
Absolute entries, `..` components, symbolic links and hard links are rejected.
Tag releases `vX.Y.Z` (the `v` is optional) and keep the manifest `version` equal to the tag of the release that serves the archive, so Discovery can offer updates and the install review does not warn about a mismatch.
When the archive is an asset of the latest release, PlaneAI verifies its SHA-256 against the digest GitHub reports for that asset.
Any other URL, and a release asset for which GitHub reports no digest, installs as unverified and the review shows the computed hash.

### Capabilities

Capabilities are an explicit contract for PlaneAI data RPC. Local plugins may request `settings`, `projects.read`, `sessions.read`, `sessions.repository-context`, `sessions.prompt`, `session-events`, `sessions.actions`, `sessions.advisories`, `sessions.complete`, `sessions.start`, `tasks.read`, `tasks.create`, `tasks.transition`, `tasks.sync`, `task-events`, `providers`, and `tools`; duplicates and all other local capabilities are rejected.

- `settings` permits sidecar callbacks `host.settings.get` and `host.settings.replace`.
- `projects.read` permits `host.projects.list`, returning non-hidden active projects.
- `sessions.read` permits `host.sessions.list`, returning safe metadata only: identity, project, name, branch, status, provider/backend, task key, timestamps, tab count, and legacy migration state. It never returns terminal names, provider session IDs, worktree paths, output, or control handles.
- `sessions.repository-context` permits `host.sessions.repositoryContext` for an explicitly supplied session ID. It returns only that session's ID, project ID, resolved working-tree path, branch, base branch, status, and linked task key. Use it for provider-neutral repository integrations; it does not grant terminal output, environment variables, provider session IDs, or access to other sessions.
- `sessions.prompt` permits a sidecar callback `host.sessions.prompt` with `{ "session_id", "text" }`. The text must be nonempty and at most 100,000 characters. PlaneAI resolves the requested active session and delivers the text using its normal prompt path (including backend routing and prompt locking); plugins never receive raw terminal or PTY access. Treat this as privileged: granting it lets the plugin submit prompts to any active session whose ID it knows.
- `sessions.actions` permits a sidecar callback `host.sessions.actions` with `{ "actions": [{ "id", "label", "providers"? }] }`. PlaneAI replaces that plugin's host-rendered session context-menu actions atomically. `providers` is an optional allowlist of configured provider IDs; omit it or send `[]` for all providers. Selecting an action invokes the owning sidecar's `plugin.sessionAction` with `{ "action_id", "session_id" }`.
- `sessions.advisories` permits `host.sessions.advisory` with `{ "session_id", "message", "severity" }`, where severity is `info`, `warning`, or `error`. PlaneAI renders the advisory; sidecars never inject UI directly.
- `sessions.complete` permits `host.sessions.complete` with `{ "session_id", "message"? }`. PlaneAI presents its standard Archive/Destroy/Keep prompt, or Done/Nothing when the selected session has a linked task. Thus completion requests reuse the host's session cleanup and linked-task behavior rather than asking plugins to control sessions.
- `tasks.read` permits the keyed single-task lookup aliases `host.tasks.read` and `host.task.get`. Each accepts `{ "key": "TASK-123" }` and returns `{ "task": ... }` (or `{ "task": null }` when no task matches).
- `tasks.create` permits `host.tasks.create` and `host.tasks.createChild`.
  `host.tasks.create` creates a top-level `todo` task.
  It requires `project_path`, `title`, and a plugin-scoped `operation_id`, accepts an optional `description`, integer `priority` (default `0`) and `tags` array, and rejects `parent_key`.
  Every two-word param also accepts its camelCase spelling (`projectPath`, `operationId`, `parentKey`).
  `title` is at most 255 characters, `description` at most 65,536, `operation_id` at most 256, `tags` at most 20 of 64 characters each, none may contain NUL, and `priority` is 0 (none) to 5 (highest), the scale the Jira plugin writes.
  PlaneAI rejects hidden or unknown projects, returns the originally created task when the same operation is retried, and refreshes the task list.
  A request PlaneAI refuses (bad params, an unknown or hidden project, a missing or foreign parent, a `start` naming a provider nothing configured or installed provides) answers `-32602`, which retrying cannot fix.
  A `start` naming the provider of an installed plugin that is not running, as right after PlaneAI launches, answers `-32004` and creates nothing, so retry it later.
  `-32603` is a host failure worth retrying.
  `host.tasks.createChild` requires `project_path` (or `projectPath`), `parent_key` (or `parentKey`), `title`, `description`, and a plugin-scoped `operation_id` (or `operationId`); PlaneAI verifies the parent belongs to the project, or is a synced task the calling plugin owns (see `tasks.sync`), and returns the originally created child when the same operation is retried.
  Both return `{ "task": ... }`.
- `sessions.start` lets `host.tasks.create` start the new task's session, as the task form's "Start session immediately" does.
  Add `start: { "provider"?: string | null, "use_worktree"?: boolean, "auto_approve"?: boolean }` to the request; omitted fields default to the configured default provider, a worktree, and auto-approve.
  A request with `start` but without `sessions.start` fails as not granted, and `host.tasks.createChild` rejects `start`.
  PlaneAI renders the branch, session name, and prompt from the configured task templates (or their defaults), launches the session, and moves the task to `in_progress`.
  Auto-approve is turned off for a plugin provider that does not support it.
  The reply does not wait for the launch: it is `{ "task": ..., "session": "starting" }`, or `"exists"` when the task already has a session.
  PlaneAI records the requested start with the operation until a session exists or the start fails, and resumes a start still pending when it next launches, so quitting mid-start loses nothing.
  Retrying the same `operationId` never starts a second session, and the first request's `start` decides: a retry answers `"starting"` while the start is pending, `"exists"` once it succeeded, and `"failed"` with `"session_error"` once it failed.
  A failed start, or one that succeeded with a warning, is shown to the user naming the plugin, as in "Routines created KEY, but its session could not start: ...".
  `sessions.start` also permits `host.sessions.providers`, returning `{ "default": "claude", "providers": [{ "key", "label", "auto_approve" }] }`: the configured providers, then those of running plugins, as the task form lists them.
  `auto_approve` is `false` for a plugin provider that does not support it.
  With the `local` session backend the agent spawns only when a terminal view attaches, so a session started this way waits until the user opens it.
- `tasks.sync` (host API v7) is for a plugin that mirrors an external tracker's issues as PlaneAI tasks, as the bundled Jira plugin does.
  It permits `host.task.create` and `host.task.update`, and the UI's `rpc.call("task.create")` and `rpc.call("task.update")`.
  `host.task.create` takes `{ "key", "title", "description"?, "status"?, "priority"?, "tags"? }` and creates a project-less task with exactly that key.
  `key` is required and must look like `PREFIX-N`: an uppercase letter, then uppercase letters or digits, a dash, and a number without leading zeros, as in `ENG-123`.
  `status` is `todo`, `in_progress`, `in_review`, or `done` (default `todo`), and `priority` an integer (default `0`).
  The task lives in a namespace of the calling plugin's own, which makes the plugin its owner.
  A key one of the plugin's own tasks already holds rewrites that task as the create describes it, keeping its status unless `status` is given, so a reinstalled plugin resumes the tasks it created; a key any other task holds answers `-32005`, which no retry fixes, so report it once rather than on every sync.
  `host.task.update` takes `{ "key", "title"?, "description"?, "status"?, "priority"?, "tags"? }`, changes only the fields given, and succeeds only on a task the calling plugin owns; a key another task holds answers `-32005`, and one no task holds answers not granted (`-32003`).
  Both return the task.
  Children go under a synced task with `host.tasks.createChild`, from any project, since the task belongs to none.
  A project's own task numbers skip keys synced tasks hold, so a tracker key like `PLA-12` never collides with a project's `PLA` tasks.
  Removing the plugin leaves its tasks as they are.
- `tasks.transition` permits `host.sessions.transitionLinkedTask` with `{ "session_id", "status" }`. PlaneAI resolves the task strictly from that session's linked task key, changes its lifecycle status, and emits the normal task lifecycle batch (including automatic parent completion).
- `task-events` permits event delivery only when the handshake also subscribes to `task.lifecycle`.
- `providers` permits declaring `providers` (host API v3 only) and the provider session contract described in [Providers](#providers). It is required exactly when `providers` is declared.
- `tools` permits declaring `tools` (host API v6) that agents in PlaneAI sessions call, described in [Tools](#tools). It is required exactly when `tools` is declared.
- `session-events` permits best-effort `plugin.sessionLifecycle` delivery only when the handshake also subscribes to `session.lifecycle`. Events describe committed host session status changes and include the session/project identity, branch, linked task key, previous status, and new status.

The sandbox UI can use the same read/create operations directly through `context.host.rpc.call("projects.list")`, `context.host.rpc.call("sessions.list")`, `context.host.rpc.call("task.get", { key })`, `context.host.rpc.call("tasks.create", params)`, `context.host.rpc.call("tasks.createChild", params)`, and `context.host.rpc.call("sessions.providers")`. The sidecar uses matching nested callbacks with the `host.` prefix. PlaneAI derives the owning plugin identity for both transports and applies identical manifest capability checks.

Settings are a JSON object that PlaneAI owns and persists atomically; its on-disk implementation location is not a plugin API. `host.settings.get` returns `{ "settings": { ... } }`; `host.settings.replace` accepts either `{ "settings": { ... } }` or an object directly and returns the same envelope. Both the fixture's `fixture.persistSettings` sidecar example and its UI settings bridge use that public host API. Do not place credentials or tokens in it.

## Native sidecar protocol

The sidecar uses JSON-RPC 2.0 over stdin/stdout, with **one UTF-8 JSON object per newline-delimited frame**. Stdout is protocol-only: never print diagnostics, progress, or stack traces there. Write diagnostics to stderr; PlaneAI drains it to the plugin's `stderr.log` while the plugin is running. Frames are limited to 64 KiB, including the newline.

PlaneAI begins with `plugin.handshake`; respond with the manifest identity and API version. The fixture also requests task lifecycle delivery:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "plugin_id": "local-fixture",
    "plugin_name": "Local Fixture",
    "plugin_version": "0.1.0",
    "host_api_version": "planeai.plugin-host.v1",
    "lifecycle_event_subscriptions": ["task.lifecycle"]
  }
}
```

The handshake params carry `host_api_version`, the manifest-granted `host_capabilities`, and `host_features`, the optional host behaviors this host offers (currently `provider_sessions.reconcile`).
Ignore params and features you do not know; newer hosts add them.

Response IDs must exactly correlate with the request ID. Return either `result` or a JSON-RPC `error`, not both. PlaneAI sends the reserved `plugin.shutdown` during disable/reload/app shutdown; acknowledge it and exit promptly. Do not expose or invoke `plugin.handshake` or `plugin.shutdown` from plugin UI.

A sidecar may make a host callback, typically while PlaneAI is waiting for its response.
Send a normal JSON-RPC request on stdout, wait for the correlated host response on stdin, then finish the original request.
PlaneAI answers callbacks as they arrive, in order, even once the request that prompted them timed out, so cancellation cleanup may still call the host.
A failed callback answers with a JSON-RPC error code: `-32601` for an unknown host method, `-32003` for a capability the plugin was not granted, `-32602` for invalid params, `-32004` for a valid request PlaneAI cannot serve yet and the plugin should retry later, `-32005` for a `tasks.sync` create or update under a key another task holds, and `-32603` for anything else. `fixture.persistSettings` demonstrates `host.settings.get` followed by `host.settings.replace`. Keep callback IDs distinct from the active request ID and continue reading until the matching response arrives.

Handshake and ordinary RPC calls have a five-second deadline; provider methods have [their own](#providers).
The method a manifest declares as its `background_service` has 40 minutes, or its `timeout_ms`, whether its interval or a UI call runs it.
On deadline expiry, PlaneAI sends the JSON-RPC `$/cancelRequest` notification with `{ "id": <original request ID> }`.
Keep reading stdin while work is active; cancel cooperatively and return the original request's JSON-RPC error `{ "code": -32800, "message": "request cancelled" }` within three seconds.
PlaneAI treats any other response or no cancellation response as a failed runtime and stops it.
A runtime that never answered the cancellation is started again, at most once every ten minutes, unless its process exited or its stdout failed.
`plugin.shutdown` also has a three-second grace period before the process is killed.
PlaneAI attempts it on every app quit and restart within that same budget, so a plugin must tolerate being killed without it.
Do not define a competing cancellation wire format in v1.

The host supplies `PLANEAI_PLUGIN_DATA_DIR` and `PLANEAI_PLUGIN_SECRETS_DIR`. Store public, replaceable plugin state in the former. Keep secrets backend-only in the latter: the UI settings bridge and sidecar settings callback never return secret files. The fixture's `fixture.status` reports both paths only to demonstrate their presence; real plugins should not surface secret paths or contents to UI.

The host also supplies `PATH`. A GUI launch (Spotlight, Finder, Dock) inherits a minimal `PATH` from the OS that excludes user-local bin directories, so PlaneAI replaces it with the same augmented `PATH` it gives agent sessions: the user's `extra_path_dirs` config, then conventional developer directories (`~/.local/bin`, `~/.cargo/bin`, `~/go/bin`, `/opt/homebrew/bin`, `/usr/local/bin`), then the inherited `PATH`. A backend that shells out to a CLI can rely on plain `PATH` lookup. If a user installs a tool somewhere unconventional (version-manager shims, for example), they add that directory to `extra_path_dirs` in `~/.config/planeai/config.json`.

Task lifecycle delivery is best-effort and isolated from PlaneAI task commits. Subscribe in the handshake, declare `task-events`, make handlers idempotent, and log failures to stderr. The fixture handles `plugin.taskLifecycle` and emits a lifecycle diagnostic.

## Providers

A plugin can run sessions itself.
Each entry in `providers` becomes a **runtime-backed provider** that users pick when creating a session, beside the command providers from their config.
The session is a normal PlaneAI session (worktree, branch, linked task, sidebar status and lifecycle), but the plugin runs the agent and its UI replaces the terminal in the session's agent tab.
See ADR-0014.

```json
"host_api_version": "planeai.plugin-host.v3",
"capabilities": ["providers"],
"providers": [
  { "id": "echo", "label": "Echo (fixture)", "entrypoint": "ui/chat.js", "supports": ["auto_approve"] }
]
```

`id` follows the plugin id rules and must be unique; the provider key stored on sessions is `<plugin id>:<provider id>`.
`entrypoint` is a package-relative UI bundle with the same rules as UI contributions.
`supports` may list `auto_approve` when the provider honors PlaneAI's auto-approve; otherwise PlaneAI disables auto-approve for it.
It may also list `handoff` when the session can continue in the agent's own terminal UI.
Provider fields and features this host does not know are ignored, so a plugin written for a newer host still loads; a duplicate feature is rejected.
The v3 contract is unstable until the first provider plugin ships, so expect changes.

### Session methods

PlaneAI calls these sidecar methods.
Each must return promptly and do its work asynchronously; `send` means the input is accepted and queued, not that the turn ran.
Results are JSON objects, and PlaneAI ignores fields it does not know.

| Method                        | Params                                                                                               | Meaning                                                                                                                                 |
| ----------------------------- | ---------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `provider.session.start`      | `session_id`, `provider_id`, `cwd`, `env`, `auto_approve`, optional `initial_prompt`, optional `mcp` | A new session was created. Run the agent in `cwd` with `env`.                                                                           |
| `provider.session.resume`     | `session_id`, `provider_id`, `cwd`, `env`, `auto_approve`, `handed_off`, optional `mcp`              | The current sidecar does not drive this existing session yet, after an app restart or plugin reload. Restore it; do not replay prompts. |
| `provider.session.send`       | `session_id`, `text`                                                                                 | Input from the chat UI, the CLI, or another plugin's `sessions.prompt`.                                                                 |
| `provider.session.interrupt`  | `session_id`                                                                                         | Stop the current turn.                                                                                                                  |
| `provider.session.stop`       | `session_id`, `reason` (`archive`, `destroy` or `exit`)                                              | The session ended. Release its process; on `destroy`, delete its data.                                                                  |
| `provider.session.handoff`    | `session_id`                                                                                         | With `handoff` only. Stop driving the session and return `{ "argv": [...] }`, the command that continues it in a terminal.              |
| `provider.session.handback`   | `session_id`, optional `mcp`                                                                         | With `handoff` only. The terminal tab closed, or the handoff answer had no runnable `argv`; drive the session again.                    |
| `provider.sessions.reconcile` | `sessions`: `[{ "session_id", "provider_id", "status" }]`                                            | Sent once after the handshake. The plugin's sessions that still exist; see [Lifecycle](#lifecycle).                                     |

`session_id` is a lowercase UUID.
`cwd` is the session's worktree, or its project's root for a session without one.
`env` is an overlay on the sidecar's own environment: `PLANEAI_SESSION_ID`, `PLANEAI_SOCKET` and the augmented `PATH`, so the agent can still spawn sub-sessions and manage tasks.

`mcp` is `{ "url", "token" }`: PlaneAI's MCP server and a bearer token that authenticates as the session, so the agent can call PlaneAI's tools and those of running plugins (ADR-0017).
It is present while the server listens and absent otherwise; older providers can ignore it.
Each one is a fresh token that revokes the session's previous one, so connect the agent you start or restore with the latest you received.
A `resume` of a handed-off session carries none, because the terminal's agent still uses the current token; the `handback` that follows brings a new one.
Treat the token as a secret: keep it out of logs, argv, and the UI, and hand it to the agent through its environment or its MCP client configuration.
The program a handoff runs gets the session's URL and a token as `PLANEAI_MCP_URL` and `PLANEAI_MCP_TOKEN` in its environment, never in its argv, which reaches PlaneAI's UI; reference the variable instead, as in Claude Code's `--mcp-config` header `Bearer ${PLANEAI_MCP_TOKEN}`.

Start, resume, handoff and handback have a 30-second deadline, since they may launch the agent; every other provider method has five seconds.
On expiry PlaneAI cancels the request as described above.

PlaneAI rejects a `send` text or `initial_prompt` over 48 KiB, measured as JSON-escaped text, so every request fits one 64 KiB frame.
Prompts from the CLI are checked against the same limit before they are queued, and a prompt PlaneAI cannot deliver to the plugin is reported in the app.
Provider UIs should apply the same limit before calling `send`.
A handoff `argv` must have 1 to 64 nonempty arguments without NUL; PlaneAI runs it as is, without a shell, and the tab closes when it exits.

`provider.*` methods are reserved for PlaneAI: plugin UI cannot call them, and `planeai-cli plugin test` scenarios cannot send them.

### Errors

Answer a provider method you cannot serve with one of these JSON-RPC error codes, which PlaneAI acts on:

| Code     | Meaning                                                                                                        |
| -------- | -------------------------------------------------------------------------------------------------------------- |
| `-32010` | The plugin does not know the session. PlaneAI resumes it once and retries the request.                         |
| `-32011` | The session is handed off to a terminal.                                                                       |
| `-32012` | The prompt is too large.                                                                                       |
| `-32013` | The agent cannot run now, for example because its CLI is missing or signed out. Put the reason in the message. |

Any other error reaches the session UI as a plugin error with its message.

### Lifecycle

- `resume` must accept any session id, including one whose state the plugin lost; restore what it can and start fresh otherwise.
- `stop` and `handback` are idempotent, and succeed for a session the plugin does not know.
  A repeated `handoff` returns the command again.
- PlaneAI sends `stop` whenever a session ends while the plugin runs, even if the current sidecar never resumed it, so the plugin can drop what it keeps for it.
  That includes a `stop` with `exit` after the plugin reports `exited` itself.
  Treat an unknown stop `reason` as `archive`.
- PlaneAI holds the handoff state.
  `resume` says whether the session is handed off in `handed_off`; never persist it, since a terminal does not outlive the app.
- After the handshake, before any start or resume, PlaneAI sends `provider.sessions.reconcile` with every session of this plugin's providers that still has a row and is not destroyed, with its PlaneAI status (`active`, `exited` or `archived`).
  Drop what you keep for any other session, as it was destroyed or deleted while the plugin was not running.
  Sessions the plugin started or resumed in this process are never dropped.

### Notifications

The sidecar reports back with JSON-RPC **notifications** (frames without an `id`), which PlaneAI reads at any time, not only during a request:

- `host.providerSession.status` with `{ "session_id", "status" }`, where status is `busy`, `idle`, `needs_attention` or `exited`.
  It drives the sidebar, attention notifications and the quit confirmation.
  It is the only status source for the session: PlaneAI ignores hook and PTY signals for provider sessions, even though the user's agent hooks still run inside them.
  `exited` marks the session exited.
  Only an `idle` or `needs_attention` that ends a `busy` turn notifies the user, so reporting `idle` when a session starts or resumes is fine.
  Once PlaneAI begins stopping the plugin, or the sidecar dies, statuses from it are ignored and its sessions stop showing as busy; they resume on next use.
- `host.providerSession.event` with `{ "session_id", "seq", "payload" }`.
  PlaneAI forwards `payload` unchanged to the session's mounted UI.
  `seq` is an integer from 1 to 2^53 - 1 that strictly increases for the session's whole lifetime, across sidecar restarts; gaps are allowed.
  PlaneAI drops an event whose `seq` does not follow the session's last one.
  Keep each frame under 64 KiB; send large outputs in pieces or let the UI fetch them.

Notifications for sessions the sidecar has not started or resumed are dropped, and PlaneAI ignores fields it does not know.
A notification to an unknown `host.*` method is logged and ignored, so a plugin can target newer hosts; any other notification is still a protocol error.

### Session UI

A provider's UI mounts in the `session.main` placement, which only providers use.
It receives the selected session in `context.session`, its provider in `context.provider` (`id`, `label` and `supports`), and the session's controls on `context.host.session`, which no other contribution gets:

- `send(text)` and `interrupt()` route through PlaneAI to `provider.session.send` and `provider.session.interrupt`, so every input path behaves the same.
- `handoff()` asks the provider for its terminal command and opens it in a new terminal tab of the session.
  `handback()` closes that tab, which returns the session to the provider; so does the command exiting.
  Both are present only when the provider supports `handoff`.
  While handed off, `send()` is refused.
- `onEvent(listener)` receives `{ seq, payload }` for this session only and returns an unsubscribe function.

A refused request rejects with an `Error` whose `code` says why: `handed_off`, `prompt_too_large`, `not_running`, `unsupported`, `unavailable` or `plugin_error`.

PlaneAI unmounts the UI when the user switches sessions, and resumes the session (if needed) before mounting it again, so the sidecar owns the transcript.
To rebuild the view, subscribe first, fetch a snapshot through a plugin-defined `context.host.call(...)` method, then drop live events whose `seq` is at or below the snapshot's.
Every response is one frame, so a long transcript must come back in pages (for example, events after a given `seq` until the sidecar reports no more), or a single oversized response stops the runtime.
The fixture's `ui/chat.js` and `fixture.providerSnapshot` show the pattern on a transcript small enough for one page.

## Tools

A plugin can give agents tools.
Agents running in PlaneAI sessions reach PlaneAI's MCP server (ADR-0016), and every tool a running plugin declares joins PlaneAI's own tools there.
See ADR-0017.

```json
"host_api_version": "planeai.plugin-host.v6",
"capabilities": ["tools"],
"tools": [
  {
    "name": "echo",
    "description": "Echo a text back, with the calling session.",
    "inputSchema": { "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] },
    "annotations": { "readOnlyHint": true },
    "timeoutMs": 5000
  }
]
```

- `name` starts with a lowercase letter and contains only lowercase letters, digits, and underscores, and is unique within the plugin.
  Agents call the tool as `<plugin id>_<name>`, which Claude Code shows as `mcp__planeai__<plugin id>_<name>`.
  That name must be at most 50 characters and must not be one of PlaneAI's own tools (`task_list`, for example, for a plugin `task` with a tool `list`), or the package does not install.
  Plugin ids contain no underscore, so two plugins' tools never share a name.
- `description` is a nonempty string, and `inputSchema` a JSON Schema whose `type` is `object`, as MCP requires.
  Agents see both as written.
- `annotations` holds MCP's `readOnlyHint`, `destructiveHint`, `idempotentHint`, and `openWorldHint` booleans, with MCP's defaults.
  They decide the tool's effect: read-only, a write (idempotent or not), or destructive.
  A tool that is not read-only is destructive unless it sets `destructiveHint: false`, so a tool without annotations counts as destructive.
  A tool cannot be both read-only and destructive.
- `timeoutMs` is how long PlaneAI waits for the tool, from 1 to 120000; the default is 30000.

Effects decide approval as for PlaneAI's own tools: an agent PlaneAI launches, from the app, `planeai-cli` or another agent, runs the read and write tools of the plugins running at its launch without asking, and asks before a destructive one or the tool of a plugin enabled later.

Unknown fields in a tool or its annotations are rejected.

### Calls

PlaneAI lists a plugin's tools while the plugin is running: enabling it adds them, and disabling, reloading or a crash removes them, without restarting the app.
The MCP server runs in stateless mode, so it cannot notify connected agents; they see the change on their next `tools/list`, which Claude Code and Codex make when they connect.
A call of a tool whose plugin stopped meanwhile answers the agent with an error.

When an agent calls the tool, PlaneAI sends the sidecar a `tool.call` request:

```json
{
  "tool": "echo",
  "arguments": { "text": "hi" },
  "caller": {
    "session_id": "1b4e28ba-2fa1-11d2-883f-0016d3cca427",
    "project": { "id": "4f6c...", "name": "planeai" },
    "task": { "key": "PLA-12", "project": { "id": "4f6c...", "name": "planeai" } },
    "auto_approve": false
  }
}
```

`tool` is the name from the manifest, without the plugin prefix.
`arguments` is what the agent sent; PlaneAI does not check it against `inputSchema`, so validate it.
`caller` is the session the agent runs in, taken from its MCP token: its project, its task when it has one (`null` otherwise) with the project owning that task, and whether it runs without permission prompts.
A plugin must not give a session more than it has: when `auto_approve` is `false`, do not do on the agent's behalf what its user has not approved, as PlaneAI's own tools refuse to start a `yolo` session for it.

Answer `{ "text": "...", "is_error": false }`; `is_error` defaults to `false`.
The agent reads `text` as the tool's whole result, so keep it compact (PlaneAI's own tools answer in TOON) and within the 64 KiB frame.
Report a failure the agent should read, such as an unknown key, with `is_error: true`.
A JSON-RPC error answer also reaches the agent as an error with its message.
After PlaneAI sends a call of a tool that is not read-only, whatever the plugin answers, it tells the plugin's UI its data changed, as `data.changed()` does.

PlaneAI waits `timeoutMs`, then cancels the request as described in [Native sidecar protocol](#native-sidecar-protocol).
A plugin that acknowledges the cancellation keeps running, and the agent reads that the plugin gave no answer within the timeout and the call was cancelled.
A plugin that does not acknowledge it is stopped, and the agent reads that `tool.call` timed out.
PlaneAI sends a plugin one request at a time, so a slow tool delays the plugin's other requests.
`tool.call` is reserved for PlaneAI: plugin UI cannot call it, and `planeai-cli plugin test` scenarios send it only through a tool line.

## UI contributions

An optional UI entrypoint is a self-contained browser ESM module default-exporting an object with `mount`:

```js
export default {
  mount(root, context) {
    // Render only inside root.
    return () => {
      // Remove listeners, timers, subscriptions, and DOM created by this mount.
      root.replaceChildren();
    };
  },
};
```

`root` is the sandboxed local iframe's `document.body`. Do not access PlaneAI's DOM, Tailwind classes, Tauri APIs, or `invoke()` directly. The bundle must contain all code it needs: v1 loads exactly the entrypoint source, not relative imports or package asset graphs.

### Local UI theme contract

PlaneAI injects a host-owned baseline stylesheet into every local UI iframe. It resets document sizing and box sizing, applies PlaneAI's default font/background/text colors, and provides low-specificity typography, borders, and focus styles for native controls. Your module's styles load afterwards, so you can override those defaults normally.

Frames sized to their content (`dialog`, `session.panel` and `sidebar.header`) get an auto-height, transparent document instead, so they shrink as their content does and show the host surface around them.
Measure your content rather than the viewport there: an element at `height: 100%` resolves to its content height.

Use only these stable semantic custom properties, never PlaneAI's internal `--color-*` or `--font-*` variables: `--planeai-font-sans`, `--planeai-font-mono`, `--planeai-canvas`, `--planeai-main`, `--planeai-surface`, `--planeai-surface-raised`, `--planeai-text`, `--planeai-text-muted`, `--planeai-text-subtle`, `--planeai-border`, `--planeai-border-strong`, `--planeai-accent`, `--planeai-on-accent`, `--planeai-accent-subtle`, `--planeai-success`, `--planeai-warning`, `--planeai-danger`, `--planeai-info`, `--planeai-radius`, and `--planeai-space-1` through `--planeai-space-6`. PlaneAI updates those properties in place whenever its active theme or appearance changes; it does not remount your UI.

The theme fonts are available in every local UI iframe, so text set in `var(--planeai-font-sans)` or `var(--planeai-font-mono)` renders in the same IBM Plex Sans and IBM Plex Mono as PlaneAI.
PlaneAI registers their Latin faces (Plex Sans at weights 100 to 700, Plex Mono at 400 and 500) in the frame's `document.fonts` before it calls `mount`, so your first layout already uses them.
The frame still cannot fetch fonts itself; characters outside the Latin range, and weights or styles outside those faces, fall back to the next family in the token's stack.

For example, local UI CSS can adopt or intentionally customize the host theme:

```css
:root {
  --planeai-radius: 6px; /* optional plugin-specific override */
}

.card {
  background: var(--planeai-surface);
  border: 1px solid var(--planeai-border);
  border-radius: var(--planeai-radius);
  color: var(--planeai-text);
}

.primary {
  background: var(--planeai-accent);
  color: var(--planeai-on-accent);
}
```

`context.host` provides:

- `call(method, params?)` — RPC scoped to the owning sidecar. Lifecycle methods are reserved.
- `settings.get<T extends Record<string, unknown>>()` and `settings.replace<T>(settings)` — typed public JSON-object settings. These are the UI counterpart to the capability-gated sidecar settings callbacks; they never expose secrets.
- `data.changed()` - tells PlaneAI the plugin's data changed, as do each run of the [background service](#background-service) and each call of a [tool](#tools) that is not read-only. A running `sidebar.section` remounts after this event.
- `data.onChanged(listener)` returns an unsubscribe function and calls `listener` when the plugin's data changed. A local `session.indicator`, `session.meta`, `sidebar.header`, `main-pane`, `dialog`, `titlebar`, or `interaction` frame receives it in place and keeps its state, so subscribe to refresh what it shows; so does a local `session.panel` on host API `planeai.plugin-host.v5` or later, while an older one remounts instead. Other local frames never call it.
- `navigation.open(pluginId, contributionId)`, `navigation.close()`, and `navigation.openPreferences()`.
- `sidebar.register(rows)`, `sidebar.select(rowId)`, and `sidebar.handleKeydown(event)` for sidebar navigation contributions. Always call the returned unregister function from your mount disposer.
- `interaction` on host API `planeai.plugin-host.v7` or later, and `data.onTaskDataChanged(listener)` for a `tasks.sync` plugin, described in [Interaction bridge](#interaction-bridge-host-api-v7).

The fixture UI calls `context.host.call("fixture.status")`, loads a saved greeting with `context.host.settings.get()`, replaces it when **Save greeting** is selected, calls `data.changed()`, and removes its click handler in its disposer.

Each `ui_contributions` item requires a unique safe `id`, `label`, `placement`, and `entrypoint`.
Supported placements are `sidebar.header`, `sidebar.navigation`, `sidebar.section`, `sidebar.footer`, `preferences`, `main-pane`, `dialog`, `session.panel`, `session.indicator`, `session.meta`, `titlebar`, and `interaction`.
An `interaction` frame requires host API `planeai.plugin-host.v7`, is mounted for as long as its plugin runs, draws nothing, and takes no pointer input or focus: it drives host-drawn toasts and forms through the [interaction bridge](#interaction-bridge-host-api-v7).
Sidebar contributions may set integer `order`.
`session.indicator` and `session.meta` are slots drawn on every sidebar row that has a session, with that row's session context; they take no shortcut and no order, and their frames get no pointer events and no focus.
A `session.indicator` is a 16px square beside the row's status.
A `session.meta` is a 16px-tall inline slot after the branch on the third line of a Cards row, for example a `#656` pull-request pill; the Classic layout does not show it.
Its local frame starts 0px wide, so post `{ type: "content-width", width }` to `parent` once the content is laid out (lay it out with `width: max-content`); the slot follows that width up to 160px, and reporting `0` collapses it.
On host API `planeai.plugin-host.v5` or later, when the plugin also declares a `session.panel`, the slot becomes a focusable button: clicking it selects that row's session and opens the plugin's first `session.panel` in PlaneAI's dialog.
On older host APIs, or without a `session.panel`, the slot stays visual-only.
When the slot is a button, describe it on hover and keyboard focus by posting `{ type: "tooltip", lines: [{ text, tone? }] }` to `parent`; PlaneAI draws the tooltip and adds the lines to the button's accessible name.
`tone` is one of `success`, `warning`, `danger`, `info`, or `muted`, and colors the line like the matching `--planeai-*` token, with `muted` as `--planeai-text-subtle`.
PlaneAI keeps the first 12 lines and the first 200 characters of each, and an empty `lines` list removes the tooltip.
A local `sidebar.header`, `dialog`, or `session.panel` frame is as tall as its content, which the host bridge measures and reports, so a compact header control takes only the space it draws.
`main-pane`, `dialog`, and `session.panel` contributions may declare an optional portable `Mod+[Shift+][Alt+]A-Z` shortcut, and `main-pane` and `dialog` contributions are discoverable in Cmd+K while running.
PlaneAI rejects duplicate declared shortcuts across installed plugins.
When a matching selected-session panel is available, it receives its declared chord before a main-pane or dialog contribution or a legacy host fallback; otherwise the host's normal shortcut behavior continues.
A `dialog` contribution opens in PlaneAI's dialog, titled with the contribution's `label`, over whatever the user is doing and with or without a selected session.
It opens from Cmd+K, from its shortcut (which also closes it), or from `navigation.open(pluginId, contributionId)` in any of the plugin's frames, such as a `sidebar.header` button.
Only one plugin dialog is open at a time, and opening a `main-pane` contribution closes it.
Escape, the dialog's close button, and `navigation.close()` close it and return focus where it was; a frame that handles Escape itself and calls `preventDefault()` keeps it, for example to close an inline editor first.
The dialog is 640px wide, or narrower in a small window, and scrolls its content past 85% of the window's height, so lay the UI out for that width and let it grow with its content instead of filling a fixed height or scrolling internally.
Its frame receives no `session` context.
A `session.panel` contribution is also discoverable as an action at the end of the titlebar whenever a session is selected; PlaneAI supplies its UI entrypoint the selected session's identity, project ID, branch, base branch, status, provider, and linked task key.
A compact `titlebar` contribution is rendered independently at the end of the titlebar with the same selected-session context.
It can call `navigation.open(pluginId, contributionId)` to open a declared `session.panel` in the same dialog, rather than replacing the main workspace.
During the GitHub migration, its plugin-owned pull-request chip deliberately appears beside the retained legacy PR-status chip or **Create PR** affordance; the legacy UI and routes remain available as a fallback when the plugin is unavailable.
UI context does not supply a working-tree path; request the separately capability-gated `host.sessions.repositoryContext` operation when needed.
Use the placement's available space conservatively; the host owns focus, navigation, keyboard routing, lifecycle, loading/retry UI, and teardown.
Inside a local plugin frame, PlaneAI forwards Cmd/Ctrl chords to its own shortcut router, so app shortcuts such as Mod+[ keep working while the frame has focus.
Call `preventDefault()` on a chord to keep it for your UI; Ctrl+Tab and Mod+N always reach PlaneAI.
A provider session's UI owns the keyboard like a terminal: PlaneAI takes focus out of its frame when the sidebar or a dialog takes over, and gives it back when its pane does.

### Interaction bridge (host API v7)

On host API `planeai.plugin-host.v7` or later, every local frame's `context.host` also has `interaction`, and a `tasks.sync` plugin's also has `data.onTaskDataChanged`; on older host APIs both are absent, and PlaneAI refuses the underlying messages.

- `interaction.showToast({ id, kind?, message, detail?, actions?, dismiss? })` shows a toast PlaneAI draws and resolves with `id` once it shows.
  `id` is scoped to the frame that shows it, so it never collides with another plugin's or another frame's, and showing the same `id` again replaces that toast in place.
  `kind` is `info` (the default), `success`, `warning`, or `error`.
  `actions` is a list of `{ key, label, run }`, where `key` is a single lowercase letter other than `d` or `c`, or `Enter`, and `run` is a function PlaneAI calls in your frame when the user picks the action.
  `dismiss` is `"enabled"` (the default) to let the user dismiss the toast, `"disabled"` to refuse, or a function PlaneAI calls in your frame once the user dismissed it.
  Picking an action or dismissing removes the toast before your callback runs, so the callback may show the next toast under the same `id`.
  A callback that throws or rejects shows its error as a PlaneAI notification.
- `interaction.dismissToast(id)` removes one of your toasts without running its callbacks.
- `interaction.openProjectForm()` opens PlaneAI's Add Project form and resolves with the new project's `{ id, name, path }`, or `null` when the user cancels.
- `data.onTaskDataChanged(listener)` returns an unsubscribe function and calls `listener` whenever PlaneAI's task data changes, for a plugin holding `tasks.sync`, for example to refresh child counts beside synced tasks.

When your frame goes away, as when the plugin stops or reloads, PlaneAI removes the toasts it showed, since their callbacks went with it.
The TypeScript shapes are `PluginFrameInteraction`, `PluginFrameToast`, and `PluginFrameProject` in `src/lib/plugin-sdk.ts`.

## Install, use, reload, and remove

1. Build and stage the current-platform fixture binary from the repository root:

   ```bash
   make local-plugin-fixture
   ```

2. To find community candidates, open **Preferences → Plugins** and choose **Discover plugins**. PlaneAI asks your existing `gh` CLI to search public, non-archived, non-fork GitHub repositories tagged with the fixed `planeai` topic, returning up to 100 results ordered by stars. Results exist only for the current app session and are refreshed only when you choose discovery again. Candidates are unverified community repositories, not compatibility claims. Review the repository and trust its author before installing anything from it.
3. Either install from the card (see [Install from Discovery](#install-from-discovery)), or choose **Install local package** and select a reviewed package directory, not an individual binary or manifest.
4. Enable the imported plugin. Open its `main-pane` from Cmd+K or interact with its declared sidebar/preferences placement. The fixture appears as **Local Fixture**.
5. Use **Reload** after a runtime error or to restart an already imported package. Reload restarts the imported immutable copy; it does not reread your source directory.
6. To publish a changed manifest, binary, or UI, rebuild and choose **Install local package** again. Selecting a package with the same installed local-plugin ID replaces its immutable package copy, preserves host-owned settings, secrets, logs, and data, and restarts the plugin if it was enabled. Imported content is copied under a SHA-256 directory, so edits to the original directory never affect an installed version.
7. Disable a plugin to stop its sidecar. **Remove plugin** is available only for local packages and deletes PlaneAI's imported package and host-owned plugin state. It does not delete your original source directory.

### Install from Discovery

Right after discovery returns, every card checks what the repository offers for this platform (see [Downloads](#downloads)) and what is already installed, and shows one of:

- **Install vX** for a repository you have not installed, or plain **Install** when there is no semver release tag.
- **Update to vY** beside **Installed vX** when the latest release vY is newer than the installed manifest version vX, or **Up to date (vX)** when it is not.
- **Reinstall latest** beside the installed version for a repository-installed plugin whose versions cannot be compared.
- **Installed locally (vY)** with **Replace with vX** when a plugin with the same `id` was installed from a folder.
- **Installed from github.com/<other repository> (vY)** with **Replace with vX** when a plugin with the same `id` was installed from a different repository, so another author's package is never offered as an update.
- A sentence explaining why nothing can be installed, for example no archive for your platform or an `id` reserved by a bundled plugin.

Choosing the action downloads the archive into a staging directory, verifies it against the release asset digest when it is a release asset, extracts and validates it, and opens a review.
The review lists the manifest `id`, name and version, the repository, the source URL, the release tag, the archive SHA-256 marked **Verified** or **Unverified**, the capabilities, UI placements and MCP tools it declares, what it replaces, and a warning when the release tag and the manifest version disagree.
The installed list shows a different hash, the SHA-256 of the imported package tree.
Nothing is installed until you choose **Install and enable**; cancelling or closing the review discards the download.
Installing keeps the plugin's host-owned settings, secrets, logs and data, exactly like **Install local package** with the same `id`, and enables the plugin.
The installed list then shows `Source: github.com/<repository> @ <tag>` instead of a folder path, and a later **Install local package** of the same `id` replaces that origin.
PlaneAI does not poll for updates: a newer release is only noticed when you open Discovery again.

## Headless contract test

After staging the current-platform executable, run the shipped harness without launching the desktop UI:

```bash
# Standard handshake, event-delivery, and shutdown checks.
planeai-cli plugin test --package src-tauri/plugins/local-fixture

# Replay declared sidecar RPC calls, including nested host callbacks.
planeai-cli plugin test \
  --package src-tauri/plugins/local-fixture \
  --scenario src-tauri/plugins/local-fixture/scenarios/persist-settings.jsonl

# Verify host-owned data and secrets directories are supplied to the sidecar.
planeai-cli plugin test \
  --package src-tauri/plugins/local-fixture \
  --scenario src-tauri/plugins/local-fixture/scenarios/state-environment.jsonl

# Verify cooperative cancellation returns JSON-RPC error -32800.
planeai-cli plugin test \
  --package src-tauri/plugins/local-fixture \
  --scenario src-tauri/plugins/local-fixture/scenarios/cancellation.jsonl

# Call declared tools as an agent in a session would.
planeai-cli plugin test \
  --package src-tauri/plugins/local-fixture \
  --scenario src-tauri/plugins/local-fixture/scenarios/tool-call.jsonl

# Run one provider turn: send a prompt and require busy, an event, then idle.
planeai-cli plugin test \
  --package src-tauri/plugins/local-fixture \
  --provider-turn "hello"
```

The command validates every declared local backend path and the current-platform executable, creates temporary host-owned `PLANEAI_PLUGIN_DATA_DIR` and `PLANEAI_PLUGIN_SECRETS_DIR` directories, forwards only manifest-granted host capabilities during handshake, delivers a task lifecycle batch only when both sides opt in, rejects malformed or mismatched JSON-RPC output, and verifies clean shutdown. A scenario is newline-delimited JSON objects containing `method`, optional `params`, and optional `timeout_ms`. A positive `timeout_ms` (at most 5000) makes the harness send `$/cancelRequest` when the call remains pending and requires the original request to finish with error code `-32800`. A line `{ "tool": "echo", "arguments": { ... } }` instead sends `tool.call` for that declared tool, with a stand-in caller and the tool's timeout, and requires a `{ text, is_error? }` result, which it prints.
Use the checked-in scenarios as executable examples. For a plugin that declares providers, the harness sends `provider.sessions.reconcile` after the handshake and, for every provider, checks the session contract with the parameters PlaneAI sends: start, a repeated stop, an interrupt of the stopped session answering `-32010`, a resume of a session the plugin never saw, and a stop of an unknown session. It validates every provider notification (known session, `seq` in range and increasing, documented status). When a provider supports `handoff`, it also checks that a repeated `provider.session.handoff` returns a runnable argv and that a repeated `provider.session.handback` succeeds. `--provider-turn <text>` additionally sends one prompt and requires a `busy` then `idle` turn with at least one event; skip it for providers that call a real model. Browser UI lifecycle remains covered by your own DOM test using the documented `mount` context and disposer.

## v1 limitations and author checklist

- Plugins are trusted and unsandboxed; network, process, filesystem, and credential safety are the author's responsibility.
- Local plugin capabilities are limited to the explicit contracts above. `sessions.actions`, `sessions.advisories`, and `sessions.complete` are host-rendered requests, while `tasks.transition` is restricted to the linked task of an identified session and `tasks.sync` to the tasks the plugin created; no direct session control/output, arbitrary task update, storage bridge, or arbitrary sidebar-navigation capability is available to local packages.
- UI is a single self-contained ESM file loaded into a ShadowRoot. No relative imports, asset graph, global PlaneAI DOM access, or direct Tauri IPC.
- UI settings are public JSON objects; secrets are backend-only. Never log secrets, including to stderr.
- Stdout must remain newline-framed JSON-RPC. Correlate IDs, stay below 64 KiB, respond to shutdown, and treat host callbacks as nested RPC.
- Task events require both the manifest capability and handshake subscription; delivery is best-effort, so handlers must tolerate missed batches and reconcile with `tasks.read`.
- Test all declared platforms and binary executable bits before distributing a package. The fixture's `make local-plugin-fixture` target validates only the current platform.
