# Daemon PTY Core

## Lifecycle Contract

The daemon is the source of truth for live PTY processes. The app (SQLite/Tauri state) is the source of truth for PlaneAI task/session metadata.

| Action                   | Effect                                                           |
| ------------------------ | ---------------------------------------------------------------- |
| Close app                | Detach from daemon sessions; they keep running                   |
| Kill                     | Terminate process tree via daemon control command                |
| Restart                  | Explicit kill/replace/reconnect; not accidental attach           |
| OS reboot / daemon crash | Live PTYs lost; metadata/logs preserved; provider resume offered |
| Spawn                    | Creates process only when spawn mode allows it                   |
| Attach                   | Never creates a process                                          |

## Session States

Daemon sessions have an explicit state machine:

```
Running  →  Exited { ended_at }
Running  →  Killed { ended_at }
```

- `Running`: PTY process is alive.
- `Exited`: PTY process exited naturally. Session remains in registry for replay/diagnostics.
- `Killed`: PTY was explicitly killed. Session remains in registry for replay/diagnostics.

Exited/Killed sessions are retained until garbage collection (default TTL: 30 minutes). The daemon's shutdown timer uses **live session count** (not total registry count) to decide whether to exit.

## Spawn Modes

The `spawn` control command accepts a `mode` field:

| Mode                       | ID missing | ID running     | ID exited/killed |
| -------------------------- | ---------- | -------------- | ---------------- |
| `create_only`              | Spawn      | Error          | Error            |
| `attach_if_running`        | Error      | AlreadyRunning | Error            |
| `replace_exited` (default) | Spawn      | Error          | Remove + Spawn   |
| `restart`                  | Spawn      | Kill + Spawn   | Remove + Spawn   |

Default mode is `replace_exited` for backward compatibility.

### Spawn Outcomes

- `spawned` — new process created (includes replace-exited case)
- `already_running` — existing live session returned (no spawn)
- `restarted` — old process killed, new process spawned

## Control Protocol

JSON-line protocol on control connections (type byte `0x00`).

### Commands

```json
{"cmd": "spawn", "session_id": "...", "command": "...", "args": [...], "cwd": "...", "env": {...}, "mode": "replace_exited"}
{"cmd": "kill", "session_id": "..."}
{"cmd": "resize", "session_id": "...", "cols": 120, "rows": 40}
{"cmd": "list"}
{"cmd": "attach", "session_id": "..."}
{"cmd": "detach", "session_id": "..."}
```

### List Response

```json
{
  "sessions": [
    {
      "session_id": "...",
      "alive": true,
      "status": "running",
      "started_at": "...",
      "ended_at": null
    }
  ]
}
```

### Events (broadcast to all control connections)

```json
{ "event": "exited", "session_id": "..." }
```

## Data Protocol

Binary framed protocol on data connections (type byte `0x01`).

### Frame Format

```
[type: 1 byte][length: 4 bytes big-endian][payload: N bytes]
```

### Frame Types

| Byte   | Name         | Direction     | Purpose                             |
| ------ | ------------ | ------------- | ----------------------------------- |
| `0x01` | FRAME_OUTPUT | daemon→client | Terminal output data                |
| `0x02` | FRAME_INPUT  | client→daemon | Terminal input data                 |
| `0x03` | FRAME_RESIZE | client→daemon | Resize (4 bytes: cols BE + rows BE) |
| `0x04` | FRAME_EOF    | daemon→client | Session exited                      |
| `0x05` | FRAME_ERROR  | daemon→client | Attach/protocol error               |
| `0x06` | FRAME_HELLO  | client→daemon | Protocol version (1 byte payload)   |
| `0x07` | FRAME_ATTACH | client→daemon | Session ID to attach                |
| `0x08` | FRAME_GAP    | daemon→client | Output was lost (broadcast lag)     |

### Attach Flow (new protocol)

1. Client sends `FRAME_HELLO` with protocol version byte
2. Client sends `FRAME_ATTACH` with session_id as payload
3. Daemon replays buffer snapshot as `FRAME_OUTPUT` chunks
4. Daemon streams live output as `FRAME_OUTPUT`
5. On session exit: daemon sends `FRAME_EOF`
6. On error: daemon sends `FRAME_ERROR`

### Legacy Attach (backward compatible)

1. Client sends `FRAME_OUTPUT` with session_id as payload (legacy handshake)
2. Daemon replays + streams as above

### Broadcast Lag

