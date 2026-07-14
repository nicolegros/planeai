# Loop Recipes

Loop recipes are declarative YAML definitions that describe AI engineering loops — repeatable, multi-agent workflows with durable state, human review gates, and worktree isolation. A recipe tells planeai _what_ agents to spin up, _how_ they hand off work, and _when_ a human must intervene.

## Loop-Engineering Principles

Recipes encode the core loop-engineering model:

| Principle             | Recipe mapping                                                                  |
| --------------------- | ------------------------------------------------------------------------------- |
| Heartbeat/tick model  | Each `step` is a tick; the loop runner advances one step at a time              |
| Worktree isolation    | `session.create` steps spawn agents in isolated git worktrees                   |
| Project knowledge     | `knowledge` field injects docs, context files, and ADRs into agent prompts      |
| Tools/connectors      | `tools` field declares which MCP servers or CLI tools are available             |
| Role-based sub-agents | `roles` field defines named agents with distinct system prompts and tool access |
| Durable state         | Loop state persists in SQLite; resumes after app restart or crash               |
| Human review          | `human.wait` steps and `merge_policy: human` enforce manual approval            |

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

Parameters the user supplies when creating a loop run. A map of input name to an input definition object.

```yaml
inputs:
  goal:
    type: textarea
    label: Goal
    description: What should the maker implement?
    required: true
  task_key:
    type: task
    label: Linked task
    required: false
  gate_command:
    type: text
    label: Gate command
    description: Command to verify the implementation
    required: false
    default: make ci
  merge_strategy:
    type: select
    label: Merge strategy
    options:
      - value: squash
        label: Squash merge
      - value: rebase
        label: Rebase
    default: squash
  draft_pr:
    type: boolean
    label: Draft PR
    default: true
  max_retries:
    type: number
    label: Max retries
    default: 3
```

**Input definition fields:**

| Field         | Type            | Default  | Description                                                             |
| ------------- | --------------- | -------- | ----------------------------------------------------------------------- |
| `required`    | bool            | `false`  | Whether the user must supply a value                                    |
| `type`        | string          | `text`   | Input widget type (see below)                                           |
| `label`       | string          | key name | Human-readable label shown in the form                                  |
| `description` | string          | null     | Help text displayed below the input field                               |
| `default`     | string/bool/num | null     | Pre-filled value; type matches `type` field                             |
| `options`     | list            | `[]`     | Choices for `select` inputs; each entry has `value` and `label` strings |

**Supported input types:**

| Type       | Widget                               | Default value type |
| ---------- | ------------------------------------ | ------------------ |
| `text`     | Single-line text input               | string             |
| `textarea` | Multi-line text input                | string             |
| `branch`   | Branch picker (populated from git)   | string             |
| `task`     | Task picker (populated from project) | string             |
| `boolean`  | Checkbox                             | bool               |
| `select`   | Dropdown with `options`              | string             |
| `number`   | Numeric input                        | number             |

When `type` is omitted, it defaults to `text` for backwards compatibility. Inputs are rendered in alphabetical order in the create form.

The builtin `maker-verifier` recipe accepts a `gate_command` input that overrides the default CI command used in the `gates.run` step. If omitted, it defaults to `make ci`.

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

| Field            | Type    | Default | Description                                  |
| ---------------- | ------- | ------- | -------------------------------------------- |
| `max_rounds`     | integer | 3       | Maximum iteration rounds                     |
| `max_ticks`      | integer | 50      | Hard cap on total steps executed             |
| `max_sessions`   | integer | 5       | Maximum concurrent agent sessions            |
| `stale_after_ms` | integer | null    | Wall-clock staleness timeout in milliseconds |
| `merge_policy`   | string  | `human` | Only `human` is supported in v1              |
| `auto_approve`   | bool    | `true`  | Launch sessions in auto-approve (yolo) mode  |

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

