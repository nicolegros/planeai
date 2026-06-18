# Iced Daemon Session Spike

Connects the Iced/alacritty native terminal UI to persistent daemon sessions via the existing daemon protocol. Supports full session lifecycle: spawn, list, attach, detach, kill, reconnect.

## Quick Start

```bash
# Build
cargo build --release -p planeai-iced-spike -p planeai-daemon

# Run with daemon sessions (planeai-pty backend)
PLANEAI_DAEMON_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-daemon-session-logs \
cargo run --release -p planeai-iced-spike --bin planeai-iced -- \
  --multi-session \
  --sessions 1 \
  --session-source planeai-daemon \
  --cols 120 --rows 40 \
  --backend iced-alacritty
```

## Architecture

```
┌─────────────────────────────────────────────────────┐
│ Iced App (planeai-iced)                             │
│  ┌───────────────────────────────────────────────┐  │
│  │ multi_session.rs                              │  │
│  │   - session list UI with status indicators    │  │
│  │   - daemon health indicator                   │  │
│  │   - attach/detach/kill lifecycle actions       │  │
│  │   - polls 16ms: try_read_batch() → vte → term│  │
│  └───────────────────────────────────────────────┘  │
│         ↕ PlaneAiTerminalSession trait              │
│  ┌───────────────────────────────────────────────┐  │
│  │ daemon_session.rs                             │  │
│  │   - spawn() / attach() / list() / kill()     │  │
│  │   - bounded buffer (512KB, backpressure)      │  │
│  │   - input_tx: mpsc channel → FRAME_INPUT      │  │
│  │   - resize_tx: mpsc channel → control conn    │  │
│  │   - detach_daemon_session() / daemon_is_connected() │
│  └───────────────────────────────────────────────┘  │
│         ↕ dedicated tokio runtime (2 threads)       │
└─────────────────────────────────────────────────────┘
         ↕ unix socket (planeai-ipc)
┌─────────────────────────────────────────────────────┐
│ planeai-daemon                                      │
│   - control: JSON-line (spawn/kill/resize/list/     │
│              attach/detach)                          │
│   - data: binary frames (FRAME_OUTPUT/FRAME_INPUT)  │
│   - PTY core: legacy or planeai-pty                 │
│   - scrollback buffer: 1MB                          │
│   - durable logs: $PLANEAI_SESSION_LOG_DIR          │
│   - sessions persist after client disconnect        │
└─────────────────────────────────────────────────────┘
```

## Daemon Session Lifecycle

### Definitions

| Action                | Behavior                                                                           |
| --------------------- | ---------------------------------------------------------------------------------- |
| **detach**            | UI stops displaying the session. Daemon keeps it running. User can reattach later. |
| **kill**              | UI sends explicit kill command. Daemon terminates the PTY process. Logs finalize.  |
| **close tab** (Cmd+W) | For daemon sessions: detach. Does NOT kill.                                        |
| **close window**      | Detaches all daemon sessions by default. Does NOT kill.                            |
| **exit-when-done**    | For benchmarks: may kill/cleanup after completion (--kill-sessions-on-exit).       |

### CLI Flags

