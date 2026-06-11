# planeai

A cross-platform agent session orchestrator. Manages multiple AI coding agents running in parallel, each in its own terminal session. Supports two session backends: tmux (persistent) and direct PTY (ephemeral).

## Glossary

| Term                | Definition                                                                                                                                                                                                           |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Project**         | A git repository registered with planeai. Stores a repo path and display name. The top-level organizational unit.                                                                                                    |
| **Session**         | A single agent working on a single task within a project. Backed by either a tmux session (persistent) or a direct PTY (ephemeral). Contains one terminal pane running the agent CLI.                                |
| **Session backend** | The process hosting strategy for a session: `tmux` (survives app quit, requires tmux binary) or `direct` (PTY spawns agent directly, dies on app quit). Resolved at session creation from the global setting.        |
| **Provider**        | A CLI-based AI coding agent (e.g., Kiro, Claude Code, Aider). Defined by a base `command` and an optional `yolo_flag`. Multiple providers can be configured; one is the `default_provider`.                          |
| **Config file**     | The single source of truth for all user preferences and provider definitions. Lives at `$XDG_CONFIG_HOME/planeai/config.json` (default `~/.config/planeai/config.json`). JSONC for reading, pretty JSON for writing. |
| **Yolo mode**       | A per-session toggle that appends the provider's `yolo_flag` to the launch command, enabling auto-approval of tool use. Disabled if the provider has no `yolo_flag`.                                                 |
| **Focus zone**      | A region of the UI that can receive keyboard input: sidebar or terminal. App-level chords (Cmd+B, Cmd+N, Cmd+1-9, Ctrl+Tab, Escape) are always intercepted regardless of which zone has focus.                       |
| **Tab switcher**    | An MRU overlay triggered by holding Ctrl+Tab. Each subsequent Tab moves selection; releasing Ctrl confirms.                                                                                                          |
| **Notification**    | (future) A signal that an agent needs human attention.                                                                                                                                                               |
| **Token**           | A semantic CSS custom property defined in the active theme file (e.g., `--color-surface-200`, `--terminal-background`). Mapped to Tailwind utilities via `@theme` block in `app.css`.                                |
| **Primitive**       | A reusable styled Svelte component in `src/components/ui/` that wraps bits-ui behavior (for complex interactives) or provides app-specific defaults (Button, Input). The building block for feature components.      |
| **Theme mode**      | One of three states: `system`, `light`, `dark`. Persisted in localStorage. Controls which color palette is active.                                                                                                   |

## Session lifecycle (v1)

```
create → active → exited → deleted
```

- **Active** — PTY is connected and the agent process is running. Visible in sidebar.
- **Exited** — agent process terminated. Terminal buffer is frozen (read-only). User can restart or delete. Detected via PTY EOF (unified for both backends).
- **Deleted** — session removed from sidebar and DB. For tmux sessions, the tmux session is killed. Irreversible.

## Architecture notes

- **Tauri v2** (Rust backend + webview frontend)
- **Svelte 5** with runes for reactive UI
- **xterm.js** for terminal rendering
- **Tailwind CSS v4** with custom `@theme` block mapping CSS custom properties to utility classes
- **Custom theming** via CSS files in `~/.config/planeai/themes/`. Theme file defines UI, terminal, and editor tokens for both light and dark modes.
- **SQLite via rusqlite** on the Rust backend for persistence
- **tmux** for optional process persistence (auto-detected; see Session backend)
- **portable-pty** for PTY management (both tmux-attach and direct-spawn go through a local PTY)
- **Tauri IPC** (commands + typed event channels) for PTY byte streaming between Rust and frontend
- **pnpm** for package management

## Key constraints

- Keyboard-first — all actions reachable without mouse
- Multiple sessions allowed per project in both checkout and worktree modes; inline warning shown when creating additional checkout sessions
- DB is source of truth; orphan tmux sessions are ignored
- tmux is optional — app works without it (direct PTY fallback)
- Cross-platform: macOS and Windows (core functionality parity; tmux gracefully unavailable on Windows)
- Project names must be unique

## Cross-platform strategy

| Concern               | macOS                                             | Windows                                                |
| --------------------- | ------------------------------------------------- | ------------------------------------------------------ |
| **Session backend**   | tmux (persistent) or direct PTY                   | direct PTY only (tmux unavailable)                     |
| **Notification IPC**  | Unix socket (`notify.sock`)                       | Named pipe (`\\.\pipe\planeai-notify`)                 |
| **Stop hook**         | Bash script (`.sh`) via `nc -U`                   | PowerShell script (`.ps1`) via `NamedPipeClientStream` |
| **Config dir**        | `$XDG_CONFIG_HOME/planeai` or `~/.config/planeai` | `%APPDATA%\planeai`                                    |
| **Home dir**          | `$HOME`                                           | `$HOME` or `%USERPROFILE%`                             |
| **Platform modifier** | Cmd (⌘)                                           | Ctrl                                                   |
| **Default font**      | Menlo                                             | Cascadia Mono                                          |
| **Title bar padding** | Left (traffic lights)                             | Right (caption buttons)                                |
| **Font enumeration**  | font-kit (cross-platform)                         | font-kit (cross-platform)                              |
| **Window style**      | Overlay title bar                                 | Overlay title bar (Tauri handles caption buttons)      |