| Field        | Description                                                                                                                                                                                                                                     |
| ------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`         | Unique step identifier (required)                                                                                                                                                                                                               |
| `kind`       | Step kind — see supported kinds below (required)                                                                                                                                                                                                |
| `role`       | Target role for session steps                                                                                                                                                                                                                   |
| `prompt`     | Message/instruction text                                                                                                                                                                                                                        |
| `branch`     | Branch override for `session.create` steps; uses an existing branch instead of generating one. Supports template rendering (e.g., `{{ inputs.branch }}`). When empty or absent, a loop-managed branch (`loop/<id>/<role>-r<round>`) is created. |
| `from`       | Source role for handoff.wait and candidates.wait                                                                                                                                                                                                |
| `on`         | Condition map for conditional steps                                                                                                                                                                                                             |
| `status`     | Target status for loop.status                                                                                                                                                                                                                   |
| `next`       | Explicit next step ID (overrides sequential order)                                                                                                                                                                                              |
| `select`     | Selection criteria                                                                                                                                                                                                                              |
| `event_kind` | Event kind for loop.event                                                                                                                                                                                                                       |
| `gates`      | List of gate declarations for gates.run steps                                                                                                                                                                                                   |
| `providers`  | Comma-separated provider list for `candidates.create` steps (template-rendered, e.g., `{{ inputs.providers }}`)                                                                                                                                 |

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

- `max_rounds` (default: 3) limits retry cycles. When reached at any `round.next` step, the loop transitions to `blocked`.
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
  gate_command:
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
      - name: ci
        command: "{{ inputs.gate_command | default('make ci') }}"
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

A successful run with no rejections (auto-advance reduces manual tick count):

```bash
# Create the loop (auto-advances through create_maker → wait_for_maker → parks at observing)
planeai-cli axi loop create --recipe maker-verifier --goal "Implement pagination" --start
# → session created, status: observing (waiting for maker handoff)

# After maker runs `planeai-cli axi loop handoff record ...`
# Auto-advance: wait_for_maker → run_gates → create_verifier → wait_for_verifier → parks at observing
# → gates: pass, status: observing (waiting for verifier handoff)

# After verifier runs `planeai-cli axi loop handoff record ...`
# Auto-advance: wait_for_verifier → completed_unreviewed (terminal)
# → status: completed_unreviewed — human should review and merge
```

### `completed_unreviewed` Terminal State

This is the safe terminal state for the maker-verifier strategy. It means:

1. The maker's implementation passed automated gates (CI checks)
2. The verifier agent reviewed and approved the changes
3. **No merge has happened** — a human must still review the PR/branch and merge

The human can then:

- Approve and merge manually
- Request another round by updating the loop status
- Close the loop if the work is no longer needed

## Built-in N-Candidates + Arbiter Recipe (Experimental)

> **⚠️ Experimental** — This strategy is new and its behavior may change between releases.

The N-Candidates + Arbiter strategy exploits PlaneAI's parallel execution capability by launching multiple independent implementations of the same task, then asking an arbiter agent to rank the results.

### When to Use

- When you want to compare approaches from different AI coding agents (Claude, Codex, Kiro, Copilot, etc.)
- When a task has multiple valid solutions and you want to evaluate trade-offs
- When you want to increase confidence in a solution by comparing independent implementations

### Cost and Runtime Implications

- **Compute:** Each candidate runs a full implementation in parallel. Running 3 providers means 3× the token/compute cost of a single-agent loop.
- **Disk:** Each candidate gets its own git worktree. Ensure sufficient disk space.
- **Time:** Candidates run in parallel, so wall-clock time is bounded by the slowest candidate plus arbiter review time.
- **Sessions:** The recipe defaults to `max_sessions: 10` to accommodate N candidates + 1 arbiter.

### Advisory Selection

The arbiter's ranking is **advisory only**. The loop completes in `completed_unreviewed` status — a human must still:

1. Review the arbiter's ranking and rationale
2. Inspect the winning candidate's diff
3. Approve and merge manually

No automatic merge occurs. The `merge_policy: human` setting is enforced.

### State Machine

```
┌────────────────────┐     ┌─────────────────────┐     ┌──────────┐
│ create_candidates  │────▶│ wait_for_candidates  │────▶│run_gates │
│ (N sessions)       │     │ (all must hand off)  │     └────┬─────┘
└────────────────────┘     └─────────────────────┘          │
                                                     pass   │  fail/error
                                                      ┌─────┘───────┐
                                                      ▼             ▼
                                               ┌──────────────┐  ┌───────────┐
                                               │create_arbiter│◀─│gates_note │
                                               └──────┬───────┘  └───────────┘
                                                      │
                                                      ▼
                                               ┌──────────────────┐
                                               │wait_for_arbiter  │
                                               └──────┬───────────┘
                                                      │
                                         completed    │  needs_human/blocked/failed
                                              ┌───────┴───────┐
                                              ▼               ▼
                                    ┌──────────────────┐  ┌────────────┐
                                    │completed_unreviewed│  │needs_human │ (terminal)
                                    └──────────────────┘  └────────────┘
