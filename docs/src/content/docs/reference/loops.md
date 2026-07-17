---
title: Loops Reference (Experimental)
description: Complete reference for loop recipe schema, step kinds, template variables, and CLI commands.
---

:::caution[Experimental]
Loops are under active development. Behavior, recipe schema, and CLI commands may change between releases.
:::

Loop recipes are declarative YAML definitions that describe multi-agent workflows with durable state, human review gates, and worktree isolation. This page documents every field and option.

## Recipe Schema

Top-level structure of a `.yaml` recipe file:

```yaml
schema: planeai.loop.recipe.v1
id: string          # unique identifier (used in CLI and DB)
name: string        # human-readable name
description: string # one-line summary

trigger: ...
inputs: ...
knowledge: ...
tools: ...
roles: ...
policy: ...
steps: ...
```

All recipes must use `schema: planeai.loop.recipe.v1`.

---

## `trigger`

Declares what event starts the recipe.

| Kind           | Status         | Description                    |
| -------------- | -------------- | ------------------------------ |
| `manual`       | Executable     | User creates the run via UI/CLI |
| `schedule`     | Future         | Cron-style schedule            |
| `github_event` | Future         | GitHub webhook event           |
| `task_event`   | Future         | Internal task state change     |
| `pr_feedback`  | Future         | PR review comments received    |
| `ci_failure`   | Future         | CI pipeline failure            |

Only `manual` is executable in v1. Future kinds are recognized in the schema but rejected at runtime.

```yaml
trigger:
  kind: manual
```

---

## `inputs`

Parameters the user supplies when creating a loop run. A map of input name → input definition.

### Input definition fields

| Field         | Type            | Default  | Description                                   |
| ------------- | --------------- | -------- | --------------------------------------------- |
| `required`    | bool            | `false`  | Whether the user must supply a value          |
| `type`        | string          | `text`   | Input widget type (see table below)           |
| `label`       | string          | key name | Human-readable label shown in the form        |
| `description` | string          | null     | Help text displayed below the input field     |
| `default`     | string/bool/num | null     | Pre-filled value; type matches `type` field   |
| `options`     | list            | `[]`     | Choices for `select` inputs (`value` + `label`) |

### Supported input types

| Type       | Widget                               | Default value type |
| ---------- | ------------------------------------ | ------------------ |
| `text`     | Single-line text input               | string             |
| `textarea` | Multi-line text input                | string             |
| `branch`   | Branch picker (populated from git)   | string             |
| `task`     | Task picker (populated from project) | string             |
| `boolean`  | Checkbox                             | bool               |
| `select`   | Dropdown with `options`              | string             |
| `number`   | Numeric input                        | number             |

When `type` is omitted, defaults to `text`. Inputs are rendered in alphabetical order in the create form.

```yaml
inputs:
  goal:
    type: textarea
    label: Goal
    description: What should the maker implement?
    required: true
  gate_command:
    type: text
    label: Gate command
    required: false
    default: make ci
  draft_pr:
    type: boolean
    label: Draft PR
    default: true
  merge_strategy:
    type: select
    label: Merge strategy
    options:
      - value: squash
        label: Squash merge
      - value: rebase
        label: Rebase
    default: squash
```

---

## `knowledge`

Files and instructions injected into agent context for every session in the loop.

| Field          | Type          | Description                                    |
| -------------- | ------------- | ---------------------------------------------- |
| `files`        | list\<string\> | Relative file paths to inject as context       |
| `instructions` | list\<string\> | Free-text instructions appended to all prompts |

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

---

## `tools`

MCP servers or CLI tools available to agents in this loop.

| Field      | Type          | Description                        |
| ---------- | ------------- | ---------------------------------- |
| `required` | list\<string\> | Tools that must be available       |
| `optional` | list\<string\> | Tools that may be used if present  |

```yaml
tools:
  required:
    - git
    - filesystem
  optional:
    - planeai-tasks
    - github
```

---

## `roles`

Named agent personas. A map of role ID → role configuration.

### Role fields

