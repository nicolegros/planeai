---
title: "Tutorial: Your First Session"
description: Go from install to a working AI agent session in under 5 minutes.
---

This tutorial walks you through launching planeai, configuring Claude as your provider, and running your first AI coding session. By the end, you'll have an agent working in your codebase.

## Prerequisites

- planeai installed ([download here](https://github.com/nicolegros/planeai/releases/latest))
- Claude CLI installed and authenticated (`claude` available on your PATH)
- A project directory with code you want to work on

:::tip
Don't have Claude? This tutorial works with any provider — just adjust the provider configuration for your agent (Kiro, Copilot, etc.). See the [Configuration guide](/planeai/guides/configuration/) for other provider setups.
:::

## Step 1: Configure your provider

Open (or create) your config file at `~/.config/planeai/config.json`:

```jsonc
{
  "providers": {
    "claude": {
      "command": "claude",
      "prompt_command": "claude --message \"{{prompt}}\"",
      "yolo_flag": "--dangerously-skip-permissions"
    }
  },
  "default_provider": "claude",
  "session_backend": "local"
}
```

Alternatively, configure this in **Preferences** (⌘, / Ctrl+,) under the **Models** tab.

This tells planeai:
- **command** — launch Claude by running `claude`
- **prompt_command** — send messages to a running Claude session
- **yolo_flag** — the flag for autonomous mode (used by auto-dispatch; not needed for manual sessions)
- **session_backend** — use the local backend (no tmux required, sessions run in-process)

## Step 2: Launch planeai and add a project

Open planeai. On first launch you'll see an empty sidebar.

1. Press **⌘⇧N** (macOS) or **Ctrl+Shift+N** (Linux/Windows) to add a project
2. Choose your project directory

Your project now appears in the sidebar.

:::tip
Press **⌘/** (macOS) or **Ctrl+/** (Linux/Windows) at any time to see all keyboard shortcuts.
:::

## Step 3: Create a session

1. Press **⌘N** (macOS) or **Ctrl+N** to open the new item modal
2. Press **S** to create a session
3. Select your project
4. Choose a branch (or create a new one)
5. Confirm — planeai launches Claude in a terminal pane

You'll see Claude's CLI boot up in the main panel, ready for input.

## Step 4: Interact with the agent

Type directly in the terminal to talk to Claude. For example:

```
Explain the structure of this project and suggest improvements to the README.
```

Claude reads your codebase and responds. You can continue the conversation, ask follow-up questions, or give it tasks to implement.

## Step 5: Review and wrap up

When the agent makes changes:

1. Open the diff viewer with **⌘D** (macOS) or **Ctrl+D** — this shows all file changes the agent made
2. Review the diff and send feedback directly to the agent if needed
3. When you're satisfied, archive the session from the context menu (right-click the session in the sidebar)

## What's next

You've just run a single agent session. Here's where planeai gets powerful:

- **Run multiple sessions** — create more sessions to work on different tasks in parallel
- **Use task management** — create tasks and let planeai dispatch them to agents automatically. See the [Task Management guide](/planeai/guides/task-management/)
- **Enable auto-dispatch** — let planeai assign tasks to agents without manual intervention. See the [Auto-Dispatch guide](/planeai/guides/auto-dispatch/)
- **Add git worktree isolation** — when creating sessions linked to tasks, planeai creates worktrees so agents don't conflict. See [Concepts](/planeai/concepts/)

:::note
With the local backend, sessions terminate when you close planeai. If you want sessions to persist across app restarts, switch to the `tmux` backend in your config. See the [Configuration guide](/planeai/guides/configuration/) for details.
:::
