---
title: CLI Reference
description: Command reference for planeai-cli — manage sessions, projects, tasks, and the orchestrator from the terminal.
---

`planeai-cli` is a companion CLI that lets you script and automate planeai from the terminal. Most commands output JSON by default and accept a `--pretty` flag for human-readable output. The `axi` subcommand outputs TOON (a token-efficient text format) for agent consumption.

## Install

The CLI is installed from within the app via **Preferences → CLI** or with the system installer bundled with each release.

Once installed, it's available as `planeai-cli` on your PATH.

## Global Behavior

| Behavior      | Details                                                    |
| ------------- | ---------------------------------------------------------- |
| Output format | JSON (single line) by default                              |
| Pretty output | Add `--pretty` to any command for indented JSON or tables  |
| Errors        | Printed to stderr as `{"error": "..."}`, exits with code 1 |
| Database      | Uses the same SQLite database as the desktop app           |
| Config        | Reads `~/.config/planeai/config.json` (same as the app)    |

---

## `session`

Manage agent sessions.

### `session create`

Create and launch a new agent session.

```bash
planeai-cli session create --project <name> --branch <branch> [options]
```

| Flag            | Description                                                                  |
| --------------- | ---------------------------------------------------------------------------- |
| `--project`     | Project name (required)                                                      |
| `--branch`      | Git branch to use (required)                                                 |
| `--name`        | Display name for the session                                                 |
| `--new-branch`  | Create the branch if it doesn't exist                                        |
| `--worktree`    | Use a git worktree instead of checking out in-place                          |
| `--base-branch` | Base branch for new branch / worktree (default: main)                        |
| `--yolo`        | Enable autonomous mode (skip confirmations)                                  |
| `--provider`    | Provider to use (overrides default_provider)                                 |
| `--task-key`    | Associate a task key with this session                                       |
| `--prompt`      | Initial prompt to send to the agent                                          |
| `--parent`      | Parent session ID (for orchestration tracking). Falls back to `$PLANEAI_SESSION_ID` env var |

### `session ls`

List active sessions.

```bash
planeai-cli session ls [--archived] [--pretty]
```

| Flag         | Description                 |
| ------------ | --------------------------- |
| `--archived` | Show archived sessions only |

### `session delete`

Permanently destroy a session and clean up its resources (worktree, tmux/daemon process).

```bash
planeai-cli session delete <id>
```

The `id` can be a prefix — it will match if unambiguous.

### `session archive`

Archive a session (stops the agent but preserves the record).

```bash
planeai-cli session archive <id>
```

### `session prompt`

Send a prompt to a running session.

```bash
planeai-cli session prompt <id> [text]
```

If `text` is omitted, the prompt is read from stdin. This is useful for piping multi-line prompts:

```bash
echo "Refactor the auth module" | planeai-cli session prompt abc123
```

Prompts are serialized per session — only one prompt can be in flight at a time. If a concurrent prompt is already being sent to the same session, the command fails immediately with an error. Concurrent prompts to different sessions proceed independently.

---

## `project`

Manage registered projects.

### `project list`

List all registered projects.

```bash
planeai-cli project list [--pretty]
```

---

## `task`

Built-in task tracker. Tasks are scoped to a project (resolved from `--project` or the current working directory).

### `task add`

Create a new task.

```bash
planeai-cli task add <title> [options]
```

| Flag            | Description                                    |
| --------------- | ---------------------------------------------- |
| `--desc`        | Task description (default: empty)              |
| `--priority`    | Priority number (default: 0)                   |
| `--tags`        | Comma-separated tags                           |
| `--blocked-by`  | Comma-separated task keys that block this task |
| `--parent`      | Parent task key (for subtasks)                 |
| `--base-branch` | Base branch for this task (default: main)      |
| `--project`     | Project name (otherwise resolved from CWD)     |

### `task show`

Show a task by key.

```bash
planeai-cli task show <key> [--project <name>]
```

### `task ls`

List tasks with optional filters.

```bash
planeai-cli task ls [--status <status>] [--tags <tags>] [--project <name>]
```

| Flag       | Description                                                  |
| ---------- | ------------------------------------------------------------ |
| `--status` | Filter by status: `todo`, `in_progress`, `in_review`, `done` |
| `--tags`   | Comma-separated tags to filter by                            |

