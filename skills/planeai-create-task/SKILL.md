---
name: planeai-create-task
description: 'Create a single task in the internal task tracker. Use when the user says "add a task", "create a ticket", "track this", "log this as a task", mentions wanting to remember or follow up on something, or describes a unit of work that should be tracked. If they''re describing something that sounds like a task to track, use this skill.'
---

## What this does

Create a single, well-formed task on the user's PlaneAI task board.
Translate whatever the user describes into a structured task with appropriate metadata.

## Which interface to use

- **PlaneAI tools available** (`mcp__planeai__*`, as in Claude Code and Codex sessions started by PlaneAI): call `mcp__planeai__task_create`.
  They may be deferred: load them with your tool search first.
- **Otherwise**: run `planeai-cli axi task add`.

Both take the same fields and print the created task, including its key.

## When NOT to use this

- Breaking down work into multiple tasks (use `planeai-plan-to-tasks`)
- Moving, editing, deleting, listing or viewing tasks (use `planeai-task-management`)

## How to create the task

**Key principle**: every task description must be a self-contained handoff.
Assume the agent picking up the task is weaker than you: be explicit, thorough, and spare no detail.
Include context, reasoning, relevant file paths and modules, and acceptance criteria.
The description is the only thing the next agent will have.

### 1. Figure out the title

Extract a concise, actionable title from what the user said.
Good titles start with a verb and are specific enough to act on without reading the description.

- "Fix the login redirect bug on Safari" ✓
- "Login bug" ✗ (too vague)
- "Investigate and potentially fix the issue where users on Safari browsers are not being properly redirected after OAuth login completes" ✗ (too long, that's description material)

### 2. Decide on metadata

| Tool field (`task_create`) | CLI flag       | When to use it                                                                                                                                              |
| -------------------------- | -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `description`              | `--desc`       | **Always.** Why this task exists, what to build, which modules and files are involved, acceptance criteria, and any decisions or rejected alternatives.     |
| `priority`                 | `--priority`   | Only on a real urgency signal ("urgent", "critical", "before the release"). Higher number = higher priority; 0 is the default.                              |
| `tags`                     | `--tags`       | When the user names a domain, component or category. Prefer short lowercase words joined by hyphens (e.g. `tech-debt`). The CLI takes them comma-separated. |
| `blocked_by`               | `--blocked-by` | Only when the user names a task key that blocks this one (e.g. "this depends on PLA-5").                                                                    |
| `parent`                   | `--parent`     | Only when the user says this is a subtask of an existing task.                                                                                              |
| `project`                  | `--project`    | Only when the user names another project, or context makes it clear the task belongs elsewhere.                                                             |

Don't force metadata that isn't there: a title alone is fine.
Without `project`, the tool uses the project of your session's task and the CLI resolves the project from the current directory.

### 3. Create it

```
mcp__planeai__task_create { "title": "...", "description": "...", "tags": ["auth"] }
```

```bash
planeai-cli axi task add "Title here" [--desc "..."] [--priority <int>] [--tags <a,b>] [--blocked-by <K1,K2>] [--parent <KEY>] [--project <name>]
```

If it fails (for example, an unknown `blocked_by` key), report the error clearly and suggest a fix.

### 4. Confirm to the user

Tell the user the key and what was created, briefly:

> Created **PLA-3**: "Fix Safari login redirect" [todo, priority 1, tags: auth, frontend]

## Examples

**User:** "track a task for adding dark mode support to the settings page"

```
mcp__planeai__task_create { "title": "Add dark mode support to settings page", "tags": ["ui"], "description": "..." }
```

**User:** "I need to fix that crash in the payment flow before release, it's urgent. The error is a nil pointer in checkout.go line 42"

```bash
planeai-cli axi task add "Fix nil pointer crash in payment flow" --priority 1 --tags payments --desc "Nil pointer dereference in checkout.go:42. Needs fix before release."
```

**User:** "add a task to the nomi project for migrating the budget chart"

```
mcp__planeai__task_create { "title": "Migrate budget chart", "project": "nomi", "tags": ["migration"], "description": "..." }
```
