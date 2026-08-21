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

`planeai-plugin.json` is strict: unknown fields are rejected. `id` may contain only lowercase ASCII letters, digits, and hyphens. The schema and host version must be exactly `planeai.plugin.v1` and `planeai.plugin-host.v1`; local packages must use `source_kind: "local"`, `backend_entrypoints`, and `ui_contributions` (the legacy `ui_entrypoint` is rejected).

Every backend and UI path must be a package-relative file path: no absolute paths and no `..`. The active platform's backend must exist and be executable. On Unix, its executable mode is preserved in the imported copy.

```json title="planeai-plugin.json" showLineNumbers
{
  "schema": "planeai.plugin.v1",
  "id": "local-fixture",
  "name": "Local Fixture",
  "version": "0.1.0",
  "host_api_version": "planeai.plugin-host.v1",
  "source_kind": "local",
  "backend_entrypoints": {
    "macos-arm64": "bin/planeai-plugin-fixture",
    "macos-x64": "bin/planeai-plugin-fixture",
    "linux-x64": "bin/planeai-plugin-fixture",
    "linux-arm64": "bin/planeai-plugin-fixture",
    "windows-x64": "bin/planeai-plugin-fixture.exe",
    "windows-arm64": "bin/planeai-plugin-fixture.exe"
  },
  "capabilities": ["settings", "tasks.read", "task-events"],
  "ui_contributions": [
    {
      "id": "fixture",
      "label": "Fixture",
      "placement": "main-pane",
      "entrypoint": "ui/entry.js"
    }
  ]
}
```

The platform key is the current OS and architecture: `macos-arm64`, `macos-x64`, `linux-arm64`, `linux-x64`, `windows-arm64`, or `windows-x64`. Ship each declared binary at its declared path; a package may declare only the platforms it actually supports, but it cannot install on a platform without a matching binary. Windows backend filenames conventionally end in `.exe`.

### Capabilities

Capabilities are an explicit contract for host callbacks. Local plugins may request only `settings`, `tasks.read`, and `task-events`; duplicates and all other local capabilities are rejected.

- `settings` permits sidecar callbacks `host.settings.get` and `host.settings.replace`.
- `tasks.read` permits `host.tasks.read` and `host.task.get` (each requires `{ "key": "TASK-123" }`).
- `task-events` permits event delivery only when the handshake also subscribes to `task.lifecycle`.

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

Response IDs must exactly correlate with the request ID. Return either `result` or a JSON-RPC `error`, not both. PlaneAI sends the reserved `plugin.shutdown` during disable/reload/app shutdown; acknowledge it and exit promptly. Do not expose or invoke `plugin.handshake` or `plugin.shutdown` from plugin UI.

A sidecar may make a host callback while PlaneAI is waiting for its response. Send a normal JSON-RPC request on stdout, wait for the correlated host response on stdin, then finish the original request. `fixture.persistSettings` demonstrates `host.settings.get` followed by `host.settings.replace`. Keep callback IDs distinct from the active request ID and continue reading until the matching response arrives.

Handshake and ordinary RPC calls have a five-second deadline. On deadline expiry, PlaneAI sends the JSON-RPC `$/cancelRequest` notification with `{ "id": <original request ID> }`. Keep reading stdin while work is active; cancel cooperatively and return the original request's JSON-RPC error `{ "code": -32800, "message": "request cancelled" }` within three seconds. PlaneAI treats any other response or no cancellation response as a failed runtime and stops it. `plugin.shutdown` also has a three-second grace period before the process is killed. Do not define a competing cancellation wire format in v1.

The host supplies `PLANEAI_PLUGIN_DATA_DIR` and `PLANEAI_PLUGIN_SECRETS_DIR`. Store public, replaceable plugin state in the former. Keep secrets backend-only in the latter: the UI settings bridge and sidecar settings callback never return secret files. The fixture's `fixture.status` reports both paths only to demonstrate their presence; real plugins should not surface secret paths or contents to UI.

Task lifecycle delivery is best-effort and isolated from PlaneAI task commits. Subscribe in the handshake, declare `task-events`, make handlers idempotent, and log failures to stderr. The fixture handles `plugin.taskLifecycle` and emits a lifecycle diagnostic.

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