## Notification IPC events

The notify socket (`notify.sock` / `\\.\pipe\planeai-notify`) accepts JSONL messages. Each message has an `event` field and a `session_id` field.

| Event             | Direction        | Payload                                                   | Purpose                                                        |
| ----------------- | ---------------- | --------------------------------------------------------- | -------------------------------------------------------------- |
| `stop`            | Hook → GUI       | `{"event":"stop","session_id":"..."}`                     | Agent finished (debounced idle detection)                      |
| `notification`    | Hook → GUI       | `{"event":"notification","session_id":"..."}`             | Agent needs human attention                                    |
| `busy`            | Hook → GUI       | `{"event":"busy","session_id":"..."}`                     | Agent started working                                          |
| `session_created` | CLI/Daemon → GUI | `{"event":"session_created","session_id":"..."}`          | New session created, GUI should refresh                        |
| `session_changed` | CLI → GUI        | `{"event":"session_changed","session_id":"..."}`          | Session state changed (archived/destroyed), GUI should refresh |
| `send_prompt`     | CLI → GUI        | `{"event":"send_prompt","session_id":"...","text":"..."}` | Write prompt text to the session's PTY (direct backend)        |

For tmux-backend sessions, the CLI sends prompts directly via `tmux send-keys -l` without going through the GUI.

## Session backend

### Resolution

The effective backend is resolved once at app startup:

```
config.session_backend ?? (tmux_on_path ? "tmux" : "direct")
```

- Config field absent → auto-detect (tmux if available, otherwise direct)
- `"session_backend": "tmux"` → force tmux (warn if not found)
- `"session_backend": "direct"` → force direct PTY

Setting changes affect new sessions only. Existing sessions keep their backend.

### PTY architecture

Both backends spawn a local PTY via `portable-pty`. The only difference is the command inside:

- **tmux backend:** PTY runs `tmux attach-session -t <name>` (tmux session created beforehand with the agent command)
- **direct backend:** PTY runs the agent command directly (e.g., `kiro-cli chat`)

The `PtyManager.attach()` method accepts a `PtyTarget` enum:

```rust
enum PtyTarget {
    TmuxAttach { tmux_name: String },
    Direct { command: String, args: Vec<String>, cwd: String },
}
```

### Exit detection

Unified for both backends: PTY reader thread gets EOF → emit `pty-exited-{session_id}` → mark session as `exited` in DB. For tmux, this works because `remain-on-exit` is disabled — when the agent exits, the tmux session dies, `tmux attach` exits, PTY gets EOF.

### Startup reconciliation (one-time, not polling)

- Direct sessions that were `active` at last quit → mark `exited`
- Tmux sessions that were `active` → check `tmux has-session`; if false → mark `exited`; if true → reattach

### Quit confirmation

When the user quits (Cmd+Q / window close) and at least one `active` direct session exists, show an in-app confirmation modal: "N active session(s) will be terminated. Quit anyway?" with Quit / Cancel. If all active sessions are tmux-backed (will survive) or no active sessions exist, quit immediately.

### Restart

Exited sessions can be restarted: same session identity (name, project, worktree), clean terminal buffer, status returns to `active`. For tmux, creates a new tmux session with the same name. For direct, spawns a new PTY.

### DB columns

- `backend TEXT NOT NULL DEFAULT 'tmux' CHECK(backend IN ('tmux', 'direct'))` — set at creation time
- `status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active', 'exited', 'deleted'))` — updated on exit/delete
- `tmux_name TEXT` — NULL for direct sessions, populated for tmux sessions

### Preferences UI

Dropdown: Auto (default) / tmux / Direct. "Auto" maps to absent in config file. Inline warning if user selects tmux but binary not found.

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

## Adding a new ui/ primitive

1. Create `src/components/ui/MyComponent.svelte`
2. Wrap the relevant bits-ui component (or plain HTML) with Skeleton token classes
3. Use `dark:` variants for all color utilities (e.g., `bg-surface-50 dark:bg-surface-900`)
4. Accept a `class` prop for consumer overrides
5. Export from `src/components/ui/index.ts`
6. Use the primitive in feature components — never hardcode colors inline
