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
- Reader thread → coalescing buffer → flusher thread (conditional 4ms coalesce) → push via `TerminalOutputSink`
- ChannelSink bridges push→pull: bounded 512KB buffer with blocking backpressure
- **High throughput: ~46 MB/s in 3-session flood** (after performance fixes, see below)
- Lossless: 0 bytes dropped under all tested loads
- Faithfully reproduces production `pty.rs` behavior (reader/flusher/FlowControl pattern)
- Supports: write, resize, pause/resume, exit detection

### Architecture

```
spike-local:
  PTY → reader thread → 512KB buffer → try_read_batch() → alacritty_terminal

planeai-local:
  PTY → reader thread → coalescing buffer → flusher thread (conditional 4ms sleep)
      → TerminalOutputSink::send() → ChannelSink (512KB blocking buffer)
      → try_read_batch() [drain loop, 2MB budget] → alacritty_terminal
```

### Which source is default?

`spike-local` is the default (via `--session-source spike-local`).

### Performance Comparison (3 sessions, flood-output.py, 5s)

| Metric | spike-local | planeai-local | tauri-xterm (baseline) |
|--------|-------------|---------------|------------------------|
| Throughput (MB/s) | 46.4 | 45.9 | ~2.6 |
| Total bytes (5s) | 244 MB | 241 MB | ~13 MB |
| p99 frame delta | 25.7 ms | 34.6 ms | several ms (many >16.7) |
| Bytes dropped | 0 | 0 | 0 |
| Max pending | 512 KB | 512 KB | N/A |

planeai-local is now at parity with spike-local and **17x faster** than the old Tauri/xterm baseline.

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

## Shared Crate: `planeai-pty`

### Location

`src-tauri/planeai-pty/` — workspace member of the `src-tauri/` Cargo workspace.

### What it is

planeai-pty is shared PTY/session I/O infrastructure. It is not a terminal emulator and does not render or parse ANSI.

### What moved into the crate

- `LocalPtySession` — local PTY spawn with reader/flusher coalescing threads
- `PtyEvent` (was `TerminalSessionEvent`) — push-based output/exit/error events
- `PtyEventSink` (was `TerminalOutputSink`) — trait for receiving events
- `LocalPtyConfig` — session spawn configuration (command, shell, cwd, env, cols, rows, coalesce params, queue policy)
- `QueuePolicy` — `Block` (default) / `DropOldest`
- `PipelineDiagnostics` (was `PtyDiagCounters`) — atomic reader/flusher counters
- `FlowControl` — internal pause/resume condvar (not public)
- Conditional coalescing fix (no 4ms sleep under flood)
- Lossless blocking flusher behavior

### What intentionally stayed outside the crate

