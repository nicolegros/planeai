# Multi-Session Iced Terminal Spike

## Overview

This spike answers: **Can the native Iced/alacritty terminal architecture support multiple active sessions while keeping switching, memory, rendering, and input latency acceptable?**

**Backend source:** `spike-local` (fallback — see Backend Integration section below)

## Quick Start

### Multi-session shell mode (interactive)

```bash
cargo run --release --bin planeai-iced-spike -- \
  --multi-session \
  --sessions 3 \
  --cols 120 \
  --rows 40 \
  --metrics bench/results/multi-shell.jsonl \
  --backend iced-alacritty
```

### Multi-session flood mode (benchmark)

```bash
cargo run --release --bin planeai-iced-spike -- \
  --multi-session \
  --sessions 3 \
  --session-command "python3 bench/flood-output.py" \
  --cols 120 \
  --rows 40 \
  --max-runtime-ms 10000 \
  --metrics bench/results/multi-flood.jsonl \
  --backend iced-alacritty \
  --exit-when-done
```

### Memory scaling (1/3/5 sessions)

```bash
for n in 1 3 5; do
  cargo run --release --bin planeai-iced-spike -- \
    --multi-session \
    --sessions $n \
    --session-command "python3 bench/flood-output.py" \
    --max-runtime-ms 10000 \
    --metrics bench/results/multi-$n.jsonl \
    --backend iced-alacritty \
    --exit-when-done
done
```

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Cmd+1..9 | Switch to session N |
| Cmd+Tab | Next session |
| Cmd+Shift+Tab | Previous session |
| Cmd+N | New session |
| Cmd+W | Close active session |
| Cmd+V | Paste to active session |
| Ctrl+C | Sent to terminal (0x03), not intercepted |

(On Linux/Windows: use Ctrl instead of Cmd)

## Input Routing

- All keyboard input goes only to the active session's PTY
- Ctrl+C is sent as byte 0x03 to the PTY — it does NOT close the app
- Session switching shortcuts (Cmd+1-9, Cmd+Tab) are intercepted before reaching the PTY
- Paste (Cmd+V) sends clipboard text to the active session's PTY

## Inactive Session Behavior

- Inactive sessions continue receiving PTY output via background reader threads
- Output is parsed through alacritty_terminal (advancing terminal state)
- Inactive sessions do NOT render (no snapshot_grid, no canvas draw)
- They mark themselves as "dirty"
- When switching to a previously inactive session, snapshot_grid runs immediately (measured as switch latency)
- No output is dropped — the bounded buffer (512KB) applies per-session

## Rendering Policy

- Active session: parse + snapshot_grid + canvas redraw every 16ms poll
- Inactive sessions: parse only (no render work)
- Switch: snapshot_grid of new active session (typically <5ms for 120x40)

## Metrics Emitted

### Event types

| Event | Fields |
|-------|--------|
| `session_created` | session_id, session_name, command, timestamp_ms |
| `session_closed` | session_id, timestamp_ms |
| `session_switched` | from_session_id, to_session_id, switch_latency_ms, active_session_dirty_rows |
| `summary` | All aggregate metrics (see below) |

### Summary fields

- `mode`: "multi-session"
- `session_count`, `total_bytes`, `total_bytes_active_sessions`, `total_bytes_inactive_sessions`
- `p50/p95/p99_session_switch_latency_ms`
- `p50/p95/p99_active_render_work_ms`
- `p50/p95/p99_active_parse_time_ms`
- `p50/p95/p99_inactive_parse_time_ms`
- `p50/p95/p99_frame_delta_ms`
- `frames_over_16_7ms`, `frames_over_33_3ms`, `frames_over_50ms`
- `max_pending_pty_output_bytes_total`, `max_pending_pty_output_bytes_per_session`
- `output_bytes_dropped_total`, `output_bytes_dropped_per_session`
- `final_rss_mb`

### Summarizer

```bash
python3 bench/summarize-metrics.py bench/results/multi-*.jsonl
```

## CLI Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--multi-session` | false | Enable multi-session mode |
| `--sessions N` | 3 | Number of sessions to start |
| `--session-command CMD` | (shell) | Command to run in each session |
| `--session-source` | spike-local | Backend source (spike-local or planeai-local) |
| `--max-runtime-ms MS` | (none) | Auto-exit after N ms |
| `--exit-when-done` | false | Exit when all commands finish |
| `--output-queue-policy` | block | Queue policy: block or drop_oldest |

## Backend Integration Audit