```

### CLI Usage

```bash
planeai-cli axi loop create \
  --strategy n-candidates-arbiter \
  --goal "Implement the feature" \
  --providers claude,codex,kiro \
  --arbiter-provider copilot \
  --max-rounds 1 \
  --start
```

Or equivalently using `--input` flags:

```bash
planeai-cli axi loop create \
  --strategy n-candidates-arbiter \
  --input goal="Implement the feature" \
  --input providers="claude,codex,kiro" \
  --input arbiter_provider="copilot" \
  --start
```

### Inputs

| Input              | Type     | Required | Default   | Description                           |
| ------------------ | -------- | -------- | --------- | ------------------------------------- |
| `goal`             | textarea | yes      | —         | What should the candidates implement? |
| `task_key`         | task     | no       | —         | Linked task key                       |
| `providers`        | text     | yes      | —         | Comma-separated provider names        |
| `arbiter_provider` | text     | no       | `default` | Provider for the arbiter session      |
| `gate_command`     | text     | no       | `make ci` | Command to verify each candidate      |

### Roles

| Role    | Mode   | Isolation | Description                                 |
| ------- | ------ | --------- | ------------------------------------------- |
| maker   | write  | worktree  | Each candidate works in its own worktree    |
| arbiter | review | readonly  | Reviews all candidates, does not edit files |

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

| Kind                | Description                                                                                                                                                                  |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `session.create`    | Spawn a new agent session (in a worktree by default) and send initial prompt. Supports an optional `branch` field to check out an existing branch instead of generating one. |
| `session.prompt`    | Send a message to an existing session (requires `select: latest`)                                                                                                            |
| `handoff.wait`      | Pause until the source role produces an accepted handoff artifact                                                                                                            |
| `loop.status`       | Set the loop run status (`observing`, `verifying`, `completed_unreviewed`, `approved`, `blocked`, `needs_human`, `failed`, `cancelled`)                                      |
| `loop.event`        | Emit a structured event into the loop's event log                                                                                                                            |
| `human.wait`        | Block until a human responds in the UI                                                                                                                                       |
| `round.next`        | Increment the round counter (enforces `max_rounds`)                                                                                                                          |
| `gates.run`         | Run verifier gate commands and branch on pass/fail/error                                                                                                                     |
| `candidates.create` | Create N candidate sessions in parallel (one per provider). Requires `providers` field (comma-separated, template-rendered). **(experimental)**                              |
| `candidates.wait`   | Wait for all candidate sessions to produce handoffs. Routes via `on: { all_complete: <step> }` when all are done. **(experimental)**                                         |
| `arbiter.rank`      | Create an arbiter session with candidate summaries injected via `{{ candidates }}` template variable. **(experimental)**                                                     |

## Runtime: Auto-Advance Tick Model

The recipe runtime executes **one step per tick**, but multiple ticks are chained automatically via **auto-advance**. When a loop is created, started, or receives a completed handoff, the runtime auto-advances through immediately-executable steps (up to 10 ticks) without requiring manual intervention.

Auto-advance stops when:

- A tick returns an error (non-zero code).
- The loop reaches a terminal or intervention-required state.
- The current step did not advance (i.e., the step is waiting for external input, such as `handoff.wait` with no handoff available).
- The current step is a `human.wait` (requires explicit user action).

This means that in practice, creating or starting a loop run will automatically execute through `session.create` → `handoff.wait` (which parks in `observing` when no handoff is ready), and a completed handoff will trigger advancement through `handoff.wait` → `gates.run` → subsequent steps without requiring a manual tick.

### Manual Tick

A manual tick via `planeai-cli axi loop tick <LOOP_ID>` also auto-advances: it executes the current step and continues until a stopping condition is reached.

Each call to:

```bash
planeai-cli axi loop tick <LOOP_ID>
```

performs one or more deterministic transitions. A tick sequence may:

- Execute steps and advance through them (update `current_step`)
- Return "waiting" when `handoff.wait` has no handoff ready
- Fail and set the loop to `blocked`, `needs_human`, or `failed`

### Runtime State

The recipe runtime state is stored in `loop_runs.policy_json` as a snapshot:

```json
{
  "recipe_schema": "planeai.loop.recipe.v1",
  "recipe_id": "maker-verifier",
  "recipe_name": "Maker + Verifier",
  "recipe_description": "One agent builds, another verifies.",
  "recipe_source": "builtin",
  "inputs": { "goal": "Fix the bug" },
  "input_defs": {
    "goal": { "required": true, "type": "textarea", "label": "Goal" }
  },
  "runtime": {
    "current_step": "wait_for_maker",
    "tick_count": 3,
    "round": 1,
    "created_session_ids": { "maker": ["session-abc123"] },
    "last_error": null,
    "last_handoff_consumed_at": null,
    "last_activity_at": "2025-07-09T21:30:00+00:00",
    "session_observations": {
      "session-abc123": { "last_cursor": 12 }
    }
  },
  "policy": {
    "max_rounds": 3,
    "max_ticks": 50,
    "max_sessions": 5,
    "stale_after_ms": 600000,
    "merge_policy": "human",
    "auto_approve": true
  }
}
```

### Terminal Status Guards

If the loop is in a terminal status (`completed_unreviewed`, `failed`, `cancelled`, `approved`, `merged`, `cleaned`), tick refuses to execute.

If the loop requires intervention (`blocked`, `needs_human`, `stale`), tick returns a guarded response without advancing.

### Stale Detection

Stale detection identifies loops where agents have stopped making progress. It is configured via `policy.stale_after_ms` and runs on every explicit tick — there is no background scheduler or timer.

**How it works:**

1. The runtime tracks `last_activity_at` in the recipe snapshot (stored in `policy_json`).
2. `last_activity_at` is refreshed only on **meaningful activity**: session creation, handoff accepted, verifier completed, or new session output observed.
3. On each tick, before dispatching to a step executor, the runner checks whether `now - last_activity_at >= stale_after_ms`.
4. If the threshold is exceeded, the loop transitions to `stale` status and the tick returns TOON next_actions.

**Per-session observation:**

Each tick observes all loop-owned sessions by checking the event log for new session-referencing events since the last observation. Per-session state is tracked in `runtime.session_observations`:

```json
{
  "session_observations": {
    "session-abc123": {
      "last_cursor": 42
    }
  }
}
```

When new activity is detected for a session, a `loop_heartbeat` event is emitted and `last_activity_at` is refreshed.

**When activity is refreshed:**

- A session is created (`session.create` step)
- A handoff is found (`handoff.wait` step matches)
- A verifier gate completes (`gates.run` step)
- New session output is detected during observation (heartbeat)

**When activity is NOT refreshed:**

- Polling ticks with no result (e.g., `handoff.wait` with no handoff found)
- Events appended by the stale detection system itself

**When stale detection does NOT trigger:**

- `stale_after_ms` is not configured (`null` or absent)
- `last_activity_at` was never set (legacy snapshots created before this feature)
- The loop is already in a terminal or intervention-required status

**TOON next_actions for stale loops:**

```
next_actions[4]:
  - inspect session output for progress
  - prompt worker to continue
  - stop loop: `planeai-cli axi loop stop <ID>`
  - mark blocked if external dependency is stalling
