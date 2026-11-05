---
title: Task Management
description: Create, organize, and dispatch tasks to AI agents — from the GUI, CLI, or directly from agent sessions.
---

planeai includes a built-in task tracker designed for AI agent workflows. Tasks represent units of work that can be manually picked up or automatically dispatched to agents. This guide covers the full lifecycle — from creating tasks to watching agents complete them.

## The task board

Tasks appear in the sidebar, sorted by priority (highest first), then by project order, then by task key. They are grouped by status:

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

### Cards and Classic layouts

The sidebar has two layouts, chosen under **Preferences → Appearance → Sidebar → Layout** or from the view options (right-click empty space or a section header in the sidebar).
Cards is the default.

In **Cards**, what needs you sits at the top: sessions that are not linked to a task, and tasks waiting for review.
Each is a card showing its project, its task key and title, and, when it has a session, the session's branch and agent.
The other tasks sit in shelves at the bottom of the list: **Running** as cards, **To do**, **Snoozed** and **Done** as slim rows.
Done starts collapsed and shows 25 tasks at a time; **Show 25 more** (also reachable from the keyboard) reveals the next ones.
A shelf with no tasks is not shown, and **Hide done tasks** removes the Done shelf.

The right side of a row shows what needs you: **Working** while its agent runs, **Unread** once the agent finished since you last opened the task, or **Exited**.
Rows with nothing to look at, such as idle sessions and tasks never started, are slightly faded until you hover or select them.
Hovering a row swaps its status for quick actions:

| Row                         | Actions                         |
| --------------------------- | ------------------------------- |
| Card with a running session | Review diff, Done               |
| Card with an exited session | Restart, Done                   |
| Card without a session      | Done                            |
| To do row                   | Start (starts a session)        |
| Done row                    | Reopen (moves it back to To do) |
| Snoozed row                 | Wake                            |

**Done** moves a task to Done, or archives a session that has no task.

### Unread tasks

A task turns **Unread** when one of its agents finishes a turn, asks for your attention, or exits, and you have not opened the task since.
It works for every task, with or without a worktree, and stays unread across restarts.
Opening the task, typing into one of its sessions, or coming back to the window while it is open marks it read.
A task you are looking at does not turn unread when its agent finishes.
A Done task is never unread.

Right-click a task and choose **Mark as unread** to keep it in view for later, or **Mark as read** to clear it without opening it.
With the sidebar focused, **u** does the same for the highlighted task.
Sessions that are not linked to a task show **Unread** too, until you open or type into them; that state lasts until PlaneAI quits.

### Snoozing tasks

Snooze a task to set it aside until later, without changing its status or stopping its agent.
Right-click a task and choose **Snooze**, then a time: **In 1 hour**, **In 3 hours**, **This evening** (18:00), **Tomorrow** (09:00) or **Next week** (Monday 09:00), soonest first.
Times that already passed today are left out, and the clock times follow your system's 12 or 24-hour format.
**Custom…** asks for a date and time, or an amount of minutes, hours or days from now.
In that dialog, **m** switches between the two, and **d**, **t**, **a** and **u** jump to the date, time, amount and unit.
Closing the picker or the dialog puts focus back where it was.
Press **⌘⌥Z** (macOS) or **Ctrl+Alt+Z** (Linux/Windows) to open the same choices for the open task, or **z** with the sidebar focused for the highlighted one; **1** to **5** pick a time, **C** opens Custom, and **Esc** closes the menu.

A snoozed task leaves its place, including the top of the list and its project folder, for the **Snoozed** shelf between To do and Done.
The shelf starts collapsed and lists every project's snoozed tasks, the soonest to wake first, each with the time left ("45m", "3h", "2d"); hover it to see when it wakes.
The task you have open stays listed even while the shelf is collapsed.
In Classic, each project gets a **Snoozed** group after its status groups, or, grouped by status, one Snoozed section before Done.

