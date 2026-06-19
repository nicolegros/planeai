# Iced Workflow Shell

> **Status:** Prototype — ready for personal dogfooding, not production.

The workflow mode is the first PlaneAI-shaped Iced app: project-scoped agent sessions managed through the daemon backend.

## Quick Start

```bash
# Build all binaries
cd src-tauri
cargo build --release -p planeai-iced-spike
cargo build --release -p planeai --bin planeai-daemon

# Run workflow mode
PLANEAI_DAEMON_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-daemon-session-logs \
PATH="$(pwd)/target/release:$PATH" \
target/release/planeai-iced -- \
  --planeai-workflow \
  --cwd /path/to/your/project \
  --agent-command "kiro-cli chat" \
  --backend iced-alacritty
```

## CLI Flags

| Flag                 | Description                       | Default          |
| -------------------- | --------------------------------- | ---------------- |
| `--planeai-workflow` | Enable workflow mode              | off              |
| `--cwd <path>`       | Project working directory         | current dir      |
| `--agent-command`    | Command to launch agent sessions  | `kiro-cli chat`  |
| `--extra-path-dirs`  | Additional PATH dirs (repeatable) | none             |
| `--backend`          | Renderer backend                  | `iced-alacritty` |
| `--cols`             | Initial terminal columns          | 120              |
| `--rows`             | Initial terminal rows             | 40               |

## Architecture

```
┌─────────────────────────────────────────────────┐
│ Iced Workflow Shell (planeai-iced --planeai-workflow) │
├────────────┬────────────────────────────────────┤
│ Session    │ Terminal Canvas                     │
│ Cards      │ (alacritty_terminal + iced canvas)  │
│            │                                    │
│ ● claude   │ $ kiro-cli chat                    │
│ ● kiro     │ > working on feature...            │
│            │                                    │
├────────────┴────────────────────────────────────┤
│ Status: ⚡ daemon | ~/my-project | kiro | 4.2KB  │
└─────────────────────────────────────────────────┘
         │
         │ IPC (Unix domain socket)
         ▼
┌─────────────────────────┐
│ planeai-daemon          │
│ (persistent, headless)  │
│                         │
│ Sessions survive app    │
│ close/restart           │
└─────────────────────────┘
```

## Keyboard Shortcuts

| Shortcut    | Action                             |
| ----------- | ---------------------------------- |
| Cmd+N       | Launch new agent session           |
| Cmd+Shift+N | Launch with custom command         |
| Cmd+B       | Worktree launch prompt             |
| Cmd+T       | Task picker                        |
| Cmd+Shift+T | Clear selected task                |
| Cmd+Enter   | Launch selected task               |
| Cmd+O       | Open project picker (path input)   |
| Cmd+R       | Refresh daemon session list        |
| Cmd+W       | Detach active session              |
| Cmd+Shift+W | Kill active session                |
| Cmd+A       | Attach to first unattached session |
| Cmd+L       | Replay session log                 |
| Cmd+1..9    | Switch to session N                |
| Cmd+V       | Paste                              |
| Cmd+/       | Show keyboard shortcuts            |

## Session Lifecycle

1. **Launch** (Cmd+N): Spawns session via daemon with project cwd and augmented PATH
2. **Running**: Terminal attached, live I/O
3. **Detach** (Cmd+W): Session keeps running in daemon, removed from UI
4. **Reattach** (Cmd+A): Reconnect to detached session, scrollback replayed
5. **Kill** (Cmd+Shift+W): Terminates the session process
6. **Exited**: Session process ended, still visible in UI

Close the Iced window → sessions persist. Restart → existing sessions shown in left panel.

## PATH Handling

PATH is constructed by `planeai_core::command::augmented_path()`:

Priority (highest → lowest):

1. `PLANEAI_EXTRA_PATH` env var (colon-separated)
2. `--extra-path-dirs` CLI flag / config `extra_path_dirs`
3. Conventional developer dirs (`~/.local/bin`, `~/.cargo/bin`, `/opt/homebrew/bin`, etc.)
4. Inherited system PATH

**No setup-specific paths are hardcoded.** User-specific shims must come from config/env.

Config example (`~/.config/planeai/config.json`):