- `ChannelSink` (Iced adapter's bounded push-to-pull bridge) — in spike's `planeai_local.rs`
- `PlaneAiTerminalSession` / `PlaneAiTerminalSession` trait — in spike's `adapter.rs`
- Terminal parsing (alacritty_terminal) — in spike's `multi_session.rs`
- Terminal rendering (Iced canvas) — in spike
- Benchmark UI, metrics, CLI — in spike
- Production Tauri `pty.rs` — unchanged
- Daemon/tmux backends — not yet integrated

### How the Iced spike uses the crate

The spike's `planeai_local.rs` implements `PtyEventSink` with a `ChannelSink` that:
1. Receives `PtyEvent::Output` pushes from the crate's flusher thread
2. Stores bytes in a bounded buffer (512KB, blocking backpressure)
3. Exposes `try_read_batch()` for the spike's poll-based UI loop

### Does production Tauri use the crate yet?

No. Production `pty.rs` remains unchanged. The next step would be implementing `PtyEventSink` for `Channel<Response>` + `AppHandle` in the Tauri backend.

### Dependencies of planeai-pty

- `portable-pty = "0.8"`
- `anyhow = "1"`

No Tauri, Iced, alacritty_terminal, xterm, serde, or tokio dependencies.

### Current performance (post-extraction, 3-session flood, 5s)

| Source | MB/s | p99 frame delta | output_bytes_dropped |
|--------|------|-----------------|---------------------|
| spike-local | 25.24 | 23.1 ms | 0 |
| planeai-local | 24.97 | 22.9 ms | 0 |

(At parity. Absolute throughput lower than previous 46 MB/s baseline due to larger terminal window 191×88 in this test environment.)

### Next step: Tauri adapter

To integrate into production:
1. Implement `PtyEventSink` for `Channel<Response>` + `AppHandle` in `src-tauri/src/pty.rs`
2. Replace the production reader/flusher with `LocalPtySession::spawn(config, tauri_sink)`
3. Remove duplicated reader/flusher logic from `pty.rs`

## Known Limitations

1. **Resize:** Only resizes the active session's PTY. Inactive sessions are not resized until switched to.
2. **No bracketed paste mode:** Paste sends raw text without bracketed paste escape sequences.
3. **No scrollback:** Terminal scrollback is not implemented in the canvas renderer.
4. **Session names:** Fixed as "Session 1", "Session 2", etc. No rename support.
5. **No session persistence:** Sessions die when the app closes. No tmux/daemon reconnect.
6. **Queue policy per-app:** All sessions share the same queue policy (from CLI flag).
7. **planeai-local throughput:** Now at parity with spike-local (~46 MB/s). Conditional flusher coalescing preserves low-load batching without penalizing flood throughput.
8. **planeai-local has no OutputObserver:** The production backend's `OutputObserver` trait (for byte-counting hooks) is not wired up in the extracted core.
9. **No Tauri adapter yet:** `planeai-pty` crate exists but the production Tauri app does not consume it yet. Only the Iced spike uses it.

## Performance Fix History

### planeai-local: 0.31 MB/s → 45.9 MB/s (148x improvement)

**Root cause:** Three compounding bottlenecks in the push-to-pull pipeline.

#### Bottleneck 1: Flusher slept unconditionally (pty_core.rs)

The flusher thread had `thread::sleep(4ms)` on every iteration, even under flood.
This capped throughput at ~`buffer_size / 4ms` regardless of data availability.

**Fix:** Conditional sleep — only coalesce when pending buffer is < 4096 bytes.
Under flood, the buffer is always large, so sleep is skipped entirely.

#### Bottleneck 2: ChannelSink deadlock on large batches (planeai_local.rs)

The sink's blocking condition `buf.len() + bytes.len() > MAX_BUFFER` would deadlock
when a single batch exceeded 512KB (because even with empty buffer, the check fails).
Under flood, the flusher coalesces many reads into one large batch (often >512KB).

**Fix:** Allow any single batch into an empty buffer regardless of size.
Only block when buffer already has data AND adding more would exceed the cap.

#### Bottleneck 3: UI drained one batch per poll (multi_session.rs)

The UI poll loop called `try_read_batch()` only once per session per 16ms tick.
Even with data available, it took only one batch, leaving the producer blocked.

**Fix:** Drain loop with 2MB-per-session budget. Multiple batches consumed per poll.
Rendering (snapshot_grid) happens once per poll after all batches are parsed.

#### Batch sizes before/after

| Metric | Before | After |
|--------|--------|-------|
| Avg batch to UI | ~100 KB (rare) | ~160-500 KB (frequent) |
| Batches per poll | 1 | 3-5 |
| Flusher batches/sec | 2-6 | 100-500 |
| Flusher sleep total (5s) | ~3000 ms | ~50 ms (flood only) |

#### 4ms coalescing status

Still present but conditional:
- Under low load (< 4096 bytes pending): sleeps 4ms to batch small writes
- Under flood (≥ 4096 bytes pending): no sleep, flushes immediately
- FLUSH_MAX_IDLE (50ms) timeout still prevents indefinite waits on empty buffer

#### Output lossless?

Yes — 0 bytes dropped in all configurations tested:
- 1/3/5 sessions × flood workload × 5 seconds
- ChannelSink uses blocking backpressure (condvar wait)
- `--output-queue-policy block` is enforced for planeai-local

#### Remaining bottlenecks

- p99 frame delta is 34.6ms for planeai-local vs 25.7ms for spike-local (extra flusher thread scheduling)
- Python flood_output.py is the actual throughput limit (~27 MB/s per session)
- Single active session rendering caps at ~26 MB/s regardless of source

#### Ready for shared-core extraction?

Done. The shared core now lives in `src-tauri/planeai-pty/`. The Iced spike's
`planeai-local` source consumes it via `planeai_pty::LocalPtySession`. Next step
is having production `pty.rs` implement `PtyEventSink` and use the same crate.