### `task move`

Move a task to a new status.

```bash
planeai-cli task move <key> <status>
```

Valid statuses: `todo`, `in_progress`, `in_review`, `done`.

### `task edit`

Edit an existing task.

```bash
planeai-cli task edit <key> [options]
```

| Flag            | Description                  |
| --------------- | ---------------------------- |
| `--title`       | New title                    |
| `--desc`        | New description              |
| `--priority`    | New priority                 |
| `--tags`        | Replace tags (comma-sep)     |
| `--blocked-by`  | Replace blockers (comma-sep) |
| `--base-branch` | New base branch              |

### `task delete`

Delete a task.

```bash
planeai-cli task delete <key> [--project <name>]
```

---

## `symphony`

Control the auto-dispatch orchestrator.

### `symphony status`

Show orchestrator status (running sessions, concurrency).

```bash
planeai-cli symphony status
```

### `symphony stop`

Stop the orchestrator.

```bash
planeai-cli symphony stop
```

---

## `axi`

Agent eXperience Interface — token-efficient TOON output designed for autonomous agents. Use `planeai-cli axi` instead of the JSON commands when building agent integrations.

Running `planeai-cli axi` with no subcommand prints a context-aware home view (current project, open tasks, active sessions).

### `axi task ls`

List tasks (TOON tabular output).

```bash
planeai-cli axi task ls [--status <status>] [--tags <tags>] [--project <name>]
```

### `axi task show`

Show task details.

```bash
planeai-cli axi task show <key> [--project <name>]
```

### `axi task add`

Create a new task.

```bash
planeai-cli axi task add <title> [--desc "..."] [--priority <int>] [--tags <a,b>] [--blocked-by <K1,K2>] [--parent <KEY>] [--project <name>]
```

### `axi task move`

Move a task to a new status.

```bash
planeai-cli axi task move <key> <status> [--project <name>]
```

Valid statuses: `todo`, `in_progress`, `in_review`, `done`.

### `axi session ls`

List sessions.

```bash
planeai-cli axi session ls [--archived]
```

### `axi session create`

Create a new session. Automatically sets the parent session from the `$PLANEAI_SESSION_ID` environment variable if present (for orchestration tracking).

```bash
planeai-cli axi session create --project <name> --branch <branch> [options]
```

| Flag            | Description                                           |
| --------------- | ----------------------------------------------------- |
| `--project`     | Project name (required)                               |
| `--branch`      | Git branch to use (required)                          |
| `--name`        | Display name for the session                          |
| `--new-branch`  | Create the branch if it doesn't exist                 |
| `--worktree`    | Use a git worktree instead of checking out in-place   |
| `--base-branch` | Base branch for new branch / worktree (default: main) |
| `--yolo`        | Enable autonomous mode (skip confirmations)           |
| `--provider`    | Provider to use (overrides default_provider)          |
| `--task-key`    | Associate a task key with this session                |
| `--prompt`      | Initial prompt to send to the agent                   |

### `axi session read`

Read the last N lines of a session's terminal output (ANSI-stripped).

```bash
planeai-cli axi session read <id> [--lines <n>]
```

| Flag      | Description                          |
| --------- | ------------------------------------ |
| `--lines` | Number of lines to read (default: 100) |

Works with both daemon and tmux backends. The `id` can be a prefix.

### `axi session prompt`

Send a prompt to a running session.

```bash
planeai-cli axi session prompt <id> [text]
```

If `text` is omitted, reads from stdin.

Prompts are serialized per session. If another prompt is already in flight, the command returns a TOON error with a retry hint:

```
error: session prompt already in progress
help[1]:
  - retry after the current prompt is sent
```

### `axi project ls`

List registered projects.

```bash
planeai-cli axi project ls
```

---

## Examples

Create a session with a worktree in autonomous mode:

```bash
planeai-cli session create \
  --project myapp \
  --branch feat/auth \
  --new-branch \
  --worktree \
  --yolo \
  --prompt "Implement JWT authentication"
```

Dispatch a task to an agent:

```bash
planeai-cli task add "Add pagination to /users" \
  --desc "Support limit/offset query params" \
  --tags backend,api \
  --priority 1
planeai-cli task move PLA-1 in_progress
```

List sessions as a formatted table:

```bash
planeai-cli session ls --pretty
```