When a slow client causes broadcast lag:

- Daemon sends `FRAME_GAP` with JSON payload: `{"lagged": N}`
- Client should display a gap indicator or request reconnect

## Command Spawning (argv preservation)

The daemon preserves argument boundaries:

- If command is a shell wrapper (`/bin/sh -c "..."` or `cmd /C "..."`), uses shell-wrapped mode via `build_command()`
- Otherwise, uses direct argv mode via `build_command_argv(program, args)` — no shell interpretation

This ensures:

- Args with spaces are preserved
- Shell metacharacters are not interpreted unless explicitly shell-wrapped
- Windows paths with spaces work

## Environment / PATH Propagation

All session spawn paths (initial launch, restart, CLI, symphony auto-dispatch, shell tabs) use the same centralized PATH resolution:

```rust
planeai_core::command::augmented_path(&extra_path_dirs)
```

Priority (highest to lowest):

1. `PLANEAI_EXTRA_PATH` env var
2. Config file `extra_path_dirs` (tilde-expanded)
3. Conventional developer directories (`~/.cargo/bin`, `/opt/homebrew/bin`, etc.)
4. Inherited system PATH

Platform path separators are preserved (`:` on Unix, `;` on Windows).

## Daemon File Logging

The daemon writes structured logs on startup:

- Default location: `~/.planeai/logs/daemon.log`
- Override via: `PLANEAI_DAEMON_LOG_DIR` env var
- Rotation: daily
- Filter: `RUST_LOG` env var (default: `info`)

Logged events include:

- Session spawn (session_id, command, args, cwd, mode, outcome)
- Session state transitions (exit, kill)
- Data attach/detach
- IPC errors
- GC events

**Not logged**: secrets, full env maps, tokens, prompts, raw terminal output.

### Session-level Durable Logs

When `PLANEAI_SESSION_LOG_DIR` is set, each session writes:

```
$PLANEAI_SESSION_LOG_DIR/sessions/<session-id>/<timestamp>_output.ansi
$PLANEAI_SESSION_LOG_DIR/sessions/<session-id>/meta.json
```

## Provider Resume

Provider resume is a recovery path, not true live process persistence.

When restarting a daemon session:

1. If provider config has `resume_command` (interactive picker) → use that
2. Otherwise → fresh provider command

If resume fails, automatically falls back to fresh command.

## Frontend Restart / Reattach

When a session transitions from exited to active (via restart):

1. Session orchestrator calls `restart()` and defers terminal pool activation until the restart resolves
2. On success, orchestrator updates session status to "active" and activates the terminal pool
3. Terminal.svelte mounts and calls `pty.attach()` with flow control channel
4. Replays buffer + resumes live output

This sequencing prevents the terminal from attaching to a still-exited daemon session (which would immediately EOF and re-emit pty-exited). Restart failures surface via `showSnackbar()`; the terminal pool is still activated so the user sees the session state.

## Architecture

```
desktop client
  → DaemonClient (async, JSON-line control)
  → DataConnection (binary frames for PTY I/O)
    → daemon process
      → SessionRegistry (HashMap<String, RegistryEntry>)
        → DaemonSession
          → DaemonPtySink (PtyEventSink)
            → planeai_pty::LocalPtySession (portable-pty)
          → RingBuffer (scrollback)
          → broadcast::Sender (live output fan-out)
      → poll_exits() loop (500ms, transitions Running→Exited)
      → gc() loop (60s, removes expired sessions)
      → shutdown_timer (30s grace, exits when no clients + no live sessions)
```

## Platform Notes

### Connection Error Recovery

If a daemon spawn fails with "Broken pipe", "Connection refused", or "No such file or directory", the Tauri GUI clears the cached `DaemonClient` connection. The next session launch attempt will reconnect to the daemon (which `ensure_daemon_running` restarts automatically if crashed). The user sees a one-time error prompting them to retry.

### macOS / Linux

- Unix domain socket at `$XDG_RUNTIME_DIR/planeai/daemon.sock` or `/tmp/planeai-<uid>/daemon.sock`
- Shell default: zsh (macOS) or sh (Linux)
- Detached daemon via `process_group(0)`

### Windows

- Named pipe at `\\.\pipe\planeai-daemon`
- ConPTY via portable-pty
- Shell default: `%COMSPEC%` (cmd.exe)
- Detached daemon via `CREATE_NO_WINDOW | DETACHED_PROCESS`
- Retry-on-busy for named pipe connections
