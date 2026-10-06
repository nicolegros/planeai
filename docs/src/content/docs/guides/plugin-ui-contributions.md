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

`planeai-plugin.json` is strict: unknown fields are rejected. `id` may contain only lowercase ASCII letters, digits, and hyphens. The schema must be `planeai.plugin.v1`. Use `planeai.plugin-host.v1` for legacy host APIs, `planeai.plugin-host.v2` for plugins that require the dynamically focused recipient API, or the unstable `planeai.plugin-host.v3` for plugins that declare [providers](#providers); local packages must use `source_kind: "local"`, `backend_entrypoints`, and `ui_contributions` (the legacy `ui_entrypoint` is rejected).

Every backend and UI path must be a package-relative file path: no absolute paths and no `..`. The active platform's backend must exist and be executable. On Unix, its executable mode is preserved in the imported copy.

```json title="planeai-plugin.json" showLineNumbers
{
  "schema": "planeai.plugin.v1",
  "id": "local-fixture",
  "name": "Local Fixture",
  "version": "0.1.0",
  "host_api_version": "planeai.plugin-host.v3",
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
    "providers"
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
  ]
}
```

The platform key is the current OS and architecture: `macos-arm64`, `macos-x64`, `linux-arm64`, `linux-x64`, `windows-arm64`, or `windows-x64`. Ship each declared binary at its declared path; a package may declare only the platforms it actually supports, but it cannot install on a platform without a matching binary. Windows backend filenames conventionally end in `.exe`.

### Capabilities

Capabilities are an explicit contract for PlaneAI data RPC. Local plugins may request `settings`, `projects.read`, `sessions.read`, `sessions.repository-context`, `sessions.prompt`, `session-events`, `sessions.actions`, `sessions.advisories`, `sessions.complete`, `tasks.read`, `tasks.create`, `tasks.transition`, `task-events`, and `providers`; duplicates and all other local capabilities are rejected.

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
  It requires `projectPath`, `title`, and a plugin-scoped `operationId`, accepts an optional `description`, and rejects `parentKey`.
  PlaneAI rejects hidden or unknown projects, returns the originally created task when the same operation is retried, and refreshes the task list.
  `host.tasks.createChild` requires `projectPath`, `parentKey`, `title`, `description`, and a plugin-scoped `operationId`; PlaneAI verifies the parent belongs to the project and returns the originally created child when the same operation is retried.
  Both return `{ "task": ... }`.
- `tasks.transition` permits `host.sessions.transitionLinkedTask` with `{ "session_id", "status" }`. PlaneAI resolves the task strictly from that session's linked task key, changes its lifecycle status, and emits the normal task lifecycle batch (including automatic parent completion).
- `task-events` permits event delivery only when the handshake also subscribes to `task.lifecycle`.
- `providers` permits declaring `providers` (host API v3 only) and the provider session contract described in [Providers](#providers). It is required exactly when `providers` is declared.
- `session-events` permits best-effort `plugin.sessionLifecycle` delivery only when the handshake also subscribes to `session.lifecycle`. Events describe committed host session status changes and include the session/project identity, branch, linked task key, previous status, and new status.

The sandbox UI can use the same read/create operations directly through `context.host.rpc.call("projects.list")`, `context.host.rpc.call("sessions.list")`, `context.host.rpc.call("task.get", { key })`, `context.host.rpc.call("tasks.create", params)`, and `context.host.rpc.call("tasks.createChild", params)`. The sidecar uses matching nested callbacks with the `host.` prefix. PlaneAI derives the owning plugin identity for both transports and applies identical manifest capability checks.

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

A sidecar may make a host callback while PlaneAI is waiting for its response. Send a normal JSON-RPC request on stdout, wait for the correlated host response on stdin, then finish the original request.
A failed callback answers with a JSON-RPC error code: `-32601` for an unknown host method, `-32003` for a capability the plugin was not granted, `-32602` for invalid params, and `-32603` for anything else. `fixture.persistSettings` demonstrates `host.settings.get` followed by `host.settings.replace`. Keep callback IDs distinct from the active request ID and continue reading until the matching response arrives.

Handshake and ordinary RPC calls have a five-second deadline; provider methods have [their own](#providers). On deadline expiry, PlaneAI sends the JSON-RPC `$/cancelRequest` notification with `{ "id": <original request ID> }`. Keep reading stdin while work is active; cancel cooperatively and return the original request's JSON-RPC error `{ "code": -32800, "message": "request cancelled" }` within three seconds. PlaneAI treats any other response or no cancellation response as a failed runtime and stops it. `plugin.shutdown` also has a three-second grace period before the process is killed. Do not define a competing cancellation wire format in v1.

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

| Method                        | Params                                                                               | Meaning                                                                                                                                 |
| ----------------------------- | ------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| `provider.session.start`      | `session_id`, `provider_id`, `cwd`, `env`, `auto_approve`, optional `initial_prompt` | A new session was created. Run the agent in `cwd` with `env`.                                                                           |
| `provider.session.resume`     | `session_id`, `provider_id`, `cwd`, `env`, `auto_approve`, `handed_off`              | The current sidecar does not drive this existing session yet, after an app restart or plugin reload. Restore it; do not replay prompts. |
| `provider.session.send`       | `session_id`, `text`                                                                 | Input from the chat UI, the CLI, recipes and loops, or another plugin's `sessions.prompt`.                                              |
| `provider.session.interrupt`  | `session_id`                                                                         | Stop the current turn.                                                                                                                  |
| `provider.session.stop`       | `session_id`, `reason` (`archive`, `destroy` or `exit`)                              | The session ended. Release its process; on `destroy`, delete its data.                                                                  |
| `provider.session.handoff`    | `session_id`                                                                         | With `handoff` only. Stop driving the session and return `{ "argv": [...] }`, the command that continues it in a terminal.              |
| `provider.session.handback`   | `session_id`                                                                         | With `handoff` only. The terminal tab closed, or the handoff answer had no runnable `argv`; drive the session again.                    |
| `provider.sessions.reconcile` | `sessions`: `[{ "session_id", "provider_id", "status" }]`                            | Sent once after the handshake. The plugin's sessions that still exist; see [Lifecycle](#lifecycle).                                     |

`session_id` is a lowercase UUID.
`cwd` is the session's worktree, or its project's root for a session without one.
`env` is an overlay on the sidecar's own environment: `PLANEAI_SESSION_ID`, `PLANEAI_SOCKET` and the augmented `PATH`, so the agent can still spawn sub-sessions and manage tasks.

Start, resume, handoff and handback have a 30-second deadline, since they may launch the agent; every other provider method has five seconds.
On expiry PlaneAI cancels the request as described above.

PlaneAI rejects a `send` text or `initial_prompt` over 48 KiB, measured as JSON-escaped text, so every request fits one 64 KiB frame.
Prompts from the CLI, loops and recipes are checked against the same limit before they are queued, and a prompt PlaneAI cannot deliver to the plugin is reported in the app.
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

Use only these stable semantic custom properties, never PlaneAI's internal `--color-*` or `--font-*` variables: `--planeai-font-sans`, `--planeai-font-mono`, `--planeai-canvas`, `--planeai-main`, `--planeai-surface`, `--planeai-surface-raised`, `--planeai-text`, `--planeai-text-muted`, `--planeai-text-subtle`, `--planeai-border`, `--planeai-border-strong`, `--planeai-accent`, `--planeai-on-accent`, `--planeai-accent-subtle`, `--planeai-success`, `--planeai-warning`, `--planeai-danger`, `--planeai-radius`, and `--planeai-space-1` through `--planeai-space-6`. PlaneAI updates those properties in place whenever its active theme or appearance changes; it does not remount your UI.

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
- `data.changed()` — tells PlaneAI the plugin's data changed. A running `sidebar.section` remounts after this event.
- `navigation.open(pluginId, contributionId)`, `navigation.close()`, and `navigation.openPreferences()`.
- `sidebar.register(rows)`, `sidebar.select(rowId)`, and `sidebar.handleKeydown(event)` for sidebar navigation contributions. Always call the returned unregister function from your mount disposer.

The fixture UI calls `context.host.call("fixture.status")`, loads a saved greeting with `context.host.settings.get()`, replaces it when **Save greeting** is selected, calls `data.changed()`, and removes its click handler in its disposer.

Each `ui_contributions` item requires a unique safe `id`, `label`, `placement`, and `entrypoint`. Supported placements are `sidebar.header`, `sidebar.navigation`, `sidebar.section`, `sidebar.footer`, `preferences`, `main-pane`, `session.panel`, and `titlebar`. Sidebar contributions may set integer `order`. `main-pane` and `session.panel` contributions may declare an optional portable `Mod+[Shift+][Alt+]A-Z` shortcut and are discoverable in Cmd+K while running. PlaneAI rejects duplicate declared shortcuts across installed plugins. When a matching selected-session panel is available, it receives its declared chord before a main-pane contribution or a legacy host fallback; otherwise the host's normal shortcut behavior continues. A `session.panel` contribution is also discoverable as an action at the end of the titlebar whenever a session is selected; PlaneAI supplies its UI entrypoint the selected session's identity, project ID, branch, base branch, status, provider, and linked task key. A compact `titlebar` contribution is rendered independently at the end of the titlebar with the same selected-session context. It can call `navigation.open(pluginId, contributionId)` to open a declared `session.panel` in a generic modal, rather than replacing the main workspace. During the GitHub migration, its plugin-owned pull-request chip deliberately appears beside the retained legacy PR-status chip or **Create PR** affordance; the legacy UI and routes remain available as a fallback when the plugin is unavailable. UI context does not supply a working-tree path—request the separately capability-gated `host.sessions.repositoryContext` operation when needed. Use the placement's available space conservatively; the host owns focus, navigation, keyboard routing, lifecycle, loading/retry UI, and teardown.
Inside a local plugin frame, PlaneAI forwards Cmd/Ctrl chords to its own shortcut router, so app shortcuts such as Mod+[ keep working while the frame has focus.
Call `preventDefault()` on a chord to keep it for your UI; Ctrl+Tab and Mod+N always reach PlaneAI.
A provider session's UI owns the keyboard like a terminal: PlaneAI takes focus out of its frame when the sidebar or a dialog takes over, and gives it back when its pane does.

## Install, use, reload, and remove

1. Build and stage the current-platform fixture binary from the repository root:

   ```bash
   make local-plugin-fixture
   ```

2. To find community candidates, open **Preferences → Plugins** and choose **Discover plugins**. PlaneAI asks your existing `gh` CLI to search public, non-archived, non-fork GitHub repositories tagged with the fixed `planeai` topic, returning up to 100 results ordered by stars. Results exist only for the current app session and are refreshed only when you choose discovery again. These results are unverified repository links—not packages, compatibility claims, downloads, or install actions. Review and obtain source from an author you trust before importing it.
3. Choose **Install local package** and select the reviewed package directory, not an individual binary or manifest.
4. Enable the imported plugin. Open its `main-pane` from Cmd+K or interact with its declared sidebar/preferences placement. The fixture appears as **Local Fixture**.
5. Use **Reload** after a runtime error or to restart an already imported package. Reload restarts the imported immutable copy; it does not reread your source directory.
6. To publish a changed manifest, binary, or UI, rebuild and choose **Install local package** again. Selecting a package with the same installed local-plugin ID replaces its immutable package copy, preserves host-owned settings, secrets, logs, and data, and restarts the plugin if it was enabled. Imported content is copied under a SHA-256 directory, so edits to the original directory never affect an installed version.
7. Disable a plugin to stop its sidecar. **Remove plugin** is available only for local packages and deletes PlaneAI's imported package and host-owned plugin state. It does not delete your original source directory.

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

# Run one provider turn: send a prompt and require busy, an event, then idle.
planeai-cli plugin test \
  --package src-tauri/plugins/local-fixture \
  --provider-turn "hello"
```

The command validates every declared local backend path and the current-platform executable, creates temporary host-owned `PLANEAI_PLUGIN_DATA_DIR` and `PLANEAI_PLUGIN_SECRETS_DIR` directories, forwards only manifest-granted host capabilities during handshake, delivers a task lifecycle batch only when both sides opt in, rejects malformed or mismatched JSON-RPC output, and verifies clean shutdown. A scenario is newline-delimited JSON objects containing `method`, optional `params`, and optional `timeout_ms`. A positive `timeout_ms` (at most 5000) makes the harness send `$/cancelRequest` when the call remains pending and requires the original request to finish with error code `-32800`. Use the checked-in scenarios as executable examples. For a plugin that declares providers, the harness sends `provider.sessions.reconcile` after the handshake and, for every provider, checks the session contract with the parameters PlaneAI sends: start, a repeated stop, an interrupt of the stopped session answering `-32010`, a resume of a session the plugin never saw, and a stop of an unknown session. It validates every provider notification (known session, `seq` in range and increasing, documented status). When a provider supports `handoff`, it also checks that a repeated `provider.session.handoff` returns a runnable argv and that a repeated `provider.session.handback` succeeds. `--provider-turn <text>` additionally sends one prompt and requires a `busy` then `idle` turn with at least one event; skip it for providers that call a real model. Browser UI lifecycle remains covered by your own DOM test using the documented `mount` context and disposer.

## v1 limitations and author checklist

- Plugins are trusted and unsandboxed; network, process, filesystem, and credential safety are the author's responsibility.
- Local plugin capabilities are limited to the explicit contracts above. `sessions.actions`, `sessions.advisories`, and `sessions.complete` are host-rendered requests, while `tasks.transition` is restricted to the linked task of an identified session; no direct session control/output, arbitrary task update, storage bridge, or arbitrary sidebar-navigation capability is available to local packages.
- UI is a single self-contained ESM file loaded into a ShadowRoot. No relative imports, asset graph, global PlaneAI DOM access, or direct Tauri IPC.
- UI settings are public JSON objects; secrets are backend-only. Never log secrets, including to stderr.
- Stdout must remain newline-framed JSON-RPC. Correlate IDs, stay below 64 KiB, respond to shutdown, and treat host callbacks as nested RPC.
- Task events require both the manifest capability and handshake subscription; delivery is best-effort, so handlers must tolerate missed batches and reconcile with `tasks.read`.
- Test all declared platforms and binary executable bits before distributing a package. The fixture's `make local-plugin-fixture` target validates only the current platform.
