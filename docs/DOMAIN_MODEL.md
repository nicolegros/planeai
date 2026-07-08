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
│  LoopService                     │
└──────────────┬───────────────────┘
               │
               ▼
┌──────────────────────────────────┐
│  SQLite (planeai.db)             │
│  ~/.../ca.nicolegros.planeai/    │
└──────────────────────────────────┘
```

Tauri's `db::create_session_with_id()` delegates to `SessionService::create()`. Both share a single migration path (`planeai_core::services::migrate`) that is safe to run on existing production databases.

## Projects

| Field  | Type   | Description                 |
| ------ | ------ | --------------------------- |
| id     | UUID   | Unique identifier           |
| name   | String | Derived from directory name |
| path   | String | Absolute filesystem path    |
| status | String | `active` or `archived`      |

**Identity:** A project is uniquely identified by its filesystem path. `ensure_project()` returns existing or creates new.

## Sessions

| Field             | Type    | Description                                 |
| ----------------- | ------- | ------------------------------------------- |
| id                | UUID    | Same ID used for daemon session             |
| project_id        | UUID    | FK to projects                              |
| name              | String  | Display name                                |
| branch            | String  | Git branch (empty if no worktree)           |
| status            | String  | `active`, `exited`, `archived`, `destroyed` |
| worktree_path     | String? | Absolute path to git worktree               |
| provider          | String? | Provider key (e.g., "kiro", "claude")       |
| backend           | String  | `daemon` or `tmux`                          |
| task_key          | String? | Link to task (e.g., "PLA-5")                |
| command           | String? | Agent launch command                        |
| cwd               | String? | Working directory                           |
| parent_session_id | String? | ID of the session that spawned this one     |

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

| Concept | Convention                                       |
| ------- | ------------------------------------------------ |
| Root    | `~/.planeai/worktrees/{project_name}/`           |
| Path    | `~/.planeai/worktrees/{project_name}/{short_id}` |
| Branch  | `{task-key}/{short_id}`                          |
| Cleanup | Remove worktree + delete branch on destroy       |

**Status:** Iced does not create worktrees yet (no task/branch picker). The shared `WorktreeService` defines the path convention for future use.

## Tasks

| Field       | Type   | Description                                |
| ----------- | ------ | ------------------------------------------ |
| key         | String | Auto-generated (e.g., "PLA-1")             |
| status      | Enum   | `todo`, `in_progress`, `in_review`, `done` |
| base_branch | String | Git base for worktree (default: "main")    |

**Session link:** `sessions.task_key` references the task key.

**Lifecycle hooks:** Configured in `config.json` under `task_management`. Hooks fire on session state transitions (start, restart, complete, notify, pr_open, pr_merge).

**Status:** Both Tauri and Iced can launch sessions from tasks, persist task_key on session records, create task-driven worktrees, and fire lifecycle hooks. Full task dispatch/assignment UI remains Tauri-only.

## Prompt Locks

| Field       | Type   | Description                                      |
| ----------- | ------ | ------------------------------------------------ |
| session_id  | String | Primary key — the session being locked           |
| owner_id    | UUID   | Unique lock owner (generated per acquisition)    |
| acquired_at | String | RFC 3339 timestamp of when the lock was acquired |

**Purpose:** Prevents concurrent prompt sends to the same session from multiple processes (GUI, CLI, AXI). The lock is acquired before sending a prompt and always released after (success or failure). Stale locks older than 2 minutes are automatically cleaned up on acquisition attempts.

**RAII guard:** `PromptLockGuard` (via `acquire_guard`) ensures the lock is released even on early `?` returns or panics. Prefer this over raw `acquire`/`release` to prevent lock leaks on error paths.

**Module:** `planeai_core::prompt_lock` — migrated via `planeai_core::services::migrate`.

## What Iced Reuses

| Concern                                  | Source                                            |
| ---------------------------------------- | ------------------------------------------------- |
| Config/provider resolution               | `planeai_core::session_launch`                    |
| Session preparation (command, env, PATH) | `planeai_core::session_launch::prepare_session()` |
| Project persistence                      | `planeai_core::services::ProjectService`          |
| Session persistence                      | `planeai_core::services::SessionService`          |
| Worktree logic                           | `planeai_core::services::WorktreeService`         |
| Task listing/prompt/lifecycle            | `planeai_core::services::TaskService`             |
| Prompt locking (cross-process)           | `planeai_core::prompt_lock`                       |
| Loop run persistence                     | `planeai_core::loop_service::LoopService`         |
| Task-driven launch resolution            | `TaskService::resolve_task_launch()`              |
| Daemon protocol                          | `planeai_daemon::protocol`                        |
| IPC transport                            | `planeai_ipc`                                     |
| Durable logging                          | Daemon-side (transparent)                         |

## How Tauri Uses Shared Services

Tauri's `db::create_session_with_id()` delegates to `planeai_core::services::SessionService::create()`. This ensures:

- Single INSERT implementation for sessions
- Same column set used by both frontends
- No schema drift between Tauri and Iced session records

Tauri still owns:

- `db::migrate()` (superset: includes settings table, tmux_name NOT NULL migration)
- Project create/archive/delete (UI-specific flows not yet extracted)
- MRU ordering, PR state updates

## What Remains Prototype-Only (Iced)

| Concern                                 | Reason                                  |
| --------------------------------------- | --------------------------------------- |
| Terminal emulation (alacritty_terminal) | UI concern, not domain                  |
| Canvas rendering                        | UI concern                              |
| Session Vec + UI state                  | Ephemeral view state                    |
| `recent_projects.json`                  | Lightweight UI cache, not authoritative |
| Daemon connection management            | Different async model than Tauri        |

## Known Gaps

1. **Worktree creation** — Iced doesn't create worktrees. Next milestone.
2. **Task dispatch** — Iced doesn't assign tasks to sessions. Next milestone.
3. **Lifecycle hooks** — Iced doesn't fire on_start/on_complete hooks. Requires task integration.
4. **Notify hooks** — Iced doesn't detect agent completion signals.
5. **PR integration** — Iced doesn't track PR status.

## Jira Integration

The `planeai-jira` crate provides two-way sync between Jira Cloud and planeai's task board.

### Jira Issues (local cache)

Stored in `jira_issues` table (managed by `JiraRepository`):

| Field          | Type     | Description                                       |
| -------------- | -------- | ------------------------------------------------- |
| issue_key      | String   | Jira issue key (e.g., `ENG-42`). Primary key.     |
| summary        | String   | Issue title from Jira                             |
| description    | String   | Issue body/description                            |
| status         | String   | Current Jira status (e.g., "In Progress")         |
| priority       | String?  | Jira priority name                                |
| labels         | String[] | Jira labels                                       |
| sync_status    | Enum     | `synced` or `departed`                            |
| last_synced_at | DateTime | Timestamp of last successful sync                 |
| source_name    | String   | Config source alias (key in `JiraConfig.sources`) |

### Sync Flow

```
JQL query → Jira REST API → JiraRepository (local cache)
                                    ↓
                          TaskProvider (upsert into planeai tasks)
