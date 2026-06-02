# planeai

A cross-platform app for running multiple AI coding agents in parallel. Each agent works in its own terminal session, backed by tmux for persistence.

## Features (v1)

- **Keyboard-first** — every action reachable without a mouse
- **Session management** — launch Kiro agents, switch between sessions
- **Sidebar** — sessions grouped by project, active session highlighted
- **Tab switcher** — Ctrl+Tab MRU overlay for fast session switching
- **tmux persistence** — agents keep running when you quit the app
- **SQLite storage** — projects and sessions persisted locally

## Requirements

- macOS (Linux/Windows planned)
- tmux (`brew install tmux`)
- `kiro-cli` on PATH

## Tech stack

- **Tauri v2** — Rust backend, webview shell
- **Svelte 5** — reactive UI with runes
- **xterm.js** — terminal rendering in the browser
- **Tailwind CSS** — utility-first styling, dark theme
- **rusqlite** — SQLite persistence on the Rust side
- **pnpm** — package management

## Architecture

The Rust backend owns tmux interaction, SQLite, and PTY management. The Svelte frontend renders the UI and terminal via xterm.js. Communication is via Tauri IPC (commands for actions, event channels for streaming terminal bytes).

See [CONTEXT.md](./CONTEXT.md) for domain glossary and [docs/adr/](./docs/adr/) for architecture decisions.

## Development

This project uses TDD. See [AGENTS.md](./AGENTS.md) for workflow guidelines.

```bash
pnpm install
pnpm tauri dev
```

## Task Manager Integration

planeai integrates with external task manager CLIs (kanban, Jira wrappers, etc.) to automatically start sessions from tasks, pre-fill session details, and manage task status through the session lifecycle.

### Configuration

Add a `task_managers` section to `~/.config/planeai/config.json`:

```jsonc
{
  "task_managers": {
    "kanban": {
      "get_task": "kanban show {key}",
      "move_task": "kanban move {key} {status}",
      "list_tasks": "kanban list --status todo",
      "templates": {
        "branch": "{key:lower}/{title:slug}",
        "name": "{key:upper}: {title}",
        "prompt": "Implement task {key}: {title}\n\n{description}"
      },
      "on_start": { "move_to": "in_progress" },
      "on_notify": { "move_to": "in_review" },
      "on_restart": { "move_to": "in_progress" },
      "on_complete": { "move_to": "done" }
    }
  },
  "default_task_manager": "kanban"
}
```

### Provider prompt_command

To inject the task prompt into the agent CLI, add `prompt_command` to your provider:

```jsonc
{
  "providers": {
    "kiro": {
      "command": "kiro-cli chat",
      "prompt_command": "{prompt}",  // positional arg
      "yolo_flag": "--trust-all-tools"
    },
    "claude": {
      "command": "claude",
      "prompt_command": "--prompt {prompt}",  // flag-based
      "yolo_flag": "--dangerously-skip-permissions"
    }
  }
}
```

When a session is launched from a task, the rendered prompt template is substituted into `prompt_command` and appended to the launch command. If no task is linked, `prompt_command` is omitted.

### Fixed JSON contract

Task manager commands must output JSON matching this structure:

```json
{
  "key": "KAN-3",
  "title": "Add dark mode support",
  "status": "todo",
  "description": "Implement dark mode for accessibility.",
  "priority": 1,
  "blocked_by": ["KAN-1"]
}
```

`list_tasks` returns an array of the same shape. If your tool outputs a different format, wrap it in a script that normalizes the output.

### Template syntax

Templates use `{variable}` with optional transforms via `{variable:transform}`:

| Transform | Effect | Example |
|-----------|--------|---------|
| (none) | Raw value | `{key}` → `KAN-3` |
| `lower` | Lowercase | `{key:lower}` → `kan-3` |
| `upper` | Uppercase | `{key:upper}` → `KAN-3` |
| `slug` | Slugify | `{title:slug}` → `add-dark-mode-support` |

Available variables: `key`, `title`, `status`, `description`, `priority`, `blocked_by`.

### Lifecycle hooks

Each hook is optional. When configured, it calls `move_task` with the specified status:

| Hook | Fires when |
|------|-----------|
| `on_start` | Session is created from a task |
| `on_notify` | Agent signals idle (notification received) |
| `on_restart` | An exited task-linked session is restarted |
| `on_complete` | Task-linked session is archived or deleted |

### Usage

1. **Command menu** — Press `Cmd+K`, select "Pick task…", choose from the list. Session form opens pre-filled.
2. **Session form** — Type a task key directly in the "Task key" field. The form pre-fills name from the configured template.

Task-linked sessions display their task key as a badge in the sidebar.
