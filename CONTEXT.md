# planeai

A cross-platform agent session orchestrator. Manages multiple AI coding agents running in parallel, each in its own terminal session. Supports three session backends: local (in-process PTY, default), tmux (persistent, requires tmux binary), and daemon (persistent, built-in, experimental).

## Glossary

| Term                  | Definition                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Project**           | A git repository registered with planeai. Stores a repo path and display name. The top-level organizational unit. `path_missing` is derived on every read (the folder is gone, e.g. moved in Finder) and never stored. A missing project shows a warning in the sidebar; **Locate folder…** relinks it through `update_project`, which requires a `.git`, runs `git worktree repair` from the new folder for every worktree session (nothing is saved if one fails), then saves the path. While the folder is missing, launches, restarts, loop start/tick and auto-dispatch are refused or skipped with `Project folder not found. Locate it first.`                                                                                                                                                                                                                      |
| **Session**           | A single agent working on a single task within a project. Backed by a local PTY (default), tmux session, the planeai daemon, or a plugin provider. Its agent tab is a terminal running the agent CLI, or the provider's UI.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| **TaskWorkspace**     | The persistent main-pane workspace for one task. It owns the task-scoped tab/split layout across linked agent sessions, shells, editors, and diffs; the focused agent session supplies the default filesystem context for new resources. A linked agent session may run in another project's repo: `sessions.task_project_id` records the task's owning project, and the workspace is keyed by `task_project_id ?? project_id`.                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| **Session backend**   | The process hosting strategy for a session: `local` (in-process PTY, default), `tmux` (survives app quit, requires tmux binary), `daemon` (survives app quit, built-in, experimental), or `plugin` (a plugin provider runs the agent; see Provider session). Resolved at session creation from the global setting, except `plugin`, which follows from choosing a runtime-backed provider.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| **Terminal view**     | The live terminal for one pty key: an xterm instance with its buffer, geometry, and PTY attachment. It outlives any container: the layout mounts and unmounts it, and it is disposed only when its tab closes. _Avoid_: terminal component, terminal pane (for this object).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| **Pane**              | One leaf of a TaskWorkspace layout: an ordered list of tabs with one in front. A single pane shows its tabs in the titlebar; split panes each have a tab bar. Inside the layout tree and its persisted format a pane is a leaf (`LeafNode`); elsewhere say pane. _Avoid_: split (for this object).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| **Pty key**           | The identity of a TaskWorkspace tab, shared with the backend and persisted in layouts: `<sessionId>`, `<sessionId>:<index>`, `<sessionId>:diff` or `<sessionId>:editor:<filePath>`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| **Terminal tab**      | A tab the layout opens on a PTY: a shell, a terminal editor or a provider handoff program. Its lifecycle (reserved, spawning, live, closing, gone) is owned by the backend, which hands out its index and reports its end once as `TabEnded`; see ADR-0015. _Avoid_: shell tab (for editors and handoffs).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| **Provider**          | An AI coding agent a session runs. A **command provider** (e.g., Kiro, Claude Code, Copilot, Codex, Aider) is defined in the config file by a base `command`, optional `yolo_flag`, optional `resume_command` for session resume. A **runtime-backed provider** is declared by a running plugin (key `<plugin id>:<provider id>`, ADR-0014). Multiple providers can be available; one configured provider is the `default_provider`.                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| **Provider session**  | A session whose runtime is a plugin provider (`plugin` backend). The host keeps its worktree, task link, lifecycle and status display; the plugin runs the agent, owns the conversation, and renders its UI in place of the terminal. Its status comes only from the provider. See ADR-0014.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| **Config file**       | The single source of truth for all user preferences and provider definitions. Lives at `$XDG_CONFIG_HOME/planeai/config.json` (default `~/.config/planeai/config.json`). JSONC for reading, pretty JSON for writing.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| **Onboarding**        | First-run setup wizard (`src/components/Onboarding.svelte`) that takes over the main window while `onboarding_completed` is `false`: agents found on the session PATH (pre-selected), projects folder, then appearance. Each step saves on Continue; Skip and Finish set the flag to `true`, and app shortcuts are suspended meanwhile. A config file created on first launch starts at `false`; one written before the field existed loads as `true`, so existing users never see it. **Preferences → General → Setup → Run setup again** resets the flag.                                                                                                                                                                                                                                                                                                                |
| **Settings registry** | `src/lib/settings-registry.ts`: one entry per user-facing setting (category, section, label, keywords, and for scalar settings the effective value and reset patch). It drives Preferences search, deep links (`?page=preferences&section=<category>#<setting>` or `&plugin=<pluginId>:<contributionId>`), and per-setting reset against `Config::default()` from `get_config_defaults`. A new setting needs a registry entry and a `SettingRow` on its category page.                                                                                                                                                                                                                                                                                                                                                                                                     |
| **Yolo mode**         | A per-session toggle that appends the provider's `yolo_flag` to the launch command, enabling auto-approval of tool use. Disabled if the provider has no `yolo_flag`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| **Task start**        | Starting a session for a task, owned by the backend `task_start` module: the provider (the chosen one or `default_provider`; a plugin provider without auto-approve forces it off), the branch, session name and prompt from `task_management.templates` or their defaults, the launch, and the move to `in_progress`. TaskForm's "Start session immediately" calls it through `start_task_session`; a plugin's `host.tasks.create` with `start` runs it in the background, at most once per task, recording the start on `plugin_task_operations` until it settles and resuming a pending one at app launch. With the `local` backend the agent spawns only when a terminal view attaches, so a background start waits until the user opens the session; the prompt waits on the session row (`pending_prompt`) and reaches that first spawn, even across an app restart. |
| **Focus zone**        | A region of the UI that can receive keyboard input: sidebar or terminal. App-level chords (Cmd/Ctrl+B, Cmd/Ctrl+N, Cmd/Ctrl+Shift+P, Cmd/Ctrl+1-9, Ctrl+Tab, Escape) are always intercepted regardless of which zone has focus.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| **Sidebar grouping**  | How the sidebar organizes tasks: by `project` (default; each project holds its loops, unlinked sessions and status groups) or by `status` (a Sessions section with every loop and unlinked session, then one cross-project section per task status). Stored as `sidebar_group_by`. Rendering, keyboard navigation and session cycling all derive their order from `buildSidebarModel` (`src/lib/sidebar-model.ts`). Project actions are only reachable when grouped by project.                                                                                                                                                                                                                                                                                                                                                                                            |
| **Form keyboard**     | A vim-like normal/insert mode controller (`createFormKeyboardController`) used by modal forms. Normal mode maps single-key mnemonics to field focus or toggle actions. Insert mode is entered on text field focus; Escape returns to normal; Escape in normal dismisses the form. `FormDialog` owns the form's initial focus: it passes `initialFocusSelector="[data-form-keyboard]"` to the shared `Dialog` primitive, which focuses that wrapper instead of letting bits-ui's focus scope grab the first tabbable field — that would skip normal mode and put `document.activeElement` outside the wrapper the app's Escape router keys off. Forms therefore do not focus themselves, except `TaskForm`, which deliberately claims its title input in a `requestAnimationFrame` and so lands after `Dialog`'s synchronous wrapper focus.                                 |
| **Tab switcher**      | An MRU overlay triggered by holding Ctrl+Tab. Each subsequent Tab moves selection; releasing Ctrl confirms. Includes both sessions and active loop dashboards (`loop:<id>` entries).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| **Notification**      | (future) A signal that an agent needs human attention.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| **Token**             | A semantic CSS custom property defined in the active theme file (e.g., `--color-surface-200`, `--terminal-background`). Mapped to Tailwind utilities via `@theme` block in `app.css`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| **Primitive**         | A reusable styled Svelte component in `src/components/ui/` that wraps bits-ui behavior (for complex interactives) or provides app-specific defaults (Button, Input). The building block for feature components.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| **Theme mode**        | One of three states: `system`, `light`, `dark`. Persisted in localStorage. Controls which color palette is active.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| **Daemon**            | (Experimental) A background process (`planeai-daemon`) that manages session PTYs. Spawned on-demand by the CLI or GUI. Sessions survive indefinitely as long as the daemon is running.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| **rmux**              | (Experimental) An opt-in persistent session backend, selected via `session_backend: "rmux"`. Intended to eventually replace `planeai-daemon`. One rmux session per TaskWorkspace; one rmux pane per terminal tab. Destroyed only on explicit task deletion. See ADR-0012.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| **AXI**               | Agent eXperience Interface — a CLI subcommand (`planeai-cli axi`) that outputs TOON instead of JSON, optimised for autonomous agent consumption. Covers task, session, project, and loop operations.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| **TOON**              | A token-efficient text output format used by the AXI interface. Supports object fields, tabular arrays, and primitive arrays with minimal overhead. Implemented in the `planeai-toon` crate.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| **Jira integration**  | Optional Jira Cloud connection managed by the bundled Jira plugin. The plugin stores its site and configured JQL sources under its own settings namespace, OAuth credentials under its backend-only secrets namespace, and sync membership/cache state in its plugin database. OAuth 2.0 (PKCE) is used for authorization. Immediate periodic configured-source sync imports Jira issues as PlaneAI tasks, updates the Jira sidebar, and queues globally departed unresolved issues for review; selecting an issue can create a child task in a PlaneAI project; Jira writeback is supported for configured lifecycle actions.                                                                                                                                                                                                                                             |
| **Loop run**          | A durable orchestration layer above sessions. Tracks rounds of agent work, verification, and human review. Optionally linked to a creating session via `created_by_session_id` (nullable). Persisted in `loop_runs` table via `planeai_core::loop_service::LoopService`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| **Loop session**      | A session enrolled in a loop run with a strategy-specific role (e.g., "maker", "verifier"). Tracked in `loop_sessions` with composite key `(loop_id, session_id)`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| **Loop event**        | An ordered, append-only log entry for a loop run (e.g., "round_started", "session_spawned"). Stored in `loop_events`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| **Loop artifact**     | A piece of evidence produced during a loop (diff, patch, test output). Stored in `loop_artifacts`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| **Verifier run**      | A verification step within a loop — either a shell command (`verifier_type = "command"`) or an agent session (`verifier_type = "agent"`). Tracks exit code and output path. Stored in `verifier_runs`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| **Loop strategy**     | A freeform identifier defining how a loop orchestrates its sessions (e.g., "maker-verifier", "multi-agent"). When a matching loop recipe exists, the strategy resolves to the recipe and the loop is driven by the recipe tick runtime.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| **Loop recipe**       | A declarative YAML definition (`planeai.loop.recipe.v1`) describing a reusable loop workflow — roles, steps, knowledge, tools, and policy. Discovered from project (`.planeai/loops/`), user (`~/.config/planeai/loops/`), or builtin sources. See `docs/LOOP_RECIPES.md`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| **Recipe snapshot**   | A runtime copy of a resolved recipe plus inputs, tick counter, and created session IDs. Stored in `policy_json` on the loop run. Includes `recipe_name`, `recipe_description`, and `input_defs` from the source recipe for UI display without re-resolving the recipe file. The recipe tick runner reads and updates it on each tick. Tracks `last_activity_at` for stale detection (refreshed on meaningful activity: handoff, verifier, new output) and per-session `session_observations` with cursor-based heartbeat tracking.                                                                                                                                                                                                                                                                                                                                         |
| **Loop trigger**      | A typed event (`LoopTrigger`) that drives loop status transitions via a declared state machine (`loop_run::apply`). Callers declare what happened (e.g., `Start`, `Cancel`, `HandoffReceived`); the transition table decides the resulting state. Replaces direct status assignment.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |

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
- Paths stored for a project (verifier logs, loop artifacts and events, recipe snapshot paths, editor tabs) are relative to the project root, or the session working dir for editor tabs, when they live under it, and absolute otherwise (`planeai_core::project_path`). They are resolved at the read boundary, so the UI and APIs still see absolute paths. A worktree session that lands in the main checkout stores no `worktree_path`.

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