| Flag                      | Default | Description                                             |
| ------------------------- | ------- | ------------------------------------------------------- |
| `--detach-on-close`       | true    | Detach daemon sessions when UI closes (don't kill)      |
| `--kill-on-close`         | false   | Kill daemon sessions when UI closes (benchmark cleanup) |
| `--kill-sessions-on-exit` | false   | Kill all sessions on exit (explicit opt-in)             |

### Close-Window Behavior

When the Iced window closes:

1. For daemon sessions with `--detach-on-close` (default): sends detach, drops data connection. Daemon keeps PTY alive.
2. For daemon sessions with `--kill-on-close`: sends kill command. Daemon terminates PTY.
3. For non-daemon sessions (spike-local, planeai-local): process terminates with the app (no persistence).

### Why Daemon Sessions Persist

The daemon's `remove_dead` only removes sessions whose PTY process has exited (`is_alive() == false`). Client disconnection does NOT mark sessions as dead. When the Iced data connection closes, the daemon simply stops forwarding output — the PTY keeps running.

## Keyboard Shortcuts

| Shortcut    | Action                                    |
| ----------- | ----------------------------------------- |
| Cmd+N       | Spawn new daemon session                  |
| Cmd+W       | Detach active daemon session (don't kill) |
| Cmd+Shift+W | Kill active daemon session                |
| Cmd+R       | Refresh daemon session list               |
| Cmd+A       | Attach to first unattached daemon session |
| Cmd+1..9    | Switch between attached sessions          |
| Cmd+Tab     | Cycle sessions                            |

## Session Status

| Status   | Icon | Meaning                                      |
| -------- | ---- | -------------------------------------------- |
| Running  | ●    | Session is alive and attached                |
| Attached | ◉    | Explicitly attached to existing session      |
| Exited   | ○    | PTY process terminated                       |
| Detached | ◌    | Session alive in daemon, not displayed in UI |

## Reconnect After Restart

1. Start Iced with `--session-source planeai-daemon`
2. Spawn a session (Cmd+N) or auto-spawn via `--sessions N`
3. Close the Iced window (daemon sessions persist)
4. Restart Iced (same command)
5. On boot, Iced connects to daemon and lists existing sessions
6. Unattached sessions shown in "detached" section of left panel
7. Press Cmd+A to attach, or Cmd+R to refresh list
8. Buffer snapshot replayed into terminal (scrollback history)
9. Live output continues from where it left off
10. Input/Ctrl-C/resize work normally

### How Attach Works

1. Iced sends `{"cmd": "attach", "session_id": "..."}` (informational)
2. Iced sends `{"cmd": "resize", ...}` to set terminal size
3. Iced opens a data connection (CONN_DATA byte)
4. Data connection handshake: sends session_id in FRAME_OUTPUT
5. Daemon replays buffer snapshot (up to 1MB scrollback) as FRAME_OUTPUT chunks
6. Daemon then live-streams new output as it arrives
7. Iced forwards input via FRAME_INPUT on same connection
8. No duplicate output: snapshot is buffer contents at attach time, live starts after

## Daemon Health

- Health checked every 5 seconds (non-blocking)
- Status shown in UI: "⚡ connected" or "⚠ disconnected"
- If daemon disconnects, UI continues without panic
- On reconnect, session list is refreshed automatically
- Metrics track daemon_connected/daemon_disconnected events

## Durable Logs

When `PLANEAI_SESSION_LOG_DIR` is set:

- Daemon writes raw `.ansi` output log per session
- `meta.json` tracks session_id, command, cwd, status, bytes
- On kill/exit: meta.json finalized with ended_at, exit_status
- session_source = "daemon" in metadata
- Logs remain compatible with Iced replay mode:

```bash
cargo run --release -p planeai-iced-spike --bin planeai-iced -- \
  --replay /tmp/planeai-daemon-session-logs/sessions/<session-id>/<timestamp>_output.ansi \
  --cols 120 --rows 40 --chunk-size 16384 --chunk-interval-ms 4 \
  --backend iced-alacritty --exit-when-done
```

- Log path shown in session list (when available)
- Logs may contain secrets (raw terminal output)

## Metrics

### Lifecycle Events

| event_type              | When                       |
| ----------------------- | -------------------------- |
| daemon_connected        | Daemon becomes reachable   |
| daemon_disconnected     | Daemon becomes unreachable |
| daemon_session_listed   | Session list refreshed     |
| daemon_session_attached | Session attached           |
| daemon_session_detached | Session detached           |
| daemon_session_killed   | Session killed             |

### Summary Fields

| Field             | Description                                  |
| ----------------- | -------------------------------------------- |
| sessions_listed   | Number of list operations                    |
| sessions_attached | Number of attach operations                  |
| sessions_detached | Number of detach operations                  |
| sessions_killed   | Number of kill operations                    |
| daemon_connected  | Whether daemon was connected at summary time |

## Session Sources

| Source         | Flag                              | Description                         |
| -------------- | --------------------------------- | ----------------------------------- |
| spike-local    | `--session-source spike-local`    | Legacy portable-pty (test fallback) |
| planeai-local  | `--session-source planeai-local`  | planeai-pty crate, in-process       |
| planeai-daemon | `--session-source planeai-daemon` | Daemon-backed persistent sessions   |

## Environment Variables

| Variable                  | Default         | Description                              |
| ------------------------- | --------------- | ---------------------------------------- |
| `PLANEAI_DAEMON_PTY_CORE` | (unset=legacy)  | Set to `planeai-pty` for new PTY backend |
| `PLANEAI_SESSION_LOG_DIR` | (unset=no logs) | Directory for durable session logs       |

## Benchmark Results (10s flood, 3 sessions)

| Metric          | planeai-local | planeai-daemon |
| --------------- | ------------- | -------------- |
| Throughput      | 20.17 MB/s    | 45.50 MB/s     |
| Total bytes     | 211 MB        | 477 MB         |
| p95 frame delta | 16.9 ms       | 18.0 ms        |
| p99 frame delta | 23.3 ms       | 22.0 ms        |
| p95 parse time  | 1.03 ms       | 2.32 ms        |
| p99 parse time  | 1.35 ms       | 3.34 ms        |
| p95 render work | 0.11 ms       | 0.12 ms        |
| Bytes dropped   | 0             | 0              |
| RSS             | 280 MB        | 277 MB         |

Daemon achieves 2.26x higher throughput due to efficient batching in the daemon's broadcast channel.

## Known Limitations

- **daemon planeai-pty remains opt-in** — requires `PLANEAI_DAEMON_PTY_CORE=planeai-pty`
- **tmux remains optional, not default** — daemon is the session backend for this spike
- **Iced UI is still prototype** — not polished, basic functionality only
- **Reconnect may show partial scrollback** — limited to daemon's 1MB ring buffer
- **Attach works for sessions created by this daemon instance** — sessions from a prior daemon process may not be available
- **Daemon crash recovery is limited** — if daemon process dies, all sessions are lost
- **Durable logs may contain secrets** — raw terminal output, no redaction
- **Log replay is read-only** — does not restore a live process
- **Production Tauri app still uses xterm.js** — this spike does not replace it
- **GUI benchmarks require a display** — cannot run headlessly in tmux

## Policy

- tmux remains an explicit optional session target (not used by this spike)
- tmux is NOT default
- tmux is NOT required
- daemon planeai-pty is opt-in via env var
- This milestone is about daemon-backed Iced sessions only

## Rollback

To revert to local-only mode, use `--session-source planeai-local`. No daemon required.

## Protocol

No protocol changes were made. Uses the existing daemon protocol:

- Control: `CONN_CONTROL` (0x00) + JSON-line requests
- Data: `CONN_DATA` (0x01) + binary frame handshake + bidirectional FRAME_OUTPUT/FRAME_INPUT
- Commands used: spawn, kill, resize, list, attach, detach

## Smoke Checklist

- [x] `--session-source planeai-daemon` spawns sessions
- [x] Output streams into terminal
- [x] Input/Ctrl-C works
- [x] Resize works
- [x] Exit/kill works
- [x] Durable logs appear in `$PLANEAI_SESSION_LOG_DIR/sessions/`
- [x] Replay mode still works
- [x] Local modes still work
- [x] Production `cargo build --release -p planeai` succeeds
- [x] Cmd+W detaches (doesn't kill) daemon sessions
- [x] Cmd+Shift+W kills daemon sessions
- [x] Cmd+R refreshes daemon session list
- [x] Cmd+A attaches to unattached session
- [x] Close window detaches by default
- [x] Daemon health indicator shows connected/disconnected
- [x] Session status shown in left panel
- [x] Unattached daemon sessions visible in left panel
- [ ] Manual: close window → restart → reattach (requires GUI)
- [ ] Manual: verify buffered output appears on attach (requires GUI)
- [ ] Manual: verify input works after attach (requires GUI)

## Dogfooding Checklist

Before daily driver use, verify manually with a display:

1. Start daemon-backed Iced app
2. Spawn a session (Cmd+N or `--sessions 1`)
3. Run `echo hello` — verify output
4. Run a long/noisy command — verify streaming
5. Detach (Cmd+W) — session disappears from active list
6. Close Iced window
7. Confirm daemon session still running: check daemon process, check log dir
8. Restart Iced with same flags
9. See "⚡ connected" status
10. See detached session in left panel
11. Cmd+A to attach
12. See buffered output (scrollback)
13. Type input — verify it works
14. Ctrl-C — verify it works
15. Resize window — verify terminal adjusts
16. Cmd+Shift+W to kill — verify session terminates
17. Check meta.json shows status=exited
18. Bytes dropped = 0
