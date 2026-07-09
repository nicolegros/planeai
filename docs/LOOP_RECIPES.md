# Loop Recipes

Loop recipes are declarative YAML definitions that describe AI engineering loops — repeatable, multi-agent workflows with durable state, human review gates, and worktree isolation. A recipe tells planeai *what* agents to spin up, *how* they hand off work, and *when* a human must intervene.

## Loop-Engineering Principles

Recipes encode the core loop-engineering model:

| Principle | Recipe mapping |
|-----------|---------------|
| Heartbeat/tick model | Each `step` is a tick; the loop runner advances one step at a time |
| Worktree isolation | `session.create` steps spawn agents in isolated git worktrees |
| Project knowledge | `knowledge` field injects docs, context files, and ADRs into agent prompts |
| Tools/connectors | `tools` field declares which MCP servers or CLI tools are available |
| Role-based sub-agents | `roles` field defines named agents with distinct system prompts and tool access |
| Durable state | Loop state persists in SQLite; resumes after app restart or crash |
| Human review | `human.wait` steps and `merge_policy: human` enforce manual approval |

## Recipe Locations and Precedence

Recipes are resolved in order (first match wins):

1. **Project** — `.planeai/loops/*.yaml` in the repo root
2. **User** — `~/.config/planeai/loops/*.yaml`
3. **Builtin** — bundled with the app

A project recipe with `id: maker-verifier` shadows the builtin of the same name.

## Schema Reference

All recipes use the `planeai.loop.recipe.v1` schema:

```yaml
schema: planeai.loop.recipe.v1
id: my-recipe
name: My Custom Loop
description: One-line summary of what this loop does.
```

## Recipe Fields

### trigger

Required. Declares what kind of event starts the recipe. Only `manual` is executable in v1.

```yaml
trigger:
  kind: manual
```

Future trigger kinds (recognized but not yet executable): `schedule`, `github_event`, `task_event`, `pr_feedback`, `ci_failure`.

### inputs

Parameters the user supplies when creating a loop run. A map of input name to options.

```yaml
inputs:
  goal:
    required: true
  branch_prefix:
    required: false
```

### knowledge

Files and instructions injected into agent context for every session in the loop.

```yaml
knowledge:
  files:
    - AGENTS.md
    - CONTEXT.md
    - docs/adr/001-loop-recipes.md
  instructions:
    - Follow TDD workflow
    - Use conventional commits
```

### tools

MCP servers or CLI tools available to agents in this loop. Separated into required and optional.

```yaml
tools:
  required:
    - git
    - filesystem
  optional:
    - planeai-tasks
```

### roles

Named agent personas. A map of role ID to role configuration. Each role has a provider, mode, isolation level, and optional instructions.

```yaml
roles:
  maker:
    provider: default
    mode: write
    isolation: worktree
    instructions: |
      You implement features using TDD. Write failing tests first,
      then make them pass with minimal code.
  verifier:
    provider: default
    mode: review
    isolation: worktree
    instructions: |
      You review code for correctness, style, and test coverage.
      Be critical. List concrete issues.
```

Supported modes: `write`, `review`, `readonly`, `plan`, `triage`, `arbiter`.
Supported isolation values: `worktree`, `project`, `readonly`.

### policy

Loop-level constraints and resource limits.

```yaml
policy:
  max_rounds: 3
  max_ticks: 20
  max_sessions: 5
  stale_after_ms: 3600000
  merge_policy: human
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_rounds` | integer | 3 | Maximum iteration rounds |
| `max_ticks` | integer | 50 | Hard cap on total steps executed |
| `max_sessions` | integer | 5 | Maximum concurrent agent sessions |
| `stale_after_ms` | integer | null | Wall-clock staleness timeout in milliseconds |
| `merge_policy` | string | `human` | Only `human` is supported in v1 |

### steps

Ordered list of actions the loop runner executes. Each step has an `id`, a `kind`, and kind-specific fields.

