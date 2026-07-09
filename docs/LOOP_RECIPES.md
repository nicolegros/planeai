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
| `gates` | List of gate declarations for gates.run steps |

## Built-in Maker-Verifier Recipe

The builtin recipe implements the full maker-verifier state machine: maker implements → gates verify automatically → verifier agent reviews → rounds cycle on rejection.

### State Machine

```
                            ┌─────────────────────────────────────────────┐
                            │                                             │
                            ▼                                             │
┌──────────────┐     ┌──────────────┐     ┌──────────┐     ┌──────────────────────┐
│ create_maker │────▶│wait_for_maker│────▶│ run_gates │────▶│   create_verifier    │
└──────────────┘     └──────────────┘     └──────────┘     └──────────────────────┘
                            ▲              │ fail/error            │
                            │              ▼                       ▼
                            │       ┌─────────────────┐    ┌──────────────────┐
                            │       │gates_failed_retry│    │ wait_for_verifier │
                            │       └────────┬────────┘    └────────┬─────────┘
                            │                ▼                      │
                            │    ┌────────────────────────┐         │ completed
                            │    │increment_round_after_  │         ▼
                            │    │        gates           │  ┌──────────────────────┐
                            │    └────────────┬───────────┘  │ completed_unreviewed │ (terminal)
                            │                 │              └──────────────────────┘
                            │                 │                     │ needs_human
                            │                 │                     ▼
                            │                 │              ┌──────────────────────┐
                            │                 │              │verifier_rejected_retry│
                            │                 │              └────────┬─────────────┘
                            │                 │                       ▼
                            │                 │              ┌────────────────────────┐
                            │                 │              │increment_round_after_  │
                            │                 │              │       review           │
                            │                 │              └────────┬───────────────┘
                            │                 │                       │
                            └─────────────────┴───────────────────────┘
                                      (cycles back to wait_for_maker)
```

**Terminal states:**
- `completed_unreviewed` — verifier approved; human should review/merge.
- `blocked` / `needs_human` / `failed` — maker declared a non-completable outcome.

**Safety:**
- `max_rounds` (default: 3) limits retry cycles. When reached at any `round.next` step, the loop transitions to `needs_human`.
- `merge_policy: human` ensures no auto-merge. `completed_unreviewed` is the furthest automated state; a human must promote it to `approved` → `merged`.

### Full Recipe YAML

```yaml
schema: planeai.loop.recipe.v1
id: maker-verifier
name: Maker + Verifier
description: >
  Full maker-verifier loop. Maker implements, gates verify automatically,
  a verifier agent reviews, and rounds cycle on rejection until max_rounds.

trigger:
  kind: manual

inputs:
  goal:
    required: true
  task_key:
    required: false

knowledge:
  files:
    - AGENTS.md
    - CONTEXT.md
  instructions:
    - Follow repository conventions.
    - Prefer existing PlaneAI services and domain types.
    - Record progress through structured handoffs, not terminal claims.

tools:
  required:
    - git
    - plane_sessions
    - plane_loops
  optional:
    - github
    - jira
    - mcp

roles:
  maker:
    provider: default
    mode: write
    isolation: worktree
    instructions: |
      You are the maker agent.
      Implement the requested change in your isolated worktree.
      Do not claim completion unless you have recorded a structured handoff.

  verifier:
    provider: default
    mode: review
    isolation: readonly
    instructions: |
      You are the verifier agent.
      Review the maker's changes. Do not edit files.
      If the changes satisfy the goal and pass your review, record a handoff with status: completed.
      If the changes need work, record a handoff with status: needs_human and describe what must change.

policy:
  max_rounds: 3
  max_ticks: 50
  max_sessions: 5
  stale_after_ms: 600000
  merge_policy: human

steps:
  - id: create_maker
    kind: session.create
    role: maker
    prompt: |
      You are running inside a PlaneAI loop.
      Loop: {{ loop_run.id }}
      Goal: {{ inputs.goal }}
      {% if inputs.task_key %}Task: {{ inputs.task_key }}{% endif %}
      Round: {{ runtime.round }}
      {% if runtime.last_error %}Previous round feedback: {{ runtime.last_error }}{% endif %}
      Project knowledge: {{ knowledge.files }}
      Instructions:
      - Work only in your assigned workspace.
      - Make the smallest safe change that satisfies the goal.
      - Run relevant checks if possible.
      - When done, write and record a planeai.handoff.v1 handoff.

  - id: wait_for_maker
    kind: handoff.wait
    from: maker
    on:
      completed: run_gates
      blocked: blocked
      needs_human: needs_human
      failed: failed

  - id: run_gates
    kind: gates.run
    role: maker
    gates:
      - name: build
        command: "if [ -f Cargo.toml ]; then cargo build 2>&1; elif [ -f package.json ]; then npm run build 2>&1; else echo 'no build system detected'; fi"
      - name: test
        command: "if [ -f Cargo.toml ]; then cargo test 2>&1; elif [ -f package.json ]; then npm test 2>&1; else echo 'no test runner detected'; fi"
    on:
      pass: create_verifier
      fail: gates_failed_retry
      error: gates_failed_retry

  - id: gates_failed_retry
    kind: session.prompt
    role: maker
    select: latest
    prompt: |
      Round {{ runtime.round }} — verification gates failed.
      Review the gate output and fix the issues. Then record a new handoff.
    next: increment_round_after_gates

  - id: increment_round_after_gates
    kind: round.next
    next: wait_for_maker

  - id: create_verifier
    kind: session.create
    role: verifier
    prompt: |
      You are the verifier agent in a PlaneAI maker-verifier loop.
      Loop: {{ loop_run.id }}
      Goal: {{ inputs.goal }}
      Round: {{ runtime.round }}
      The maker has completed their implementation and gates have passed.
      Review the diff and changed files. Check correctness, edge cases, missing tests.
      Do NOT make changes yourself.
      Record a planeai.handoff.v1 handoff:
      - status: completed — if the changes are good and ready for human review.
      - status: needs_human — if the changes need work.

  - id: wait_for_verifier
    kind: handoff.wait
    from: verifier
    on:
      completed: completed_unreviewed
      needs_human: verifier_rejected_retry
      blocked: blocked
      failed: failed

  - id: verifier_rejected_retry
    kind: session.prompt
    role: maker
    select: latest
    prompt: |
      Round {{ runtime.round }} — the verifier has requested changes.
      Verifier feedback: {{ runtime.last_error }}
      Address the issues raised above, then record a new handoff.
    next: increment_round_after_review

  - id: increment_round_after_review
    kind: round.next
    next: wait_for_maker

  - id: completed_unreviewed
    kind: loop.status
    status: completed_unreviewed

  - id: blocked
    kind: loop.status
    status: blocked

  - id: needs_human
    kind: loop.status
    status: needs_human

  - id: failed
    kind: loop.status
    status: failed
```

