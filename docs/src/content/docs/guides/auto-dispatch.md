---
title: Auto-Dispatch (Symphony Mode)
description: Automatically dispatch tasks to AI agents based on priority and availability.
draft: false
---

Auto-dispatch (internally called Symphony Mode) polls your task board, picks the highest-priority ready task, and dispatches it to an available agent session — fully autonomously.

## Quick Start

```jsonc
{
  "auto_dispatch": {
    "enabled": true,
    "poll_interval_ms": 5000,
    "max_concurrent": 3,
    "provider": "kiro",
    "terminal_states": ["done", "cancelled"],
  },
}
```

## How It Works

The dispatch loop follows four phases:

1. **Poll** — checks the task board for tasks in a ready state on each interval
2. **Dispatch** — assigns the highest-priority ready task to a free session slot
3. **Reconcile** — monitors running sessions for completion signals
4. **Notify** — fires lifecycle hooks and updates task status

## Configuration Reference

| Field              | Type       | Default    | Description                             |
| ------------------ | ---------- | ---------- | --------------------------------------- |
| `enabled`          | `boolean`  | `false`    | Enable auto-dispatch                    |
| `poll_interval_ms` | `number`   | `5000`     | Milliseconds between task board polls   |
| `max_concurrent`   | `number`   | `3`        | Maximum simultaneous agent sessions     |
| `provider`         | `string`   | —          | Provider to use for dispatched sessions |
| `terminal_states`  | `string[]` | `["done"]` | Task statuses that mean "finished"      |

## Behavior Details

### Yolo Mode

When auto-dispatch launches a session, it appends the provider's `yolo_flag` so agents run without asking for confirmation. This enables fully unattended execution.

### Git Worktrees

Each dispatched task gets its own git worktree, created automatically from the current `main` branch. Tasks run in complete isolation — no merge conflicts between parallel agents.

### Concurrency

`max_concurrent` caps the number of simultaneously running sessions. When a slot opens (task completes or is cancelled), the next ready task is dispatched.

### Blocked Tasks

Tasks with unmet dependencies (blockers) are skipped during polling. They become eligible once all blockers reach a terminal state.

### Reconciliation

planeai watches each session for completion markers (exit code 0, or a configurable output pattern). On detection, it moves the task to `done` and fires `on_complete`.

### No Auto-Retry

Failed tasks are **not** retried automatically. They move to a `failed` state and require manual intervention or an explicit restart.

### Startup Recovery

On app launch, planeai re-attaches to any tmux sessions from a previous run and resumes reconciliation. No work is lost across restarts.

:::tip
Start with `max_concurrent: 1` until you're comfortable with the dispatch behavior, then increase.
:::

## GUI Indicators

- **Session badge** — shows "auto" for dispatched sessions
- **Task board** — live status with elapsed time per task
- **Notification bell** — alerts on task completion or failure

## CLI Commands

Control auto-dispatch from the command menu (Cmd+K):

| Command           | Description                 |
| ----------------- | --------------------------- |
| `dispatch:start`  | Start the dispatch loop     |
| `dispatch:stop`   | Pause dispatching           |
| `dispatch:status` | Show current dispatch state |
| `dispatch:next`   | Manually dispatch next task |

:::note
CLI commands are also available via keyboard shortcuts configurable in the settings.
:::