### Architecture target

```
Iced UI
  → PlaneAiTerminalSession adapter trait
    → Backend implementation
      → spike-local (Shell in shell.rs) ← current
      → planeai-local (LocalBackend in pty.rs) ← future
      → planeai-daemon (DaemonBackend in pty.rs) ← future
      → planeai-tmux (TmuxAttach in pty.rs) ← future
```

### Existing PlaneAI backend (files inspected)

| File | Purpose |
|------|---------|
| `src-tauri/src/session_backend.rs` | SessionBackend trait: write, resize, pause, resume, detach |
| `src-tauri/src/pty.rs` | PtyManager, LocalBackend, DaemonBackend, reader/flusher threads |
| `src-tauri/src/commands/sessions/attach.rs` | Tauri commands: attach, write_to_pty, resize_pty, pause_pty |
| `src-tauri/src/commands/sessions/tabs.rs` | Tab spawn via Shell PtyTarget |
| `src-tauri/src/commands/sessions/launch.rs` | Session creation (tmux/daemon backends) |
| `src-tauri/src/output_observer.rs` | Output observation trait |
| `src-tauri/src/daemon_client.rs` | Async daemon IPC client |

### Reusable as-is

- `SessionBackend` trait shape (write, resize, pause, resume)
- `portable-pty` crate usage patterns
- `FlowControl` Condvar-based pause/resume
- Reader thread architecture (16KB reads into buffer)
- Command building (CommandBuilder with env setup)

### Needs extraction

