# planeai

A native macOS agent session orchestrator. Manages multiple AI coding agents running in parallel, each isolated in its own git worktree, with GPU-accelerated terminal rendering.

## Glossary

| Term | Definition |
|------|-----------|
| **Project** | A git repository registered with planeai. Stores a repo path, default provider, default auto-approve setting, and default branch strategy. The top-level organizational unit. |
| **Session** | A single agent working on a single task within a project. Backed by a tmux session. Contains a primary pane (the agent) and zero or more secondary panes (manual shells). |
| **Provider** | A CLI-based AI coding agent (e.g., Claude Code, Kiro, Codex, Copilot). Defined by a launch command, icon, auto-approve flag, and notify hook template. |
| **Primary pane** | The terminal pane within a session that runs the agent CLI. Visually distinct from secondary panes. Its process exit triggers session completion. |
| **Secondary pane** | A split terminal pane within a session used for manual work (running tests, git commands, etc.). Defaults to the session's working directory. |
| **Worktree** | A git worktree created for a session to isolate its changes from other sessions. Auto-created at `../<project-name>-<branch-name>` relative to the repo root. |
| **Template** | (v1.1) A saved session configuration (provider + branch strategy + auto-approve) for one-keystroke launch. |
| **Notification** | A signal that an agent needs human attention. Detected via OSC 9/99/777 sequences or the `planeai notify` CLI. Manifests as sidebar badge, macOS notification, and unread queue entry. |
| **Archive** | Soft-removing a completed session. Hides from sidebar, persists scrollback to disk, optionally prunes worktree. Restorable. |
| **Delete** | Hard-removing a session. Scrollback purged, tmux session killed, worktree removed. Irreversible. |

## Architecture decisions

See `docs/adr/` for recorded decisions.

## Key constraints

- macOS 14+ (Sonoma) minimum deployment target
- 100% keyboard-usable — all actions reachable without mouse
- Bundle ID: `ca.nicolegros.planeai`
- tmux required as a runtime dependency (validated at launch)