```json
{
  "extra_path_dirs": ["~/.local/bin", "~/.cargo/bin", "/opt/homebrew/bin"]
}
```

## Durable Logs

When `PLANEAI_SESSION_LOG_DIR` is set:

- Raw `.ansi` output logged per session
- `meta.json` sidecar with command, cwd, timestamps, bytes_written, bytes_dropped
- Session cards show 📄 indicator when log exists
- bytes_dropped shown with ⚠ if nonzero

Replay existing logs:

```bash
cargo run --release -p planeai-iced-spike --bin planeai-iced -- \
  --replay /tmp/planeai-daemon-session-logs/sessions/<id>/<ts>_output.ansi \
  --cols 120 --rows 40 --backend iced-alacritty --exit-when-done
```

## Smoke Test

```bash
PLANEAI_DAEMON_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-workflow-smoke-logs \
PATH="$(pwd)/target/release:$PATH" \
target/release/planeai-workflow-smoke \
  --cwd /tmp/planeai-smoke-project \
  --agent-command "python3 -c 'import time; print(\"agent ready\", flush=True); time.sleep(30)'" \
  --metrics bench/results/workflow-smoke.jsonl
```

Verifies: daemon start, spawn with cwd, output, list, input, detach, reattach, kill, durable log, meta.json fields, bytes_dropped=0.

## Known Limitations

- Project picker is text-based (no native file dialog)
- ~~No recent projects list~~ → Recent projects stored in `~/.config/planeai/recent_projects.json`, max 20
- ~~No log replay within workflow mode~~ → Cmd+L opens embedded read-only log replay
- Config loaded from `~/.config/planeai/config.json` (or `--config` flag)
- Daemon crash = sessions lost (no crash recovery)
- Scrollback limited to daemon 1MB ring buffer
- No multi-project support (one project per window)
- ~~No task management integration~~ → Task picker (Cmd+T), task launch (Cmd+Enter), lifecycle hooks
- ~~No git worktree integration~~ → Worktree launch (Cmd+B), task-driven worktrees
- Session card UI is keyboard-only (no click actions)
- Log replay loads full file at once (no time-scrubbing)
- No full task board/editing — only pick and launch
- No auto-dispatch in Iced (production Orchestrator only)

## Relationship to Production Tauri App

- Production app (`pnpm tauri dev`) is unchanged
- Workflow mode is a separate Iced prototype binary
- They share: planeai-core, planeai-daemon, planeai-pty, daemon protocol
- **Both use `planeai_core::session_launch::prepare_session` for session creation**
- Workflow mode does NOT replace the Tauri app
- tmux remains optional, not default

### Session Creation Parity

As of this milestone, Tauri and Iced share session creation semantics via a shared service in `planeai-core`.

**Shared service owns:**

- Command resolution (via `shell_args` — proper `/bin/sh -c` wrapping)
- CWD validation
- PATH/env construction (via `augmented_path` with dedup)
- Session ID propagation (PLANEAI_SESSION_ID env var)
- TERM env var setting
- Error types for invalid cwd / empty command

**UI-specific (NOT shared):**

- Daemon connection management (Tauri: async DaemonClient, Iced: block_on with shared runtime)
- DB persistence (Tauri only)
- Git worktree creation (Tauri only)
- tmux routing (Tauri only, via config)
- Notify hook registration (Tauri only)
- Provider session ID discovery (Tauri only)
- Iced UI session cards / attach/detach lifecycle

**Parity verified by:**

- 10 unit tests in `planeai-core/tests/session_launch_parity_test.rs`
- Workflow smoke test confirms shared service path end-to-end
- Production Tauri release build succeeds

**How to test parity:**

```bash
cargo test -p planeai-core --test session_launch_parity_test
```

## Rollback

Stop using workflow mode. The daemon backend, protocol, and durable logs are shared infrastructure used by both apps. No migration needed.

---

## Session Creation Parity Audit

**Date:** 2026-06-18

### Production Tauri path (`src-tauri/src/commands/sessions/launch.rs`)

