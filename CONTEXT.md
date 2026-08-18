# planeai

A cross-platform agent session orchestrator. Manages multiple AI coding agents running in parallel, each in its own terminal session. Supports three session backends: local (in-process PTY, default), tmux (persistent, requires tmux binary), and daemon (persistent, built-in, experimental).

## Glossary

| Term                 | Definition                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Project**          | A git repository registered with planeai. Stores a repo path and display name. The top-level organizational unit.                                                                                                                                                                                                                                                                                                                                                                                                                  |
| **Session**          | A single agent working on a single task within a project. Backed by a local PTY (default), tmux session, or the planeai daemon. Contains one terminal pane running the agent CLI.                                                                                                                                                                                                                                                                                                                                                  |
| **Session backend**  | The process hosting strategy for a session: `local` (in-process PTY, default), `tmux` (survives app quit, requires tmux binary), or `daemon` (survives app quit, built-in, experimental). Resolved at session creation from the global setting.                                                                                                                                                                                                                                                                                    |
| **Provider**         | A CLI-based AI coding agent (e.g., Kiro, Claude Code, Aider). Defined by a base `command`, optional `yolo_flag`, optional `resume_command` for session resume. Multiple providers can be configured; one is the `default_provider`.                                                                                                                                                                                                                                                                                                |
| **Config file**      | The single source of truth for all user preferences and provider definitions. Lives at `$XDG_CONFIG_HOME/planeai/config.json` (default `~/.config/planeai/config.json`). JSONC for reading, pretty JSON for writing.                                                                                                                                                                                                                                                                                                               |
| **Yolo mode**        | A per-session toggle that appends the provider's `yolo_flag` to the launch command, enabling auto-approval of tool use. Disabled if the provider has no `yolo_flag`.                                                                                                                                                                                                                                                                                                                                                               |
| **Focus zone**       | A region of the UI that can receive keyboard input: sidebar or terminal. App-level chords (Cmd/Ctrl+B, Cmd/Ctrl+N, Cmd/Ctrl+Shift+P, Cmd/Ctrl+1-9, Ctrl+Tab, Escape) are always intercepted regardless of which zone has focus.                                                                                                                                                                                                                                                                                                    |
| **Form keyboard**    | A vim-like normal/insert mode controller (`createFormKeyboardController`) used by modal forms (PR form, PR panel). Normal mode maps single-key mnemonics to field focus or toggle actions. Insert mode is entered on text field focus; Escape returns to normal; Escape in normal dismisses the form.                                                                                                                                                                                                                              |
| **Tab switcher**     | An MRU overlay triggered by holding Ctrl+Tab. Each subsequent Tab moves selection; releasing Ctrl confirms. Includes both sessions and active loop dashboards (`loop:<id>` entries).                                                                                                                                                                                                                                                                                                                                               |
| **Notification**     | (future) A signal that an agent needs human attention.                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| **Token**            | A semantic CSS custom property defined in the active theme file (e.g., `--color-surface-200`, `--terminal-background`). Mapped to Tailwind utilities via `@theme` block in `app.css`.                                                                                                                                                                                                                                                                                                                                              |
| **Primitive**        | A reusable styled Svelte component in `src/components/ui/` that wraps bits-ui behavior (for complex interactives) or provides app-specific defaults (Button, Input). The building block for feature components.                                                                                                                                                                                                                                                                                                                    |
| **Theme mode**       | One of three states: `system`, `light`, `dark`. Persisted in localStorage. Controls which color palette is active.                                                                                                                                                                                                                                                                                                                                                                                                                 |
| **Daemon**           | (Experimental) A background process (`planeai-daemon`) that manages session PTYs. Spawned on-demand by the CLI or GUI. Sessions survive indefinitely as long as the daemon is running.                                                                                                                                                                                                                                                                                                                                             |
| **AXI**              | Agent eXperience Interface — a CLI subcommand (`planeai-cli axi`) that outputs TOON instead of JSON, optimised for autonomous agent consumption. Covers task, session, project, and loop operations.                                                                                                                                                                                                                                                                                                                               |
| **TOON**             | A token-efficient text output format used by the AXI interface. Supports object fields, tabular arrays, and primitive arrays with minimal overhead. Implemented in the `planeai-toon` crate.                                                                                                                                                                                                                                                                                                                                       |
| **Jira integration** | Optional Jira Cloud connection managed by the bundled Jira plugin. The plugin stores its site and configured JQL sources under its own settings namespace, OAuth credentials under its backend-only secrets namespace, and sync membership/cache state in its plugin database. OAuth 2.0 (PKCE) is used for authorization. Manual source sync imports tasks and updates the Jira sidebar; task writeback and periodic sync are deferred.                                                                                           |
| **Loop run**         | A durable orchestration layer above sessions. Tracks rounds of agent work, verification, and human review. Optionally linked to a creating session via `created_by_session_id` (nullable). Persisted in `loop_runs` table via `planeai_core::loop_service::LoopService`.                                                                                                                                                                                                                                                           |
| **Loop session**     | A session enrolled in a loop run with a strategy-specific role (e.g., "maker", "verifier"). Tracked in `loop_sessions` with composite key `(loop_id, session_id)`.                                                                                                                                                                                                                                                                                                                                                                 |
| **Loop event**       | An ordered, append-only log entry for a loop run (e.g., "round_started", "session_spawned"). Stored in `loop_events`.                                                                                                                                                                                                                                                                                                                                                                                                              |
| **Loop artifact**    | A piece of evidence produced during a loop (diff, patch, test output). Stored in `loop_artifacts`.                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| **Verifier run**     | A verification step within a loop — either a shell command (`verifier_type = "command"`) or an agent session (`verifier_type = "agent"`). Tracks exit code and output path. Stored in `verifier_runs`.                                                                                                                                                                                                                                                                                                                             |
| **Loop strategy**    | A freeform identifier defining how a loop orchestrates its sessions (e.g., "maker-verifier", "multi-agent"). When a matching loop recipe exists, the strategy resolves to the recipe and the loop is driven by the recipe tick runtime.                                                                                                                                                                                                                                                                                            |
| **Loop recipe**      | A declarative YAML definition (`planeai.loop.recipe.v1`) describing a reusable loop workflow — roles, steps, knowledge, tools, and policy. Discovered from project (`.planeai/loops/`), user (`~/.config/planeai/loops/`), or builtin sources. See `docs/LOOP_RECIPES.md`.                                                                                                                                                                                                                                                         |
| **Recipe snapshot**  | A runtime copy of a resolved recipe plus inputs, tick counter, and created session IDs. Stored in `policy_json` on the loop run. Includes `recipe_name`, `recipe_description`, and `input_defs` from the source recipe for UI display without re-resolving the recipe file. The recipe tick runner reads and updates it on each tick. Tracks `last_activity_at` for stale detection (refreshed on meaningful activity: handoff, verifier, new output) and per-session `session_observations` with cursor-based heartbeat tracking. |
| **Loop trigger**     | A typed event (`LoopTrigger`) that drives loop status transitions via a declared state machine (`loop_run::apply`). Callers declare what happened (e.g., `Start`, `Cancel`, `HandoffReceived`); the transition table decides the resulting state. Replaces direct status assignment.                                                                                                                                                                                                                                               |

