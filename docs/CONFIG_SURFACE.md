# Config Surface Policy

> **Date:** 2026-06-18  
> **Status:** Post PR #147, config surface stabilized for dogfooding.

## Classification

### 1. Stable user-facing config

These are normal options users should configure.

| Field                     | Type     | Default      | Description                                                         |
| ------------------------- | -------- | ------------ | ------------------------------------------------------------------- |
| `providers`               | map      | kiro, claude | Provider name → {command, yolo_flag, ...}                           |
| `default_provider`        | string   | `"kiro"`     | Which provider to use when launching sessions                       |
| `session_backend`         | string   | `"local"`    | Where sessions run: `local`, `tmux`, `daemon`                       |
| `session_log_dir`         | string   | unset        | Directory for durable `.ansi` session logs                          |
| `extra_path_dirs`         | string[] | `[]`         | Extra dirs prepended to PATH for sessions                           |
| `appearance`              | object   | —            | Theme, dark/light mode, terminal themes                             |
| `terminal`                | object   | —            | font_family, font_size, option_as_meta                              |
| `projects_base_path`      | string   | unset        | Base directory for project worktrees                                |
| `task_management`         | object   | unset        | Task lifecycle hooks and dispatch config                            |
| `daemon_scrollback_bytes` | number   | 1MB          | Daemon ring buffer size                                             |
| `integrations`            | object   | unset        | External service integrations (currently: `jira`)                   |
| `scrollback_lines`        | number   | —            | Terminal scrollback line limit                                      |
| `sound_enabled`           | bool     | `true`       | Play a chime when an agent finishes a task                          |
| `post_merge_action`       | string   | `"archive"`  | Default action after PR merge timeout: `archive`, `destroy`, `keep` |

### 2. Advanced compatibility config

Power-user options. Not required for normal use.

| Field       | Type | Default | Description                               |
| ----------- | ---- | ------- | ----------------------------------------- |
| `web_links` | bool | —       | Enable clickable links in terminal output |
| `vim_mode`  | bool | false   | Enable vim-style key bindings             |

### 3. Env-only debug/rollback flags

These are NOT in the config file by default. Used for debugging, migration, and rollback.

| Variable                     | Values                  | Default           | Description                                          |
| ---------------------------- | ----------------------- | ----------------- | ---------------------------------------------------- |
| `PLANEAI_LOCAL_PTY_CORE`     | `legacy`, `planeai-pty` | `legacy`          | Select PTY implementation for local sessions         |
| `PLANEAI_DAEMON_PTY_CORE`    | `legacy`, `planeai-pty` | `legacy`          | Select PTY implementation for daemon sessions        |
| `PLANEAI_SESSION_LOG_DIR`    | path                    | unset             | Override `session_log_dir` config (highest priority) |
| `PLANEAI_DAEMON_LOG_DIR`     | path                    | `~/.planeai/logs` | Override daemon process log directory                |
| `PLANEAI_EXTRA_PATH`         | colon-separated dirs    | unset             | Override `extra_path_dirs` config (highest priority) |
| `PLANEAI_DOGFOOD_LOG_VIEWER` | `1`, `true`             | unset             | Enable in-app log viewer (Tauri only)                |
| `PLANEAI_BENCH_REPLAY`       | path                    | unset             | Benchmark replay fixture                             |
| `PLANEAI_BENCH_COLS`         | number                  | —                 | Benchmark terminal columns                           |
| `PLANEAI_BENCH_ROWS`         | number                  | —                 | Benchmark terminal rows                              |

### 4. Temporary migration options (config fields)

These exist in the config file for compatibility but are migration-period options. Normal users should NOT set them. They will be removed once planeai-pty is the default.

| Field             | Values                  | Default  | Description                            |
| ----------------- | ----------------------- | -------- | -------------------------------------- |
| `local_pty_core`  | `legacy`, `planeai-pty` | `legacy` | PTY implementation for local sessions  |
| `daemon_pty_core` | `legacy`, `planeai-pty` | `legacy` | PTY implementation for daemon sessions |

### 5. Candidates for future removal/restructuring

| Field             | Reason                                |
| ----------------- | ------------------------------------- |
| `pr_status`       | May move into `task_management`       |
| `hide_done_tasks` | UI preference, may become per-project |

---

## session_backend

### Definition

`session_backend` chooses where PlaneAI sessions are managed by default.

### Values

| Value    | Meaning                                                   |
| -------- | --------------------------------------------------------- |
| `local`  | Sessions run as local child processes (default)           |
| `tmux`   | Sessions run in tmux (requires tmux installed)            |
| `daemon` | Sessions run in the PlaneAI daemon process (experimental) |

### Policy

- **`local` is always the default.** When `session_backend` is unset or null, local is used.
- **`tmux` is explicit optional behavior.** It is never auto-detected or silently selected.
- **`daemon` is experimental** and provides session persistence across app restarts.
- PTY core selection (`local_pty_core`, `daemon_pty_core`) is a separate concern and must NOT be conflated with session backend.

### Naming decision

The name `session_backend` is retained for this milestone. Alternatives considered:

- `session_target` — slightly clearer but migration cost not justified yet
- `default_session_backend` — more explicit but verbose

No rename is planned unless a future breaking-change release provides an opportunity.

### What users should think

