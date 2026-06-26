# planeai

A cross-platform agent session orchestrator. Manages multiple AI coding agents running in parallel, each in its own terminal session. Supports three session backends: local (in-process PTY, default), tmux (persistent, requires tmux binary), and daemon (persistent, built-in, experimental).

## Glossary

| Term                | Definition                                                                                                                                                                                                                                                                                            |
| ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Project**         | A git repository registered with planeai. Stores a repo path and display name. The top-level organizational unit.                                                                                                                                                                                     |
| **Session**         | A single agent working on a single task within a project. Backed by a local PTY (default), tmux session, or the planeai daemon. Contains one terminal pane running the agent CLI.                                                                                                                     |
| **Session backend** | The process hosting strategy for a session: `local` (in-process PTY, default), `tmux` (survives app quit, requires tmux binary), or `daemon` (survives app quit, built-in, experimental). Resolved at session creation from the global setting.                                                       |
| **Provider**        | A CLI-based AI coding agent (e.g., Kiro, Claude Code, Aider). Defined by a base `command`, optional `yolo_flag`, optional `resume_flag` + `resume_command` for session resume. Multiple providers can be configured; one is the `default_provider`.                                                   |
| **Config file**     | The single source of truth for all user preferences and provider definitions. Lives at `$XDG_CONFIG_HOME/planeai/config.json` (default `~/.config/planeai/config.json`). JSONC for reading, pretty JSON for writing.                                                                                  |
| **Yolo mode**       | A per-session toggle that appends the provider's `yolo_flag` to the launch command, enabling auto-approval of tool use. Disabled if the provider has no `yolo_flag`.                                                                                                                                  |
| **Focus zone**      | A region of the UI that can receive keyboard input: sidebar or terminal. App-level chords (Cmd/Ctrl+B, Cmd/Ctrl+N, Cmd/Ctrl+Shift+P, Cmd/Ctrl+1-9, Ctrl+Tab, Escape) are always intercepted regardless of which zone has focus.                                                                       |
| **Form keyboard**   | A vim-like normal/insert mode controller (`createFormKeyboardController`) used by modal forms (PR form, PR panel). Normal mode maps single-key mnemonics to field focus or toggle actions. Insert mode is entered on text field focus; Escape returns to normal; Escape in normal dismisses the form. |
| **Tab switcher**    | An MRU overlay triggered by holding Ctrl+Tab. Each subsequent Tab moves selection; releasing Ctrl confirms.                                                                                                                                                                                           |
| **Notification**    | (future) A signal that an agent needs human attention.                                                                                                                                                                                                                                                |
| **Token**           | A semantic CSS custom property defined in the active theme file (e.g., `--color-surface-200`, `--terminal-background`). Mapped to Tailwind utilities via `@theme` block in `app.css`.                                                                                                                 |
| **Primitive**       | A reusable styled Svelte component in `src/components/ui/` that wraps bits-ui behavior (for complex interactives) or provides app-specific defaults (Button, Input). The building block for feature components.                                                                                       |
| **Theme mode**      | One of three states: `system`, `light`, `dark`. Persisted in localStorage. Controls which color palette is active.                                                                                                                                                                                    |
| **Daemon**          | (Experimental) A background process (`planeai-daemon`) that manages session PTYs. Spawned on-demand by the CLI or GUI. Sessions survive indefinitely as long as the daemon is running.                                                                                                                |
| **AXI**             | Agent eXperience Interface — a CLI subcommand (`planeai-cli axi`) that outputs TOON instead of JSON, optimised for autonomous agent consumption. Covers task, session, and project operations.                                                                                                        |
| **TOON**            | A token-efficient text output format used by the AXI interface. Supports object fields, tabular arrays, and primitive arrays with minimal overhead. Implemented in the `planeai-toon` crate.                                                                                                          |

## Session lifecycle (v1)

```
create → active → exited → deleted
```

- **Active** — PTY is connected and the agent process is running. Visible in sidebar.
- **Exited** — agent process terminated. Terminal buffer is frozen (read-only). User can restart or delete. Detected via PTY EOF (tmux) or daemon exit event (daemon).
- **Deleted** — session removed from sidebar and DB. For tmux sessions, the tmux session is killed. For daemon sessions, a kill command is sent to the daemon. Irreversible.

## Architecture notes

- **Tauri v2** (Rust backend + webview frontend)
- **Svelte 5** with runes for reactive UI
- **xterm.js** for terminal rendering
- **Tailwind CSS v4** with custom `@theme` block mapping CSS custom properties to utility classes
- **Custom theming** via CSS files in `~/.config/planeai/themes/`. Theme file defines UI, terminal, and editor tokens for both light and dark modes.
- **SQLite via rusqlite** on the Rust backend for persistence
- **tmux** for optional process persistence (explicit opt-in; see Session backend)
- **planeai-daemon** for built-in process persistence (no external dependencies)
- **portable-pty** for PTY management (tmux-attach goes through a local PTY; daemon sessions are managed directly by the daemon process)
- **Tauri IPC** (commands + typed event channels) for PTY byte streaming between Rust and frontend
- **Typed API layer** (`src/lib/api.ts`) — all `invoke()` calls consolidated behind domain-grouped typed methods; components never call `invoke()` directly (see ADR-0009)
- **pnpm** for package management

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
| **Subprocess spawn**  | No special handling                               | `CREATE_NO_WINDOW` flag via `no_window()` helpers      |