## Session lifecycle (v1)

```
create → active → exited → deleted
```

- **Active** — PTY is connected and the agent process is running. Visible in sidebar.
- **Exited** — agent process terminated. Terminal buffer is frozen (read-only). User can restart or delete. Detected via PTY EOF (tmux) or daemon exit event (daemon).
- **Deleted** — session removed from sidebar and DB. For tmux sessions, the tmux session is killed. For daemon sessions, a kill command is sent to the daemon. Irreversible.

### Launch-failure rollback

If session creation fails after git branch/worktree setup (e.g., daemon spawn fails, DB write fails), the launch command automatically rolls back:

- **Worktree mode** — calls `cleanup_worktree()` to remove the worktree directory and branch.
- **Checkout mode (new branch)** — checks out the previous branch and deletes the newly created branch.

Rollback is best-effort: errors are logged as warnings but do not propagate. If the daemon connection fails with "Broken pipe", "Connection refused", or "No such file", the stale connection is cleared so the next attempt auto-reconnects to a restarted daemon.

## Architecture notes

- **Tauri v2** (Rust backend + webview frontend)
- **Svelte 5** with runes for reactive UI
- **xterm.js** for terminal rendering
- **Tailwind CSS v4** with custom `@theme` block mapping CSS custom properties to utility classes
- **Custom theming** via CSS files in `~/.config/planeai/themes/`. Theme file defines UI, terminal, and editor tokens for both light and dark modes.
- **SQLite via rusqlite** on the Rust backend for persistence
- **planeai-plugin-jira** executable for Jira Cloud connection ownership (OAuth 2.0 PKCE, plugin-scoped settings and backend-only secrets); manual configured-source synchronization imports and updates tasks and the Jira sidebar; writeback and periodic sync remain deferred
- **tmux** for optional process persistence (explicit opt-in; see Session backend)
- **planeai-daemon** for built-in process persistence (no external dependencies)
- **portable-pty** for PTY management (tmux-attach goes through a local PTY; daemon sessions are managed directly by the daemon process)
- **Tauri IPC** (commands + typed event channels) for PTY byte streaming between Rust and frontend
- **Typed API layer** (`src/lib/api.ts`) — all `invoke()` calls consolidated behind domain-grouped typed methods; components never call `invoke()` directly (see ADR-0009)
- **pnpm** for package management
- **Loop status derivation** — for recipe-driven loops, `LoopStatus` is derived from the recipe step pointer (`snapshot.runtime.current_step`), never set independently by recipe executors. `persist_snapshot` is the single choke point: it serializes the snapshot, derives status from the current step kind (with `status_override` for blocking cases), and writes both atomically. Lifecycle transitions (`Start`, `Cancel`, `Approve`, `HandoffReceived`) still use `transition_loop` since they operate outside the recipe tick. The `current_round` column is deprecated (kept for schema compat) — round lives only in `snapshot.runtime.round`.

