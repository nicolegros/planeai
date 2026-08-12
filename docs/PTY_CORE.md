# PTY Core Architecture

## Overview

PlaneAI uses a shared PTY crate (`planeai-pty`) for local terminal session management. The crate owns the low-level PTY lifecycle — spawn, read, write, resize, exit detection, and output coalescing — without depending on any frontend framework.

## What `planeai-pty` owns

- `LocalPtySession` — spawns a PTY child process, manages reader/flusher threads
- `PtyEvent` / `PtyEventSink` — push-based output delivery (Output, Exit, Error)
- `LocalPtyConfig` / `QueuePolicy` — session configuration (cols, rows, coalesce timing, buffer policy)
- `PipelineDiagnostics` — atomic counters for reader/flusher throughput monitoring
- `FlowControl` — pause/resume backpressure on the reader thread
- Coalescing flusher — batches small reads into larger output events (4ms coalesce, 50ms max idle)

The crate has **no** Tauri, Iced, alacritty, or xterm dependencies.

## What production `pty.rs` still owns

- `PtyManager` — session lifecycle, attach/detach, routing to backends
- `LocalBackend` — legacy local PTY implementation (duplicates planeai-pty's logic)
- `DaemonBackend` — connects to planeai-daemon for persistent sessions
- Tmux integration (`PtyTarget::TmuxAttach`)
- `SessionBackend` trait — unified write/resize/pause/resume/detach interface
- Benchmark capture (`PLANEAI_BENCH_CAPTURE`)
- Observer pattern for notification hooks

## How the Tauri adapter works

`src-tauri/src/pty_planeai_core_adapter.rs` provides:

1. **`TauriPtySink`** — implements `PtyEventSink` to forward events to the Tauri frontend:
   - `PtyEvent::Output` → `Channel<Response>::send(bytes)` (same path as legacy)
   - `PtyEvent::Exit` → `app.emit("pty-exited", { pty_key })` (same event as legacy)
   - `PtyEvent::Error` → `tracing::error!`

2. **`PlaneaiPtyBackend`** — implements `SessionBackend` wrapping `LocalPtySession`:
   - `write()` → `session.write()`
   - `resize()` → `session.resize()`
   - `pause()/resume()` → `session.pause()/resume()`
   - `detach()` → `session.kill()`

3. **`TeeSink`** — multiplexes events to primary (frontend) + secondary (log) sinks

4. **`TrackingLogSink`** — wraps `LogSink` with byte counting and metadata finalization:
   - Tracks `bytes_written` and `bytes_dropped` via atomic counters
   - On `PtyEvent::Exit`: finalizes `meta.json` with ended_at, exit_status, status, final byte counts
   - Write errors are logged but never crash the app

5. **`LogSink`** — writes raw output bytes to `.ansi` log file in append mode

The adapter is only used for `PtyTarget::Shell` (local tab sessions). Tmux and daemon targets are unaffected.

## How to select legacy vs planeai-pty

Set the `PLANEAI_LOCAL_PTY_CORE` environment variable:

```bash
# Legacy (default) — uses existing production pty.rs code path
PLANEAI_LOCAL_PTY_CORE=legacy make dev

# planeai-pty — uses the shared crate via the adapter
PLANEAI_LOCAL_PTY_CORE=planeai-pty make dev
```

The app logs which mode is active at startup:

```
local PTY core: legacy
```

## Whether planeai-pty is default

**No.** The default is `legacy`. Set `PLANEAI_LOCAL_PTY_CORE=planeai-pty` to opt in.

## Durable session logs

When `PLANEAI_SESSION_LOG_DIR` is set, sessions using the planeai-pty path write raw output to:

```
$PLANEAI_SESSION_LOG_DIR/sessions/<session-id>/<YYYYMMDDTHHMMSSZ>_output.ansi
```

A JSON metadata sidecar is written alongside:

```
$PLANEAI_SESSION_LOG_DIR/sessions/<session-id>/meta.json
```

Properties:

- Raw bytes only in `.ansi` (preserves ANSI escapes, cursor movement, colors)
- No JSON envelope in the .ansi file
- Directory per session — multiple runs create separate timestamped logs
- Append-mode, synchronous-buffered writes
- Write errors are logged but don't crash the app or block UI delivery
- Only active when `PLANEAI_LOCAL_PTY_CORE=planeai-pty`
- Parent directories are created automatically

Example:

```bash
PLANEAI_LOCAL_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-session-logs \
make dev
```

## Metadata sidecar schema

Each session log directory contains a `meta.json` with schema version 1:

```json
{
  "schema_version": 1,
  "session_id": "abc123-def456",
  "pty_core": "planeai-pty",
  "started_at": "2026-06-17T19:24:00+00:00",
  "ended_at": "2026-06-17T19:30:00+00:00",
  "command": "/bin/zsh",
  "cwd": "/Users/me/projects",
  "cols": 80,
  "rows": 24,
  "ansi_log_file": "20260617T192400Z_output.ansi",
  "bytes_written": 123456,
  "bytes_dropped": 0,
  "exit_status": 0,
  "status": "exited"
}
```

**Lifecycle:**

1. Written at session start with `status: "running"`, `bytes_written: 0`, `ended_at: null`
2. Updated on session exit with final values

## Log Catalog Backend

`src-tauri/src/session_logs.rs` provides Tauri commands for log discovery and replay:

| Command                                        | Description                                                 |
| ---------------------------------------------- | ----------------------------------------------------------- |
| `get_session_log_dir()`                        | Returns configured log directory path                       |
| `is_dogfood_log_viewer_enabled()`              | Returns true if `PLANEAI_DOGFOOD_LOG_VIEWER=1`              |
| `list_session_logs()`                          | Scans sessions dir, returns metadata for all saved sessions |
| `get_session_log_metadata(session_id)`         | Returns metadata for a specific session                     |
| `read_session_log_chunk(path, offset, length)` | Reads a chunk of a `.ansi` file (capped at 256 KiB)         |
| `open_session_log_folder(path)`                | Opens the log directory in the OS file manager              |

### Security

- All file operations validate the path is strictly under `PLANEAI_SESSION_LOG_DIR/sessions/`
- Uses `fs::canonicalize()` to resolve symlinks and prevent path traversal
- `read_session_log_chunk` caps at 256 KiB per request
- Corrupt/missing metadata is handled gracefully (returns empty list or error)

## Log replay

### In the Tauri app (dogfood log viewer)

Gated behind `PLANEAI_DOGFOOD_LOG_VIEWER=1`. Access via Command Palette → "Session log viewer".

- Streams `.ansi` file in 64 KiB chunks at 16ms intervals
- Uses xterm.js in read-only mode (disableStdin: true)
- Supports pause/resume/stop/restart
- Shows bytes replayed counter

### In the Iced spike

```bash
cargo run --release -p planeai-iced-spike -- \
  --replay /tmp/planeai-session-logs/sessions/<session-id>/<timestamp>_output.ansi \
  --cols 120 \
  --rows 40 \
  --chunk-size 16384 \
  --chunk-interval-ms 4 \
  --metrics bench/results/replay-dogfood.jsonl \
  --backend iced-alacritty \
  --exit-when-done
```

## Performance

Iced spike benchmark (headless, 25MB flood test):

| Metric          | spike-local (legacy) | planeai-local (planeai-pty) |
| --------------- | -------------------- | --------------------------- |
| Throughput      | ~21 MB/s             | ~23 MB/s                    |
| Bytes dropped   | 0                    | 0                           |
| p50 frame delta | 16.6 ms              | 16.6 ms                     |

Both paths are at parity. The planeai-pty path uses the same coalescing strategy (4ms coalesce, 50ms max idle, 16KB read buffer).

## What remains before daemon/tmux integration

1. **Make planeai-pty the default** — requires more production dogfooding
2. **Remove legacy `LocalBackend`** — once planeai-pty is proven stable in production
3. ~~**Daemon backend via planeai-pty**~~ — ✅ Done. See [DAEMON_PTY_CORE.md](./DAEMON_PTY_CORE.md)
4. **Tmux backend via planeai-pty** — if desired (currently tmux attach is simple enough)
5. **Background-threaded LogSink** — current implementation is synchronous-buffered; could move to a dedicated write thread if latency becomes an issue
6. **Log rotation/cleanup** — not yet implemented

## Lifecycle Semantics

### Session states

A `LocalPtySession` can be in one of these states:

| State       | Description                                                                              |
| ----------- | ---------------------------------------------------------------------------------------- |
| **Running** | Child process alive, reader/flusher threads active                                       |
| **Exited**  | Child exited (naturally or killed), reader thread saw EOF, flusher sent `PtyEvent::Exit` |

### Transition triggers

| Trigger                         | Behavior                                                                                                          |
| ------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Shell exits naturally           | Reader gets EOF → sets `done` flag → flusher drains pending → sends `PtyEvent::Exit` → metadata finalized         |
| Command exits naturally         | Same as shell exit                                                                                                |
| User closes terminal (UI)       | `PlaneaiPtyBackend::detach()` → calls `session.kill()` → kills child process → reader gets EOF → normal exit path |
| App window closes               | Session is dropped → `Drop` impl calls `kill()` → same as above                                                   |
| App process is killed (SIGKILL) | Child process receives SIGHUP (PTY master fd closed by OS). Metadata may not finalize.                            |
| Backend receives PTY EOF        | Reader thread breaks its loop → sets `done` → flusher detects `done` → sends Exit event                           |
| `kill()` called                 | Sends signal to child process via `portable_pty::Child::kill()`. PTY fd closure follows.                          |

### Child process cleanup

- **`kill()`**: Sends kill signal to the child. The child dies, PTY master fd is closed by OS cleanup.
- **`Drop`**: Always calls `kill()`. No orphaned child processes after session is dropped.
- **Forced app exit (SIGKILL)**: The OS closes the PTY master fd when the app process dies. The child receives SIGHUP and typically exits. This is OS-level cleanup, not application-level.

### Metadata finalization

- **Normal exit**: `TrackingLogSink` receives `PtyEvent::Exit` → writes final `meta.json` with `ended_at`, `exit_status`, `bytes_written`, `bytes_dropped`, `status: "exited"`.
- **`kill()` / session drop**: Same path — `PtyEvent::Exit` is sent by flusher after reader EOF.
- **App crash (SIGKILL)**: Metadata remains `status: "running"` with `ended_at: null`. The log viewer shows this as "running" (stale). No automatic recovery/correction is performed.

### Distinction between close / kill / detach

| Operation                      | Implementation                       | Effect                                                 |
| ------------------------------ | ------------------------------------ | ------------------------------------------------------ |
| **close** (UI "close session") | Calls `detach()` on `SessionBackend` | Calls `session.kill()` — terminates child              |
| **kill**                       | `LocalPtySession::kill()`            | Signals child process to die                           |
| **detach**                     | `PlaneaiPtyBackend::detach()`        | Equivalent to kill (no true detach for local sessions) |

**Important:** There is no "detach-and-keep-running" for planeai-pty local sessions. Detach is kill. This is intentional — persistent sessions use the daemon/tmux backends instead.

## Known limitations

- planeai-pty is only wired for `PtyTarget::Shell` (local tabs). Session attach (tmux/daemon) still uses legacy paths.
- The `session_id` in `LocalPtyConfig` is typed as `usize` while production uses `String`. The adapter passes `0` — routing is done at the `PtyManager` level.
- Durable logs are only available in planeai-pty mode, not legacy mode.
- Full Tauri+xterm live PTY benchmarks require manual GUI testing.
- Read-only replay does not restore a live process.
- xterm.js rendering bottleneck still exists for very large replays.
- Iced UI is still a prototype.
- Long multi-hour sessions are not fully stress-tested.
- Log cleanup/rotation is not implemented.
- Sensitive data may be stored in raw logs.
- The `delete_to_trash_removes_from_listing` test fails on macOS due to AppleScript permissions (unrelated to planeai-pty).