### Agent notification hooks

planeai installs a small notify script into each supported agent's own config so the agent reports `busy`, `stop` and `notification` instead of relying on silence detection.
`AgentKind` (`planeai-core/src/agent_hooks.rs`) identifies the agent from a provider command's executable name and owns detection, installation and the startup script refresh.

| Agent   | Config written                                | Events -> signal                                                                                       |
| ------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| Kiro    | `~/.kiro/agents/default.json`                 | `userPromptSubmit` -> busy, `stop` -> stop                                                             |
| Claude  | `~/.claude/settings.json`                     | `UserPromptSubmit`/`PostToolUse` -> busy, `Stop` -> stop, `Notification`/`StopFailure` -> notification |
| Copilot | `$COPILOT_HOME/hooks/planeai-notify.json`     | `userPromptSubmitted` -> busy, `agentStop` -> stop, `errorOccurred` -> notification                    |
| Codex   | `$CODEX_HOME/hooks.json` (default `~/.codex`) | `UserPromptSubmit`/`PostToolUse` -> busy, `Stop` -> stop                                               |

`PostToolUse` flips a session back to busy after a permission prompt is approved, since PTY output does not override the idle state of a hook-enabled session.
Notify scripts ignore any event they do not map explicitly, so a hook config newer than its script can never flood the user with alerts.
Codex `PermissionRequest` is not hooked: with `approvals_reviewer = "auto_review"` it fires for every escalated command even though no human is needed.
Codex only runs hooks the user has trusted: it asks on the first launch after installation.
Until the hooks are trusted, Codex sessions stay busy because hook-enabled sessions skip silence detection.
The Codex hook command keeps `$HOME` unexpanded so a `hooks.json` kept in dotfiles stays portable, and planeai rewrites the file in place so a symlinked `hooks.json` is preserved.

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
| rmux    | `rmux:<line_count>:<hash>` | Same capture-based strategy as tmux — rmux exposes no byte-offset resume.               |
| local   | —                          | Not supported. Returns an error.                                                        |