## Key constraints

- Keyboard-first — all actions reachable without mouse
- Multiple sessions allowed per project in both checkout and worktree modes; inline warning shown when creating additional checkout sessions
- DB is source of truth; orphan tmux sessions are ignored
- tmux is optional — app works without it (daemon fallback)
- Cross-platform: macOS and Windows (core functionality parity; tmux gracefully unavailable on Windows)
- Project names must be unique

## Cross-platform strategy

| Concern               | macOS                                             | Windows                                                |
| --------------------- | ------------------------------------------------- | ------------------------------------------------------ |
| **Session backend**   | tmux (persistent) or daemon (persistent)          | daemon only (tmux unavailable)                         |
| **Notification IPC**  | Unix socket (`notify.sock`)                       | Named pipe (`\\.\pipe\planeai-notify`)                 |
| **Daemon IPC**        | Unix socket (`daemon.sock` in XDG_RUNTIME_DIR)    | Named pipe (`\\.\pipe\planeai-daemon`)                 |
| **Stop hook**         | Bash script (`.sh`) via `nc -U`                   | PowerShell script (`.ps1`) via `NamedPipeClientStream` |
| **Config dir**        | `$XDG_CONFIG_HOME/planeai` or `~/.config/planeai` | `%APPDATA%\planeai`                                    |
| **Home dir**          | `$HOME`                                           | `$HOME` or `%USERPROFILE%`                             |
| **Platform modifier** | Cmd (⌘)                                           | Ctrl                                                   |
| **Default font**      | Menlo                                             | Cascadia Mono                                          |
| **Title bar padding** | Left (traffic lights)                             | Right (caption buttons)                                |
| **Font enumeration**  | font-kit (cross-platform)                         | font-kit (cross-platform)                              |
| **Window style**      | Overlay title bar                                 | Overlay title bar (Tauri handles caption buttons)      |
| **Subprocess spawn**  | `raise_fd_limit()` via `pre_exec` (daemon child)  | `CREATE_NO_WINDOW` flag via `no_window()` helpers      |
| **FD soft limit**     | Raised to min(hard, 10240) at startup             | N/A (Windows has no equivalent low default)            |

