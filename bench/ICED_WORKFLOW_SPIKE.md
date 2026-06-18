# Iced Workflow Shell

> **Status:** Prototype — ready for personal dogfooding, not production.

The workflow mode is the first PlaneAI-shaped Iced app: project-scoped agent sessions managed through the daemon backend.

## Quick Start

```bash
# Build all binaries
cd src-tauri
cargo build --release -p planeai-iced-spike
cargo build --release -p planeai --bin planeai-daemon

# Run workflow mode
PLANEAI_DAEMON_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-daemon-session-logs \
PATH="$(pwd)/target/release:$PATH" \
target/release/planeai-iced -- \
  --planeai-workflow \
  --cwd /path/to/your/project \
  --agent-command "kiro-cli chat" \
  --backend iced-alacritty
```

## CLI Flags

| Flag                | Description                              | Default            |
| ------------------- | ---------------------------------------- | ------------------ |
| `--planeai-workflow` | Enable workflow mode                     | off                |
| `--cwd <path>`       | Project working directory                | current dir        |
| `--agent-command`    | Command to launch agent sessions         | `kiro-cli chat`    |
| `--extra-path-dirs`  | Additional PATH dirs (repeatable)        | none               |
| `--backend`          | Renderer backend                         | `iced-alacritty`   |
| `--cols`             | Initial terminal columns                 | 120                |
| `--rows`             | Initial terminal rows                    | 40                 |

## Architecture

```
┌─────────────────────────────────────────────────┐
│ Iced Workflow Shell (planeai-iced --planeai-workflow) │
├────────────┬────────────────────────────────────┤
│ Session    │ Terminal Canvas                     │
│ Cards      │ (alacritty_terminal + iced canvas)  │
│            │                                    │
│ ● claude   │ $ kiro-cli chat                    │
│ ● kiro     │ > working on feature...            │
│            │                                    │
├────────────┴────────────────────────────────────┤
│ Status: ⚡ daemon | ~/my-project | kiro | 4.2KB  │
└─────────────────────────────────────────────────┘
         │
         │ IPC (Unix domain socket)
         ▼
┌─────────────────────────┐
│ planeai-daemon          │
│ (persistent, headless)  │
│                         │
│ Sessions survive app    │
│ close/restart           │
└─────────────────────────┘
```

## Keyboard Shortcuts

| Shortcut         | Action                              |
| ---------------- | ----------------------------------- |
| Cmd+N            | Launch new agent session            |
| Cmd+O            | Open project picker (path input)    |
| Cmd+R            | Refresh daemon session list         |
| Cmd+W            | Detach active session               |
| Cmd+Shift+W      | Kill active session                 |
| Cmd+A            | Attach to first unattached session  |
| Cmd+1..9         | Switch to session N                 |
| Cmd+V            | Paste                               |

## Session Lifecycle

1. **Launch** (Cmd+N): Spawns session via daemon with project cwd and augmented PATH
2. **Running**: Terminal attached, live I/O
3. **Detach** (Cmd+W): Session keeps running in daemon, removed from UI
4. **Reattach** (Cmd+A): Reconnect to detached session, scrollback replayed
5. **Kill** (Cmd+Shift+W): Terminates the session process
6. **Exited**: Session process ended, still visible in UI

Close the Iced window → sessions persist. Restart → existing sessions shown in left panel.

## PATH Handling

PATH is constructed by `planeai_core::command::augmented_path()`:

Priority (highest → lowest):
1. `PLANEAI_EXTRA_PATH` env var (colon-separated)
2. `--extra-path-dirs` CLI flag / config `extra_path_dirs`
3. Conventional developer dirs (`~/.local/bin`, `~/.cargo/bin`, `/opt/homebrew/bin`, etc.)
4. Inherited system PATH

**No setup-specific paths are hardcoded.** User-specific shims must come from config/env.

Config example (`~/.config/planeai/config.json`):
```json
{
  "extra_path_dirs": [
    "~/.local/bin",
    "~/.cargo/bin",
    "/opt/homebrew/bin"
  ]
}
```

## Durable Logs

When `PLANEAI_SESSION_LOG_DIR` is set:
- Raw `.ansi` output logged per session
- `meta.json` sidecar with command, cwd, timestamps, bytes_written, bytes_dropped
- Session cards show 📄 indicator when log exists
- bytes_dropped shown with ⚠ if nonzero

Replay existing logs:
```bash
cargo run --release -p planeai-iced-spike --bin planeai-iced -- \
  --replay /tmp/planeai-daemon-session-logs/sessions/<id>/<ts>_output.ansi \
  --cols 120 --rows 40 --backend iced-alacritty --exit-when-done
```

## Smoke Test

```bash
PLANEAI_DAEMON_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-workflow-smoke-logs \
PATH="$(pwd)/target/release:$PATH" \
target/release/planeai-workflow-smoke \
  --cwd /tmp/planeai-smoke-project \
  --agent-command "python3 -c 'import time; print(\"agent ready\", flush=True); time.sleep(30)'" \
  --metrics bench/results/workflow-smoke.jsonl
```

Verifies: daemon start, spawn with cwd, output, list, input, detach, reattach, kill, durable log, meta.json fields, bytes_dropped=0.

## Known Limitations

- Project picker is text-based (no native file dialog)
- No recent projects list (manual path entry only)
- No log replay within workflow mode (use `--replay` flag separately)
- No config file loading yet (CLI/env only)
- Daemon crash = sessions lost (no crash recovery)
- Scrollback limited to daemon 1MB ring buffer
- No multi-project support (one project per window)
- No task management integration
- No git worktree integration
- Session card UI is basic (no click actions, keyboard only)

## Relationship to Production Tauri App

- Production app (`pnpm tauri dev`) is unchanged
- Workflow mode is a separate Iced prototype binary
- They share: planeai-core, planeai-daemon, planeai-pty, daemon protocol
- Workflow mode does NOT replace the Tauri app
- tmux remains optional, not default

## Rollback

Stop using workflow mode. The daemon backend, protocol, and durable logs are shared infrastructure used by both apps. No migration needed.
