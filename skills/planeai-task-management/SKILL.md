---
name: planeai-task-management
description: "Manage tasks in the planeai task tracker — update, move, delete, list, and show tasks. Use when the user wants to edit a task, change task status, move a task, delete a task, list tasks, view task details, or any general task management operation that is NOT creating new tasks or breaking plans into tasks."
---

# Task Management

Manage existing tasks using `planeai-cli`. Covers viewing, updating, moving, and deleting tasks.

## When to use

- User wants to see tasks: "show me PLA-3", "list my tasks", "what's in progress?"
- User wants to update a task: "rename PLA-2", "change the description", "add tags", "update priority"
- User wants to move a task: "mark PLA-1 as done", "move to in_progress", "start PLA-4"
- User wants to delete a task: "delete PLA-5", "remove that task"

## When NOT to use

- Creating a new task from scratch → use `planeai-create-task`
- Breaking a plan into multiple tasks → use `planeai-plan-to-tasks`

## CLI Reference

Use the AXI interface for token-efficient TOON output:

```
planeai-cli axi task show <key>                   # View task details
planeai-cli axi task ls [flags]                   # List tasks
planeai-cli axi task move <key> <status>          # Change task status
```

For edit and delete (not yet in axi), use the JSON interface:

```
planeai-cli task edit <key> [flags]               # Update task fields
planeai-cli task delete <key>                     # Delete a task
```

## Operations

### Show a task

```bash
planeai-cli axi task show <key>
```

Use when the user asks to see, view, or inspect a task.

### List tasks

```bash
planeai-cli axi task ls [--status <status>] [--tags <a,b>] [--project <name>]
```

Use when the user asks what tasks exist, what's in a certain status, or wants an overview. Omit flags to list all tasks in the current project.

### Edit a task

```bash
planeai-cli task edit <key> [--title "..."] [--desc "..."] [--priority <int>] [--tags <a,b>] [--blocked-by <K1,K2>]
```

Only pass flags for fields the user wants to change. Don't overwrite fields unnecessarily.

### Move a task

```bash
planeai-cli axi task move <key> <status>
```

Status values: `todo`, `in_progress`, `done`, `cancelled`.

Common phrasings mapped to statuses:

- "start", "working on", "pick up" → `in_progress`
- "done", "finish", "complete", "close" → `done`
- "cancel", "drop", "won't do" → `cancelled`
- "reopen", "back to todo" → `todo`

### Delete a task

```bash
planeai-cli task delete <key>
```

Confirm with the user before deleting unless they were explicit (e.g., "delete PLA-5").

## Guidelines

- When the user references a task by key, use that key directly.
- When the user references a task vaguely ("that auth task"), run `planeai-cli axi task ls` to find it, then confirm the key before acting.
- After any operation, briefly confirm what happened (e.g., "Moved **PLA-3** to done.").
- If a command fails, report the error and suggest a fix.
- Use `--project <name>` only when the user names a specific project or context makes it clear.