## Notification IPC events

The notify socket (`notify.sock` / `\\.\pipe\planeai-notify`) accepts JSONL messages. Each message has an `event` field and a `session_id` field.

| Event               | Direction        | Payload                                            | Purpose                                                        |
| ------------------- | ---------------- | -------------------------------------------------- | -------------------------------------------------------------- |
| `stop`              | Hook → GUI       | `{"event":"stop","session_id":"..."}`              | Agent finished (debounced idle detection)                      |
| `notification`      | Hook → GUI       | `{"event":"notification","session_id":"..."}`      | Agent needs human attention                                    |
| `busy`              | Hook → GUI       | `{"event":"busy","session_id":"..."}`              | Agent started working                                          |
| `session_created`   | CLI/Daemon → GUI | `{"event":"session_created","session_id":"..."}`   | New session created, GUI should refresh                        |
| `session_changed`   | CLI → GUI        | `{"event":"session_changed","session_id":"..."}`   | Session state changed (archived/destroyed), GUI should refresh |
| `session_restarted` | Backend → GUI    | `{"event":"session-restarted","session_id":"..."}` | Exited session restarted, GUI updates status to active         |

For tmux-backend sessions, the CLI sends prompts directly via `tmux send-keys -l` without going through the GUI.
For daemon-backend sessions, the CLI sends prompts via the daemon data connection (FRAME_INPUT).

### Prompt locking

Prompts are serialized per session via a SQLite-backed cross-process lock (`prompt_locks` table). Before sending a prompt, the caller acquires a lock keyed by `session_id`. If another process already holds the lock, the request fails immediately with a "session prompt already in progress" error. The lock is always released after the prompt send completes (success or failure). Stale locks older than 2 minutes are automatically cleaned up on acquisition attempts.

This guarantees that concurrent prompts to the same session cannot interleave, regardless of whether they originate from the GUI, CLI, or AXI. Concurrent prompts to different sessions proceed independently.

### Session reads (incremental / cursor-based)

The AXI session read command supports two modes:

1. **Tail mode** (default): `planeai-cli axi session read <id> --lines N` — returns the last N lines.
2. **Cursor mode**: `planeai-cli axi session read <id> --after <cursor> [--max-bytes N]` — returns only output produced since the cursor.

**Cursor format** — opaque strings, backend-specific:

| Backend | Format                     | Semantics                                                                               |
| ------- | -------------------------- | --------------------------------------------------------------------------------------- |
| daemon  | `daemon:<u64_byte_offset>` | Monotonic byte offset from ring buffer. O(1) incremental reads.                         |
| tmux    | `tmux:<line_count>:<hash>` | Line count + content hash of first 5 and last 10 lines. Used to detect history rolloff. |
| local   | —                          | Not supported. Returns an error.                                                        |

**Cursor-mode TOON output**:

```
session_id: <short_id>
backend: daemon | tmux
cursor: <opaque_cursor_for_next_read>
truncated: false | true
text: <new_output_since_cursor>
```

- `truncated: true` means data was lost between the cursor position and the earliest available content (ring buffer eviction for daemon, history rolloff for tmux). The cursor is reset to the current position.
- `--max-bytes` caps the returned text (0 = unlimited). The cursor advances only to cover the content actually returned, so remaining data is delivered on the next poll.
- Agents should persist the `cursor` value and pass it back on the next `--after` call to receive only new output.

**Workflow for loop observation**:

