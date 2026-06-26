---
name: planeai-plan-to-tasks
description: 'Break a plan, spec, PRD, or conversation into a structured set of tasks with parent/subtask relationships and blockers. Use when user wants to turn a plan into tasks, break down work into trackable items, says "create tasks from this plan", "break this down into tasks", "convert this to tickets", or has a multi-step project they want tracked. Even if they just say "let''s track this" after discussing a plan — use this skill.'
---

# Plan to Tasks

Break a plan into independently-grabbable tasks using vertical slices (tracer bullets). Each task is a thin end-to-end slice, not a horizontal layer.

Each task description must be a **mini-handoff** — self-contained enough for an agent with zero prior context to pick it up and implement it. Assume the next agent is weaker: be explicit and thorough.

## Process

### 1. Gather context

Work from whatever is already in the conversation — a plan, spec, PRD, bullet list, or discussion. If the user points to a file or document, read it. If context is thin, ask clarifying questions before proceeding.

### 2. Explore the codebase

**Required.** Before generating tasks, explore the codebase to understand:

- Project structure and relevant modules/directories
- Existing patterns and conventions
- Domain vocabulary

Task descriptions must reference real modules and use the project's actual terminology.

### 3. Draft vertical slices

Break the plan into **tracer bullet** tasks. Each task is a thin vertical slice that cuts through ALL integration layers end-to-end — not a horizontal slice of one layer.

Principles:

- Each slice delivers a narrow but COMPLETE path through every layer (schema, API, UI, tests)
- A completed slice is demoable or verifiable on its own
- Prefer many thin slices over few thick ones
- Use parent tasks to group related slices when there's a natural hierarchy
- Use `--blocked-by` for dependency ordering between tasks

### 4. Write rich descriptions

Each task's `--desc` is a structured mini-handoff. Spare no detail — there is no length limit. Thoroughness is the priority.

#### Parent task description template

```
## Context

[Why this work exists. What problem it solves. Key decisions made during planning and rejected alternatives with reasons.]

## What to build

[High-level approach and architecture. Which modules/directories are involved. How the slices fit together.]

## Acceptance criteria

- [ ] Overall criterion 1
- [ ] Overall criterion 2
```

#### Subtask description template

```
## Context

[One sentence: why this slice exists and how it fits into the parent.]
[See parent PARENT-KEY for full project context, decisions, and rejected alternatives.]

## What to build

[Concrete implementation details for this slice. Which modules to touch. What the slice does end-to-end.]

## Acceptance criteria

- [ ] Specific criterion 1
- [ ] Specific criterion 2
```

### 5. Ask user for approval

Present in details the tasks that will be created and ask for approval.

### 6. Create the tasks

Use `planeai-cli task add` for each task:

```bash
# Parent task
planeai-cli axi task add "Parent title" --desc "..." --tags feature-name

# Subtasks (reference parent key)
planeai-cli axi task add "Subtask title" --parent PLA-1 --desc "..." --blocked-by PLA-1
```

### 7. Present the result

After all tasks are created, present a summary table showing:

- Key → Title → Status → Dependencies

## CLI Reference

```
planeai-cli axi task add "Title" [flags]

Flags:
  --desc <string>         Task description
  --priority <int>        Priority (1 = highest, 0 = default)
  --tags <a,b,c>          Comma-separated tags
  --blocked-by <K1,K2>    Comma-separated blocker keys
  --parent <KEY>          Parent task key
  --project <name>        Target project (default: resolve from CWD)

planeai-cli axi task show <key>
planeai-cli axi task ls [--status <status>] [--tags <a,b>]
planeai-cli axi task move <key> <status>
planeai-cli task edit <key> [--title ...] [--desc ...] [--priority ...] [--tags ...] [--blocked-by ...]
planeai-cli task delete <key>
```
