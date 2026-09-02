# Local plugin fixture

This is the reference package for PlaneAI's **trusted local plugin** contract. It deliberately demonstrates the whole v1 surface:

- `planeai-plugin.json` requests the local-only `settings`, `tasks.read`, and `task-events` capabilities.
- `bin/<platform>/planeai-plugin-fixture[.exe]` is a JSON-RPC/JSONL sidecar after `make local-plugin-fixture` builds it for the current platform.
- `ui/entry.js` is a self-contained browser ESM module. It uses `context.host.call("fixture.status")` and the public `context.host.settings.get()` / `.replace()` bridge, then returns a disposer that removes its click handler and Shadow DOM content.
- The Rust sidecar advertises `task.lifecycle`, logs startup/lifecycle/shutdown to stderr, implements `fixture.persistSettings` with correlated `host.settings.get` and `host.settings.replace` callbacks, implements `fixture.awaitCancellation` for cooperative `$/cancelRequest` handling, and implements `fixture.requireStateDirectories` to verify host-owned state paths.

## Build and install locally

From the repository root, build the current-platform binary and copy it into this package:

```bash
make local-plugin-fixture
```

In PlaneAI, open **Preferences → Plugins**, select **Install local package**, and choose this `src-tauri/plugins/local-fixture` directory. Enable it, then open **Local Fixture** from Cmd+K. The generated `bin/` directory is intentionally ignored because it is a platform-specific build artifact.

The manifest lists every supported platform, but the Make target materializes only the binary for the host platform. Release authors must place matching executable binaries at every declared `backend_entrypoints` path before distributing a cross-platform package.

## Quick manual checks

1. Enter a greeting and select **Save greeting**. Reload the plugin; the greeting remains because UI settings are stored under the plugin's host-owned data directory.
2. Inspect the plugin's `stderr.log` from the Plugins manager after enabling, reloading, delivering a lifecycle event, or disabling it. Sidecar diagnostics must use stderr, never stdout.
3. The sidecar accepts `fixture.status` and `fixture.persistSettings`. The latter is intentionally a sidecar-host-callback example; the UI uses the public settings bridge instead.
4. Run `planeai-cli plugin test --package src-tauri/plugins/local-fixture --scenario src-tauri/plugins/local-fixture/scenarios/state-environment.jsonl` to verify `PLANEAI_PLUGIN_DATA_DIR` and `PLANEAI_PLUGIN_SECRETS_DIR`, and run the corresponding `cancellation.jsonl` scenario to verify cooperative cancellation. The checked-in `persist-settings.jsonl` scenario exercises nested settings callbacks.

See the [Plugin author guide](../../../docs/src/content/docs/guides/plugin-ui-contributions.md) for the full package and protocol contract.
