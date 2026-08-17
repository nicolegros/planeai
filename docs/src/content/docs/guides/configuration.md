---
title: Configuration
description: Full configuration reference for planeai providers, sessions, and task management.
draft: false
---

planeai is configured via a single JSON file at `~/.config/planeai/config.json` (or `%APPDATA%\planeai\config.json` on Windows). The file supports JSONC (comments allowed).

## Providers

Each provider defines how planeai launches and communicates with an AI agent CLI.

```jsonc
{
  "providers": {
    "kiro": {
      // Command to start the agent
      "command": "kiro-cli chat",
      // Command to send a prompt to an existing session
      "prompt_command": "kiro-cli chat --message \"{{prompt}}\"",
      // Template for autonomous prompts (task dispatch)
      "autonomous_prompt_template": "Complete this task: {{task.title}}\n\n{{task.description}}",
      // Flag to enable autonomous/yolo mode (no confirmations)
      "yolo_flag": "--trust",
      // Command used when restarting an exited session
      "resume_command": "kiro-cli chat --resume",
    },
    "claude": {
      "command": "claude",
      "prompt_command": "claude --message \"{{prompt}}\"",
      "autonomous_prompt_template": "{{task.title}}: {{task.description}}",
      "yolo_flag": "--dangerously-skip-permissions",
    },
  },
}
```

| Field                        | Description                                                       |
| ---------------------------- | ----------------------------------------------------------------- |
| `command`                    | Shell command to start a new agent session                        |
| `prompt_command`             | Command to send a prompt to a running session                     |
| `autonomous_prompt_template` | Template rendered when auto-dispatch sends a task                 |
| `yolo_flag`                  | Flag appended in autonomous mode to skip confirmations            |
| `resume_command`             | Command to resume interactively when restarting an exited session |

## Session Backend

Controls how planeai manages terminal sessions.

```jsonc
{
  "session_backend": "local", // "local" | "tmux" | "daemon"
}
```

| Value    | Behavior                                                                      |
| -------- | ----------------------------------------------------------------------------- |
| `local`  | In-process PTY — lightweight, no external dependencies (default)              |
| `tmux`   | Requires tmux — sessions persist via tmux                                     |
| `daemon` | Built-in daemon process — sessions persist across app restarts (experimental) |

:::tip
The local backend is the default and recommended for most users. Sessions terminate when the app closes.
:::

:::caution
The `daemon` backend is experimental. It provides session persistence across app restarts but may have stability issues.
:::

:::note
tmux is not supported on Windows. The `local` backend is used automatically on Windows regardless of this setting.
:::

## Sound

Controls whether planeai plays audio notifications.

```jsonc
{
  "sound_enabled": true, // default: true
}
```

| Value   | Behavior                                             |
| ------- | ---------------------------------------------------- |
| `true`  | Play a chime when an agent finishes a task (default) |
| `false` | Disable all sound notifications                      |

This setting is also available in **Preferences → Sound**.

## Task Manager Integration

### Templates

Templates control how tasks map to branches, session names, and prompts.

```jsonc
{
  "task_manager": {
    "templates": {
      // Git branch name for the task
      "branch": "{{task.key | slugify}}/{{task.title | slugify}}",
      // Session display name
      "name": "{{task.key}}: {{task.title | truncate(40)}}",
      // Prompt sent to the agent
      "prompt": "{{task.description}}",
    },
  },
}
```

#### Template Syntax

Templates use `{{variable}}` interpolation with optional transforms via `|`:

| Transform     | Description                 |
| ------------- | --------------------------- |
| `slugify`     | Converts to URL-safe slug   |
| `truncate(n)` | Truncates to `n` characters |
| `lowercase`   | Converts to lowercase       |
| `uppercase`   | Converts to uppercase       |

Available variables: `task.key`, `task.title`, `task.description`, `task.status`, `task.priority`.

### Lifecycle Hooks

Hooks run shell commands at task state transitions.

```jsonc
{
  "task_manager": {
    "lifecycle_hooks": {
      // Runs when a task is dispatched to an agent
      "on_start": "echo 'Starting {{task.key}}'",
      // Runs when the agent signals completion
      "on_complete": "git add -A && git commit -m 'feat({{task.key | slugify}}): {{task.title}}'",
      // Runs when a notification is received
      "on_notify": "say '{{task.key}} needs attention'",
      // Runs when a failed task is retried
      "on_restart": "git stash && git pull --rebase",
    },
  },
}
```

| Hook          | Trigger                             |
| ------------- | ----------------------------------- |
| `on_start`    | Task dispatched to an agent session |
| `on_complete` | Agent signals task completion       |
| `on_notify`   | Task receives a notification        |
| `on_restart`  | Task is retried after failure       |

:::note
Hooks run in the working directory of the task's git worktree.
:::

## Extra PATH Directories

GUI apps inherit a minimal system PATH that may not include directories where your AI agent CLIs are installed. planeai automatically prepends conventional developer directories (`~/.local/bin`, `~/.cargo/bin`, `~/go/bin`, `/opt/homebrew/bin`, `/usr/local/bin`), but if your CLI lives in a custom location, configure `extra_path_dirs`:

```jsonc
{
  // Directories prepended to PATH when launching sessions
  "extra_path_dirs": ["~/.guardrails/shims", "~/custom-tools/bin"],
}
```

These directories are prepended before the conventional ones, giving them highest priority.

### Environment Override

Set `PLANEAI_EXTRA_PATH` (colon-separated) to override `extra_path_dirs` without editing the config file:

```bash
export PLANEAI_EXTRA_PATH="$HOME/.guardrails/shims:$HOME/my-tools/bin"
```

When `PLANEAI_EXTRA_PATH` is set, `extra_path_dirs` from the config file is ignored.

## Integrations

### Jira

Jira is currently a **connection-only** bundled plugin. Configure the Jira Cloud site in **Preferences → Jira**, then use OAuth 2.0 with PKCE to connect. The plugin stores public settings in its own namespace and keeps OAuth credentials in backend-only plugin secrets.

Source sync, JQL filters, task import, writeback, and periodic polling are deferred to a later parity slice. Existing `integrations.jira` configuration and legacy tokens are intentionally not imported.

#### Authentication

Connect via **Preferences → Jira → Connect**. Building from source requires `JIRA_CLIENT_ID` and `JIRA_CLIENT_SECRET`; without them, the build succeeds but OAuth cannot work at runtime.

Jira source synchronization, including departed-issue handling, is not available in this release.
