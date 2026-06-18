# Daemon PTY Core

## Daemon Backend Audit

### Where daemon sessions are created

`DaemonServer::handle_request(Request::Spawn)` in `planeai-daemon/src/server.rs`
→ calls `SessionRegistry::spawn()` in `registry.rs`
→ calls `DaemonSession::spawn()` in `session.rs`

### Where daemon PTYs are spawned today

`DaemonSession::spawn()` in `planeai-daemon/src/session.rs`:
- Uses `portable_pty::native_pty_system().openpty(PtySize { rows: 24, cols: 80 })`
- Builds command via `CommandBuilder::new(command)` with args/cwd/env
- Spawns via `pair.slave.spawn_command(cmd)`

### How daemon output is read

Std reader thread spawned in `DaemonSession::spawn()`:
- Reads 16KB chunks in a loop
- On data: writes to `Arc<Mutex<RingBuffer>>` + sends via `broadcast::Sender<Vec<u8>>`
- On EOF/error: sets `alive` AtomicBool to false

### How daemon output is framed/sent to clients

`data.rs::handle_data_connection()`:
1. Handshake: reads FRAME_OUTPUT frame containing session_id
2. Replay: sends buffer snapshot in 64KB chunks as FRAME_OUTPUT frames
3. Live: `forward_output()` receives from broadcast channel, writes FRAME_OUTPUT frames
4. Binary frame format: `[type:1][length:4 BE][payload:N]`

### How input bytes are written

`data.rs::forward_input()`:
- Reads FRAME_INPUT frames from data connection
- Calls `session.write(&payload)` which does `writer.write_all(data)`

### How resize is handled

`Request::Resize { session_id, cols, rows }` on control connection:
- Server calls `session.resize(cols, rows)`
- `DaemonSession::resize()` calls `master.resize(PtySize { rows, cols, ... })`

### How attach/reconnect works

- Client opens new data connection (CONN_DATA byte)
- Sends FRAME_OUTPUT frame with session_id as handshake
- Server replays buffer snapshot then streams live output
- Multiple concurrent data connections supported (broadcast channel)

### How session exit is handled

- Reader thread detects EOF → sets `alive = false`
- `DaemonServer::poll_exits()` runs every 500ms, calls `registry.remove_dead()`
- Dead sessions are removed from registry
- "exited" event broadcast to all control connections

### How backpressure is handled

- Broadcast channel capacity: 64 messages
- On lag: `RecvError::Lagged(n)` is logged but output continues
- No pause/resume mechanism in daemon today

### How multiple clients are handled

- Multiple data connections can attach to same session
- Each gets independent broadcast::Receiver
- Each gets buffer snapshot on attach

### Where session IDs are assigned

Client-assigned: UUID string passed in the `spawn` request from the desktop app.

### Where command/cwd/env are configured

Passed in the `Request::Spawn` JSON payload from the client:
```json
{ "cmd": "spawn", "session_id": "...", "command": "...", "args": [...], "cwd": "...", "env": {...} }
```

### Where logs are written

No durable output logs in the daemon today. Only `tracing` diagnostic logs.

---

## Daemon PTY Core Selection

### Environment variable

```
PLANEAI_DAEMON_PTY_CORE=legacy      # default — uses portable-pty directly
PLANEAI_DAEMON_PTY_CORE=planeai-pty # uses planeai-pty crate (opt-in)
```

### Config field

```json
{ "daemon_pty_core": "legacy" }
```

### Resolution order

1. `PLANEAI_DAEMON_PTY_CORE` env var (highest priority)
2. `daemon_pty_core` config field
3. Default: `"legacy"`

Invalid values log a warning and fall back to legacy.

Local PTY selection (`PLANEAI_LOCAL_PTY_CORE`) remains independent.
Tmux remains legacy always.

---

## Architecture

```
desktop client
  → existing daemon client/protocol (unchanged)
    → daemon process
      → DaemonPtySink (implements PtyEventSink)
        → planeai_pty::LocalPtySession
      → existing daemon output broadcast/frame path
      → optional durable .ansi log
    → frontend terminal (unchanged)
```

The frontend does not know whether the daemon uses legacy or planeai-pty internally.

---

## Durable Logs for Daemon Sessions

When `PLANEAI_SESSION_LOG_DIR` is set, daemon planeai-pty sessions write:

```
$PLANEAI_SESSION_LOG_DIR/sessions/<session-id>/<YYYYMMDDTHHMMSSZ>_output.ansi
$PLANEAI_SESSION_LOG_DIR/sessions/<session-id>/meta.json
```

Metadata includes `"session_source": "daemon"` and `"pty_core": "planeai-pty"`.

---

## How to Enable

```bash
PLANEAI_DAEMON_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-daemon-session-logs \
pnpm tauri dev
```

To roll back:
```bash
PLANEAI_DAEMON_PTY_CORE=legacy pnpm tauri dev
```

---

## Known Limitations

- Daemon planeai-pty is opt-in only (not default)
- Tmux remains legacy
- Attach/reconnect: buffer replay works; live reconnect uses same broadcast mechanism
- Durable logs may contain secrets (raw terminal output)
- Log viewer replay is read-only — does not restore a process
- xterm.js rendering bottleneck still exists in Tauri
- Iced UI daemon attach is not implemented yet
- Long multi-hour daemon sessions are not fully stress-tested
- No pause/resume backpressure from daemon to planeai-pty (backpressure is client-side only)

---

## Legacy vs planeai-pty Daemon Comparison

| Behavior | Legacy | planeai-pty |
|----------|--------|-------------|
| Session creation | ✅ works | ✅ works |
| Output streaming | ✅ via broadcast | ✅ via DaemonPtySink → broadcast |
| Input/write | ✅ | ✅ |
| Ctrl-C | ✅ | ✅ |
| Resize | ✅ | ✅ |
| Exit detection | ✅ alive flag | ✅ alive flag (via PtyEvent::Exit) |
| Buffer snapshot/replay | ✅ | ✅ |
| Multiple clients | ✅ broadcast | ✅ broadcast |
| Protocol | unchanged | unchanged |
| Frontend behavior | unchanged | unchanged |
| Durable logs | ❌ not available | ✅ when PLANEAI_SESSION_LOG_DIR set |
| Diagnostics | ❌ none | ✅ PipelineDiagnostics |
| Coalescing | ❌ raw 16KB reads | ✅ 4ms coalesce + threshold |
| Bytes dropped | 0 | 0 |

---

## Smoke Test Results

### Automated Tests

| Suite | Tests | Result |
|-------|-------|--------|
| `cargo test -p planeai-pty` | 7 | ✅ all pass |
| `cargo test -p planeai-daemon` | 41 | ✅ all pass |
| `cargo test -p planeai` | 223 | ✅ 222 pass, 1 pre-existing macOS failure |
| `cargo test -p planeai-iced-spike` | 14 | ✅ all pass |
| `npx svelte-check` | — | 1 pre-existing BenchmarkRunner error |
| `cargo build --release -p planeai` | — | ✅ builds |
| `cargo build --release -p planeai-iced-spike` | — | ✅ builds |

### Manual Smoke Test Commands

Legacy daemon:
```bash
PLANEAI_DAEMON_PTY_CORE=legacy pnpm tauri dev
```

PlaneAI PTY daemon:
```bash
PLANEAI_DAEMON_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-daemon-session-logs \
PLANEAI_DOGFOOD_LOG_VIEWER=1 \
pnpm tauri dev
```