A snoozed task wakes on its own at its time, without a notification, and goes back to where its status puts it.
It also wakes early when one of its agents asks for your attention or exits, but not when an agent just finishes a turn.
A woken task shows **Woke** until you open it; click the pill to clear it without opening the task.
To wake a task yourself, use **Wake** on its row or in its menu, press **z** or **⌘⌥Z** again, or run `planeai-cli task wake`.
Moving a task to Done ends its snooze, and auto-dispatch leaves snoozed tasks alone until they wake.

**Classic** shows compact rows in collapsible status groups, as described below.

### Grouping by status

To see the work of every project in one list, group the sidebar by status.
Choose **Status** under **Preferences → Appearance → Sidebar** (⌘, / Ctrl+,), or right-click empty space or a group header in the sidebar to open the view options.

In Cards, grouping only changes the top of the list.
Grouped by project, the sessions and tasks waiting for review are split into one folder per project; a folder with nothing in it starts collapsed, and a collapsed folder shows a dot for its most urgent row (red for an exited session, amber for a review, green for a working agent).
Grouped by status, they form one list.
The shelves are the same either way and mix every project.

In Classic, the sidebar then shows a **Sessions** section first, holding sessions that are not linked to a task, followed by one section per status: Running, Needs review, To do, Snoozed, and Done.
Each section mixes the tasks of every visible project, sorted by priority, then by project order, then by task key.
Each row shows its project name before the task key.
Project actions, such as editing a project or toggling auto-dispatch, are only available when the sidebar is grouped by project.

The same places turn off task keys ("Show task keys") and project names ("Show project labels").
Project names apply when grouping by status or in Cards, where the option drops the project line from cards outside a project folder.
These choices are saved in `config.json` as `sidebar_layout`, `sidebar_group_by`, `hide_task_keys`, and `hide_project_labels`.
Done and Snoozed sections start collapsed, and the sidebar remembers which sections you collapse.

### Finding a task from the keyboard