1. `launch_session` Tauri command receives frontend args
2. Phase 1: Reads config (provider, command, backend, scrollback_bytes, extra_path_dirs)
3. Phase 2: Git worktree/branch creation, session_id generation
4. Phase 3 (daemon): ensures daemon running → **calls `prepare_session()`** → spawns via `DaemonClient`
5. Phase 3 (tmux): `tmux::create_session_with_cmd`
6. Phase 4: DB write, notify registration, task hooks

### Current Iced path (`planeai-iced-spike/src/daemon_session.rs`)

1. `DaemonSession::spawn_with_cwd` called from workflow UI
2. Ensures daemon running
3. **Calls `prepare_session()`** — resolves command, env, PATH
4. Sends spawn JSON over control socket (block_on)
5. Opens data connection for I/O
6. No DB, no git, no notify, no tmux

### Duplicated logic (ELIMINATED)

Before this milestone, both paths independently:

- Split the command string (Iced used naive whitespace split; Tauri used `shell_args`)
- Built env with TERM + PATH
- Called `augmented_path` separately

Now both call `planeai_core::session_launch::prepare_session()`.

### Missing production semantics in Iced (ACCEPTABLE)

These remain Tauri-only by design:

- DB persistence
- Git worktree creation
- Notify hook registration
- Provider session ID discovery
- tmux routing

### Recommended shared boundary

```
planeai_core::session_launch::prepare_session()
```

Owns: command resolution, CWD validation, env construction, PATH augmentation.
Does NOT own: daemon connection, spawn call, DB, git, UI state.

---

## Config/Provider Parity

**Date:** 2026-06-18

### Config loading

Iced workflow now loads the same PlaneAI config file (`~/.config/planeai/config.json`) as the production Tauri app.

### Precedence (shared by Tauri and Iced)

1. **CLI flags** (`--agent-command`, `--cwd`, `--extra-path-dirs`)
2. **Environment variables** (`PLANEAI_EXTRA_PATH`, `PLANEAI_SESSION_LOG_DIR`, `PLANEAI_DAEMON_PTY_CORE`)
3. **Config file** (`~/.config/planeai/config.json`)
4. **Defaults** (kiro-cli chat, daemon backend, conventional PATH dirs)

### Shared resolver

```rust
planeai_core::session_launch::resolve_from_config(&LaunchConfig, &SessionLaunchOverrides)
    → Result<ResolvedLaunchConfig, CreateSessionError>
```

### Default backend

`daemon` is always the default. Users who want tmux set `"session_backend": "tmux"` in config.

### Config-based smoke test

```bash
PLANEAI_DAEMON_PTY_CORE=planeai-pty \
cargo run --release -p planeai-iced-spike --bin planeai-workflow-smoke -- \
  --config /tmp/planeai-smoke-config.json \
  --cwd /tmp/planeai-smoke-project \
  --metrics bench/results/workflow-config-smoke.jsonl
```

---

## Domain Parity Audit

**Date:** 2026-06-18

### Production project model

- **Storage:** SQLite table `projects` (id, name, path, status, auto_mode, task_manager)
- **Identity:** UUID v4
- **Path selection:** User provides via Tauri file dialog or Iced text input
- **Recent projects:** Tauri sidebar tracks implicitly; Iced uses `~/.config/planeai/recent_projects.json`
- **Metadata:** Name derived from directory name; status active/archived

### Production session model

- **Storage:** SQLite table `sessions` (id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, provider_session_id, tab_count, auto_approve, task_key, base_branch, mru_position, pr_url, pr_state, auto_dispatched, command, cwd)
- **Status:** active → exited → active (restart) | active → archived | active → destroyed
- **Daemon mapping:** session_id in DB = session_id passed to daemon. No separate mapping.
- **Durable logs:** Filesystem at `$PLANEAI_SESSION_LOG_DIR/sessions/{session_id}/`

### Production worktree model

- **Path:** `~/.planeai/worktrees/{project_name}/{short_id}`
- **Branch:** `{task-key}/{short_id}` for task-linked; user-specified for manual
- **Cleanup:** `destroy` command removes worktree + deletes branch
- **CWD:** Worktree path becomes session CWD

### Production task model

- **Storage:** `planeai-tasks` crate, SQLite (tasks/task_blockers/task_tags)
- **Status:** todo → in_progress → in_review → done
- **Session link:** `sessions.task_key` column
- **Lifecycle hooks:** on_start, on_restart, on_complete, on_notify, on_pr_open, on_pr_merge

