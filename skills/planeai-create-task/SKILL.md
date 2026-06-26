---
name: planeai-create-task
description: 'Create a single task in the internal task tracker. Use when the user says "add a task", "create a ticket", "track this", "log this as a task", mentions wanting to remember or follow up on something, or describes a unit of work that should be tracked. If they''re describing something that sounds like a task to track, use this skill.'
---

## What this does

Create a single, well-formed task in the user's task board using the `planeai-cli task add` command. Translate whatever the user describes into a structured task with appropriate metadata.

## When NOT to use this

- Breaking down work into multiple tasks (that's a different skill)
- Moving, editing, or deleting existing tasks
- Listing or viewing tasks

## How to create the task

**Key principle**: Every task description must be a self-contained handoff. Assume the agent picking up the task is weaker than you — be explicit, thorough, and spare no detail. Include context, reasoning, relevant file paths/modules, and acceptance criteria. The description is the only thing the next agent will have.

### 1. Figure out the title

Extract a concise, actionable title from what the user said. Good titles start with a verb and are specific enough to act on without reading the description.

- "Fix the login redirect bug on Safari" ✓
- "Login bug" ✗ (too vague)
- "Investigate and potentially fix the issue where users on Safari browsers are not being properly redirected after OAuth login completes" ✗ (too long — that's description material)

### 2. Decide on metadata

From the user's message, infer what metadata to attach:

| Flag           | When to use it                                                                                                                                                                                                                                                             |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `--desc`       | **Always provide a description.** Include: why this task exists, what to build, which modules/files are involved, acceptance criteria, and any decisions or rejected alternatives. The more context the better — a weaker agent will rely entirely on this to do the work. |
| `--priority`   | If the user signals urgency ("urgent", "critical", "high priority", "before the release"). Use 1 = highest. Only set if there's a real signal; default (0) is fine.                                                                                                        |
| `--tags`       | If the user mentions a domain area, component, or category. Must be lowercase-alphanumeric-with-hyphens, max 30 chars. Comma-separated. Examples: `backend`, `auth`, `high-priority`, `tech-debt`.                                                                         |
| `--blocked-by` | Only if the user explicitly names a task key that blocks this one (e.g., "this depends on PLA-5"). Comma-separated.                                                                                                                                                        |
| `--parent`     | Only if the user explicitly says this is a subtask of an existing task.                                                                                                                                                                                                    |

Don't force metadata that isn't there. A bare `planeai-cli task add "Title"` is perfectly fine.

### 3. Target the right project

Use `--project <name>` when:

- The user names a specific project ("add this to the nomi project")
- Context makes it clear the task belongs to a project other than the CWD

Otherwise, omit the flag and let planeai resolve from CWD (which is the default and usually correct).

### 4. Run the command

```bash
planeai-cli axi task add "Title here" [--desc "..."] [--priority <int>] [--tags <a,b>] [--blocked-by <K1,K2>] [--parent <KEY>] [--project <name>]
```

The command outputs TOON with the created task (including the auto-assigned key like `PLA-3`) and a next-step hint. Confirm the creation to the user with the key and a brief summary.

If the command fails (e.g., a referenced `--blocked-by` key doesn't exist), report the error clearly and suggest a fix.

### 5. Confirm to the user

After creation, tell the user the key and what was created. Keep it brief:

> Created **PLA-3**: "Fix Safari login redirect" [todo, priority 1, tags: auth, frontend]

## Examples

**User:** "track a task for adding dark mode support to the settings page"

```bash
planeai-cli axi task add "Add dark mode support to settings page" --tags ui
```

**User:** "I need to fix that crash in the payment flow before release, it's urgent. The error is a nil pointer in checkout.go line 42"

```bash
planeai-cli axi task add "Fix nil pointer crash in payment flow" --priority 1 --tags payments --desc "Nil pointer dereference in checkout.go:42. Needs fix before release."
```

**User:** "add a task to the nomi project for migrating the budget chart"

```bash
planeai-cli axi task add "Migrate budget chart" --project nomi --tags migration
```