`root` is a host-owned open `ShadowRoot`. Do not access PlaneAI's DOM, Tailwind classes, Tauri APIs, or `invoke()` directly. The bundle must contain all code it needs: v1 loads exactly the entrypoint source, not relative imports or package asset graphs. Use inherited custom properties such as `--color-t1`, `--color-panel`, `--color-border`, and `--font-sans` rather than hardcoded app colors.

`context.host` provides:

- `call(method, params?)` — RPC scoped to the owning sidecar. Lifecycle methods are reserved.
- `settings.get<T extends Record<string, unknown>>()` and `settings.replace<T>(settings)` — typed public JSON-object settings. These are the UI counterpart to the capability-gated sidecar settings callbacks; they never expose secrets.
- `data.changed()` — tells PlaneAI the plugin's data changed. A running `sidebar.section` remounts after this event.
- `navigation.open(pluginId, contributionId)`, `navigation.close()`, and `navigation.openPreferences()`.
- `sidebar.register(rows)`, `sidebar.select(rowId)`, and `sidebar.handleKeydown(event)` for sidebar navigation contributions. Always call the returned unregister function from your mount disposer.

The fixture UI calls `context.host.call("fixture.status")`, loads a saved greeting with `context.host.settings.get()`, replaces it when **Save greeting** is selected, calls `data.changed()`, and removes its click handler in its disposer.

Each `ui_contributions` item requires a unique safe `id`, `label`, `placement`, and `entrypoint`. Supported placements are `sidebar.header`, `sidebar.navigation`, `sidebar.section`, `sidebar.footer`, `preferences`, and `main-pane`. Sidebar contributions may set integer `order`. A `main-pane` contribution may specify an optional portable `Mod+…` shortcut and is discoverable in Cmd+K while running. Use the placement's available space conservatively; the host owns focus, navigation, keyboard routing, lifecycle, loading/retry UI, and teardown.

## Install, use, reload, and remove

1. Build and stage the current-platform fixture binary from the repository root:

   ```bash
   make local-plugin-fixture
   ```

2. In PlaneAI, open **Preferences → Plugins** and choose **Install local package**. Select the package directory, not an individual binary or manifest.
3. Enable the imported plugin. Open its `main-pane` from Cmd+K or interact with its declared sidebar/preferences placement. The fixture appears as **Local Fixture**.
4. Use **Reload** after a runtime error or to restart an already imported package. Reload restarts the imported immutable copy; it does not reread your source directory.
5. To publish a changed manifest, binary, or UI, rebuild, then **remove** the installed package before installing the changed one. A duplicate plugin ID is rejected; removal deletes host-owned settings, secrets, logs, and data. Imported content is copied under a SHA-256 directory, so edits to the original directory never affect an installed version.
6. Disable a plugin to stop its sidecar. **Remove plugin** is available only for local packages and deletes PlaneAI's imported package and host-owned plugin state. It does not delete your original source directory.

## Headless contract test

After staging the current-platform executable, run the shipped harness without launching the desktop UI:

```bash
planeai-cli plugin test --package src-tauri/plugins/local-fixture
```

The command validates the local manifest and executable, drives handshake, handles nested settings callbacks, delivers a task lifecycle batch when subscribed, rejects malformed or mismatched JSON-RPC output, and verifies clean shutdown. Use it as the baseline test before manually installing a package; browser UI lifecycle remains covered by your own DOM test using the documented `mount` context and disposer.

## v1 limitations and author checklist

- Plugins are trusted and unsandboxed; network, process, filesystem, and credential safety are the author's responsibility.
- Local plugin capabilities are limited to `settings`, `tasks.read`, and `task-events`; no task mutation, arbitrary storage bridge, or arbitrary sidebar-navigation capability is available to local packages.
- UI is a single self-contained ESM file loaded into a ShadowRoot. No relative imports, asset graph, global PlaneAI DOM access, or direct Tauri IPC.
- UI settings are public JSON objects; secrets are backend-only. Never log secrets, including to stderr.
- Stdout must remain newline-framed JSON-RPC. Correlate IDs, stay below 64 KiB, respond to shutdown, and treat host callbacks as nested RPC.
- Task events require both the manifest capability and handshake subscription; delivery is best-effort, so handlers must tolerate retries and errors.
- Test all declared platforms and binary executable bits before distributing a package. The fixture's `make local-plugin-fixture` target validates only the current platform.