### Typical Tick Sequence

A successful run with no rejections:

```bash
# Create the loop
planeai-cli axi loop create --strategy maker-verifier --goal "Implement pagination"
# → loop created, status: draft

# Tick 1: create_maker → spawns maker session in worktree
planeai-cli axi loop tick <LOOP_ID>
# → session created, status: observing

# Tick 2+: wait_for_maker → no handoff yet
planeai-cli axi loop tick <LOOP_ID>
# → waiting for handoff from maker

# After maker runs `planeai-cli axi loop handoff record ...`
# Tick N: wait_for_maker → detects completed handoff → advances to run_gates
planeai-cli axi loop tick <LOOP_ID>
# → handoff detected, next: run_gates

# Tick N+1: run_gates → builds and tests pass
planeai-cli axi loop tick <LOOP_ID>
# → gates: pass, next: create_verifier

# Tick N+2: create_verifier → spawns verifier session
planeai-cli axi loop tick <LOOP_ID>
# → verifier session created, status: observing

# Tick N+3+: wait_for_verifier → verifier approves
planeai-cli axi loop tick <LOOP_ID>
# → handoff detected (completed), next: completed_unreviewed

# Tick final: completed_unreviewed → terminal
planeai-cli axi loop tick <LOOP_ID>
# → status: completed_unreviewed — human should review and merge
```

### `completed_unreviewed` Terminal State

This is the safe terminal state for the maker-verifier strategy. It means:

1. The maker's implementation passed automated gates (build + test)
2. The verifier agent reviewed and approved the changes
3. **No merge has happened** — a human must still review the PR/branch and merge

The human can then:
- Approve and merge manually
- Request another round by updating the loop status
- Close the loop if the work is no longer needed

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
| `session.create` | Spawn a new agent session (in a worktree by default) and send initial prompt |
| `session.prompt` | Send a message to an existing session (requires `select: latest`) |
| `handoff.wait` | Pause until the source role produces an accepted handoff artifact |
| `loop.status` | Set the loop run status (`observing`, `verifying`, `completed_unreviewed`, `blocked`, `needs_human`, `failed`, `cancelled`) |
| `loop.event` | Emit a structured event into the loop's event log |
| `human.wait` | Block until a human responds in the UI |
| `round.next` | Increment the round counter (enforces `max_rounds`) |
| `gates.run` | Run verifier gate commands and branch on pass/fail/error |

## Runtime: Explicit Tick Model

The recipe runtime executes **exactly one step per tick**. Each call to:

```bash
planeai-cli axi loop tick <LOOP_ID>
```