Users should think: "I use daemon sessions" or "I use tmux sessions."

Users should NOT normally need to think about PTY core selection.

---

## PTY Core Config

### Separation of concerns

| Concept                                      | User-facing?  | Where configured                    |
| -------------------------------------------- | ------------- | ----------------------------------- |
| Session backend (daemon/tmux/local)          | Yes           | `session_backend` in config         |
| PTY core implementation (legacy/planeai-pty) | No (advanced) | Env vars or migration config fields |

### Normal users

Configure these:

- `session_backend` — where sessions run
- `providers` — what agent commands to use
- `default_provider` — which provider by default
- `extra_path_dirs` — custom PATH directories
- `session_log_dir` — enable durable logs (optional)

### Advanced/debug users

Use env vars to control PTY implementation:

```bash
# Roll back daemon PTY to legacy
PLANEAI_DAEMON_PTY_CORE=legacy pnpm tauri dev

# Use modern planeai-pty for daemon sessions
PLANEAI_DAEMON_PTY_CORE=planeai-pty pnpm tauri dev

# Roll back local PTY to legacy
PLANEAI_LOCAL_PTY_CORE=legacy pnpm tauri dev
```

### Resolution order

1. `PLANEAI_DAEMON_PTY_CORE` env var (highest priority)
2. `daemon_pty_core` config field
3. Default: `"legacy"`

Same pattern for local: `PLANEAI_LOCAL_PTY_CORE` → `local_pty_core` → `"legacy"`.

### What these flags do NOT control

- tmux sessions (always use tmux's own PTY)
- Session backend selection (that's `session_backend`)
- Provider command resolution
- PATH construction

### Migration plan

1. **Current:** legacy is default, planeai-pty is opt-in
2. **Next:** planeai-pty becomes default after successful dogfooding
3. **Later:** legacy removed, config fields removed, env vars become no-ops

### Do not encourage

Do not encourage normal users to set `daemon_pty_core` or `local_pty_core` in their config. These are migration/debugging tools, not product configuration.

---

## Iced Workflow CLI Flags

These are specific to the Iced workflow binary (`planeai-iced`):

| Flag                      | Description                       | Default                         |
| ------------------------- | --------------------------------- | ------------------------------- |
| `--planeai-workflow`      | Enable workflow mode              | off                             |
| `--cwd <path>`            | Project working directory         | current dir                     |
| `--agent-command <cmd>`   | Override agent command            | config default_provider         |
| `--extra-path-dirs <dir>` | Additional PATH dirs (repeatable) | none                            |
| `--config <path>`         | Explicit config file path         | `~/.config/planeai/config.json` |
| `--backend <name>`        | Renderer backend                  | `iced-alacritty`                |
| `--cols <n>`              | Terminal columns                  | 120                             |
| `--rows <n>`              | Terminal rows                     | 40                              |
| `--replay <path>`         | Replay a log file (read-only)     | —                               |

### Precedence

1. CLI flags (highest)
2. Environment variables
3. Config file (`~/.config/planeai/config.json` or `--config`)
4. Built-in defaults (lowest)

---

## Env Override Precedence

```
CLI flags
  ↓ (overrides)
Environment variables (PLANEAI_*)
  ↓ (overrides)
Config file fields
  ↓ (overrides)
Built-in defaults
```

Specific examples:

- `--agent-command` > config `providers[default_provider].command`
- `PLANEAI_SESSION_LOG_DIR` > config `session_log_dir`
- `PLANEAI_EXTRA_PATH` > config `extra_path_dirs`
- `PLANEAI_DAEMON_PTY_CORE` > config `daemon_pty_core`

---

## integrations.jira

### Definition

Optional Jira Cloud integration that syncs issues into planeai's task board and writes back status transitions.

### Schema

| Field                                | Type   | Default | Description                                   |
| ------------------------------------ | ------ | ------- | --------------------------------------------- |
| `integrations.jira.site`             | string | —       | Jira Cloud site URL (required to enable)      |
| `integrations.jira.sync_interval_ms` | number | 60000   | Polling interval in milliseconds              |
| `integrations.jira.sources`          | map    | `{}`    | Named JQL sync sources (key = source alias)   |
| `sources.<name>.jql`                 | string | —       | JQL filter selecting issues to sync           |
| `sources.<name>.status_map`          | map    | `{}`    | Jira status name → planeai status             |
| `sources.<name>.writeback`           | object | null    | Optional writeback config                     |
| `writeback.on_start`                 | string | null    | Jira status to transition to on work start    |
| `writeback.on_complete`              | string | null    | Jira status to transition to on work complete |
| `writeback.comment`                  | bool   | false   | Add a timestamped comment on each transition  |

### Build-time env vars

| Variable             | Required | Description                                      |
| -------------------- | -------- | ------------------------------------------------ |
| `JIRA_CLIENT_ID`     | Yes\*    | OAuth 2.0 client ID (from Atlassian dev console) |
| `JIRA_CLIENT_SECRET` | Yes\*    | OAuth 2.0 client secret                          |

\* Build succeeds without them (placeholder values used) but OAuth will not work at runtime.

### Policy

- Jira is entirely optional. Absent `integrations.jira` config means the feature is inactive.
- Auth tokens are stored in the app data directory (file-based, `0600` permissions on Unix).
- All Jira network calls are async and never block the main thread.
