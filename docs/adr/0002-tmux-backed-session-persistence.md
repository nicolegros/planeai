# ADR-0002: tmux-backed session persistence

## Status

Accepted

## Context

When the user quits planeai, agent processes need to keep running. Options considered:

1. **No persistence** — processes die on quit, rely on agent-native resume commands (`claude --continue`). Simple but agents lose state and must restart.
2. **Custom daemon** — a launchd agent owns the ptys. Complex to build and maintain.
3. **tmux as the persistence layer** — each session is a tmux session. The app attaches ghostty surfaces to `tmux attach`. Processes survive app quit because tmux keeps them alive.

## Decision

Use tmux as the process persistence layer. Each planeai session maps to a tmux session named `planeai-<project>-<session-id>`. Ghostty surfaces render `tmux attach` with all tmux chrome disabled (status off, prefix key unreachable). On relaunch, detect existing `planeai-*` tmux sessions and reattach.

## Consequences

- Agents survive app quit/crash — zero data loss.
- tmux becomes a required runtime dependency (user-friendly error if missing).
- tmux's escape sequence handling sits in the rendering pipeline — occasional edge cases possible.
- Users can `tmux attach` from a raw terminal in emergencies.
- Split panes map to tmux panes, getting layout persistence for free.
- The app must disable tmux's own UI (status bar, prefix key) to avoid conflicts.
