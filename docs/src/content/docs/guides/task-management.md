---
title: Task Management
description: Create, organize, and dispatch tasks to AI agents — from the GUI, CLI, or directly from agent sessions.
---

planeai includes a built-in task tracker designed for AI agent workflows. Tasks represent units of work that can be manually picked up or automatically dispatched to agents. This guide covers the full lifecycle — from creating tasks to watching agents complete them.

## The task board

Tasks appear in the sidebar, grouped by project and status. Each status group shows tasks sorted by priority (highest first):

| Status          | Meaning                                |
| --------------- | -------------------------------------- |
| **Todo**        | Ready to be worked on                  |
| **In Progress** | An agent is actively working on it     |
| **In Review**   | Work is done, waiting for human review |
| **Done**        | Completed                              |

Click any task to interact with it:

- If the task has a linked session → jumps to that session's terminal
- If the task has no session → starts a new session for it (creates a worktree, launches the agent, and sends the task description as the initial prompt)

Right-click a task to open the context menu with quick actions: start session, edit, or move to a different status.

## Creating tasks

There are three ways to create tasks, each suited to a different workflow.

### From the GUI

Press **⌘N** (macOS) or **Ctrl+N** (Linux/Windows) to open the new item modal, then press **T** to create a task. Alternatively, open the command menu (**⌘K** / **Ctrl+K**) and search for "create task". The create dialog has fields for:

- **Title** — short, actionable description (required)
- **Description** — detailed context for the agent. Be thorough — the agent relies entirely on this.
- **Priority** — numeric value. Higher priority tasks get dispatched first.
- **Base branch** — which git branch to create the worktree from (defaults to `main`)

:::tip
Write descriptions as if handing off to a colleague who has never seen your codebase. Include file paths, acceptance criteria, and relevant context. The more detail you provide, the better the agent performs.
:::

### From the CLI

Use `planeai-cli` for scripting or quick additions from the terminal:

```bash
planeai-cli task add "Fix Safari login redirect" \
  --desc "Nil pointer in auth.go:42 when OAuth callback returns without state param" \
  --priority 1 \
  --tags auth,bugfix
```

See the [CLI Reference](/planeai/reference/cli/) for the full set of task commands (`task add`, `task ls`, `task show`, `task move`, `task edit`, `task delete`).

### From agent sessions (skills)

This is the most powerful workflow: **agents create tasks for other agents**.

planeai ships with agent skills that let a running agent create follow-up tasks directly. When an agent finishes a piece of work and identifies follow-up items, it can use the built-in skills to:

- **Create a single task** — the agent describes the work, and it appears on your task board immediately
- **Break a plan into tasks** — the agent takes a plan or spec and produces a structured set of tasks with parent/subtask relationships and dependency ordering

This enables a flywheel: you describe a feature to one agent, it breaks the work into tasks, and auto-dispatch assigns those tasks to other agents running in parallel.

Install the skills with:

```bash
skl install nicolegros/planeai --all
```

Once installed, agents that support skill/tool loading (like Kiro or Claude) can use them automatically.

## Task fields in detail

| Field           | Purpose                                                                         |
| --------------- | ------------------------------------------------------------------------------- |
| **Key**         | Auto-assigned identifier (e.g., `PLA-1`). Used to reference tasks everywhere.   |
| **Title**       | Short, actionable. Starts with a verb.                                          |
| **Description** | Full context for the agent. Include file paths, acceptance criteria, decisions. |
| **Priority**    | Numeric (0 = default). Higher values get dispatched before lower ones.          |
| **Status**      | `todo` → `in_progress` → `in_review` → `done`                                   |
| **Blocked by**  | Task keys that must complete first. Blocked tasks are skipped by auto-dispatch. |
| **Parent**      | Groups subtasks under a parent for hierarchy.                                   |
| **Tags**        | Comma-separated labels for filtering (e.g., `backend`, `auth`, `tech-debt`).    |
| **Base branch** | Git branch used as the starting point for the task's worktree.                  |

## Linking tasks to sessions

When you click a task that has no session, planeai:

