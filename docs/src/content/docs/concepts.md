---
title: Concepts
description: The mental model behind planeai — sessions, providers, backends, projects, tasks, and orchestration.
---

planeai orchestrates multiple AI coding agents in parallel. Before diving into configuration and workflows, it helps to understand how the core pieces fit together: **projects** contain **tasks**, **sessions** run **providers** on a **backend**, and **auto-dispatch** ties it all together by assigning tasks to sessions automatically.

## Sessions

A session is a persistent terminal running an AI agent. Each session has its own shell, its own working directory, and (optionally) its own git branch or worktree. You can have as many sessions running in parallel as your machine can handle.

Sessions have a lifecycle: they start when you create them (or when auto-dispatch launches one), they run while the agent is working, and they can be archived or deleted when the work is done. If the agent process exits, the session enters an "exited" state but remains visible so you can restart it or inspect its output.

Sessions can be linked to a task. When a session is linked, planeai tracks which task it's working on, and the sidebar shows agent status (busy, idle, needs review) next to the task.

## Providers

A provider is the configuration layer between planeai and an AI agent CLI. It tells planeai how to start a session, how to send a prompt to a running agent, and what flags to use in autonomous mode.

planeai ships with no hardcoded agent — it works with any CLI that accepts text input. You configure providers for the agents you use (Kiro, Claude, Copilot, or anything else). Each provider defines:

- **command** — how to start a new agent session
- **prompt_command** — how to send a message to an existing session
- **yolo_flag** — what flag enables autonomous mode (skipping confirmations)
- **autonomous_prompt_template** — how task details are formatted when dispatched

You can have multiple providers configured and choose which one to use per session or per dispatch rule.

## Session Backends

The backend controls how planeai manages terminal processes behind the scenes. There are three options:

**Local** (default) — runs the agent as an in-process PTY. Lightweight, no dependencies, but sessions terminate when you close the app. Best for most users who keep the app open while working.

**tmux** — delegates session management to tmux. Sessions survive app restarts because tmux keeps them alive in the background. Requires tmux installed on your system. Not available on Windows.

**Daemon** (experimental) — a built-in background process that persists sessions across app restarts without requiring tmux. Still under active development and may have stability issues.

Choose based on whether you need sessions to survive app closures. If you do, use tmux (stable) or daemon (experimental). If not, local is simpler.

## Projects

A project is a registered directory on your filesystem — typically a git repository. Registering a project tells planeai where your code lives so it can create sessions, worktrees, and tasks scoped to that codebase.

The sidebar organizes everything by project: tasks appear under the project they belong to, and sessions are grouped by their associated project. You can have multiple projects registered simultaneously.

## Tasks

Tasks are planeai's built-in work tracker. Each task represents a unit of work — a bug to fix, a feature to build, a refactor to perform. Tasks live inside a project and have:

- **Key** — auto-assigned identifier (e.g., `PLA-1`, `PLA-2`)
- **Title** — short, actionable description
- **Description** — detailed context for the agent that will work on it
- **Status** — `todo`, `in_progress`, `in_review`, or `done`
- **Priority** — numeric priority (higher = more important for dispatch)
- **Blocked by** — other task keys that must complete before this one is eligible
- **Parent** — optional parent task for subtask hierarchies
- **Base branch** — which git branch to start from when creating a worktree

Tasks appear in the sidebar grouped by status. Clicking a task either jumps to its linked session (if one exists) or starts a new session for it. You can create tasks from the GUI, the CLI, or directly from an agent session using the built-in agent skills.

Tasks can also be synced from Jira Cloud. When configured, planeai pulls issues matching JQL filters and displays them in a dedicated sidebar section. You assign a Jira issue to a project to create a local child task, which can then be dispatched to an agent like any other task. See the [Task Management guide](/planeai/guides/task-management/#jira-integration) for details.

## Auto-Dispatch (Symphony Mode)

Auto-dispatch is planeai's orchestrator. When enabled, it continuously polls your task board, picks the highest-priority ready task, and dispatches it to an available session slot — fully autonomously.

The loop is simple: poll → dispatch → reconcile → notify. Tasks with unmet blockers are skipped. When a session completes its work, planeai moves the task to done and frees the slot for the next one. Lifecycle hooks fire at each transition, letting you automate commits, notifications, or any shell command.

Auto-dispatch turns planeai from a multi-terminal manager into a task orchestration system. You feed it tasks, it feeds agents work. See the [Auto-Dispatch guide](/planeai/guides/auto-dispatch/) for configuration details.

## Git Worktrees

When you run multiple agents in parallel on the same repository, they'll conflict if they share a working directory. planeai solves this with git worktrees — each session gets its own checkout of the repository on its own branch.

Worktrees are created automatically when you start a session with a task (or when auto-dispatch launches one). Each agent works in complete isolation: no merge conflicts, no stash juggling, no stepping on each other's changes. When the work is done, you merge the branch like any other feature branch.

This is what makes true parallel development possible — three agents can work on three different features simultaneously without interference.