| Field           | Type   | Default    | Description                                                        |
| --------------- | ------ | ---------- | ------------------------------------------------------------------ |
| `provider`      | string | `default`  | Which AI provider to use for this role                             |
| `mode`          | string | (required) | Agent mode (see table below)                                       |
| `isolation`     | string | `worktree` | Git isolation strategy (see table below)                           |
| `instructions`  | string | null       | System-level instructions injected into the agent's context        |
| `session_reuse` | bool   | `true`     | Reuse existing session on subsequent rounds instead of spawning new |

### Supported modes

| Mode       | Description                                           |
| ---------- | ----------------------------------------------------- |
| `write`    | Full read/write access to the codebase                |
| `review`   | Read access only; produces review feedback            |
| `readonly` | Read access only; no modifications                    |

The `mode` field is a string — you can use custom values (e.g., `plan`, `triage`) as conventions in your prompts, but only `write`, `review`, and `readonly` have runtime semantics.

### Supported isolation values

| Isolation  | Description                                                       |
| ---------- | ----------------------------------------------------------------- |
| `worktree` | Dedicated git worktree per session (full isolation)               |
| `project`  | Works in the main project directory (shared working tree)         |
| `readonly` | Read-only access to the project; no worktree creation             |

### Session reuse behavior

When `session_reuse: true` (default), a `session.create` step for a role that already has an active session **re-prompts the existing session** instead of spawning a new one. The agent retains conversational context from prior rounds, and no new process/worktree is created.

When `session_reuse: false`, each `session.create` spawns a fresh session (clean slate each round).

**Fallback:** If the existing session is no longer active (stopped/crashed), the runner creates a new session regardless of `session_reuse`.

```yaml
roles:
  maker:
    provider: default
    mode: write
    isolation: worktree
    session_reuse: true
    instructions: |
      You implement features using TDD. Write failing tests first,
      then make them pass with minimal code.
  verifier:
    provider: default
    mode: review
    isolation: readonly
    session_reuse: false
    instructions: |
      You review code for correctness, style, and test coverage.
```

---

## `policy`

Loop-level constraints and resource limits.

| Field            | Type    | Default  | Description                                         |
| ---------------- | ------- | -------- | --------------------------------------------------- |
| `max_rounds`     | integer | `3`      | Maximum iteration rounds before blocking            |
| `max_ticks`      | integer | `50`     | Hard cap on total steps executed                    |
| `max_sessions`   | integer | `5`      | Maximum concurrent agent sessions                   |
| `stale_after_ms` | integer | null     | Wall-clock staleness timeout in milliseconds        |
| `merge_policy`   | string  | `human`  | Only `human` is supported in v1                     |
| `auto_approve`   | bool    | `true`   | Launch sessions in auto-approve (yolo) mode         |

```yaml
policy:
  max_rounds: 3
  max_ticks: 50
  max_sessions: 5
  stale_after_ms: 600000
  merge_policy: human
  auto_approve: true
```

---

## `steps`

Ordered list of actions the loop runner executes. Each step has an `id`, a `kind`, and kind-specific fields.

### Step fields

| Field        | Type   | Description                                                                                                    |
| ------------ | ------ | -------------------------------------------------------------------------------------------------------------- |
| `id`         | string | Unique step identifier (required)                                                                              |
| `kind`       | string | Step kind — see supported kinds below (required)                                                               |
| `role`       | string | Target role for session steps                                                                                  |
| `prompt`     | string | Message/instruction text (supports template variables)                                                         |
| `branch`     | string | Branch override for `session.create`; supports templates. When absent, generates `loop/<id>/<role>-r<round>`   |
| `from`       | string | Source role for `handoff.wait`                                                                                 |
| `on`         | map    | Condition map for conditional routing (`status → step_id`)                                                     |
| `status`     | string | Target status for `loop.status`                                                                                |
| `next`       | string | Explicit next step ID (overrides sequential order)                                                             |
| `select`     | string | Selection criteria for `session.prompt` (e.g., `latest`)                                                       |
| `event_kind` | string | Event kind for `loop.event`                                                                                    |
| `gates`      | list   | Gate declarations for `gates.run` steps                                                                        |

