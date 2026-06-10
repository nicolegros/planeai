# Auto-Dispatch (Symphony Mode)

Auto-dispatch turns planeai into an autonomous orchestrator. Instead of manually creating sessions for each task, planeai watches your task board and spawns agent sessions automatically — one per task, each in its own worktree.

## Quick Start

1. Configure a task manager in `~/.config/planeai/config.json` with an `auto_dispatch` section:

```jsonc
{
  "task_managers": {
    "kanban": {
      "get_task": "kanban show {key}",
      "move_task": "kanban move {key} {status}",
      "list_tasks": "kanban list --status todo --project {project}",
      "templates": {
        "prompt": "Implement task {key}: {title}\n\n{description}",
      },
      "on_start": { "move_to": "in_progress" },
      "on_notify": { "move_to": "in_review" },
      "auto_dispatch": {
        "poll_interval_ms": 30000,
        "max_concurrent": 3,
        "provider": "kiro",
        "terminal_states": ["done", "cancelled"],
      },
    },
  },
}
```

2. Right-click a project in the sidebar → select **Auto-dispatch**. A ⚡ icon appears next to the project name.

3. The orchestrator daemon starts automatically. It polls your task board every 30 seconds and spawns sessions for eligible tasks.

## How It Works

```
[Task Board] → poll → [Orchestrator] → dispatch → [Worktree + Agent Session]
                            ↕
                    [reconcile on each tick]
```

- **Poll**: runs `list_tasks` to find work, filters out blocked and terminal-state tasks
- **Dispatch**: creates a git worktree, spawns a tmux session with the agent, moves the task to `in_progress`
- **Reconcile**: checks if running sessions' tasks moved to a terminal state (done/cancelled) — if so, kills the session
- **Notify**: when the agent signals idle, the task moves to `in_review` and the session stays alive for human inspection

## Configuration Reference

Add `auto_dispatch` inside any task manager definition:

```jsonc
"auto_dispatch": {
  "poll_interval_ms": 30000,   // How often to poll for new tasks (default: 30s)
  "max_concurrent": 3,         // Max parallel sessions (counts active + exited non-archived)
  "provider": "kiro",          // Agent provider (falls back to default_provider)
  "terminal_states": ["done", "cancelled"]  // States that mean "task is finished"
}
```

| Field              | Default                             | Description                                             |
| ------------------ | ----------------------------------- | ------------------------------------------------------- |
| `poll_interval_ms` | `30000`                             | Milliseconds between task board polls                   |
| `max_concurrent`   | `3`                                 | Maximum auto-dispatched sessions running simultaneously |
| `provider`         | `default_provider`                  | Which provider to use for auto-dispatched sessions      |
| `terminal_states`  | `["done", "cancelled", "canceled"]` | Task states that trigger session kill on reconciliation |

### list_tasks with {project}

When auto-dispatch is enabled, the `list_tasks` command supports a `{project}` template variable. This is substituted with the project name so your CLI can filter tasks per project:

```jsonc
"list_tasks": "kanban list --status todo --project {project}"
```

## Behavior Details

### Yolo mode

Auto-dispatched sessions always run in yolo mode (auto-approve tool use). Unattended agents can't ask for confirmation — if they stall, you review manually.

### Worktrees

Every auto-dispatched session gets its own git worktree at `~/.planeai/worktrees/<project>/<short-id>/`. This allows full parallel execution without git conflicts.

### Concurrency

Concurrency is counted as all non-archived auto-dispatched sessions (both active and exited). To free a slot, archive a completed session from the sidebar.

### Blocked tasks

Tasks with unresolved blockers are skipped. A blocker is resolved when its status is in `terminal_states`. The orchestrator checks blockers by cross-referencing the task list, falling back to `get_task` for blockers not in the current list.

### Reconciliation

Every poll tick, the orchestrator checks if running sessions' tasks have moved to a terminal state (e.g., someone manually marked a task "done" or "cancelled"). If so, the session is killed immediately and the slot is freed.

### No auto-retry

If an agent fails (crashes, non-zero exit), the session is marked as exited and a notification appears in the GUI. You decide whether to restart it manually. This avoids burning resources on permanently broken tasks.

### Startup recovery

When the daemon restarts, it re-reads active auto-dispatched sessions from the database and resumes tracking them. Tmux sessions survive daemon restarts; direct PTY sessions are marked exited and re-dispatched if the task is still active.

## GUI

### Titlebar indicator

When the orchestrator is running, the titlebar shows:

```
⚡ 2/3
```

This means 2 of 3 available slots are in use.

### Sidebar

- Projects with auto-dispatch enabled show a ⚡ icon next to their name
- Auto-dispatched sessions appear in the sidebar like any other session
- Right-click a project → toggle "Auto-dispatch" on/off

### Enabling/disabling

Toggle auto-dispatch per project via right-click context menu. When you enable it:

- The daemon starts (if not already running)
- Polling begins on the next tick
- Existing "todo" tasks are dispatched immediately

When you disable it on all projects:

- The daemon continues running until stopped
- No new tasks are dispatched for that project
- Already-running sessions continue until completion

## CLI

### Check orchestrator status

```sh
planeai-cli symphony status
```

Returns JSON:

```json
{ "running": ["KAN-1", "KAN-3"], "max_concurrent": 3, "slots_used": 2, "active": true }
```

### Stop the orchestrator

```sh
planeai-cli symphony stop
```

Running sessions continue (they're tmux-backed). The daemon just stops polling and dispatching.

## Architecture

The orchestrator runs as a separate binary (`planeai-symphony`) communicating via:

- **Shared SQLite database** — sessions, projects, auto_mode flag
- **`notify.sock`** — daemon → GUI event notifications (session_created)
- **`symphony.sock`** — control socket (status, stop commands from CLI/GUI)

The daemon is automatically launched by the GUI when needed and runs detached (survives app restarts).