1. Creates a git worktree from the task's base branch
2. Launches a new agent session in that worktree
3. Sends the task description as the initial prompt (using the provider's `autonomous_prompt_template`)
4. Links the session to the task — status indicators appear in the sidebar

The session and task stay linked. When the agent signals completion, the task moves to `in_review` or `done` depending on your lifecycle hooks.

## Lifecycle hooks

Hooks run shell commands at task state transitions. Configure them in **Preferences → Task Management** (⌘, / Ctrl+,) or directly in your `config.json`:

```jsonc
{
  "task_manager": {
    "lifecycle_hooks": {
      "on_start": "echo 'Starting {{task.key}}'",
      "on_complete": "git add -A && git commit -m 'feat({{task.key | slugify}}): {{task.title}}'",
      "on_notify": "say '{{task.key}} needs attention'",
      "on_restart": "git stash && git pull --rebase",
    },
  },
}
```

| Hook          | Fires when                                                |
| ------------- | --------------------------------------------------------- |
| `on_start`    | A task is dispatched to an agent session                  |
| `on_complete` | The agent signals it's done (session archived or deleted) |
| `on_notify`   | The agent signals idle (needs attention)                  |
| `on_restart`  | A failed task's session is restarted                      |
| `on_pr_open`  | A pull request is opened for the session's branch         |
| `on_pr_merge` | The pull request is merged                                |

Hooks run in the working directory of the task's git worktree. They support the same `{{template}}` syntax as other planeai templates (see [Configuration](/planeai/guides/configuration/)).

:::tip
All task management settings — templates, lifecycle hooks, and auto-dispatch — are available in **Preferences → Task Management**. You don't need to edit JSON if you prefer a GUI.
:::

## Auto-dispatch: the full lifecycle

When [auto-dispatch](/planeai/guides/auto-dispatch/) is enabled, tasks flow through the system without manual intervention:

1. **You create tasks** — from the GUI, CLI, or via agent skills
2. **Dispatch polls the board** — every few seconds, it looks for the highest-priority `todo` task with no unmet blockers
3. **A session is launched** — planeai creates a worktree, starts an agent with `yolo_flag` (no confirmations), and sends the task description
4. **The agent works** — you can watch in real-time or ignore it
5. **Completion is detected** — when the agent exits cleanly, `on_complete` fires (e.g., auto-commit)
6. **The slot opens** — the next ready task is dispatched

### Enabling auto-dispatch

Auto-dispatch is enabled **per project**. Right-click a project in the sidebar and select **Auto-dispatch** to toggle it on. When active, a ⚡ icon appears next to the project name.

You also need the global auto-dispatch configuration — either toggle it in **Preferences → Task Management** (⌘, / Ctrl+,) or set it in your `config.json`:

```jsonc
{
  "auto_dispatch": {
    "enabled": true,
    "poll_interval_ms": 5000,
    "max_concurrent": 3,
    "provider": "claude",
  },
}
```

The `max_concurrent` setting caps how many agents run simultaneously across all projects. Start with 1 to get comfortable, then increase.

### Manual dispatch

You don't have to enable auto-dispatch to use tasks. The manual workflow is:

1. Create tasks on the board
2. Click a task to start a session for it
3. The agent receives the task description and works on it
4. You review the diff (**⌘D** / **Ctrl+D**) and move the task to done

This gives you full control over when and how agents pick up work.

## Tips

:::tip
Use **parent tasks** to group related work. Create a parent like "Implement auth system" with subtasks for each vertical slice. Auto-dispatch respects `blocked_by` — subtasks won't start until their dependencies complete.
:::

:::tip
High-priority tasks always dispatch first. Use priority to ensure critical bugs get picked up before nice-to-have features, even if the feature was created first.
:::

:::note
Tasks are stored locally in planeai's SQLite database, scoped to each project. For external issue tracking, see the [Jira integration](#jira-integration) below.
:::

## Jira Integration

The bundled Jira plugin currently provides OAuth connection management only. It does not yet sync issues, show Jira issues in the sidebar, assign them to projects, or write task status changes back to Jira. Those workflows are deferred to a later parity slice.
