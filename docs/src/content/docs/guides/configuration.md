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

planeai can sync issues from Jira Cloud into the local task board and write status changes back to Jira. Configured via the `integrations.jira` block.

```jsonc
{
  "integrations": {
    "jira": {
      // Your Jira Cloud site URL
      "site": "https://mycompany.atlassian.net",
      // How often to poll Jira (milliseconds, default: 60000)
      "sync_interval_ms": 60000,
      // Named sync sources — each is a JQL filter
      "sources": {
        "my-sprint": {
          // JQL query selecting which issues to sync
          "jql": "project = ENG AND assignee = currentUser() AND sprint in openSprints()",
          // Map Jira status names to planeai statuses
          "status_map": {
            "In Progress": "in_progress",
            "In Review": "in_review",
            "Done": "done",
          },
          // Optional: write local status changes back to Jira
          "writeback": {
            // Transition the Jira issue to this status when work starts
            "on_start": "In Progress",
            // Transition the Jira issue to this status when work completes
            "on_complete": "Done",
            // Add a comment to the Jira issue on each transition
            "comment": true,
          },
        },
      },
    },
  },
}
```

| Field                         | Description                                                    |
| ----------------------------- | -------------------------------------------------------------- |
| `site`                        | Jira Cloud site URL (e.g., `https://myco.atlassian.net`)       |
| `sync_interval_ms`            | Polling interval in milliseconds (default: 60000)              |
| `sources`                     | Named JQL filters to sync                                      |
| `sources.<name>.jql`          | JQL query selecting issues to import                           |
| `sources.<name>.status_map`   | Map of Jira status → planeai status (`todo`, `in_progress`, `in_review`, `done`) |
| `sources.<name>.writeback`    | Optional writeback configuration                               |
| `writeback.on_start`          | Jira status to transition to when work starts locally          |
| `writeback.on_complete`       | Jira status to transition to when work completes locally       |
| `writeback.comment`           | Whether to add a comment on each transition (default: false)   |

#### Authentication

Jira integration uses OAuth 2.0 with PKCE. Connect via **Preferences → Jira → Connect to Jira**, which opens the Atlassian consent screen in your browser. Tokens are stored locally in the app data directory.

:::note
Building from source requires `JIRA_CLIENT_ID` and `JIRA_CLIENT_SECRET` environment variables (see `.env.example`). Without them the build succeeds but OAuth will not work at runtime.
:::

#### Departed issues

When a synced issue no longer matches its source JQL (e.g., it was reassigned or moved to another project), planeai marks it as "departed" and shows a prompt asking whether to mark the local task as done or dismiss it.