```bash
# Initial read — get current output + cursor (use backend-appropriate zero cursor)
OUTPUT=$(planeai-cli axi session read $CHILD --after "tmux:0:0")
CURSOR=$(echo "$OUTPUT" | grep "^cursor:" | cut -d' ' -f2)

# Poll loop — only new output each iteration
while true; do
  RESULT=$(planeai-cli axi session read $CHILD --after "$CURSOR")
  CURSOR=$(echo "$RESULT" | grep "^cursor:" | cut -d' ' -f2)
  TEXT=$(echo "$RESULT" | sed -n '/^text:/,$ p' | tail -n +2)
  if [ -n "$TEXT" ]; then
    # Process new output...
  fi
  sleep 5
done
```

## Session backend

### Resolution

The effective backend is resolved once at app startup:

```
config.session_backend ?? "local"
```

- Config field absent → local (always the default)
- `"session_backend": "tmux"` → force tmux (warn if not found)
- `"session_backend": "daemon"` → force daemon (experimental)

Setting changes affect new sessions only. Existing sessions keep their backend.

### Backend comparison

| Feature      | local (default)       | tmux                         | daemon                                                 |
| ------------ | --------------------- | ---------------------------- | ------------------------------------------------------ |
| Persistence  | Dies with app         | Survives app quit            | Survives app quit                                      |
| Dependencies | None                  | Requires tmux binary         | Built-in (planeai-daemon binary)                       |
| CLI headless | N/A                   | Works (tmux manages process) | Works (daemon spawned on-demand)                       |
| Scrollback   | xterm.js buffer       | Managed by tmux              | Ring buffer (configurable via daemon_scrollback_bytes) |
| Platform     | macOS, Linux, Windows | macOS, Linux                 | macOS, Linux, Windows                                  |

### PTY architecture

For tmux backend, a local PTY is spawned via `portable-pty` running `tmux attach-session -t <name>`. The tmux session is created beforehand with the agent command.

For daemon backend, the daemon process owns the PTY directly. The GUI/CLI communicates with the daemon via its IPC socket (control for spawn/kill/list, data for I/O streaming).

### Shell tabs

Each session can have multiple tabs. Tab 0 is the agent; tabs 1+ are shell tabs.

- **Daemon backend**: shell tabs are spawned in the daemon with composed ID `{session_id}:{tab_index}`. They receive the same augmented PATH environment as agent sessions (via `build_daemon_env`). They persist across app restarts (same as the agent session). On archive/destroy, all shell tabs are killed alongside the agent session.
- **Tmux backend**: shell tabs use a local PTY (`PtyTarget::Shell`) and are ephemeral — they die with the app.
- **Dynamic tab titles**: shell tabs listen for OSC title changes from the terminal. When the shell reports a running command (e.g., `vim`, `cargo`), the tab label updates to show that command name (`src/lib/shell-title.ts` extracts the binary name, filtering out shell resets and cwd paths). Tabs with a custom title are preserved across relabeling.

### Exit detection

- **tmux**: PTY reader thread gets EOF → emit `pty-exited` → mark session as `exited` in DB.
- **daemon**: daemon broadcasts an `exited` event on the control socket → GUI marks session exited.

### Startup reconciliation (one-time, not polling)

- Local sessions that were `active` → mark `exited` (local sessions cannot survive app restart)
- Tmux sessions that were `active` → check `tmux has-session`; if false → mark `exited`; if true → leave as-is
- Daemon sessions → left as-is (daemon manages their lifecycle)

### Restart

Exited sessions can be restarted: same session identity (name, project, worktree), clean terminal buffer, status returns to `active`. For tmux, creates a new tmux session with the same name. For daemon, sends a spawn command to the daemon. For local, the session status is restored and a new PTY is spawned on attach.

Provider resume is attempted on restart: if `resume_command` is set → use that; otherwise → fresh provider command. If resume fails, automatically falls back to fresh launch.

Selecting an exited session triggers restart automatically. The terminal pool activation is deferred until restart completes, preventing attach-to-exited-session loops (especially on daemon backend).

### DB columns

