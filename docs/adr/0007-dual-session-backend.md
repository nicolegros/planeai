# ADR-0007: Dual session backend (tmux + direct PTY)

## Status

Partially superseded by ADR-0010 — the `direct` backend has been replaced by the `daemon` backend, which provides persistence without requiring tmux. The tmux backend remains unchanged.

## Context

planeai originally required tmux as a hard dependency (ADR-0002) for session persistence. This blocks users who don't have tmux installed or don't want to install it. We need to support non-tmux users while keeping tmux as an option for those who want persistence.

Emdash (a similar Electron-based tool) solves this with an optional `tmux: boolean` per-project setting. When disabled, it spawns commands directly in a PTY with no persistence layer.

## Decision

Support two session backends behind a unified PTY interface:

1. **tmux** — creates a tmux session, then attaches via `tmux attach-session` inside a local PTY. Agent survives app quit.
2. **direct** — spawns the agent command directly inside a local PTY. Agent dies on app quit.

The backend is chosen globally (not per-session) via a config field `session_backend`. When absent, auto-detect: use tmux if the binary is on PATH, otherwise fall back to direct. The user can override in Preferences.

Both backends use the same `PtyManager` — the only difference is the `CommandBuilder` arguments. Exit detection is unified: PTY reader EOF marks the session as exited. `remain-on-exit` is removed from tmux sessions so that agent exit propagates cleanly.

A quit confirmation dialog appears only when active direct sessions would be terminated.

## Consequences

- tmux is no longer a hard dependency — the app works out of the box on any system.
- Users who want persistence still get it via tmux (auto-detected or explicitly chosen).
- Direct sessions are ephemeral — a quit confirmation protects against accidental loss.
- The `PtyTarget` enum keeps the PTY layer clean; no duplication in streaming/resize/write logic.
- DB gains a `backend` column and an `exited` status; startup reconciliation handles stale rows.
- ADR-0002 is superseded (tmux is now optional, not required).
