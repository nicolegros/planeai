# planeai

A cross-platform agent session orchestrator. Manages multiple AI coding agents running in parallel, each in its own terminal session backed by tmux for persistence.

## Glossary

| Term | Definition |
|------|-----------|
| **Project** | A git repository registered with planeai. Stores a repo path and display name. The top-level organizational unit. |
| **Session** | A single agent working on a single task within a project. Backed by a tmux session named `planeai-<project>-<8-hex-id>`. Contains one terminal pane running the agent CLI. |
| **Provider** | A CLI-based AI coding agent (e.g., Kiro). Defined by a launch command. v1 supports Kiro only (`kiro-cli chat --trust-all-tools`). |
| **Focus zone** | A region of the UI that can receive keyboard input: sidebar or terminal. App-level chords (Cmd+B, Cmd+N, Cmd+1-9, Ctrl+Tab, Escape) are always intercepted regardless of which zone has focus. |
| **Tab switcher** | An MRU overlay triggered by holding Ctrl+Tab. Each subsequent Tab moves selection; releasing Ctrl confirms. |
| **Notification** | (future) A signal that an agent needs human attention. |

## Session lifecycle (v1)

```
create → active → deleted
```

- **Active** — tmux session exists, agent may or may not still be running. Visible in sidebar.
- **Deleted** — tmux session killed, removed from sidebar and DB. Irreversible.

## Architecture notes

- **Tauri v2** (Rust backend + webview frontend)
- **Svelte 5** with runes for reactive UI
- **xterm.js** for terminal rendering
- **Tailwind CSS**, dark theme only
- **SQLite via rusqlite** on the Rust backend for persistence
- **tmux** for process persistence (ADR-0002)
- **Tauri IPC** (commands + typed event channels) for PTY byte streaming between Rust and frontend
- **pnpm** for package management

## Key constraints

- Keyboard-first — all actions reachable without mouse
- One active non-worktree session per project; multiple worktree sessions allowed
- DB is source of truth; orphan tmux sessions are ignored
- macOS primary target, cross-platform future
- Project names must be unique

## Worktree support

Sessions can run in two modes:

- **Checkout mode** (default) — `git checkout` on the project's repo path. Only one active checkout session per project.
- **Worktree mode** — `git worktree add` creates an isolated working copy at `~/.planeai/worktrees/<project-name>/<session-id>/`. Multiple worktree sessions can run in parallel on the same project.

### Lifecycle

- **Archive** — kills tmux, marks session archived. Worktree directory is preserved on disk.
- **Destroy** — kills tmux, runs `git worktree remove --force`, deletes directory, removes from DB.

### Data model

Sessions table has `worktree_path TEXT NULL`. Non-null indicates worktree mode.

### Form flow (worktree mode)

Project → Session name → ✅ Create worktree → Base branch (existing) → New branch name (editable, defaults to session name slugified)

## Architecture decisions

See `docs/adr/` for recorded decisions.