```yaml
steps:
  - id: create-maker
    kind: session.create
    role: maker

  - id: prompt-maker
    kind: session.prompt
    role: maker
    prompt: "Implement: {{inputs.goal}}"

  - id: wait-handoff
    kind: handoff.wait
    from: maker

  - id: prompt-verifier
    kind: session.prompt
    role: verifier
    prompt: "Review the changes in the maker's branch."

  - id: wait-human
    kind: human.wait
    prompt: "Review the verifier's feedback and approve or request changes."

  - id: done
    kind: loop.status
    status: completed_unreviewed
```

Step fields reference:

| Field | Description |
|-------|-------------|
| `id` | Unique step identifier (required) |
| `kind` | Step kind — see supported kinds below (required) |
| `role` | Target role for session steps |
| `prompt` | Message/instruction text |
| `from` | Source role for handoff.wait |
| `on` | Condition map for conditional steps |
| `status` | Target status for loop.status |
| `next` | Explicit next step ID (overrides sequential order) |
| `select` | Selection criteria |
| `event_kind` | Event kind for loop.event |

## Built-in Maker-Verifier Recipe

The simplest built-in recipe proves the system works end-to-end:

```yaml
schema: planeai.loop.recipe.v1
id: maker-verifier
name: Maker → Verifier
description: One agent implements, another reviews, human merges.

trigger:
  kind: manual

inputs:
  goal:
    required: true

knowledge:
  files: []
  instructions: []

tools:
  required:
    - git
    - filesystem
  optional: []

roles:
  maker:
    provider: default
    mode: write
    isolation: worktree
    instructions: "Implement the goal using TDD."
  verifier:
    provider: default
    mode: review
    isolation: worktree
    instructions: "Review for correctness and test coverage."

policy:
  max_ticks: 12
  merge_policy: human

steps:
  - id: create-maker
    kind: session.create
    role: maker
  - id: prompt-maker
    kind: session.prompt
    role: maker
    prompt: "{{inputs.goal}}"
  - id: wait-handoff
    kind: handoff.wait
    from: maker
  - id: prompt-verifier
    kind: session.prompt
    role: verifier
    prompt: "Review the maker's changes."
  - id: wait-human
    kind: human.wait
    prompt: "Approve, request changes, or abort."
  - id: done
    kind: loop.status
    status: completed_unreviewed
```

## CLI Commands

### List available recipes

```bash
planeai-cli axi loop recipe ls
```

Shows all recipes from all sources with their ID, name, and origin (project/user/builtin).

### Show a recipe

```bash
planeai-cli axi loop recipe show <id-or-path>
```

Prints the full YAML of a recipe by ID or file path.

### Validate a recipe

```bash
planeai-cli axi loop recipe validate <id-or-path>
```

Checks schema conformance, references valid step kinds, and verifies role/tool consistency. Exits non-zero on errors.

### Create a loop run from a recipe

```bash
planeai-cli axi loop create --recipe <id-or-path> --goal "Add pagination to /users"
```

Instantiates a new `LoopRun`, resolves inputs, and begins executing steps. Use `--dry-run` to preview without executing.

## Supported Step Kinds (v1)

| Kind | Description |
|------|-------------|
| `session.create` | Spawn a new agent session (optionally in a worktree) |
| `session.prompt` | Send a message to an existing session |
| `handoff.wait` | Pause until the source role produces a handoff artifact |
| `loop.status` | Set the loop run status (`observing`, `completed_unreviewed`, `blocked`, `needs_human`, `failed`, `cancelled`) |
| `loop.event` | Emit a structured event into the loop's event log |
| `human.wait` | Block until a human responds in the UI |

## Future Step Kinds (Not Yet Supported)

These are reserved in the schema but not implemented:

| Kind | Intent |
|------|--------|
| `gates.run` | Run a gate check (tests, lint, type-check) |
| `pr.feedback.wait` | Wait for PR review comments |
| `arbiter.rank` | Have a judge agent rank multiple outputs |
| `task.create` | Create a task in the internal tracker |
| `connector.call` | Call an external connector (Jira, Slack, etc.) |

## Safety Rules

1. **Bounded execution** — Every recipe must declare `max_ticks` in `policy`. The runner refuses to start unbounded loops.
2. **Human merge only** — `merge_policy` only accepts `human` in v1. No auto-merge.
3. **No auto-merge** — Even if all agents agree, a human must approve before changes land on the target branch.
4. **No arbitrary shell** — Steps cannot execute raw shell commands. Agents interact through declared `tools` only.

## Example: Planner → Implementer → Reviewer

A three-role recipe for larger features:

```yaml
schema: planeai.loop.recipe.v1
id: plan-implement-review
name: Plan → Implement → Review
description: Planner breaks down work, implementer builds, reviewer validates.

trigger:
  kind: manual

inputs:
  goal:
    required: true
  context:
    required: false

knowledge:
  files:
    - CONTEXT.md
    - AGENTS.md
  instructions: []

tools:
  required:
    - git
    - filesystem
  optional: []

roles:
  planner:
    provider: default
    mode: plan
    isolation: worktree
    instructions: "Break the goal into a concrete implementation plan with files to change."
  implementer:
    provider: default
    mode: write
    isolation: worktree
    instructions: "Implement the plan using TDD. Follow AGENTS.md conventions."
  reviewer:
    provider: default
    mode: review
    isolation: worktree
    instructions: "Review for correctness, missed edge cases, and adherence to the plan."

policy:
  max_rounds: 3
  max_ticks: 30
  max_sessions: 5
  merge_policy: human

steps:
  - id: create-planner
    kind: session.create
    role: planner
  - id: prompt-planner
    kind: session.prompt
    role: planner
    prompt: "Plan: {{inputs.goal}}. Context: {{inputs.context}}"
  - id: wait-plan-handoff
    kind: handoff.wait
    from: planner
  - id: create-implementer
    kind: session.create
    role: implementer
  - id: prompt-implementer
    kind: session.prompt
    role: implementer
    prompt: "Implement according to the plan in loop_artifacts/plan.md"
  - id: wait-impl-handoff
    kind: handoff.wait
    from: implementer
  - id: create-reviewer
    kind: session.create
    role: reviewer
  - id: prompt-reviewer
    kind: session.prompt
    role: reviewer
    prompt: "Review the implementer's changes against the plan."
  - id: review-event
    kind: loop.event
    event_kind: review.complete
  - id: wait-human
    kind: human.wait
    prompt: "Review complete. Approve, iterate, or abort."
  - id: done
    kind: loop.status
    status: completed_unreviewed
```

## Relationship to Handoffs

`handoff.wait` steps bridge agent sessions. When a role finishes its work:

1. The agent writes a handoff artifact to `loop_artifacts/` (e.g., `handoff.md`, `plan.md`).
2. The loop runner detects the artifact and advances to the next step.
3. The receiving role's session gets created (if not already running) and receives the artifact as context.

The runner checks both `loop_artifacts/` files and the loop's event log to determine when a handoff is satisfied. Agents can also emit `loop.event` steps to signal completion programmatically.

## Domain Model Glossary

| Term | Description |
|------|-------------|
| **LoopRecipe** | A YAML definition describing a reusable loop template |
| **LoopRun** | A single execution instance of a recipe, with its own state and history |
| **LoopStep** | One tick in a run — the atomic unit of execution |
| **LoopRole** | A named agent persona with specific prompt, tools, and constraints |
| **LoopPolicy** | Constraints governing a run (max ticks, duration, merge policy) |
| **LoopSignal** | An event or condition that causes the runner to advance or pause |
| **LoopMemory** | Durable state attached to a run — artifacts, events, and step outputs persisted in SQLite |