## Notification IPC events

The notify socket (`notify.sock` / `\\.\pipe\planeai-notify`) accepts JSONL messages. Each message has an `event` field and a `session_id` field.

| Event               | Direction        | Payload                                            | Purpose                                                        |
| ------------------- | ---------------- | -------------------------------------------------- | -------------------------------------------------------------- |
| `stop`              | Hook → GUI       | `{"event":"stop","session_id":"..."}`              | Agent finished (debounced idle detection)                      |
| `notification`      | Hook → GUI       | `{"event":"notification","session_id":"..."}`      | Agent needs human attention                                    |
| `busy`              | Hook → GUI       | `{"event":"busy","session_id":"..."}`              | Agent started working                                          |
| `session_created`   | CLI/Daemon → GUI | `{"event":"session_created","session_id":"..."}`   | New session created, GUI should refresh                        |
| `session_changed`   | CLI → GUI        | `{"event":"session_changed","session_id":"..."}`   | Session state changed (archived/destroyed), GUI should refresh |
| `session_restarted` | Backend → GUI    | `{"event":"session-restarted","session_id":"..."}` | Exited session restarted, GUI should re-attach PTY             |

For tmux-backend sessions, the CLI sends prompts directly via `tmux send-keys -l` without going through the GUI.
For daemon-backend sessions, the CLI sends prompts via the daemon data connection (FRAME_INPUT).

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

- **Daemon backend**: shell tabs are spawned in the daemon with composed ID `{session_id}:{tab_index}`. They persist across app restarts (same as the agent session). On archive/destroy, all shell tabs are killed alongside the agent session.
- **Tmux backend**: shell tabs use a local PTY (`PtyTarget::Shell`) and are ephemeral — they die with the app.

### Exit detection

- **tmux**: PTY reader thread gets EOF → emit `pty-exited` → mark session as `exited` in DB.
- **daemon**: daemon broadcasts an `exited` event on the control socket → GUI marks session exited.

### Startup reconciliation (one-time, not polling)

- Local sessions that were `active` → mark `exited` (local sessions cannot survive app restart)
- Tmux sessions that were `active` → check `tmux has-session`; if false → mark `exited`; if true → leave as-is
- Daemon sessions → left as-is (daemon manages their lifecycle)

### Restart

Exited sessions can be restarted: same session identity (name, project, worktree), clean terminal buffer, status returns to `active`. For tmux, creates a new tmux session with the same name. For daemon, sends a spawn command to the daemon. For local, the session status is restored and a new PTY is spawned on attach.

Provider resume is attempted on restart: if `resume_flag` + stored `provider_session_id` → resume command; if `resume_command` is set (interactive picker) → use that; otherwise → fresh provider command. If resume fails, automatically falls back to fresh launch.

Selecting an exited session triggers restart automatically. The frontend re-attaches to the new PTY via the `session-restarted` event.

### DB columns

- `backend TEXT NOT NULL DEFAULT 'tmux'` — set at creation time, values: `'local'`, `'tmux'`, or `'daemon'`
- `status TEXT NOT NULL DEFAULT 'active'` — updated on exit/delete
- `tmux_name TEXT` — NULL for daemon sessions, populated for tmux sessions

### Preferences UI

Dropdown: Local (default) / tmux / Daemon (experimental). Inline warning if user selects tmux but binary not found.

## Worktree support

Sessions can run in two modes:

- **Checkout mode** (default) — `git checkout` on the project's repo path. Only one active checkout session per project.
- **Worktree mode** — `git worktree add` creates an isolated working copy at `~/.planeai/worktrees/<project-name>/<session-id>/`. Multiple worktree sessions can run in parallel on the same project.

### Lifecycle

- **Archive** — kills tmux/daemon session, marks session archived. Worktree directory is preserved on disk.
- **Destroy** — kills tmux/daemon session, runs `git worktree remove --force`, deletes directory, removes from DB.

### Data model

Sessions table has `worktree_path TEXT NULL`. Non-null indicates worktree mode.

### Form flow (worktree mode)

Project → Session name → ✅ Create worktree → Base branch (existing) → New branch name (editable, defaults to session name slugified)

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

## Adding a new ui/ primitive

1. Create `src/components/ui/MyComponent.svelte`
2. Wrap the relevant bits-ui component (or plain HTML) with Skeleton token classes
3. Use `dark:` variants for all color utilities (e.g., `bg-surface-50 dark:bg-surface-900`)
4. Accept a `class` prop for consumer overrides
5. Export from `src/components/ui/index.ts`
6. Use the primitive in feature components — never hardcode colors inline
