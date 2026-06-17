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

4. **`LogSink`** — writes raw output bytes to `.ansi` log file for durable session capture

The adapter is only used for `PtyTarget::Shell` (local tab sessions). Tmux and daemon targets are unaffected.

## How to select legacy vs planeai-pty

Set the `PLANEAI_LOCAL_PTY_CORE` environment variable:

```bash
# Legacy (default) — uses existing production pty.rs code path
PLANEAI_LOCAL_PTY_CORE=legacy pnpm tauri dev

# planeai-pty — uses the shared crate via the adapter
PLANEAI_LOCAL_PTY_CORE=planeai-pty pnpm tauri dev
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
$PLANEAI_SESSION_LOG_DIR/sessions/<session-id>/<timestamp>_output.ansi
```

Where `<timestamp>` is UTC compact ISO 8601 (e.g., `20260617T192400Z`).

A JSON metadata sidecar is written alongside:
```
$PLANEAI_SESSION_LOG_DIR/sessions/<session-id>/meta.json
```

Properties:
- Raw bytes only (preserves ANSI escapes, cursor movement, colors)
- No JSON envelope in the .ansi file
- Directory per session — multiple runs create separate timestamped logs
- Append-mode, synchronous-buffered writes (in the flusher thread)
- Write errors are logged but don't crash the app or block UI delivery
- Only active when `PLANEAI_LOCAL_PTY_CORE=planeai-pty`
- Parent directories are created automatically

Example:
```bash
PLANEAI_LOCAL_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-session-logs \
pnpm tauri dev
```

## Metadata sidecar

Each session log directory contains a `meta.json` with:

```json
{
  "session_id": "abc123-def456",
  "started_at": "2026-06-17T19:24:00+00:00",
  "pty_core": "planeai-pty",
  "command": "/bin/zsh",
  "cols": 80,
  "rows": 24,
  "log_file": "20260617T192400Z_output.ansi"
}
```

The sidecar is written once at session start. It is not updated during the session.

## Log replay

Durable `.ansi` logs can be replayed through the Iced spike for visual inspection or benchmarking:

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

This feeds the log file through the alacritty terminal emulator in the Iced window, simulating the original session output at the configured chunk rate.

To replay without metrics collection (just visual inspection):
```bash
cargo run --release -p planeai-iced-spike -- \
  --replay /tmp/planeai-session-logs/sessions/<session-id>/<timestamp>_output.ansi \
  --cols 120 --rows 40
```

## Performance

Iced spike benchmark (headless, 25MB flood test):

| Metric | spike-local (legacy) | planeai-local (planeai-pty) |
|--------|---------------------|-----------------------------|
| Throughput | ~21 MB/s | ~23 MB/s |
| Bytes dropped | 0 | 0 |
| p50 frame delta | 16.6 ms | 16.6 ms |

Both paths are at parity. The planeai-pty path uses the same coalescing strategy (4ms coalesce, 50ms max idle, 16KB read buffer).

## Manual GUI smoke checklist

Run in both modes and verify each item:

### Legacy mode
```bash
PLANEAI_LOCAL_PTY_CORE=legacy pnpm tauri dev
```

### PlaneAI PTY mode
```bash
PLANEAI_LOCAL_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-session-logs \
pnpm tauri dev
```

### Checklist

| # | Check | Legacy | PlaneAI PTY |
|---|-------|--------|-------------|
| 1 | App starts without crash | | |
| 2 | Startup log says which PTY core is active | | |
| 3 | Create local terminal session | | |
| 4 | Type `echo hello` — output appears | | |
| 5 | Paste multi-line command | | |
| 6 | Run noisy command (`find / -name '*.rs' 2>/dev/null`) | | |
| 7 | Ctrl-C interrupts running command | | |
| 8 | Resize terminal window | | |
| 9 | Close session cleanly | | |
| 10 | No backend panic in logs | | |
| 11 | No frontend error overlay | | |
| 12 | `.ansi` log file created (PTY mode only) | N/A | |
| 13 | `.ansi` log grows during output | N/A | |
| 14 | `meta.json` sidecar exists | N/A | |
| 15 | App can restart after using new path | | |

## Known test failures

| Test | Crate | Related to planeai-pty | Notes |
|------|-------|------------------------|-------|
| `file_explorer::tests::delete_to_trash_removes_from_listing` | `planeai` (main binary) | **No** | macOS sandbox/permissions issue |

**Details:**

```
thread 'file_explorer::tests::delete_to_trash_removes_from_listing' panicked at src/file_explorer.rs:225:68:
called `Result::unwrap()` on an `Err` value: "Error during a `trash` operation:
Os { code: 1, description: \"The AppleScript exited with error. stderr:
29:131: execution error: Not authorized to send Apple events to Finder. (-1743)\n\" }"
```

- **Root cause:** The `trash` crate uses AppleScript to move files to Trash on macOS, which requires Accessibility/Automation permissions that CI runners and some dev environments don't grant.
- **Affects main branch:** Yes — this is a pre-existing failure, not introduced by planeai-pty work.
- **Workaround:** Grant Terminal/IDE automation access to Finder in System Settings → Privacy & Security → Automation, or run with `--skip delete_to_trash`.

## What remains before daemon/tmux integration

1. **Make planeai-pty the default** — requires more production dogfooding
2. **Remove legacy `LocalBackend`** — once planeai-pty is proven stable in production
3. **Daemon backend via planeai-pty** — add a `DaemonPtySession` or equivalent
4. **Tmux backend via planeai-pty** — if desired (currently tmux attach is simple enough)
5. **Background-threaded LogSink** — current implementation is synchronous-buffered; could move to a dedicated write thread if latency becomes an issue

## Known limitations

- planeai-pty is only wired for `PtyTarget::Shell` (local tabs). Session attach (tmux/daemon) still uses legacy paths.
- The `session_id` in `LocalPtyConfig` is typed as `usize` while production uses `String`. The adapter currently passes `0` — this is fine because the session ID routing is done at the `PtyManager` level.
- Durable logs are only available in planeai-pty mode, not legacy mode.
- Full Tauri+xterm live PTY benchmarks require manual GUI testing.
- The Tauri benchmark harness (`run-tauri-matrix.sh`) tests replay mode only, not live PTY throughput.
- The `delete_to_trash_removes_from_listing` test fails on macOS due to AppleScript permissions (unrelated to planeai-pty).
