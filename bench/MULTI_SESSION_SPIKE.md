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

# Multi-session shell (interactive)
cargo run --release --bin planeai-iced-spike -- \
  --multi-session --sessions 3 --cols 120 --rows 40 \
  --metrics bench/results/smoke-multi-shell.jsonl --backend iced-alacritty

# Multi-session flood (automated)
cargo run --release --bin planeai-iced-spike -- \
  --multi-session --sessions 3 \
  --session-command "python3 bench/flood-output.py" \
  --cols 120 --rows 40 --max-runtime-ms 5000 \
  --metrics bench/results/smoke-multi-flood.jsonl \
  --backend iced-alacritty --exit-when-done

# Summarize all
python3 bench/summarize-metrics.py bench/results/smoke-*.jsonl
```

## Known Limitations

1. **Backend:** Uses `spike-local` only. PlaneAI's real backend (LocalBackend, DaemonBackend) is not integrated yet due to Tauri coupling.
2. **Resize:** Only resizes the active session's PTY. Inactive sessions are not resized until switched to.
3. **No bracketed paste mode:** Paste sends raw text without bracketed paste escape sequences.
4. **No scrollback:** Terminal scrollback is not implemented in the canvas renderer.
5. **Session names:** Fixed as "Session 1", "Session 2", etc. No rename support.
6. **No session persistence:** Sessions die when the app closes. No tmux/daemon reconnect.
7. **Queue policy per-app:** All sessions share the same queue policy (from CLI flag).
