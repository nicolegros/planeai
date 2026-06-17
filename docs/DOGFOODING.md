# Dogfooding — planeai-pty

> **Readiness level:** Limited daily dogfooding recommended for local shell sessions.  
> **Default:** Legacy PTY remains default. Opt in via env vars.

## Quick Start

```bash
# Full dogfood mode: planeai-pty + durable logs + log viewer
PLANEAI_LOCAL_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-session-logs \
PLANEAI_DOGFOOD_LOG_VIEWER=1 \
pnpm tauri dev
```

## Environment Variables

| Variable | Values | Default | Description |
|----------|--------|---------|-------------|
| `PLANEAI_LOCAL_PTY_CORE` | `legacy`, `planeai-pty` | `legacy` | Which PTY backend to use for local sessions |
| `PLANEAI_SESSION_LOG_DIR` | absolute path | unset | Enable durable raw `.ansi` logs |
| `PLANEAI_DOGFOOD_LOG_VIEWER` | `1`, `true` | unset | Enable the in-app session log viewer |

## How to Enable

### Legacy mode (default)

```bash
PLANEAI_LOCAL_PTY_CORE=legacy pnpm tauri dev
```

Or simply unset the variable — legacy is default.

### planeai-pty mode

```bash
PLANEAI_LOCAL_PTY_CORE=planeai-pty pnpm tauri dev
```

### planeai-pty + durable logs

```bash
PLANEAI_LOCAL_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-session-logs \
pnpm tauri dev
```

### Full dogfood mode (logs + viewer)

```bash
PLANEAI_LOCAL_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-session-logs \
PLANEAI_DOGFOOD_LOG_VIEWER=1 \
pnpm tauri dev
```

## Rollback Instructions

To immediately revert to legacy:

```bash
unset PLANEAI_LOCAL_PTY_CORE
unset PLANEAI_SESSION_LOG_DIR
unset PLANEAI_DOGFOOD_LOG_VIEWER
```

Or explicitly:

```bash
PLANEAI_LOCAL_PTY_CORE=legacy pnpm tauri dev
```

No data loss occurs on rollback. Existing durable logs remain on disk.

## Durable Session Logs

### Directory format

```
$PLANEAI_SESSION_LOG_DIR/
  sessions/
    <session-id>/
      meta.json
      <YYYYMMDDTHHMMSSZ>_output.ansi
```

### Metadata sidecar schema (`meta.json`)

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

**Status values:** `running` (session active), `exited` (session ended normally).

- `meta.json` is written at session start with status `running`
- Updated on session exit with `ended_at`, `exit_status`, `status`, `bytes_written`, `bytes_dropped`
- `.ansi` file contains raw bytes only (preserves all ANSI escapes, cursor movement, colors)
- Write errors are logged but never crash the app
- Repeated sessions create separate timestamped logs (no overwrites)

## Dogfood Log Viewer

When `PLANEAI_DOGFOOD_LOG_VIEWER=1` is set:

1. Open the command palette (`Cmd+K`)
2. Search for "Session log viewer"
3. The viewer shows all saved session logs
4. Select a session to see metadata details
5. Click **▶ Replay** to stream the `.ansi` log through a read-only xterm.js terminal
6. Use **Copy Path** or **Open Folder** to access log files directly

### Replay behavior

- Streams in 64 KiB chunks at ~16ms intervals
- Read-only terminal (no input accepted)
- Preserves ANSI sequences, colors, cursor movement
- Shows bytes replayed counter
- Supports pause, resume, stop, restart
- Clearly labeled "READ-ONLY REPLAY"

## Replaying Logs in the Iced Spike

Durable `.ansi` logs can also be replayed through the Iced spike:

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

Visual inspection without metrics:

```bash
cargo run --release -p planeai-iced-spike -- \
  --replay /tmp/planeai-session-logs/sessions/<session-id>/<timestamp>_output.ansi \
  --cols 120 --rows 40
```

