# Iced Daemon Session Spike

Connects the Iced/alacritty native terminal UI to persistent daemon sessions via the existing daemon protocol.

## Quick Start

```bash
# Build
cargo build --release -p planeai-iced-spike -p planeai-daemon

# Run with daemon sessions (planeai-pty backend)
PLANEAI_DAEMON_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-daemon-session-logs \
cargo run --release -p planeai-iced-spike --bin planeai-iced -- \
  --multi-session \
  --sessions 3 \
  --session-source planeai-daemon \
  --session-command "python3 bench/flood-output.py" \
  --cols 120 --rows 40 \
  --max-runtime-ms 10000 \
  --metrics bench/results/iced-daemon-3-flood.jsonl \
  --backend iced-alacritty \
  --exit-when-done
```

## Architecture

```
┌─────────────────────────────────────────────────────┐
│ Iced App (planeai-iced)                             │
│  ┌───────────────────────────────────────────────┐  │
│  │ multi_session.rs                              │  │
│  │   polls 16ms: try_read_batch() → vte → term  │  │
│  └───────────────────────────────────────────────┘  │
│         ↕ PlaneAiTerminalSession trait              │
│  ┌───────────────────────────────────────────────┐  │
│  │ daemon_session.rs                             │  │
│  │   - bounded buffer (512KB, backpressure)      │  │
│  │   - input_tx: mpsc channel → FRAME_INPUT      │  │
│  │   - resize_tx: mpsc channel → control conn    │  │
│  └───────────────────────────────────────────────┘  │
│         ↕ dedicated tokio runtime (2 threads)       │
└─────────────────────────────────────────────────────┘
         ↕ unix socket (planeai-ipc)
┌─────────────────────────────────────────────────────┐
│ planeai-daemon                                      │
│   - control: JSON-line (spawn/kill/resize/list)     │
│   - data: binary frames (FRAME_OUTPUT/FRAME_INPUT)  │
│   - PTY core: legacy or planeai-pty                 │
│   - scrollback buffer: 1MB                          │
│   - durable logs: $PLANEAI_SESSION_LOG_DIR          │
└─────────────────────────────────────────────────────┘
```

## Async Runtime Strategy

The Iced UI thread must not block on daemon I/O. Solution:

- **Shared `OnceLock<Runtime>`** with 2 worker threads, created on first daemon session
- Each session spawns two async tasks:
  1. **Data loop**: reads FRAME_OUTPUT → writes to bounded buffer (blocks producer if full)
  2. **Resize loop**: receives resize commands → sends via control connection
- **Input path**: `write()` sends to unbounded mpsc channel → async task forwards as FRAME_INPUT
- This is the same bounded-buffer + Condvar pattern used by `planeai-local`

## Session Sources

| Source | Flag | Description |
|--------|------|-------------|
| spike-local | `--session-source spike-local` | Legacy portable-pty (test fallback) |
| planeai-local | `--session-source planeai-local` | planeai-pty crate, in-process |
| planeai-daemon | `--session-source planeai-daemon` | Daemon-backed persistent sessions |

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `PLANEAI_DAEMON_PTY_CORE` | (unset=legacy) | Set to `planeai-pty` for new PTY backend |
| `PLANEAI_SESSION_LOG_DIR` | (unset=no logs) | Directory for durable session logs |

## What Works

- Spawn 1-N daemon sessions from Iced
- Bidirectional I/O (output streams into alacritty_terminal, input forwarded)
- Ctrl-C works (sends \x03 via FRAME_INPUT)
- Resize works (via control connection)
- Exit detection (data connection EOF → has_exited)
- Scrollback replay on attach (daemon sends buffer snapshot)
- Durable logs (ANSI + meta.json)
- Replay of daemon logs via `--replay`
- Zero bytes dropped under flood (lossless backpressure)
- Cmd+N creates new daemon sessions
- Cmd+W closes sessions
- Cmd+1-9 / Cmd+Tab switches sessions

## What's Missing

- **Detach/reattach**: closing the Iced window kills all sessions (no persistent-after-close yet)
- **Session list/attach UI**: no UI to list and attach to pre-existing daemon sessions
- **Reconnect on daemon restart**: if daemon dies, sessions are lost
- **Daemon auto-start with specific config**: relies on daemon binary being adjacent to Iced binary
- **Daemon status indicator**: no UI showing daemon health

## Benchmark Results (10s flood, 3 sessions)

| Metric | planeai-local | planeai-daemon |
|--------|--------------|----------------|
| Throughput | 20.17 MB/s | 45.50 MB/s |
| Total bytes | 211 MB | 477 MB |
| p95 frame delta | 16.9 ms | 18.0 ms |
| p99 frame delta | 23.3 ms | 22.0 ms |
| p95 parse time | 1.03 ms | 2.32 ms |
| p99 parse time | 1.35 ms | 3.34 ms |
| p95 render work | 0.11 ms | 0.12 ms |
| Bytes dropped | 0 | 0 |
| RSS | 280 MB | 277 MB |

Daemon achieves 2.26x higher throughput due to efficient batching in the daemon's broadcast channel.

## Rollback

To revert to local-only mode, use `--session-source planeai-local`. No daemon required.

## Protocol

No protocol changes were made. Uses the existing daemon protocol:
- Control: `CONN_CONTROL` (0x00) + JSON-line requests
- Data: `CONN_DATA` (0x01) + binary frame handshake + bidirectional FRAME_OUTPUT/FRAME_INPUT

## Smoke Checklist

- [ ] `--session-source planeai-daemon` spawns sessions
- [ ] Output streams into terminal
- [ ] Input/Ctrl-C works
- [ ] Resize works (Cmd+Shift+F for fullscreen, drag window)
- [ ] Exit/kill works
- [ ] Durable logs appear in `$PLANEAI_SESSION_LOG_DIR/sessions/`
- [ ] Replay mode still works
- [ ] Local modes still work
- [ ] Production `cargo build --release -p planeai` succeeds