**⌘1**-**⌘9** / **Ctrl+1**-**Ctrl+9** open the sidebar row at that position, counting from the top and skipping rows inside collapsed sections.
Hold **⌘** / **Ctrl** on its own for a moment to see each row's shortcut in place of its status; the hints disappear as soon as you release it or press another key.
**⌘}** and **⌘{** / **Ctrl+}** and **Ctrl+{** step through the same rows, top to bottom, and open the highlighted one when you release the modifier.

Open the command menu (**⌘K** / **Ctrl+K**) and start typing a task key or title. Tasks from every project are searchable, so you do not need an active session in the right project first. Selecting a task does the same thing as clicking it in the sidebar.

Tasks appear once you type — the menu's default view stays a short list of actions. Tasks in the project you are currently working in are listed first. Completed tasks are included unless "Hide done tasks" is enabled.

### Starting and renaming sessions

While a task is focused, every "new session" entry point (**⌘N** then **S**, "New session" in the command menu, the sidebar button) opens the form with that task and its project already selected.
The session name defaults to the task title; additional sessions on the same task are numbered, for example "Fix login (2)", and the name stays editable.

A task's work can span repositories.
Pick another project in the form and the task stays linked: the picker keeps it pinned with its project name, and a hint shows which project owns it.
The new agent runs in the other project's repository but opens in the task's workspace, next to its sibling agents, with the repository's project name shown on its tab.
If that repository lacks the task's base branch, the form uses the repository's default branch instead.

To rename a session, pick "Rename session" in the command menu or double-click its agent tab, type the new name, and press **Enter**.
The workspace tabs follow the new name.

## Creating tasks

There are three ways to create tasks, each suited to a different workflow.

### From the GUI

Press **⌘N** (macOS) or **Ctrl+N** (Linux/Windows) to open the new item modal, then press **T** to create a task. Alternatively, open the command menu (**⌘K** / **Ctrl+K**) and search for "create task". The create dialog has fields for:

- **Project** - which project the task belongs to (required). **New Project** (**⇧O** in normal mode) opens Add Project on top of the form without losing what you typed; the new project is then selected. Changing the project clears the parent, blockers and base branch, since they belong to the previous project.
- **Title** — short, actionable description (required). Prefilled with a random name such as `jubilant-waffle`; focusing the untouched name selects it, so typing replaces it.
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

### From agent sessions

This is the most powerful workflow: **agents create tasks for other agents**.

A running agent can create follow-up tasks directly.
When it finishes a piece of work and identifies follow-up items, it can:

- **Create a single task**: the agent describes the work, and it appears on your task board immediately
- **Break a plan into tasks**: the agent takes a plan or spec and produces a structured set of tasks with parent/subtask relationships and dependency ordering

This enables a flywheel: you describe a feature to one agent, it breaks the work into tasks, and auto-dispatch assigns those tasks to other agents running in parallel.

Claude Code and Codex sessions started by planeai get native `mcp__planeai__*` tools for tasks and sessions through the [MCP server](/planeai/guides/configuration/#planeai-tools-mcp).
Other agents use `planeai-cli axi`.

planeai also ships agent skills that teach either interface: they use the PlaneAI tools when the agent has them and the CLI otherwise.
Install them with:

```bash
skl install nicolegros/planeai --all
```

Once installed, agents that support skills (like Kiro or Claude) can use them automatically.

## Changing status from the keyboard

Press **⌘⌥S** (macOS) or **Ctrl+Alt+S** (Linux/Windows) to move the open task to done.
Moving a task to done archives its sessions and stops their agents, so it waits 5 seconds first.
The toast counts down, and its **Undo** during that time leaves everything as it was.
Dismissing the toast applies the move right away.
Quitting during the countdown applies it too, at the latest when PlaneAI next starts.
Press **⌘Z** / **Ctrl+Z** to undo the last status change, snooze or wake you made, once, from the shortcut, the sidebar, or a menu.
If that change is a move to done still counting down, it is cancelled; otherwise it is reverted while the task still shows it.
Undoing a snooze puts back the task's earlier snooze, if it had one; the snooze and wake toasts have their own **Undo** too.
Changes made outside the app, such as an early wake or the CLI, can't be undone this way.
A move to done can't be undone once it has applied.
Text fields and the editor keep **⌘Z** for their own undo.

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

Hooks move the linked task to a status when its agent session changes state.
New installs start with task management on and the hooks below set.
Configure them in **Preferences → Tasks** (⌘, / Ctrl+,) or directly in your `config.json`:

```jsonc
{
  "task_management": {
    "on_start": { "move_to": "in_progress" },
    "on_notify": { "move_to": "in_review" },
    "on_resume": { "move_to": "in_progress" },
    "on_restart": { "move_to": "in_progress" },
    "on_complete": { "move_to": "done" },
  },
}
```

| Hook          | Fires when                                          |
| ------------- | --------------------------------------------------- |
| `on_start`    | A session is created from the task                  |
| `on_notify`   | The agent goes idle and waits for you               |
| `on_resume`   | The agent's hook reports new work after going idle  |
| `on_restart`  | A linked session's exited agent is relaunched       |
| `on_complete` | A session linked to the task is archived or deleted |

A hook left unset never fires.
`on_resume` needs an agent with PlaneAI hooks installed, since terminal output alone cannot tell a redraw from new work.
It only moves a task whose status is still the `on_notify` target, and does nothing without `on_notify`.
Valid statuses are `todo`, `in_progress`, `in_review`, and `done`.

:::tip
All task management settings — templates, lifecycle hooks, and auto-dispatch — are available in **Preferences → Tasks**. You don't need to edit JSON if you prefer a GUI.
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

Auto-dispatch is enabled **per project**. With the sidebar grouped by project, right-click a project and select **Auto-dispatch** to toggle it on. When active, a ⚡ icon appears next to the project name.

You also need the global auto-dispatch configuration — either toggle it in **Preferences → Tasks** (⌘, / Ctrl+,) or set it in your `config.json`:

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
4. You review the diff (**⌘\\** / **Ctrl+\\**) and move the task to done (**⌘⌥S** / **Ctrl+Alt+S**)

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

The bundled Jira plugin manages OAuth connection and manual configured JQL-source synchronization. Syncing imports matching Jira issues as PlaneAI tasks and displays them in the Jira sidebar. Select an issue, choose a PlaneAI project, and PlaneAI creates a child task under the synced Jira task. Jira task writeback and periodic synchronization are deferred to a later parity slice.