### Current Iced project/session model

- **Projects:** `~/.config/planeai/recent_projects.json` (temporary UI convenience) + shared `planeai_core::services::ProjectService` (durable)
- **Sessions:** Local `Vec<Session>` for UI + shared `planeai_core::services::SessionService` (durable DB records)
- **Session IDs:** UUID v4 (same format as Tauri)
- **Status tracking:** UI-local SessionStatus enum + DB status column updated on kill/detach

### Duplicated state (documented, intentional)

- `recent_projects.json` — lightweight Iced-only cache for UI picker responsiveness. Not authoritative. Shared `ProjectService` is the source of truth.

### Missing semantics (next milestone)

- Iced does not create git worktrees (no branch picker/task dispatch UI yet)
- Iced does not have task assignment UI
- Iced does not fire lifecycle hooks (on_start/on_complete)
- Iced does not track provider_session_id

### Recommended shared boundary

```
planeai_core::services::{ProjectService, SessionService, WorktreeService, TaskService}
```

- **ProjectService:** ensure_project, list_active, get_by_path
- **SessionService:** create, list_for_project, set_status, get, durable_log_dir
- **WorktreeService:** worktree_root, worktree_path, branch_name
- **TaskService:** session_task_key

Both Tauri and Iced call into these services for durable state. UI-specific logic (terminal rendering, tmux, daemon connection management, notify hooks) remains in the respective frontend.

### Domain smoke test

```bash
PLANEAI_DAEMON_PTY_CORE=planeai-pty \
PLANEAI_SESSION_LOG_DIR=/tmp/planeai-domain-smoke-logs \
PATH="$(pwd)/target/release:$PATH" \
cargo run --release -p planeai-iced-spike --bin planeai-domain-smoke -- \
  --cwd /tmp/planeai-smoke-project \
  --agent-command "python3 -c 'print(\"agent ready\")'" \
  --metrics bench/results/domain-smoke.jsonl
```

Verifies: project resolved, session record, daemon start, output, detach/reattach, status destroyed, durable log linked, bytes_dropped=0.


## Worktree Audit Findings (PLA-123)

### Where worktrees are created

| Path | Role |
|------|------|
| `planeai-core/src/git.rs` → `worktree_add()` | Raw git worktree add command |
| `planeai-core/src/session.rs` → `SessionDispatcher::dispatch()` | Auto-dispatch orchestrator calls backend.create_worktree |
| `src-tauri/src/session_ops.rs` | Tauri GUI manual launch creates worktree |
| `src-tauri/src/cli.rs` | CLI `build_session_plan()` creates worktree |

### Worktree root

Convention: `~/.planeai/worktrees/{project_name}/{short_id}`

- `WorktreeService::worktree_root(project_name)` → `$HOME/.planeai/worktrees/{project_name}`
- Auto-dispatch uses `dispatch_config.worktree_root` (configurable) + project_name + short_id

### Worktree/path naming

- `short_id` = first 8 hex chars of UUID with dashes removed: `&session_id.replace('-', "")[..8]`
- Full path: `{worktree_root}/{project_name}/{short_id}`

### Branch naming

- Auto-dispatch: `{task_key_lower}/{short_id}` (spaces→dashes)
- Manual: user-provided branch name
- `WorktreeService::branch_name(task_key, short_id)` → `{task_key_lower}/{short_id}`

### How git worktree add is called

```rust
// planeai-core/src/git.rs
pub fn worktree_add(repo_path, worktree_path, new_branch, base_branch) {
    let resolved = resolve_base_branch(repo_path, base_branch)?;
    Command::new("git").args(["worktree", "add", "-b", new_branch, worktree_path, &resolved])
        .current_dir(repo_path)
}
```

Base branch resolution: fetches `origin/{name}` first, falls back to local `{name}`.

### Existing worktree detection

No explicit detection. System relies on fresh UUID-based short_id per session (collision astronomically unlikely). `git worktree add` will fail if branch already exists.

### Cleanup

Three-step in `cleanup.rs` on session destroy:
1. `git worktree remove --force {worktree_path}` (from project repo path)
2. `fs::remove_dir_all(worktree_path)` (fallback)
3. `git branch -D {branch}` (from project repo path)