---

### `session.create`

Spawns a new agent session (or re-prompts an existing one if `session_reuse: true`) and sends the initial prompt.

**Fields used:** `role` (required), `prompt` (required), `branch` (optional)

```yaml
- id: create_maker
  kind: session.create
  role: maker
  prompt: |
    Goal: {{ inputs.goal }}
    Round: {{ runtime.round }}
```

When `branch` is set, checks out that branch instead of generating one. Supports template rendering:

```yaml
- id: create_maker
  kind: session.create
  role: maker
  branch: "{{ inputs.branch }}"
  prompt: "Implement: {{ inputs.goal }}"
```

---

### `session.prompt`

Sends a message to an existing session.

**Fields used:** `role` (required), `prompt` (required), `select` (required — must be `latest`)

```yaml
- id: retry_maker
  kind: session.prompt
  role: maker
  select: latest
  prompt: |
    Round {{ runtime.round }} — fix the issues:
    {{ runtime.last_error }}
```

---

### `handoff.wait`

Pauses until the source role produces an accepted handoff artifact. Routes to the next step based on handoff status.

**Fields used:** `from` (required), `on` (required — maps status → step_id)

```yaml
- id: wait_for_maker
  kind: handoff.wait
  from: maker
  on:
    completed: run_gates
    blocked: blocked
    needs_human: needs_human
    failed: failed
```

**Acceptance criteria** — a handoff is accepted only when:

1. Recorded through `LoopService::record_handoff` (not arbitrary `add_artifact`)
2. `content_json.schema` equals `"planeai.handoff.v1"`
3. `content_json.status` is one of: `completed`, `blocked`, `needs_human`, `failed`
4. Recorded **after** `last_handoff_consumed_at` (prevents re-consuming stale handoffs from prior rounds)

The runtime does not scan the filesystem for handoff files. Only database-recorded handoffs count.

---

### `loop.status`

Sets the loop run to a terminal or intervention-required status.

**Fields used:** `status` (required)

Valid statuses: `observing`, `verifying`, `completed_unreviewed`, `approved`, `blocked`, `needs_human`, `failed`, `cancelled`

```yaml
- id: done
  kind: loop.status
  status: completed_unreviewed
```

---

### `loop.event`

Emits a structured event into the loop's event log.

**Fields used:** `event_kind` (required)

```yaml
- id: review_complete_event
  kind: loop.event
  event_kind: review.complete
```

---

### `human.wait`

Blocks execution until a human responds in the UI.

**Fields used:** `prompt` (required — shown to the human)

```yaml
- id: await_approval
  kind: human.wait
  prompt: "Review the verifier's feedback and approve or request changes."
```

---

### `round.next`

Increments the round counter. Enforces `policy.max_rounds` — when the limit is reached, the loop transitions to `blocked`.

**Fields used:** `next` (required — step to jump to after incrementing)

```yaml
- id: increment_round
  kind: round.next
  next: wait_for_maker
```

---

### `gates.run`

Runs verifier gate commands and routes based on the result. Gates execute in order and stop on first failure.

**Fields used:** `role` (required — session context for execution), `gates` (required), `on` (required — maps `pass`/`fail`/`error` → step_id)

**Gate object:**

| Field     | Type   | Description                                        |
| --------- | ------ | -------------------------------------------------- |
| `name`    | string | Human-readable gate name                           |
| `command` | string | Shell command to execute (supports templates)      |

On failure, the gate's captured output (truncated to 100 KB) is stored in `runtime.last_error` for the retry prompt. When truncated, a reference to the full log file path is appended.

```yaml
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
```

---

## Template Variables