```

- New issues: created as tasks with their Jira key as the task key
- Updated issues: title/description/status updated in local task store
- Departed issues (removed from JQL results): marked `departed`, UI prompted

### Writeback Flow

```
Local task status change → JiraWriteback
                              ↓
                    Jira REST API (transition + optional comment)
```

- `on_start`: triggered when a Jira task's first child is assigned to a project
- `on_complete`: triggered when a Jira task is marked done locally

### Authentication

OAuth 2.0 with PKCE via `JiraAuth`. Tokens stored file-based in `<app_data>/jira-tokens/` (600 perms). Refresh tokens used for silent re-auth.

## Loop Runs

A durable loop is an orchestration layer above sessions. It tracks rounds of agent work, verification, and human review — enabling strategies like maker-verifier or multi-agent loops without coupling session lifecycle to loop lifecycle.

### Loop Run

| Field                 | Type    | Description                                                                         |
| --------------------- | ------- | ----------------------------------------------------------------------------------- |
| id                    | UUID    | Unique identifier                                                                   |
| project_id            | String  | FK to projects                                                                      |
| task_key              | String? | Optional link to a tracked task                                                     |
| created_by_session_id | String? | Session that created this loop (nullable — loops can be CLI/UI/scheduler-initiated) |
| strategy              | String  | Freeform strategy identifier (e.g., "maker-verifier")                               |
| goal                  | String  | What the loop is trying to accomplish                                               |
| status                | Enum    | See loop statuses below                                                             |
| current_round         | Integer | Current iteration (0-based)                                                         |
| max_rounds            | Integer | Maximum rounds before auto-failure                                                  |
| created_at            | String  | RFC 3339 timestamp                                                                  |
| updated_at            | String  | RFC 3339, advances on any write                                                     |
| executor_finished_at  | String? | Set when executor is done (completed_unreviewed/failed/cancelled)                   |
| policy_json           | JSON?   | Retry/timeout/escalation rules (opaque)                                             |
| budget_json           | JSON?   | Token/cost/time limits (opaque)                                                     |

**Loop statuses:** `draft` → `running` → `observing` → `verifying` → `completed_unreviewed` → `approved` → `merged` → `cleaned`. Also: `blocked`, `needs_human`, `stale`, `failed`, `cancelled`.

**Ownership:** `created_by_session_id` tracks who spawned the loop but does not imply lifecycle coupling. Future fields (`owner_session_id`, `cleanup_policy`) will handle parent-death cleanup independently.

**executor_finished_at semantics:** Set only when the executor finishes producing a reviewable result — specifically when status transitions to `completed_unreviewed`, `failed`, or `cancelled`. Post-executor lifecycle states (`approved`, `merged`, `cleaned`) do NOT set this field because they represent human/CI actions after the executor is done.

### Loop Session

| Field      | Type    | Description                                        |
| ---------- | ------- | -------------------------------------------------- |
| loop_id    | String  | FK to loop_runs                                    |
| session_id | String  | FK to sessions                                     |
| role       | String  | Strategy-specific role (e.g., "maker", "verifier") |
| round      | Integer | Which round this session belongs to                |
| provider   | String? | Agent provider used for this session               |
| status     | String  | Session-within-loop status                         |
| created_at | String  | RFC 3339 timestamp                                 |

**Primary key:** `(loop_id, session_id)` — a session can only belong to one loop.

### Loop Event

| Field        | Type    | Description                        |
| ------------ | ------- | ---------------------------------- |
| id           | Integer | Auto-incrementing, ordered         |
| loop_id      | String  | FK to loop_runs                    |
| ts           | String  | RFC 3339 timestamp                 |
| kind         | String  | Event type (e.g., "round_started") |
| payload_json | JSON    | Event-specific payload             |

### Loop Artifact

| Field        | Type    | Description                           |
| ------------ | ------- | ------------------------------------- |
| id           | UUID    | Unique identifier                     |
| loop_id      | String  | FK to loop_runs                       |
| session_id   | String? | Which session produced this artifact  |
| kind         | String  | Artifact type (e.g., "diff", "patch") |
| path         | String? | File path if applicable               |
| content_json | JSON?   | Structured content if applicable      |
| created_at   | String  | RFC 3339 timestamp                    |

### Verifier Run

| Field         | Type     | Description                              |
| ------------- | -------- | ---------------------------------------- |
| id            | UUID     | Unique identifier                        |
| loop_id       | String   | FK to loop_runs                          |
| session_id    | String?  | Session if agent-based verifier          |
| verifier_type | String   | "command" or "agent"                     |
| name          | String   | Human-readable name (e.g., "cargo test") |
| command       | String   | The command or agent launch command      |
| status        | String   | "pending", "running", "passed", "failed" |
| exit_code     | Integer? | Process exit code (command verifiers)    |
| output_path   | String?  | Path to captured output                  |
| created_at    | String   | RFC 3339 timestamp                       |
| finished_at   | String?  | Set when verifier completes              |

**Module:** `planeai_core::loop_service::LoopService` — migrated via `LoopService::migrate(conn)` (called from each binary's DB migration chain).

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
