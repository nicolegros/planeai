# PlaneAI Domain Model

> Shared domain model used by both the production Tauri app and the Iced workflow prototype.

## Overview

PlaneAI manages AI coding agent sessions. The domain model is shared across frontends via `planeai-core::services`.

```
┌──────────────┐     ┌──────────────┐
│  Tauri App   │     │ Iced Workflow │
│  (production)│     │ (prototype)  │
└──────┬───────┘     └──────┬───────┘
       │                    │
       ▼                    ▼
┌──────────────────────────────────┐
│  planeai_core::services          │
│  ProjectService                  │
│  SessionService                  │
│  WorktreeService                 │
│  TaskService                     │
└──────────────┬───────────────────┘
               │
               ▼
┌──────────────────────────────────┐
│  SQLite (planeai.db)             │
│  ~/.../ca.nicolegros.planeai/    │
└──────────────────────────────────┘
```

## Projects

| Field | Type | Description |
|-------|------|-------------|
| id | UUID | Unique identifier |
| name | String | Derived from directory name |
| path | String | Absolute filesystem path |
| status | String | `active` or `archived` |

**Identity:** A project is uniquely identified by its filesystem path. `ensure_project()` returns existing or creates new.

## Sessions

| Field | Type | Description |
|-------|------|-------------|
| id | UUID | Same ID used for daemon session |
| project_id | UUID | FK to projects |
| name | String | Display name |
| branch | String | Git branch (empty if no worktree) |
| status | String | `active`, `exited`, `archived`, `destroyed` |
| worktree_path | String? | Absolute path to git worktree |
| provider | String? | Provider key (e.g., "kiro", "claude") |
| backend | String | `daemon` or `tmux` |
| task_key | String? | Link to task (e.g., "PLA-5") |
| command | String? | Agent launch command |
| cwd | String? | Working directory |

**ID mapping:** The session UUID is passed directly to the daemon as its session_id. No secondary mapping exists — the DB record ID **is** the daemon session ID.

**Status lifecycle:**
```
active → exited (agent process ended)
active → destroyed (user killed)
active → archived (soft close with cleanup)
exited → active (restart)
archived → active (restore)
```

**Durable logs:** Stored at `$PLANEAI_SESSION_LOG_DIR/sessions/{session_id}/` containing:
- `{timestamp}_output.ansi` — raw terminal output
- `meta.json` — session metadata (command, cwd, timestamps, bytes_written, bytes_dropped)

## Worktrees

| Concept | Convention |
|---------|-----------|
| Root | `~/.planeai/worktrees/{project_name}/` |
| Path | `~/.planeai/worktrees/{project_name}/{short_id}` |
| Branch | `{task-key}/{short_id}` |
| Cleanup | Remove worktree + delete branch on destroy |

**Status:** Iced does not create worktrees yet (no task/branch picker). The shared `WorktreeService` defines the path convention for future use.

## Tasks

| Field | Type | Description |
|-------|------|-------------|
| key | String | Auto-generated (e.g., "PLA-1") |
| status | Enum | `todo`, `in_progress`, `in_review`, `done` |
| base_branch | String | Git base for worktree (default: "main") |

**Session link:** `sessions.task_key` references the task key.

**Lifecycle hooks:** Configured in `config.json` under `task_management`. Hooks fire on session state transitions (start, restart, complete, notify, pr_open, pr_merge).

**Status:** Iced can read task_key from session records. Full task dispatch/assignment UI is deferred.

## What Iced Reuses

| Concern | Source |
|---------|--------|
| Config/provider resolution | `planeai_core::session_launch` |
| Session preparation (command, env, PATH) | `planeai_core::session_launch::prepare_session()` |
| Project persistence | `planeai_core::services::ProjectService` |
| Session persistence | `planeai_core::services::SessionService` |
| Daemon protocol | `planeai_daemon::protocol` |
| IPC transport | `planeai_ipc` |
| Durable logging | Daemon-side (transparent) |

## What Remains Prototype-Only (Iced)

| Concern | Reason |
|---------|--------|
| Terminal emulation (alacritty_terminal) | UI concern, not domain |
| Canvas rendering | UI concern |
| Session Vec + UI state | Ephemeral view state |
| `recent_projects.json` | Lightweight UI cache, not authoritative |
| Daemon connection management | Different async model than Tauri |

## Known Gaps

1. **Worktree creation** — Iced doesn't create worktrees. Next milestone.
2. **Task dispatch** — Iced doesn't assign tasks to sessions. Next milestone.
3. **Lifecycle hooks** — Iced doesn't fire on_start/on_complete hooks. Requires task integration.
4. **Provider session ID discovery** — Iced doesn't track the agent's internal session ID.
5. **Notify hooks** — Iced doesn't detect agent completion signals.
6. **PR integration** — Iced doesn't track PR status.

## Running Tests

```bash
# Shared services parity tests
cargo test -p planeai-core --test services_parity_test

# Domain smoke test (full lifecycle)
PLANEAI_DAEMON_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-domain-smoke-logs \
PATH="$(pwd)/target/release:$PATH" \
cargo run --release -p planeai-iced-spike --bin planeai-domain-smoke -- \
  --cwd /tmp/planeai-smoke-project \
  --agent-command "python3 -c 'print(\"ready\")'; sleep 30"
```
