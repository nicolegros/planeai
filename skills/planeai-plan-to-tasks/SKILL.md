---
name: planeai-plan-to-tasks
description: 'Break a plan, spec, PRD, or conversation into a structured set of tasks with parent/subtask relationships and blockers. Use when user wants to turn a plan into tasks, break down work into trackable items, says "create tasks from this plan", "break this down into tasks", "convert this to tickets", or has a multi-step project they want tracked. Even if they just say "let''s track this" after discussing a plan, use this skill.'
---

# Plan to Tasks

Break a plan into independently-grabbable tasks using vertical slices (tracer bullets).
Each task is a thin end-to-end slice, not a horizontal layer.

Each task description must be a **mini-handoff**: self-contained enough for an agent with zero prior context to pick it up and implement it.
Assume the next agent is weaker: be explicit and thorough.

## Which interface to use

- **PlaneAI tools available** (`mcp__planeai__*`, as in Claude Code and Codex sessions started by PlaneAI): use `mcp__planeai__task_create` and `mcp__planeai__task_list`.
  They may be deferred: load them with your tool search first.
- **Otherwise**: use `planeai-cli axi task add` and `planeai-cli axi task ls`.

## Process

### 1. Gather context

Work from whatever is already in the conversation: a plan, spec, PRD, bullet list, or discussion.
If the user points to a file or document, read it.
If context is thin, ask clarifying questions before proceeding.

### 2. Explore the codebase

**Required.** Before generating tasks, explore the codebase to understand:

- Project structure and relevant modules and directories
- Existing patterns and conventions
- Domain vocabulary

Task descriptions must reference real modules and use the project's actual terminology.

### 3. Draft vertical slices

Break the plan into **tracer bullet** tasks.
Each task is a thin vertical slice that cuts through all integration layers end to end, not a horizontal slice of one layer.

Principles:

- Each slice delivers a narrow but complete path through every layer (schema, API, UI, tests)
- A completed slice is demoable or verifiable on its own
- Prefer many thin slices over few thick ones
- Use parent tasks to group related slices when there's a natural hierarchy
- Use blockers for dependency ordering between tasks

### 4. Write rich descriptions

Each task's description is a structured mini-handoff.
Spare no detail: there is no length limit, and thoroughness is the priority.

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

Present the tasks that will be created in detail and ask for approval.

### 6. Create the tasks

Create the parent first, then each subtask with the parent's key.

```
mcp__planeai__task_create { "title": "Parent title", "description": "...", "tags": ["feature-name"] }
mcp__planeai__task_create { "title": "Subtask title", "description": "...", "parent": "PLA-1", "blocked_by": ["PLA-2"] }
```

```bash
planeai-cli axi task add "Parent title" --desc "..." --tags feature-name
planeai-cli axi task add "Subtask title" --desc "..." --parent PLA-1 --blocked-by PLA-2
```

`task_create` also takes `priority`, `base_branch` and `project`; the CLI takes the same as `--priority`, `--base-branch` and `--project`.

### 7. Present the result

After all tasks are created, present a summary table: Key, Title, Status, Dependencies.
