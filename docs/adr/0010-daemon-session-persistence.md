# ADR-0010: Daemon Session Persistence

## Status

Accepted

## Context

The direct PTY backend is ephemeral — sessions die when the GUI quits. tmux provides persistence but is not available on Windows and requires external installation. We need session persistence that works out-of-the-box on all platforms.

## Decision

Replace the direct backend with a custom daemon (`planeai-daemon`) that:

1. Owns agent PTYs and keeps them alive independently of the GUI
2. Serves PTY I/O over Unix sockets (macOS/Linux) or named pipes (Windows)
3. Provides scrollback replay via in-memory ring buffer (1MB default)
4. Runs as a Tauri sidecar, started on-demand
5. Auto-exits 30s after no clients connected AND no sessions alive
6. Uses a hybrid protocol: JSON lines for control + binary frames for data

### Protocol

- **Control connection**: JSON lines over the main socket. Commands: `create_session`, `attach`, `detach`, `resize`, `list`, `kill`, `write`.
- **Data connection**: After `attach`, a dedicated connection streams binary PTY output. Input goes through the control connection's `write` command.

### Integration

- DB backend column changes from `'direct'` to `'daemon'`
- GUI's daemon backend connects to daemon socket instead of spawning PTY locally
- CLI becomes a full daemon client (can create/interact with sessions headlessly)
- tmux backend is completely unaffected

### Lifecycle

- On-demand start: GUI or CLI spawns daemon if not already running
- Idle shutdown: daemon exits 30s after last client disconnects AND no live sessions
- Sessions survive app quit — reattach on relaunch with scrollback

## Consequences

- Session persistence works on all platforms without tmux
- CLI can operate fully headless (no GUI needed for daemon sessions)
- Slightly more complex architecture (extra process), but single codebase
- direct backend code paths removed entirely

## Rejected alternatives

- **Save-and-resume**: Relies on agent `--continue` support, loses live state
- **Per-session broker processes**: No central coordination, orphan cleanup messy
- **OS-level service**: Heavy, platform-specific registration
- **nohup/setsid**: Minimal tmux reimplementation without the benefits