- `backend TEXT NOT NULL DEFAULT 'tmux'` — set at creation time, values: `'local'`, `'tmux'`, or `'daemon'`
- `status TEXT NOT NULL DEFAULT 'active'` — updated on exit/delete
- `tmux_name TEXT` — NULL for daemon sessions, populated for tmux sessions
- `attached_once INTEGER NOT NULL DEFAULT 0` — set to 1 on first attach; determines whether attach runs launch command (0) or resume command (1)
- `parent_session_id TEXT` — optional reference to the session that spawned this one (for orchestration/parent-child tracking)

### Session tree inspection

Parent/child relationships are observable via `session children` and `session tree` commands:

- `planeai-cli session children <id>` — lists direct child sessions (JSON output)
- `planeai-cli session tree <id>` — walks up to root, returns full tree in BFS order (JSON output)
- `planeai-cli axi session children <id>` — direct children (TOON output)
- `planeai-cli axi session tree <id>` — full tree from root (TOON output)

**Design notes**:

- Child sessions are linked for observability only. Cleanup remains explicit — killing a parent does not automatically kill children.
- Future loop runs may own cleanup policy (cascading kill on parent exit).
- `tree` always walks to the root first, then returns the full subtree in BFS order. If the parent referenced by `parent_session_id` no longer exists (deleted/dangling), the walk stops and the orphan becomes the effective root.
- `session ls` (TOON) now includes `parent_session_id` in the tabular output.

### Preferences UI

Dropdown: Local (default) / tmux / Daemon (experimental). Inline warning if user selects tmux but binary not found.

## Worktree support

Sessions can run in two modes:

- **Checkout mode** (default) — `git checkout` on the project's repo path. Only one active checkout session per project.
- **Worktree mode** — `git worktree add` creates an isolated working copy at `~/.planeai/worktrees/<project-name>/<session-id>/`. Multiple worktree sessions can run in parallel on the same project.

### Lifecycle

- **Archive** — kills tmux/daemon session, marks session archived. Worktree directory is preserved on disk.
- **Destroy** — kills tmux/daemon session, runs `git worktree remove --force`, deletes directory, removes from DB.

### Cleanup safety

Session cleanup only deletes worktrees and branches for **loop-managed branches** (those whose name starts with `loop/`). User-created branches (e.g., `feature/my-branch`) are never removed, even if a loop session was pointed at them via a step's `branch` field. This prevents accidental deletion of work that exists independently of a loop.

### Branch redirect