- Output delivery: currently push-based via Tauri `Channel<Response>`. The adapter trait requires pull-based `try_read_batch()`. Would need adding a shared buffer (like spike's shell.rs) that the flusher writes to instead of `on_data.send()`.
- Remove `AppHandle` dependency from `PtyManager::attach()` signature
- Make `PtyManager` generic over output destination

### Tauri-coupled

- `attach_session` command requires `AppHandle`, `Channel<Response>`, `State<>`
- Flusher thread sends output via `on_data.send(Response::new(chunk))`
- Exit notification uses `app.emit("pty-exited", ...)`
- DaemonBackend uses `tauri::async_runtime::spawn`

### Spike-only fallback

- `shell.rs` in planeai-iced-spike: simpler bounded buffer with pull-based drain()
- Uses the same `portable-pty` crate as the real backend
- Same PTY behavior (xterm-256color, reader thread, queue policy)
- Labeled explicitly as `session_source: "spike-local"` in metrics

### Recommended first backend to integrate

`planeai-local` (LocalBackend) — requires:
1. Add a pull-based output buffer to LocalBackend (shared Vec<u8> + Condvar, like shell.rs)
2. Make the flusher optional (or replace with direct buffer fill)
3. Extract PTY spawn logic from `PtyManager::attach()` into a standalone function

### Known risks

- Refactoring pty.rs could affect the production Tauri app
- DaemonBackend requires async runtime not present in the iced spike
- tmux backend requires tmux binary availability

## Smoke Test Commands

```bash
# Build
cargo build --release -p planeai-iced-spike
cargo test -p planeai-iced-spike

# Existing modes (must still work)
bash bench/smoke-test.sh

# Multi-session shell — spike-local (interactive)
cargo run --release --bin planeai-iced-spike -- \
  --multi-session --sessions 3 --session-source spike-local \
  --cols 120 --rows 40 \
  --metrics bench/results/smoke-multi-shell.jsonl --backend iced-alacritty

# Multi-session flood — spike-local (automated)
cargo run --release --bin planeai-iced-spike -- \
  --multi-session --sessions 3 --session-source spike-local \
  --session-command "python3 bench/flood-output.py" \
  --cols 120 --rows 40 --max-runtime-ms 5000 \
  --metrics bench/results/smoke-multi-spike-local.jsonl \
  --backend iced-alacritty --exit-when-done

# Multi-session flood — planeai-local (automated)
cargo run --release --bin planeai-iced-spike -- \
  --multi-session --sessions 3 --session-source planeai-local \
  --session-command "python3 bench/flood-output.py" \
  --cols 120 --rows 40 --max-runtime-ms 5000 \
  --metrics bench/results/smoke-multi-planeai-local.jsonl \
  --backend iced-alacritty --exit-when-done

# Summarize all
python3 bench/summarize-metrics.py bench/results/smoke-*.jsonl
```

## Session Sources

### `spike-local` (default)

- Uses `shell.rs` in planeai-iced-spike
- Simple reader thread → bounded buffer (512KB) → pull via `try_read_batch()`
- No output coalescing — reader appends directly to shared buffer
- High raw throughput (~41 MB/s in flood test)
- Spike-only code, not shared with production backend

### `planeai-local`

- Uses `pty_core.rs` — Tauri-independent extraction of the production PTY backend
- Reader thread → coalescing buffer → flusher thread (4ms coalesce) → push via `TerminalOutputSink`
- ChannelSink bridges push→pull: bounded 512KB buffer with blocking backpressure
- Lower throughput (~0.33 MB/s in flood test) due to flusher coalescing on critical path
- Lossless: 0 bytes dropped under all tested loads
- Faithfully reproduces production `pty.rs` behavior (reader/flusher/FlowControl pattern)
- Supports: write, resize, pause/resume, exit detection

### Architecture

```
spike-local:
  PTY → reader thread → 512KB buffer → try_read_batch() → alacritty_terminal

planeai-local:
  PTY → reader thread → coalescing buffer → flusher thread (4ms sleep)
      → TerminalOutputSink::send() → ChannelSink (512KB blocking buffer)
      → try_read_batch() → alacritty_terminal
```

### Which source is default?

`spike-local` is the default (via `--session-source spike-local`).

### Performance Comparison (3 sessions, flood-output.py, 5s)

| Metric | spike-local | planeai-local |
|--------|-------------|---------------|
| Throughput (MB/s) | 41 | 0.33 |
| Total bytes (5s) | 215 MB | 1.7 MB |
| p99 frame delta | 42 ms | 33 ms |
| p95 parse time | 2.2 ms | 4.2 ms |
| p95 render work | 0.11 ms | 0.04 ms |
| Bytes dropped | 0 | 0 |
| Max pending | 512 KB | 507 KB |
| RSS | 284 MB | 306 MB |

The throughput difference is expected: planeai-local's flusher adds 4ms coalescing sleep per batch, which creates backpressure when the bounded buffer fills. In production, this coalescing reduces frontend render calls. In the spike, it limits peak throughput but produces smoother output delivery.

### Usage

```bash
# spike-local (default, high throughput)
cargo run --release --bin planeai-iced-spike -- \
  --multi-session --sessions 3 --session-source spike-local

# planeai-local (production-faithful, coalesced output)
cargo run --release --bin planeai-iced-spike -- \
  --multi-session --sessions 3 --session-source planeai-local
```

## What Remains: Daemon Integration

- `planeai-daemon` crate is fully Tauri-independent (tokio + portable-pty)
- Integration would require: async runtime in the spike, data connection via IPC socket
- Would add `--session-source planeai-daemon` that connects to an existing daemon session
- Blocked on: tokio integration into iced event loop, session discovery/listing

## What Remains: tmux Integration

- tmux backend requires the `tmux` binary on PATH
- Integration would add `--session-source planeai-tmux`
- Would need: session listing (tmux list-sessions), attach via PtyTarget::TmuxAttach
- Blocked on: tmux session lifecycle management, detach/reattach semantics in the spike

## What Remains: Full Production Backend Extraction

The shared core (`pty_core.rs`) currently lives inside the spike crate. To fully unify:

1. Move `pty_core.rs` to `planeai-core` or a new `planeai-pty` crate
2. Make the production Tauri `pty.rs` use the shared core (implement `TerminalOutputSink` for `Channel<Response>` + `AppHandle`)
3. Remove duplicated reader/flusher logic from `pty.rs`

This was intentionally deferred to avoid risking the production Tauri app.

## Known Limitations

1. **Resize:** Only resizes the active session's PTY. Inactive sessions are not resized until switched to.
2. **No bracketed paste mode:** Paste sends raw text without bracketed paste escape sequences.
3. **No scrollback:** Terminal scrollback is not implemented in the canvas renderer.
4. **Session names:** Fixed as "Session 1", "Session 2", etc. No rename support.
5. **No session persistence:** Sessions die when the app closes. No tmux/daemon reconnect.
6. **Queue policy per-app:** All sessions share the same queue policy (from CLI flag).
7. **planeai-local throughput:** Coalescing flusher limits peak throughput to ~0.3 MB/s under flood conditions. This is a faithful reproduction of production behavior, not a bug.
8. **planeai-local has no OutputObserver:** The production backend's `OutputObserver` trait (for byte-counting hooks) is not wired up in the extracted core.
9. **No Tauri adapter yet:** The shared core is not yet consumed by the production Tauri app. It lives spike-side only.