**Cursor-mode TOON output**:

```
session_id: <short_id>
backend: daemon | tmux | rmux
cursor: <opaque_cursor_for_next_read>
truncated: false | true
text: <new_output_since_cursor>
```

- `truncated: true` means data was lost between the cursor position and the earliest available content (ring buffer eviction for daemon, history rolloff for tmux and rmux). The cursor is reset to the current position.
- `--max-bytes` caps the returned text (0 = unlimited). The cursor advances only to cover the content actually returned, so remaining data is delivered on the next poll.
- Agents should persist the `cursor` value and pass it back on the next `--after` call to receive only new output.

**Workflow for loop observation**:

```bash
# Initial read — get current output + cursor (use backend-appropriate zero cursor)
OUTPUT=$(planeai-cli axi session read $CHILD --after "tmux:0:0")
# TOON quotes any value containing a colon, so the printed cursor is quoted.
# Passing it back verbatim works — the parsers strip the quotes.
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

### TaskWorkspace layout

The tabs and splits of the loaded TaskWorkspace live in one module, `src/lib/task-workspace-layout.svelte.ts` (`taskWorkspaceLayout`).
It owns the layout, which workspace it belongs to, and loading and saving it; it places terminal tabs the backend opened and drops those the backend reports ended.
`App.svelte` only reacts to what its operations report: focus, snackbars, session selection, and parking an agent whose tab was closed.

- **Layout tree**: `src/lib/layout-tree.ts` is the pure value behind it, a strict binary tree of splits whose leaves (panes) hold tabs. Every operation returns a new layout, or its input when nothing changed.
  Its JSON is the persisted `layout_json`, and `restoreLayout` repairs what older versions saved (missing tab types, a stale focused pane, repeated pty keys).
- **Pty key**: the identity of a tab, shared with the backend. `src/lib/pty-key.ts` owns the grammar: `<sessionId>` (agent), `<sessionId>:<index>` (shell, index ≥ 1), `<sessionId>:diff`, `<sessionId>:editor:<filePath>`.
- **Pty key uniqueness**: tabs render keyed by pty key, so a duplicate crashes the whole workspace (`each_key_duplicate`). The layout tree enforces at most one tab per pty key at every entry point: adding an existing key updates it in place, and creating or restoring a layout drops repeats.
- **Reconciliation**: when the workspace's sessions change, each new agent gets a tab in the focused pane and every tab of a session that left is dropped. The front tab only moves for the session the workspace was shown for.
- **Single pane vs split**: a single pane shows its tabs in the titlebar; once split, each pane has its own tab bar. Both render the same pane tabs and call the same operations.
- **Split shortcuts**: they act only on a visible layout, never behind a loop dashboard, a plugin page or an empty TaskWorkspace.
- **Empty TaskWorkspace description**: rendered as markdown by `MarkdownView.svelte` (`src/lib/markdown.ts`, markdown-it). Descriptions come from agents and plugins, so raw HTML is escaped, images render as links and never load, and link clicks open in the system browser (`http`, `https`, `mailto` only) instead of navigating the webview.
- **Dragging tabs**: a tab dropped on a tab bar is inserted at that position; dropped on a pane, it moves into that pane, or splits it when dropped within a quarter of an edge. Tab dragging uses pointer events (`src/lib/tab-drag.svelte.ts`), because Tauri's native drag-drop handler swallows HTML5 drag events.
- **Dropping files**: a file dragged from the OS onto a pane types its shell-escaped path into the terminal in front of that pane, as terminal apps do. The paths come from Tauri's native drag-drop event.

### Shell tabs

Each agent session can have shell tabs (pty keys `<sessionId>:<index>`).

- **Terminal tab lifecycle (ADR-0015)**: shells, terminal editors and provider handoffs are terminal tabs, owned by the backend's Terminal tabs module (`src-tauri/src/terminal_tabs.rs`).
  Each goes reserved → spawning → live → closing → gone; the layout opens one (`open_tab` with what it runs), its terminal attaches to it (`attach_tab`), and closing it is one call (`close_tab`).
  The backend hands out indices from a counter persisted per session, so a pty key is never reused, across webview reloads and app restarts.
  Every tab reports its end exactly once as a `tab-ended` event (`closed`, `exited`, `failed_to_start` with its error, or `session_ended`), whoever caused it; the layout drops it then, or as soon as its own close succeeds.
  Ended tabs are persisted, so a saved layout still holding one (saved while it ran in a workspace not shown) drops it when it loads (`ended_tabs`).
  Opening a terminal tab saves the layout immediately rather than on the debounce, so its process cannot outlive the record of its tab.
- **Daemon backend**: shell tabs are spawned in the daemon with composed ID `{session_id}:{tab_index}`. They receive the same augmented PATH environment as agent sessions (via `build_daemon_env`). They persist across app restarts (same as the agent session). On archive/destroy, every live daemon session prefixed `{session_id}:` is killed alongside the agent session.
- **Tmux backend**: shell tabs use a local PTY (`PtyTarget::Shell`) and are ephemeral — they die with the app.
- **Closing**: a terminal tab is ended on the backend first and its tab stays if that fails, so no shell keeps running without a tab to reach it. A tab still reserved or starting can be closed; a close during a spawn waits for it, then ends what it started.
- **Terminal editor tabs**: a terminal tab that runs the configured terminal editor is opened with that command, so the terminal that mounts for it starts the editor instead of a bare shell.
- **Dynamic tab titles**: shell tabs listen for OSC title changes from the terminal. When the shell reports a running command (e.g., `vim`, `cargo`), the tab label updates to show that command name (`src/lib/shell-title.ts` extracts the binary name, filtering out shell resets and cwd paths). Tabs with a custom title are preserved across relabeling.
- **Reaching a mounted editor tab**: app-level commands cannot call into an editor tab directly. Each mounted `EditorTab` registers a save handle under its pty key in `src/lib/editor-resources.ts`; `saveActiveEditorResource` resolves the front tab of the focused pane from the layout and saves that one. Vim `:w` takes a separate route through `vim-registry`, which is keyed by `EditorView`.

### Exit detection

- **tmux**: PTY reader thread gets EOF → emit `pty-exited` → mark session as `exited` in DB.
- **daemon**: an exit reaches the GUI two ways: the `exited` event on the control socket, and `FRAME_EOF` plus close on each attached data connection.
  Both feed the `PtyManager` exit sink, whose consumers are idempotent (`pty-exited` sets the status again, a closing terminal tab only records the exit), so whichever arrives second changes nothing.

### Startup reconciliation (one-time, not polling)

- Local sessions that were `active` → mark `exited` (local sessions cannot survive app restart)
- Tmux sessions that were `active` → check `tmux has-session`; if false → mark `exited`; if true → leave as-is
- Daemon sessions → left as-is (daemon manages their lifecycle)

### Terminal views

A Terminal view (`src/lib/terminal-view.ts`) owns one pty key's xterm, buffer, and PTY connection.
Views live in a registry keyed by pty key (`src/lib/terminal-views.ts`).
`Terminal.svelte` is only a slot that mounts and unmounts a view, so TaskWorkspace switches, splits, and tab moves re-parent the view instead of re-creating it.
A view is disposed only when its shell tab closes or its session is deleted, archived, or parked.

- **Geometry invariant**: xterm's size and the PTY's size change together.
  Sizes are measured only while the view is visible; hidden or unmounted views keep receiving output at their frozen size, so showing one again needs no replay and no redraw from the child.
  Showing a view re-asserts its size to the PTY, since another client (a second app instance, an external tmux attach) may have resized it.
  A real size change reaches the child as `SIGWINCH`; the kernel ignores a same-size resize, which is why a re-created view used to stay garbled until the window was resized.
- **Connect on mount**: a view connects (`attach_session` or `attach_tab`) once its surface is opened (font loaded) and it is mounted in the current layout, visible or not; on the local backend the attach is what starts the agent.
  A visible view connects at its measured size.
  A hidden view that was never measured waits briefly (100 ms) for a visible sibling to measure, then connects at the last size any view measured, or at xterm's default 80×24 if none did.
  Either way surface and PTY start at the same size, and both resize together on first show if it was wrong.
- **Failed connections** are reported once and retried only on restart or when the view moves to a new container.
- **WebGL while visible**: only visible views hold a WebGL renderer; browsers cap live WebGL contexts at about 16.

### Restart

Exited sessions can be restarted: same session identity (name, project, worktree), clean terminal buffer, status returns to `active`. For tmux, creates a new tmux session with the same name. For daemon, sends a spawn command to the daemon. For local, the session status is restored and a new PTY is spawned on attach.

Provider resume is attempted on restart: if `resume_command` is set → use that; otherwise → fresh provider command. If resume fails, automatically falls back to fresh launch.

Selecting an exited session triggers restart automatically.
The session stays `exited` until restart completes, and a Terminal view never connects while exited, preventing attach-to-exited-session loops (especially on daemon backend).
On restart the same Terminal view resets its buffer and reconnects.

### DB columns

- `backend TEXT NOT NULL DEFAULT 'tmux'` — set at creation time, values: `'local'`, `'tmux'`, or `'daemon'`
- `status TEXT NOT NULL DEFAULT 'active'` — updated on exit/delete
- `tmux_name TEXT` — NULL for daemon sessions, populated for tmux sessions
- `attached_once INTEGER NOT NULL DEFAULT 0` — set to 1 on first attach; determines whether attach runs launch command (0) or resume command (1)
- `pending_prompt TEXT` - the task prompt of a `local` session whose agent has not spawned yet; the attach that spawns the agent takes it in the same database pass, and gives it back only if the spawn fails, so the prompt is delivered at most once
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

Preferences → Sessions offers Local (default) / tmux / Daemon (experimental) / rmux (experimental). An inline warning appears if the selected backend's binary is not found.

### Plugin providers

A plugin with the `providers` capability (host API `planeai.plugin-host.v3`, ADR-0014) offers runtime-backed providers.
Choosing one creates a session with the `plugin` backend; there is no PTY or command.

- Launch creates the worktree, calls `provider.session.start` on the owning sidecar with the working directory, `PLANEAI_SESSION_ID`, `PLANEAI_SOCKET`, the augmented `PATH`, auto-approve and the task prompt, then creates the DB row and commits the launch.
- The agent tab mounts the provider's UI instead of `Terminal`; shell tabs in the same session are ordinary local PTYs.
- Before the UI mounts, `provider_session_ensure` resumes the session if the current sidecar instance does not drive it yet (after an app restart or plugin reload).
  The resume carries `handed_off` from the host's in-memory handoff state, which an app restart clears.
- A request the plugin answers with `-32010` (unknown session) unbinds the session, resumes it and retries once.
  Other refusals reach the session UI as typed `ProviderError { code, message }` values.
- `send_prompt` routes `plugin` sessions through the GUI notify socket like `local` ones; the GUI delivers them with `provider.session.send`.
- `host.providerSession.status` maps onto the notify state machine (`busy`, `idle`, `needs_attention` as an attention notification, `exited`), and notify-socket status for provider sessions is ignored.
  `host.providerSession.event` seqs must keep increasing per session; the host keeps each session's last seq until it stops.
- Archive, destroy and exit stop the provider session on two paths: the session lifecycle dispatch for sessions the GUI ends, and `plugin_providers::reconcile` for sessions the CLI or task completion ends.
  The lifecycle dispatch tells the plugin even when no sidecar drives the session, routing by the session's provider key.
- A provider plugin that starts receives `provider.sessions.reconcile` before it is published as running, listing its sessions that still exist, so it can drop the data of sessions destroyed while it was not running.
- A launch that fails before its session row exists stops the provider it started.
- Restarting an exited provider session only restores the row; the provider resumes it on next use.
- Providers that support `handoff` can continue a session in a terminal tab running their agent's TUI (`provider.session.handoff` returns its argv, run directly without a shell, so the tab closes when the TUI exits); every terminal tab that ends goes through `providerHandoff.tabEnded`, which hands a handoff tab's session back to the provider.
- While a session continues in a terminal, the host refuses provider prompts for it; feedback from the Diff and editor tabs is typed into that terminal instead.
- A session leaving the app ends its handoff terminal's program, so no conversation is driven unseen.
  Archived, destroyed and exited sessions (from the GUI or the CLI) end it on the backend (`TerminalTabs::session_ended`, run by the session lifecycle dispatch and by provider reconciliation); parked ones, and a terminal that opened after its session left, are closed by `providerHandoff.release`.
  A handoff answered without a runnable argv is handed straight back, so it never sits in a terminal that never opened.
- The handoff terminal's tab is marked in the saved layout. After a webview reload its program still runs, so the tab is adopted again and its close still returns the session to its chat; after an app restart the tab is a plain shell and is not adopted.

### rmux (experimental)

`rmux` is an opt-in fourth backend intended to eventually replace `daemon`. It is wired into production code behind the `session_backend: "rmux"` setting, and the daemon binary is currently resolved from `PATH` rather than bundled — sidecar embedding (a pinned, SHA256-verified upstream prebuilt) is deliberately deferred until the backend is at parity with the others. All ten go/no-go probes passed against rmux 0.10.0 on macOS ahead of implementation; Windows is out of scope and unverified. The throwaway spike that ran them has been removed — see ADR-0012 for the full probe results.

rmux is used as a **process host, not a UI**: the pane output stream is byte-exactly the child process's own output (verified by byte-for-byte comparison), the pane occupies the whole window with no status row reserved, and raw control bytes — including rmux's own `0x02` prefix — reach the child rather than a key table. PlaneAI keeps rendering in xterm.

| PlaneAI concept                          | rmux concept                 |
| ---------------------------------------- | ---------------------------- |
| TaskWorkspace (`project_id`, `task_key`) | session                      |
| Terminal tab/resource (`pty_key`)        | window holding a single pane |

A session with no task owns its workspace alone, so it stays isolated and still
gets cleaned up. The `pty_key` → pane mapping lives in the `rmux_resources`
table; pane ids are renewable handles valid for one daemon lifetime, so startup
reconciliation prunes stale rows and marks the affected sessions `exited`.
Terminal tabs find their pane by its window name, set to the `pty_key` when the window is created: spawning adopts it, and reconnecting, closing and archiving a session never trust a recorded pane id, which may since name another pane.
Lookups and teardown never start a daemon (`rmux_client::with_existing`): none running hosts nothing.
A program renaming its window to a tab's key would confuse that lookup.

Reconciliation sweeps both directions, because a pane and the row that owns it
are written one after the other and either can be the survivor:

- **Record without a pane** — `prune_dead_resources` drops rows whose pane the daemon no longer hosts, and marks `exited` the sessions whose agent pane is gone; a dead shell tab does not end its session.
  It reads the rows before listing the panes, and drops a row only while it names the pane read, so a pane recorded meanwhile is kept.
  Only no daemon listening means no pane is live: a daemon found listening after all is asked again, and one that cannot be read prunes nothing, the sweep below then skipped until the next start.
- **Pane without a session** — `sweep_orphan_panes` closes live panes that no session row owns, whether they never got a row (a launch that spawned then failed to persist) or lost it (a session deleted while its pane ran). Panes are matched by `pane_id` here, against the recorded rows. The sweep runs only during startup reconciliation — a pane is legitimately unrecorded between its spawn and its row being committed — and is skipped entirely when the database holds no sessions at all, since an empty database cannot be told apart from a lost one and every live pane would look orphaned.

A launch that cannot record its pane closes it rather than returning an error and
leaving it running, so a failed launch leaks nothing for the sweep to collect.

Terminal attach consumes rmux's **recovery stream**, not a raw stream anchored at
`Oldest`. It opens with a `Rebase` whose keyframe is ANSI bytes that reset and
reconstruct the screen — alternate buffer, title, geometry included — and the
daemon re-issues one whenever continuation breaks: lag, resize, cleared history, a
terminal reset, or a new process generation. A rebase _replaces_ undelivered bytes
rather than appending to them, because epochs must never be stitched together. A
keyframe that could not carry every retained row reports the shortfall as a gap.

Prompts are submitted with a carriage return, matching the daemon (`push(b'\r')`)
and tmux (`send-keys Enter`, which tmux emits as CR). Agents run their TUI in raw
mode, where the terminal driver does no CR/LF translation — a `\n` is accepted
into the input buffer and submits nothing, so the prompt sits on the input line
while the send reports success. `RmuxClient::submit_text` owns the byte so no call
site has to know it.

The CR is also a separate input, sent only once the pane's echo of the text has
gone quiet (capped at a few seconds). An agent that receives text and CR in one
input reads the whole burst as a paste and keeps the CR as a newline, so a
multi-line prompt, such as editor feedback from the nvim plugin, is typed but
never submitted. A fixed delay is not enough, because a large prompt takes longer
to ingest than a short one.

Ownership rules:

- One rmux session per TaskWorkspace, created when its first resource is spawned, destroyed **only on explicit task deletion** (`planeai-cli task delete`). Moving a task to `Done` archives its agent panes but keeps the workspace and its rmux session.
- A tab maps to a _window_, not a sibling pane: `Pane::resize` is a no-op for a sole pane, while `Window::resize` reaches the child process. Windows are the resizable geometry unit and tabs are independently sized.
- Each pane is spawned with its own `cwd` — a TaskWorkspace can group agent sessions living in different worktrees.
- Archive/delete/restart of one agent session closes only its own windows, never a sibling agent's panes. Shell tabs are windows too, so they persist with the workspace instead of dying with the app. The shared rmux session survives as long as a sibling resource remains — rmux drops a session with its last window, so archiving the _only_ agent on a task removes the workspace as well. It is re-created by name on the next spawn, and the orphan sweep depends on this: it closes panes and never workspaces, and is complete only because the workspace goes with them.
- `pty_key` stays the canonical PlaneAI identity (it is persisted in `task_workspaces.layout_json`). rmux pane IDs are renewable runtime handles held in a backend-side map and never reach the frontend or persisted layout.
- Session cleanup policy is `Preserve` — `KillOnOwnerExit` would kill every agent when the app quits.
- Killing the last session stops the daemon, so all paths use `connect_or_start` and treat a closed transport as restart-and-retry.
- rmux applies no backpressure to the child: a consumer that stops draining loses output (reported via lag notices with a resume point). So the adapter drains continuously into its own bounded buffer and implements `pause()`/`resume()` PlaneAI-side, like the existing `DaemonBackend`. The read loop is performance-critical — a debug-build consumer lost 97.6% of a 6.29 MiB burst that a release build delivered intact.
- A pane does not inherit the client's environment: rmux gives it a minimal PATH of its own. Every spawn passes PlaneAI's augmented PATH and TERM explicitly (`build_daemon_env`), or tools like the agent CLI are not found.
- Agent and shell commands use explicit argv, never `.shell()`, which would run the user's login shell (fish, nu) instead of `sh`.
- `rmux-daemon` is a separate binary from `rmux` and is what gets spawned. PlaneAI finds it through its augmented PATH and points `RMUX_SDK_DAEMON_BINARY` at the absolute path, since a launchd-started app only has the system PATH.
- xterm remains the renderer, replaying the recovery stream's `keyframe` then following its live byte events; snapshots are additive for AXI/automation only.
- The AXI cursor is capture-based (like tmux), not a byte offset: rmux has no "resume at sequence N" input.
- Agent state continues to come from provider hooks, identically to the local, tmux, and daemon backends. rmux events are used for exit detection only.
- Existing sessions are never migrated in place: `backend` is recorded per session at creation.

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

Changing the project after a task is selected keeps that task linked, so the session runs in the new project's repo inside the task's TaskWorkspace.
The task's base branch is kept only if the new repo has it; otherwise the form falls back to that repo's default branch and says so.
The session name is numbered across every agent on the task, while the branch suffix and the isolated-worktree default only count agents in the same repo.

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

- **Sidebar**: Loops render inside each project (above orphan sessions and tasks); when the sidebar is grouped by status they render, with orphan sessions, in the Sessions section above the status sections. Each loop shows a status dot, strategy label, round counter, and hover-revealed quick action buttons (start, tick, stop). Loop sessions are nested as collapsible children under their parent loop; they do not appear as orphans. Loops also participate in the MRU tab switcher (Ctrl+Tab) and session navigation (next/prev) as `loop:<id>` entries.
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
