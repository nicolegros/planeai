# Configuration

planeai is configured via a single JSON file at `~/.config/planeai/config.json` (or `%APPDATA%\planeai\config.json` on Windows). The file supports JSONC (comments allowed) for reading.

## Providers

A provider is a CLI-based AI coding agent. Define one or more providers:

```jsonc
{
  "providers": {
    "kiro": {
      "command": "kiro-cli chat",
      "prompt_command": "{prompt}",
      "yolo_flag": "--trust-all-tools",
    },
    "claude": {
      "command": "claude",
      "prompt_command": "--prompt {prompt}",
      "yolo_flag": "--dangerously-skip-permissions",
    },
    "copilot": {
      "command": "gh copilot",
      "prompt_command": "{prompt}",
    },
  },
  "default_provider": "kiro",
}
```

- `command` — the base CLI command to launch the agent
- `prompt_command` — how to pass a task prompt to the agent (positional or flag-based). Omitted when no task is linked.
- `autonomous_prompt_template` — wraps the task prompt content in Symphony auto-dispatch mode. Use `{prompt}` to include the rendered task prompt. Example: `"You are autonomous.\n{prompt}"`. Falls back to using the raw task prompt if not set.
- `yolo_flag` — optional flag appended when "yolo mode" is enabled for a session (auto-approves tool use)

## Session Backend

Controls how agent processes are hosted:

```jsonc
{
  "session_backend": "auto", // "auto" | "tmux" | "direct"
}
```

- `auto` (default) — uses tmux if available on PATH, otherwise direct PTY
- `tmux` — forces tmux (warns if not found)
- `direct` — forces direct PTY (sessions die on app quit)

## Task Manager Integration

planeai has a built-in task manager that tracks tasks per project. The `task_managers` config section controls lifecycle hooks (auto-moving task status on events), templates (branch names, session names, prompts), and auto-dispatch settings.

### Configuration

Add a `task_managers` section to your config file:

```jsonc
{
  "task_managers": {
    "kanban": {
      "templates": {
        "branch": "{key:lower}/{title:slug}",
        "name": "{key:upper}: {title}",
        "prompt": "Implement task {key}: {title}\n\n{description}",
      },
      "on_start": { "move_to": "in_progress" },
      "on_notify": { "move_to": "in_review" },
      "on_restart": { "move_to": "in_progress" },
      "on_complete": { "move_to": "done" },
    },
  },
  "default_task_manager": "kanban",
}
```

### Template Syntax

Templates use `{variable}` with optional transforms via `{variable:transform}`:

| Transform | Effect    | Example                                  |
| --------- | --------- | ---------------------------------------- |
| (none)    | Raw value | `{key}` → `KAN-3`                        |
| `lower`   | Lowercase | `{key:lower}` → `kan-3`                  |
| `upper`   | Uppercase | `{key:upper}` → `KAN-3`                  |
| `slug`    | Slugify   | `{title:slug}` → `add-dark-mode-support` |

Available variables: `key`, `title`, `status`, `description`, `priority`, `blocked_by`.

### Lifecycle Hooks

Each hook is optional. When configured, it automatically moves the linked task to the specified status:

| Hook          | Fires when                                 |
| ------------- | ------------------------------------------ |
| `on_start`    | Session is created from a task             |
| `on_notify`   | Agent signals idle (notification received) |
| `on_restart`  | An exited task-linked session is restarted |
| `on_complete` | Task-linked session is archived or deleted |

### Usage

1. **Command menu** — Press `Cmd+K`, select "Pick task…", choose from the list. Session form opens pre-filled.
2. **Session form** — Type a task key directly in the "Task key" field. The form pre-fills name from the configured template.

Task-linked sessions display their task key as a badge in the sidebar.
