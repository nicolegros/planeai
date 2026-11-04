---
name: planeai-task-management
description: "Manage tasks in the planeai task tracker: update, move, delete, list, and show tasks, and start sibling agent sessions. Use when the user wants to edit a task, change task status, move a task, delete a task, list tasks, view task details, or any general task management operation that is NOT creating new tasks or breaking plans into tasks."
---

# Task Management

View, update, move and delete existing tasks, and start sibling agent sessions.

## Which interface to use

- **PlaneAI tools available** (`mcp__planeai__*`, as in Claude Code and Codex sessions started by PlaneAI): use them.
  They may be deferred: load them with your tool search first.
  Inside a session, `key` and `project` default to your session's own task.
- **Otherwise**: use `planeai-cli`.

| Operation     | PlaneAI tool                                                                        | CLI                                                                                                                                |
| ------------- | ----------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| Show a task   | `task_show { key }`                                                                 | `planeai-cli axi task show <key>`                                                                                                  |
| List tasks    | `task_list { status?, tags?, project? }`                                            | `planeai-cli axi task ls [--status <s>] [--tags <a,b>] [--project <name>]`                                                         |
| Edit a task   | `task_update { key, title?, description?, priority?, tags?, blocked_by?, parent? }` | `planeai-cli task edit <key> [--title ...] [--desc ...] [--priority <int>] [--tags <a,b>] [--blocked-by <K1,K2>] [--parent <KEY>]` |
| Move a task   | `task_update { key, status }`                                                       | `planeai-cli axi task move <key> <status>`                                                                                         |
| Delete a task | `task_delete { key }`                                                               | `planeai-cli task delete <key>`                                                                                                    |

`tags` and `blocked_by` replace the existing values, and an empty `parent` clears it.

## When NOT to use

- Creating a new task from scratch: use `planeai-create-task`
- Breaking a plan into multiple tasks: use `planeai-plan-to-tasks`

## Operations

### Show or list

Use show when the user asks to see, view or inspect a task.
Use list when they ask what tasks exist or what's in a status; with no filter it lists the current project.

### Edit

Only pass the fields the user wants to change.

### Move

Statuses: `todo`, `in_progress`, `in_review`, `done`.

- "start", "working on", "pick up" → `in_progress`
- "ready for review", "in review" → `in_review`
- "done", "finish", "complete", "close" → `done`
- "reopen", "back to todo" → `todo`

### Delete

Confirm with the user before deleting unless they were explicit (e.g. "delete PLA-5").

## Guidelines

- When the user references a task by key, use that key directly.
- When the user references a task vaguely ("that auth task"), list tasks to find it, then confirm the key before acting.
- After any operation, briefly confirm what happened (e.g. "Moved **PLA-3** to done.").
- If a call fails, report the error and suggest a fix.
- Pass a project only when the user names one or context makes it clear.

## Start a sibling agent session on the current task

When you run inside a PlaneAI agent session and need parallel work on the same task, start a sibling session instead of an unlinked one.
It gets its own worktree and branch, inherits your project and task, and records your session as its parent.

```
mcp__planeai__session_create { "mode": "sibling", "prompt": "Investigate the historical data and report the impact" }
```

```bash
planeai-cli axi session spawn --prompt "Investigate the historical data and report the impact"
```

Both also take a name, branch, base branch, provider and yolo setting.
The tool's yolo defaults to your own session's setting; `axi session spawn` defaults to yolo, so pass `--yolo false` to opt out.
The CLI needs `PLANEAI_SESSION_ID`, and both fail outside a task-linked PlaneAI session rather than creating an unlinked session.