Prompt templates use [minijinja](https://github.com/mitsuhiko/minijinja) syntax (Jinja2-compatible, sandboxed — no file inclusion or shell execution).

| Variable                   | Description                                |
| -------------------------- | ------------------------------------------ |
| `{{ inputs.goal }}`        | The goal passed at loop creation           |
| `{{ inputs.task_key }}`    | The task key (if provided)                 |
| `{{ inputs.<key> }}`       | Any custom input defined in the recipe     |
| `{{ loop_run.id }}`        | The loop run ID                            |
| `{{ recipe.id }}`          | The recipe ID                              |
| `{{ knowledge.files }}`    | Rendered list of knowledge file references |
| `{{ runtime.round }}`      | Current round number                       |
| `{{ runtime.last_error }}` | Last error message (if any)                |

### Conditional blocks

```jinja
{% if inputs.task_key %}
Task: {{ inputs.task_key }}
{% endif %}
```

Blocks are removed entirely when the referenced input is absent or falsy.

### Filters

Standard Jinja2 filters are available. The most useful for recipes:

```jinja
{{ inputs.gate_command | default('make ci') }}
```

---

## Recipe Locations & Precedence

Recipes are resolved in order (first match by `id` wins):

1. **Project** — `.planeai/loops/*.yaml` in the repository root
2. **User** — `~/.config/planeai/loops/*.yaml`
3. **Builtin** — bundled with the app

A project recipe with `id: maker-verifier` shadows the builtin of the same name.

---

## CLI Commands

### Recipe management

```bash
# List all available recipes (project + user + builtin)
planeai-cli axi loop recipe ls

# Show full YAML of a recipe by ID or path
planeai-cli axi loop recipe show <id-or-path>

# Validate schema, step references, and role consistency
planeai-cli axi loop recipe validate <id-or-path>
```

### Loop run lifecycle

```bash
# Create a new loop run
planeai-cli axi loop create --recipe <id> [--goal <text>] [--input key=val]... [--start] [--dry-run]

# Advance the loop (auto-advances through executable steps)
planeai-cli axi loop tick <ID>

# Check current status and step pointer
planeai-cli axi loop status <ID>

# Stop a running loop
planeai-cli axi loop stop <ID>
```

| Command                          | Description                                              |
| -------------------------------- | -------------------------------------------------------- |
| `axi loop recipe ls`            | List recipes from all sources with ID, name, and origin  |
| `axi loop recipe show <id>`     | Print full recipe YAML                                   |
| `axi loop recipe validate <id>` | Check schema conformance; exits non-zero on errors       |
| `axi loop create`               | Instantiate a run, resolve inputs, optionally start      |
| `axi loop tick <ID>`            | Execute current step + auto-advance                      |
| `axi loop status <ID>`          | Show run status, current step, round, tick count         |
| `axi loop stop <ID>`            | Cancel and stop the loop                                 |

`--dry-run` on `create` previews the resolved recipe and inputs without executing.

---

## Safety Rules

1. **Bounded execution** — `max_ticks` (default: 50) caps total steps. The loop transitions to `blocked` when the limit is reached.
2. **Bounded auto-advance** — Auto-advance executes at most 10 ticks per trigger. Stops at `human.wait`, terminal states, intervention-required states, or when the current step does not advance (waiting for external input).
3. **Human merge only** — `merge_policy` only accepts `human` in v1. No auto-merge.
4. **No auto-merge** — Even if all agents agree, a human must approve before changes land on the target branch.
5. **No arbitrary shell** — Steps cannot execute arbitrary shell commands. `gates.run` executes only recipe-authored gate commands declared in the YAML — agents cannot inject commands at runtime.

---

## Builtin: No Mistakes

The builtin `no-mistakes` recipe implements a full post-implementation validation pipeline: rebase → review → test → document → lint → push → PR. It validates committed work on a branch and opens a draft PR when all checks pass.

### Pipeline

```
┌────────┐     ┌────────────┐     ┌───────────┐     ┌──────────┐     ┌──────┐     ┌─────────┐
│ rebase │────▶│   review   │────▶│ run_tests │────▶│ document │────▶│ lint │────▶│ push+PR │
└────────┘     └────────────┘     └───────────┘     └──────────┘     └──────┘     └─────────┘
                     ▲              │ fail                                │ fail        │
                     │              ▼                                     ▼             ▼
                     │       ┌─────────────┐                      ┌───────────┐   completed
                     │       │  fix tests  │                      │ fix lint  │   (approved)
                     │       └──────┬──────┘                      └─────┬─────┘
                     │              │                                    │
                     └──────────────┴────────────────────────────────────┘
                              (all fixes cycle back to review)
```

**Roles:**

| Role        | Mode   | Isolation | Description                                                |
| ----------- | ------ | --------- | ---------------------------------------------------------- |
| reviewer    | review | readonly  | Reviews the diff, classifies findings by severity/action   |
| gatekeeper  | write  | project   | Applies fixes, updates docs, pushes branch, opens PR       |

**Terminal states:**

- `approved` — all checks passed, PR opened, ready for human merge.
- `blocked` — `max_rounds` reached.
- `needs_human` — rebase conflicts too complex, or ask-user findings need human decision.

### Full Recipe YAML

```yaml
schema: planeai.loop.recipe.v1
id: no-mistakes
name: No Mistakes
description: >
  Post-implementation validation pipeline: review → test → document → lint → push → PR.
  Replicates the no-mistakes workflow natively in planeai.

trigger:
  kind: manual

inputs:
  branch:
    type: branch
    label: Implementation branch
    description: The branch with committed work to validate
    required: true
  target_branch:
    type: branch
    label: PR target branch
    description: Branch to open the PR against
    required: false
    default: main
  gate_command:
    type: text
    label: Test command
    description: Command to run tests (exit 0 = pass)
    required: false
    default: make ci
  lint_command:
    type: text
    label: Lint command
    description: Command to run linters (exit 0 = pass). Leave empty to skip.
    required: false

knowledge:
  files:
    - AGENTS.md
    - CONTEXT.md
  instructions:
    - Infer the intent of the work from the commit history on the branch.
    - Do not introduce unrelated changes.
    - Keep fixes minimal and focused on the issue at hand.
    - Every fact has one authoritative owner document. Do not create new docs surfaces for perceived gaps.

tools:
  required:
    - git
    - plane_sessions
    - plane_loops
  optional:
    - github

roles:
  reviewer:
    provider: default
    mode: review
    isolation: readonly
    instructions: |
      You are the reviewer agent in a no-mistakes validation pipeline.
      Your job is to review the diff and assess quality — correctness, intent alignment,
      edge cases, missing tests, documentation gaps, and style.
      You do NOT edit files. You only review.

      For each issue found, classify its severity:
      - error: must be fixed before merge (bugs, security, data loss)
      - warning: should be fixed (missing tests, unclear logic, doc gaps)
      - info: informational, no action needed

      And classify its action:
      - auto-fix: mechanical fix the gatekeeper can apply safely
      - ask-user: challenges the author's intent or changes product behavior
      - no-op: informational only

      If all findings are info/no-op, or there are no findings, hand off with status: completed.
      If any finding is error or warning, hand off with status: needs_human and list the findings.

  gatekeeper:
    provider: default
    mode: write
    isolation: project
    instructions: |
      You are the gatekeeper agent in a no-mistakes validation pipeline.
      You operate directly on the implementation branch.

      When fixing review findings:
      - Fix only the specific issues listed. Do not refactor unrelated code.
      - For auto-fix findings, apply the mechanical fix.
      - For ask-user findings: do NOT fix them. Note them in your handoff summary.
      - Commit fixes with conventional commit messages.

      When fixing test/lint failures:
      - Fix the root cause. Do NOT skip tests or suppress errors.

      When pushing:
      - Push the branch and open a draft PR with gh pr create --draft.

      Do not claim completion unless you have recorded a structured handoff.

policy:
  max_rounds: 5
  max_ticks: 50
  max_sessions: 10
  stale_after_ms: 600000
  merge_policy: human

steps:
  - id: rebase
    kind: session.create
    role: gatekeeper
    branch: "{{ inputs.branch }}"
    prompt: |
      Ensure branch is up to date with {{ inputs.target_branch | default('main') }}.
      Rebase if needed. Record handoff with status: completed when done.
    next: wait_for_rebase

  - id: wait_for_rebase
    kind: handoff.wait
    from: gatekeeper
    on:
      completed: create_reviewer
      needs_human: needs_human
      blocked: blocked
      failed: failed

  - id: create_reviewer
    kind: session.create
    role: reviewer
    prompt: |
      Review the diff between {{ inputs.target_branch | default('main') }} and {{ inputs.branch }}.
      Round: {{ runtime.round }}
      {% if runtime.last_error %}Previous fixes applied: {{ runtime.last_error }}{% endif %}

  - id: wait_for_review
    kind: handoff.wait
    from: reviewer
    on:
      completed: run_tests
      needs_human: review_rejected
      blocked: blocked
      failed: failed

  - id: review_rejected
    kind: session.create
    role: gatekeeper
    branch: "{{ inputs.branch }}"
    prompt: |
      The reviewer found issues: {{ runtime.last_error }}
      Fix auto-fix findings. Escalate ask-user findings in your handoff.
    next: wait_for_review_fix

  - id: wait_for_review_fix
    kind: handoff.wait
    from: gatekeeper
    on:
      completed: increment_round_after_review
      blocked: blocked
      needs_human: needs_human
      failed: failed

  - id: increment_round_after_review
    kind: round.next
    next: create_reviewer

  - id: run_tests
    kind: gates.run
    role: gatekeeper
    gates:
      - name: test
        command: "{{ inputs.gate_command | default('make ci') }}"
    on:
      pass: run_document_check
      fail: tests_failed
      error: tests_failed

  - id: tests_failed
    kind: session.create
    role: gatekeeper
    branch: "{{ inputs.branch }}"
    prompt: |
      Tests failed: {{ runtime.last_error }}
      Fix the root cause. Do NOT skip tests.
    next: wait_for_test_fix

  - id: wait_for_test_fix
    kind: handoff.wait
    from: gatekeeper
    on:
      completed: increment_round_after_tests
      blocked: blocked
      needs_human: needs_human
      failed: failed

  - id: increment_round_after_tests
    kind: round.next
    next: create_reviewer

  - id: run_document_check
    kind: session.create
    role: gatekeeper
    branch: "{{ inputs.branch }}"
    prompt: |
      Check if documentation needs updating for the changes on this branch.
      Update only authoritative owner documents. Do not create new doc surfaces.
    next: wait_for_document

  - id: wait_for_document
    kind: handoff.wait
    from: gatekeeper
    on:
      completed: run_lint
      blocked: blocked
      needs_human: needs_human
      failed: failed

  - id: run_lint
    kind: gates.run
    role: gatekeeper
    gates:
      - name: lint
        command: "{{ inputs.lint_command | default('true') }}"
    on:
      pass: push_and_pr
      fail: lint_failed
      error: lint_failed

  - id: lint_failed
    kind: session.create
    role: gatekeeper
    branch: "{{ inputs.branch }}"
    prompt: |
      Lint failed: {{ runtime.last_error }}
      Fix lint errors. Do not suppress warnings.
    next: wait_for_lint_fix

  - id: wait_for_lint_fix
    kind: handoff.wait
    from: gatekeeper
    on:
      completed: increment_round_after_lint
      blocked: blocked
      needs_human: needs_human
      failed: failed

  - id: increment_round_after_lint
    kind: round.next
    next: create_reviewer

  - id: push_and_pr
    kind: session.create
    role: gatekeeper
    branch: "{{ inputs.branch }}"
    prompt: |
      All checks passed. Push the branch and open a draft PR against {{ inputs.target_branch | default('main') }}.
    next: wait_for_push

  - id: wait_for_push
    kind: handoff.wait
    from: gatekeeper
    on:
      completed: completed_unreviewed
      blocked: blocked
      needs_human: needs_human
      failed: failed

  - id: completed_unreviewed
    kind: loop.status
    status: approved

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
