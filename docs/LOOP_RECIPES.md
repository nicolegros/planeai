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

Optional. Declares when the recipe can be auto-suggested or auto-started.

```yaml
trigger:
  on: task.assigned          # event type
  filter:
    label: backend           # only tasks with this label
```

### inputs

Parameters the user supplies when creating a loop run.

```yaml
inputs:
  - name: goal
    type: string
    required: true
    description: What the loop should accomplish
  - name: branch_prefix
    type: string
    default: loop/
```

### knowledge

Files and globs injected into agent context for every session in the loop.

```yaml
knowledge:
  - path: AGENTS.md
  - path: CONTEXT.md
  - glob: docs/adr/*.md
```

### tools

MCP servers or CLI tools available to agents in this loop.

```yaml
tools:
  - id: git
  - id: filesystem
  - id: planeai-tasks
    config:
      project: current
```

### roles

Named agent personas. Each role gets its own system prompt, tool subset, and constraints.

```yaml
roles:
  - id: maker
    agent: kiro
    prompt: |
      You implement features using TDD. Write failing tests first,
      then make them pass with minimal code.
    tools: [git, filesystem]

  - id: verifier
    agent: kiro
    prompt: |
      You review code for correctness, style, and test coverage.
      Be critical. List concrete issues.
    tools: [git, filesystem]
```

### policy

Loop-level constraints and resource limits.

```yaml
policy:
  max_ticks: 20              # hard cap on total steps executed
  max_duration: 2h           # wall-clock timeout
  merge_policy: human        # only 'human' is supported in v1
  retry_on_failure: true     # retry a failed step once before halting
```

### steps

Ordered list of actions the loop runner executes. Each step has a `kind` and kind-specific fields.

```yaml
steps:
  - kind: session.create
    role: maker
    worktree: true
    branch: "{{inputs.branch_prefix}}{{run.id}}"

  - kind: session.prompt
    role: maker
    message: "Implement: {{inputs.goal}}"

  - kind: handoff.wait
    from: maker
    to: verifier
    artifact: loop_artifacts/handoff.md

  - kind: session.prompt
    role: verifier
    message: "Review the changes in {{step.prev.branch}}."

  - kind: human.wait
    prompt: "Review the verifier's feedback and approve or request changes."

  - kind: loop.status
    set: completed
```

## Built-in Maker-Verifier Recipe

The simplest built-in recipe proves the system works end-to-end:

```yaml
schema: planeai.loop.recipe.v1
id: maker-verifier
name: Maker → Verifier
description: One agent implements, another reviews, human merges.

inputs:
  - name: goal
    type: string
    required: true

roles:
  - id: maker
    agent: kiro
    prompt: "Implement the goal using TDD."
    tools: [git, filesystem]
  - id: verifier
    agent: kiro
    prompt: "Review for correctness and test coverage."
    tools: [git, filesystem]

policy:
  max_ticks: 12
  merge_policy: human

steps:
  - kind: session.create
    role: maker
    worktree: true
  - kind: session.prompt
    role: maker
    message: "{{inputs.goal}}"
  - kind: handoff.wait
    from: maker
    to: verifier
  - kind: session.prompt
    role: verifier
    message: "Review the maker's changes."
  - kind: human.wait
    prompt: "Approve, request changes, or abort."
  - kind: loop.status
    set: completed
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
| `loop.status` | Set the loop run status (running, paused, completed, failed) |
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

1. **Bounded execution** — Every recipe must declare `max_ticks` or `max_duration` in `policy`. The runner refuses to start unbounded loops.
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

inputs:
  - name: goal
    type: string
    required: true
  - name: context
    type: string
    description: Additional context or constraints

knowledge:
  - path: CONTEXT.md
  - path: AGENTS.md

roles:
  - id: planner
    agent: kiro
    prompt: "Break the goal into a concrete implementation plan with files to change."
    tools: [git, filesystem]
  - id: implementer
    agent: kiro
    prompt: "Implement the plan using TDD. Follow AGENTS.md conventions."
    tools: [git, filesystem]
  - id: reviewer
    agent: kiro
    prompt: "Review for correctness, missed edge cases, and adherence to the plan."
    tools: [git, filesystem]

policy:
  max_ticks: 30
  max_duration: 4h
  merge_policy: human

steps:
  - kind: session.create
    role: planner
    worktree: true
  - kind: session.prompt
    role: planner
    message: "Plan: {{inputs.goal}}. Context: {{inputs.context}}"
  - kind: handoff.wait
    from: planner
    to: implementer
    artifact: loop_artifacts/plan.md
  - kind: session.create
    role: implementer
    worktree: true
  - kind: session.prompt
    role: implementer
    message: "Implement according to the plan in loop_artifacts/plan.md"
  - kind: handoff.wait
    from: implementer
    to: reviewer
    artifact: loop_artifacts/handoff.md
  - kind: session.create
    role: reviewer
  - kind: session.prompt
    role: reviewer
    message: "Review the implementer's changes against the plan."
  - kind: loop.event
    name: review.complete
  - kind: human.wait
    prompt: "Review complete. Approve, iterate, or abort."
  - kind: loop.status
    set: completed
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