Only runs if `session.worktree_path` is `Some(...)`.

### Session CWD after worktree creation

- Worktree mode: `cwd = worktree_path`
- Checkout mode: `cwd = project.path`
- `session_cwd()` helper returns `session.worktree_path.unwrap_or(project.path)`

### Session record worktree fields

Schema columns:
- `worktree_path TEXT` — NULL for checkout mode
- `branch TEXT NOT NULL` — feature branch name
- `base_branch TEXT` — resolved base branch
- `task_key TEXT` — optional task association

`has_active_checkout()` checks for sessions WHERE `worktree_path IS NULL`.

### Design: no explicit WorktreeMode enum yet

Worktree vs checkout is implicit via `worktree_path IS NULL`. The shared domain model will make this explicit.

---

## Task Integration Parity

**Date:** 2026-06-19

### Task parity audit

| Concern | Production (Tauri) | Iced Workflow |
| --- | --- | --- |
| Task storage | `planeai-tasks` SQLite crate | Same — shared via `TaskService` |
| Task listing | `list_task_items` command | `TaskService::list_for_project` |
| Task prompt | `build_provider_launch_command` | Same — shared via `resolve_task_launch` |
| Task/session link | `sessions.task_key` column | Same — `CreateSessionParams.task_key` |
| Worktree naming | `{task-key-lower}/{short_id}` | Same — `WorktreeService::branch_name` |
| Lifecycle hooks | `fire_task_hook` in `session_ops.rs` | `TaskService::fire_lifecycle_hook` |
| on_start | Move to `in_progress` | Same |
| on_complete | Move to `done` | Same (on natural exit) |
| Kill behavior | Destroy + cleanup | Same + reset task to `todo` |
| Auto-dispatch | Orchestrator polls tasks, autonomous=true | Not in Iced (deferred) |
| Autonomous template | Applied only when autonomous=true | Same |
| Task picker | Frontend task panel | Cmd+T picker overlay |

### Task picker behavior

- **Cmd+T**: Opens task picker for the current project
- **↑/↓**: Navigate tasks
- **Enter**: Select task (stored as `selected_task`)
- **Cmd+Shift+T**: Clear selected task
- **Cmd+Enter**: Launch session from selected task

### Task launch behavior

When launching from a selected task:
1. Resolve task prompt: `{title}\n\n{description}` (or custom template)
2. Build provider command via `build_provider_launch_command` with `autonomous=false`
3. Apply yolo/auto-approve flag if configured
4. Inject prompt via `provider.prompt_command`
5. Create worktree: branch = `{task-key-lower}/{short_id}`
6. Persist session record with `task_key`, `worktree_path`, `branch_name`, `base_branch`
7. Fire `on_start` lifecycle hook (move task to `in_progress`)
8. Spawn daemon session in worktree cwd

### Task/worktree/session linkage

- `session.task_key` → task key (e.g., "PLA-5")
- `session.worktree_path` → absolute path to worktree
- `session.branch` → `pla-5/{short_id}`
- `session.base_branch` → base branch for worktree

### Lifecycle/status behavior

| Event | Session status | Task status |
| --- | --- | --- |
| Launch from task | `active` | `in_progress` |
| Natural exit | `exited` | `done` |
| Kill | `destroyed` | `todo` (reset) |
| Detach | `active` (stays) | unchanged |

### Known limitations

- No auto-dispatch in Iced (Orchestrator runs only in production Tauri)
- No task editing from Iced (read-only task list)
- No drag/drop task board
- Task list limited to 15 visible items in picker
- No filter/search in task picker
- `on_notify` and `on_restart` hooks not implemented in Iced

### Task smoke test

```bash
PLANEAI_DAEMON_PTY_CORE=planeai-pty \
cargo run --release -p planeai-iced-spike --bin planeai-task-smoke -- \
  --project /tmp/planeai-task-smoke/project \
  --task-key PLA-123 \
  --agent-command "python3 -c 'print(\"agent ready\")'" \
  --metrics bench/results/task-smoke.jsonl
```

Verifies: task creation → prompt resolution → prompt injection → worktree creation → session with task_key → output → detach/reattach → kill → bytes_dropped=0.