When a `session.create` step specifies an existing branch (via the step's `branch` field) and that branch is already checked out in another worktree, the session creation redirects to the existing worktree path rather than failing. The session records that worktree path for gate execution and agent work, but cleanup will not delete it (see cleanup safety above).

### Data model

Sessions table has `worktree_path TEXT NULL`. Non-null indicates worktree mode.

### Form flow (worktree mode)

Project → Session name → ✅ Create worktree → Base branch (existing) → New branch name (editable, defaults to session name slugified)

When a task is selected (via sidebar pick or task picker in the form), the task's `base_branch` field auto-fills the base branch selector.

## Logging

Uses the `tracing` crate with a rolling daily file appender. Logs go to `<app_data_dir>/logs/planeai.log`.

- **Initialization**: `planeai::logging::init(&log_dir)` — returns a `WorkerGuard` that must be held for the app lifetime.
- **Both binaries init logging**: the Tauri app (`main.rs`) and the CLI (`bin/cli.rs`).
- **Filter**: `RUST_LOG` env var (defaults to `info`).
- **Convention**: Use `tracing::info!` for happy-path events, `tracing::warn!` for recoverable failures, `tracing::error!` for unrecoverable failures. Include relevant identifiers (session_id, tmux_name) as structured fields.

## Performance

Performance is critical — planeai is a real-time terminal multiplexer. The UI must never stutter or freeze.

**Cardinal rule: never block the main thread.**

- All Tauri commands that perform I/O (subprocess calls, network, filesystem) **must** be `async` and use `tokio` (e.g., `tokio::process::Command`, `tokio::fs`).
- Release Mutex locks **before** awaiting I/O — hold locks only for in-memory reads/writes.
- Synchronous `std::process::Command` is forbidden in Tauri commands. Use `tokio::process::Command` instead.
- On Windows, all subprocess spawns must use `planeai_core::command::no_window()` (sync) or `no_window_tokio()` (async) to suppress console window flashes from the GUI.
- Frontend polling intervals should be reasonable (≥30s for non-critical data) and stop when data is no longer needed (tab hidden, task complete).
- Batch IPC calls where possible — prefer one `invoke` returning a list over N individual calls.

## Architecture decisions

See `docs/adr/` for recorded decisions.

## Loop Runs UI

The loop system (CLI-based via `planeai-cli axi loop ...`) has a corresponding UI surface for human visibility and control.

### Entry points

- **Sidebar**: Loops render inside each project (above orphan sessions and tasks). Each loop shows a status dot, strategy label, round counter, and hover-revealed quick action buttons (start, tick, stop). Loop sessions are nested as collapsible children under their parent loop — they do not appear as orphans. Loops also participate in the MRU tab switcher (Ctrl+Tab) and session navigation (next/prev) as `loop:<id>` entries.
- **Dashboard**: Selecting a loop in the sidebar shows `LoopDashboard` in the main content area. For recipe-driven loops, displays a step-centric `LoopTimeline` showing recipe definition (collapsible), each step with status/progress, associated sessions, verifier runs, and artifacts inline. For non-recipe loops, shows flat sections for goal, sessions table, verifier runs, artifacts, and events. Keyboard shortcuts: `R` refresh, `S` start (draft only), `T` tick (active loops), `X` stop (active loops), `1`–`9` open session by index.
- **Create form**: `Cmd+N` → `l` opens `LoopForm` — project picker (when multiple projects), goal, recipe picker, task link, base branch, max rounds, draft checkbox. Keyboard-driven (p=project, g=goal, r=recipe, t=task, b=base branch, m=max rounds, d=toggle draft, Mod+Enter=submit, Esc=cancel).
- **Actions**: Refresh, Tick (fire-and-forget), Stop from the dashboard. Quick tick/stop from sidebar hover.

### State management

- `src/lib/loop-store.svelte.ts` — reactive store with `refreshAllLoops()`, `setActiveLoopId()`, `getSessionsForLoop()`, `getLoopIdForSession()`, and `startLoopEventListener()`. Eagerly fetches detail for non-draft loops to maintain session-to-loop mappings.
- Backend emits `loop-state-changed` Tauri event on mutations (create, tick, stop). Frontend also listens to `sessions-changed` and `agent-state-change` (debounced 2 s) to catch auto-advance ticks triggered by agent handoffs.

### Backend commands

Seven async Tauri commands in `src-tauri/src/commands/loops.rs`:

- `list_loop_runs(project_id)` → `Vec<LoopRunSummary>`
- `get_loop_run_detail(loop_id)` → `LoopRunDetail` (sessions, events, artifacts, verifier runs, recipe snapshot)
- `list_loop_recipes(project_id)` → `Vec<RecipeSummary>`
- `create_loop_run(project_id, goal, recipe_id, task_key, max_rounds, base_branch, start)` → `LoopRunSummary` — creates a loop and auto-ticks immediately when `start` is true
- `tick_loop(loop_id)` → auto-advances through immediately-executable steps until a wait/terminal state
- `stop_loop(loop_id)` → transition to cancelled
- `start_loop(loop_id)` → transition to running and auto-tick

All commands use `blocking()` for DB access to avoid blocking the main thread.

## Adding a new ui/ primitive

1. Create `src/components/ui/MyComponent.svelte`
2. Wrap the relevant bits-ui component (or plain HTML) with Skeleton token classes
3. Use `dark:` variants for all color utilities (e.g., `bg-surface-50 dark:bg-surface-900`)
4. Accept a `class` prop for consumer overrides
5. Export from `src/components/ui/index.ts`
6. Use the primitive in feature components — never hardcode colors inline