## Manual GUI Smoke Checklist

Run with both backends and compare:

### Legacy mode

```bash
PLANEAI_LOCAL_PTY_CORE=legacy pnpm tauri dev
```

### PlaneAI PTY mode

```bash
PLANEAI_LOCAL_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-session-logs \
PLANEAI_DOGFOOD_LOG_VIEWER=1 \
pnpm tauri dev
```

### Checklist

| # | Check | Legacy | PlaneAI PTY |
|---|-------|--------|-------------|
| 1 | App starts without crash | | |
| 2 | Startup log shows PTY core mode | | |
| 3 | Startup log shows session log directory | N/A | |
| 4 | Create local terminal/session | | |
| 5 | `echo hello` — output appears | | |
| 6 | Paste a multi-line command | | |
| 7 | Run noisy command (`find / -name '*.rs' 2>/dev/null`) | | |
| 8 | Ctrl-C interrupts running command | | |
| 9 | Resize terminal window | | |
| 10 | Close session cleanly | | |
| 11 | `.ansi` log exists | N/A | |
| 12 | `meta.json` exists | N/A | |
| 13 | `meta.json` updates after exit/close | N/A | |
| 14 | Dogfood log viewer lists the session | N/A | |
| 15 | Replay opens in read-only terminal | N/A | |
| 16 | Replay output matches original closely | N/A | |
| 17 | App restarts cleanly | | |
| 18 | Legacy mode works after unsetting env vars | | |
| 19 | No backend panic in logs | | |
| 20 | No frontend error overlay | | |

### Compare legacy vs planeai-pty

| Test | Legacy | PlaneAI PTY |
|------|--------|-------------|
| Startup | | |
| Local terminal creation | | |
| Output correctness | | |
| Input echo | | |
| Paste | | |
| Resize | | |
| Ctrl-C | | |
| Session close | | |
| Frontend errors | None | None |
| Backend errors | None | None |
| Logs created | No | Yes (when LOG_DIR set) |

## Tests

```bash
# planeai-pty crate tests (7 tests)
cargo test -p planeai-pty

# Session log catalog tests (8 tests, requires serial execution)
cargo test -p planeai -- session_logs --test-threads=1

# Iced spike tests
cargo test -p planeai-iced-spike
```

## Is Limited Dogfooding Recommended?

**Yes**, with the following conditions:

1. ✅ planeai-pty is proven lossless in benchmark tests (0 bytes dropped)
2. ✅ Output delivery uses the same Channel/Event patterns as legacy
3. ✅ Durable logs provide an audit trail for debugging issues
4. ✅ Metadata sidecar tracks bytes_written/bytes_dropped for observability
5. ✅ Fallback to legacy is instant (unset env var or set to `legacy`)
6. ✅ Path traversal prevented in log reader API
7. ✅ Log viewer is gated behind a separate flag (won't appear unless explicitly enabled)
8. ✅ Performance at parity with legacy (same coalescing strategy)

**Do not use for:**

- Critical production work where session loss would be costly
- Long multi-hour sessions (not yet stress-tested beyond minutes)
- Sessions that rely on tmux/daemon features (those still use legacy)
- Environments where sensitive data in raw logs is a concern (logs contain all terminal output)

## Known Limitations

- planeai-pty is only wired for local shell sessions (`PtyTarget::Shell`)
- Daemon/tmux sessions still use the legacy path
- Read-only replay does not restore a live process
- xterm.js rendering bottleneck exists in Tauri UI for very large replays
- Iced UI is still a prototype (not production)
- GUI smoke test must be run manually
- Long multi-hour sessions are not fully stress-tested
- Log cleanup/rotation is not implemented (logs accumulate indefinitely)
- Sensitive data (passwords, tokens typed in terminal) is stored in raw logs
- Log viewer is a basic dev panel, not a polished user feature
- Tests require `--test-threads=1` due to shared env vars