performs one deterministic transition. A tick may:
- Execute a step and complete it (advance `current_step`)
- Return "waiting" (keep `current_step` unchanged)
- Fail a step and set the loop to `needs_human` or `failed`

A tick will **never** execute an unbounded chain of steps.

### Runtime State

The recipe runtime state is stored in `loop_runs.policy_json` as a snapshot:

```json
{
  "recipe_schema": "planeai.loop.recipe.v1",
  "recipe_id": "maker-verifier",
  "recipe_source": "builtin",
  "inputs": { "goal": "Fix the bug" },
  "runtime": {
    "current_step": "wait_for_maker",
    "tick_count": 3,
    "round": 1,
    "created_session_ids": { "maker": ["session-abc123"] },
    "last_error": null
  },
  "policy": { "max_rounds": 3, "max_ticks": 50, "max_sessions": 5, "merge_policy": "human" }
}
```

### Terminal Status Guards

If the loop is in a terminal status (`completed_unreviewed`, `failed`, `cancelled`, `approved`, `merged`, `cleaned`), tick refuses to execute.

If the loop requires intervention (`blocked`, `needs_human`, `stale`), tick returns a guarded response without advancing.

### `round.next` Step

Increments the round counter. Used for retry loops (verifier fails → increment round → re-prompt maker).

```yaml
- id: next_round
  kind: round.next
  next: prompt_maker_again
```

Enforces `policy.max_rounds`. When the limit is reached, the loop transitions to `needs_human`.

### `gates.run` Step

Runs verifier gate commands declared inline. Each gate has a `name` and `command`:

```yaml
- id: run_gates
  kind: gates.run
  gates:
    - name: rust-tests
      command: "cargo test"
    - name: lint
      command: "cargo clippy -- -D warnings"
  on:
    pass: completed_unreviewed
    fail: prompt_maker_again
    error: needs_human
```

Gates execute in order and stop on the first failure. Results are persisted to `verifier_runs`.

### `handoff.wait` Acceptance Model

A handoff is considered "accepted" only when:
1. It was recorded through `LoopService::record_handoff` (not arbitrary `add_artifact`)
2. Its `content_json.schema` equals `"planeai.handoff.v1"`
3. Its `content_json.status` is one of: `completed`, `blocked`, `needs_human`, `failed`

The runtime does **not** scan the filesystem for handoff files. Only database-recorded handoffs count.

## Future Step Kinds (Not Yet Supported)

These are reserved in the schema but not implemented:

| Kind | Intent |
|------|--------|
| `pr.feedback.wait` | Wait for PR review comments |
| `arbiter.rank` | Have a judge agent rank multiple outputs |
| `task.create` | Create a task in the internal tracker |
| `connector.call` | Call an external connector (Jira, Slack, etc.) |

## Supported Template Variables (v1)

Prompt templates use [minijinja](https://github.com/mitsuhiko/minijinja) syntax (Jinja2-compatible, sandboxed — no file inclusion or shell execution).

| Variable | Description |
|----------|-------------|
| `{{ inputs.goal }}` | The goal passed at loop creation |
| `{{ inputs.task_key }}` | The task key (if provided) |
| `{{ inputs.<key> }}` | Any custom input defined in the recipe |
| `{{ loop_run.id }}` | The loop run ID |
| `{{ recipe.id }}` | The recipe ID |
| `{{ knowledge.files }}` | Rendered list of knowledge file references |
| `{{ runtime.round }}` | Current round number |
| `{{ runtime.last_error }}` | Last error message (if any) |

**Conditional blocks:**

```
{% if inputs.task_key %}
Task: {{ inputs.task_key }}
{% endif %}
```

Blocks are removed entirely when the referenced input is absent.

## Safety Rules

1. **Bounded execution** — Every recipe must declare `max_ticks` in `policy`. The runner refuses to start unbounded loops.
2. **Human merge only** — `merge_policy` only accepts `human` in v1. No auto-merge.
3. **No auto-merge** — Even if all agents agree, a human must approve before changes land on the target branch.
4. **No arbitrary shell** — Steps cannot execute arbitrary shell commands. The `gates.run` step kind executes only recipe-authored gate commands declared in the YAML — agents cannot inject commands at runtime.

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

1. The agent calls `planeai-cli axi loop handoff record` with a structured JSON file.
2. `LoopService::record_handoff` atomically persists the artifact and event.
3. On the next tick, `handoff.wait` detects the accepted handoff and advances.

The `on` mapping determines which step to advance to based on handoff status:

```yaml
- id: wait_for_maker
  kind: handoff.wait
  from: maker
  on:
    completed: run_gates
    blocked: blocked_status
    needs_human: needs_human_status
    failed: failed_status
```

**Important:** The runtime does NOT scan the filesystem for handoff files. Only handoffs recorded through the `record_handoff` API and stored in `loop_artifacts` are considered.

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