```

**Recovering from stale:** A human (or orchestrator) must update the loop status back to `running` or `observing` before ticks will execute again. This prevents runaway detection loops.

**Example configuration:**

```yaml
policy:
  stale_after_ms: 600000 # 10 minutes
```

**Important:** Stale detection is deterministic and tick-driven. If no one calls `loop tick`, stale detection does not fire. This is by design — planeai does not run background work for loop observation.

### `round.next` Step

Increments the round counter. Used for retry loops (verifier fails → increment round → re-prompt maker).

```yaml
- id: next_round
  kind: round.next
  next: prompt_maker_again
```

Enforces `policy.max_rounds`. When the limit is reached, the loop transitions to `blocked`.

### `gates.run` Step

Runs verifier gate commands declared inline. Each gate has a `name` and `command`. Gate commands support the same [minijinja template variables](#supported-template-variables-v1) as prompt fields (e.g., `{{ inputs.gate_command | default('make ci') }}`):

```yaml
- id: run_gates
  kind: gates.run
  gates:
    - name: ci
      command: "{{ inputs.gate_command | default('make ci') }}"
  on:
    pass: completed_unreviewed
    fail: prompt_maker_again
    error: needs_human
```

Gates execute in order and stop on the first failure. The loop transitions to `verifying` while gates are running and back to `running` when they complete (via `GatesStarted` / `GatesCompleted` triggers). Results are persisted to `verifier_runs`. On failure, the gate's captured output (truncated to 100 KB) is stored in `runtime.last_error` so the retry prompt can include the failure details. When the output is truncated, a reference to the full output log file path is appended.

### `handoff.wait` Acceptance Model

A handoff is considered "accepted" only when:

1. It was recorded through `LoopService::record_handoff` (not arbitrary `add_artifact`)
2. Its `content_json.schema` equals `"planeai.handoff.v1"`
3. Its `content_json.status` is one of: `completed`, `blocked`, `needs_human`, `failed`
4. It was recorded **after** the last consumed handoff timestamp (`last_handoff_consumed_at`)

The `last_handoff_consumed_at` field prevents re-consuming stale handoffs from previous rounds. When a handoff is consumed, the runtime records the current timestamp; subsequent `handoff.wait` steps only consider handoffs newer than that timestamp.

The runtime does **not** scan the filesystem for handoff files. Only database-recorded handoffs count.

## Future Step Kinds (Not Yet Supported)

These are reserved in the schema but not implemented:

| Kind               | Intent                                         |
| ------------------ | ---------------------------------------------- |
| `pr.feedback.wait` | Wait for PR review comments                    |
| `task.create`      | Create a task in the internal tracker          |
| `connector.call`   | Call an external connector (Jira, Slack, etc.) |

## Supported Template Variables (v1)

Prompt templates use [minijinja](https://github.com/mitsuhiko/minijinja) syntax (Jinja2-compatible, sandboxed — no file inclusion or shell execution).

| Variable                   | Description                                                              |
| -------------------------- | ------------------------------------------------------------------------ |
| `{{ inputs.goal }}`        | The goal passed at loop creation                                         |
| `{{ inputs.task_key }}`    | The task key (if provided)                                               |
| `{{ inputs.<key> }}`       | Any custom input defined in the recipe                                   |
| `{{ loop_run.id }}`        | The loop run ID                                                          |
| `{{ recipe.id }}`          | The recipe ID                                                            |
| `{{ knowledge.files }}`    | Rendered list of knowledge file references                               |
| `{{ runtime.round }}`      | Current round number                                                     |
| `{{ runtime.last_error }}` | Last error message (if any)                                              |
| `{{ candidates }}`         | Formatted candidate summaries (available in `arbiter.rank` prompts only) |

**Conditional blocks:**

```
{% if inputs.task_key %}
Task: {{ inputs.task_key }}
{% endif %}
```

Blocks are removed entirely when the referenced input is absent.

## Safety Rules

1. **Bounded execution** — Every recipe must declare `max_ticks` in `policy`. The runner refuses to start unbounded loops.
2. **Bounded auto-advance** — Auto-advance executes at most 10 ticks per trigger and stops at `human.wait`, terminal, intervention-required states, or when the current step does not advance (waiting for external input). This prevents runaway execution chains.
3. **Human merge only** — `merge_policy` only accepts `human` in v1. No auto-merge.
4. **No auto-merge** — Even if all agents agree, a human must approve before changes land on the target branch.
5. **No arbitrary shell** — Steps cannot execute arbitrary shell commands. The `gates.run` step kind executes only recipe-authored gate commands declared in the YAML — agents cannot inject commands at runtime.

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
3. Auto-advance triggers immediately: `handoff.wait` detects the accepted handoff and advances through subsequent steps (stopping at `gates.run`, `human.wait`, terminal, or observing states).

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

| Term           | Description                                                                               |
| -------------- | ----------------------------------------------------------------------------------------- |
| **LoopRecipe** | A YAML definition describing a reusable loop template                                     |
| **LoopRun**    | A single execution instance of a recipe, with its own state and history                   |
| **LoopStep**   | One tick in a run — the atomic unit of execution                                          |
| **LoopRole**   | A named agent persona with specific prompt, tools, and constraints                        |
| **LoopPolicy** | Constraints governing a run (max ticks, duration, merge policy)                           |
| **LoopSignal** | An event or condition that causes the runner to advance or pause                          |
| **LoopMemory** | Durable state attached to a run — artifacts, events, and step outputs persisted in SQLite |
