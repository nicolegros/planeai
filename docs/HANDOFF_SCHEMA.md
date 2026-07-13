# Structured Handoff Schema (`planeai.handoff.v1`)

Structured handoff files allow agents to report loop completion, blockers, risks,
tests, and evidence in a machine-readable format. This replaces relying on
terminal output like "done" for loop state transitions.

## Overview

A handoff is a JSON document that an agent writes to disk and then records via
the AXI CLI. The system validates the document, stores it as a loop artifact,
appends a loop event, and optionally transitions loop/session state.

### Push Model

Handoffs use a **push model**: the loop orchestrator does not poll for handoff
files. The agent must explicitly record the handoff via:

```bash
planeai-cli axi loop handoff record --loop <LOOP_ID> --session <SESSION_ID> --path <PATH>
```

The `handoff path` helper command prints the expected file path and creates the
directory structure:

```bash
planeai-cli axi loop handoff path --loop <LOOP_ID> --session <SESSION_ID>
```

## Schema (v1)

```json
{
  "schema": "planeai.handoff.v1",
  "loop_id": "loop_...",
  "session_id": "uuid",
  "task_key": "PLA-201",
  "status": "completed",
  "branch": "pla-201/shortid",
  "commit": "abc123",
  "changed_files": ["path/to/file.rs"],
  "summary": "What changed",
  "risks": ["Known risk"],
  "next_actions": ["Suggested next step"],
  "evidence": [
    {
      "kind": "test",
      "name": "cargo test -p planeai-core",
      "result": "pass",
      "source": "direct",
      "output_path": ".planeai/loops/loop_.../verifier/cargo-test.log"
    }
  ]
}
```

### Required Fields

| Field        | Type   | Description                                  |
| ------------ | ------ | -------------------------------------------- |
| `schema`     | string | Must be `"planeai.handoff.v1"`               |
| `loop_id`    | string | ID of the loop this handoff belongs to       |
| `session_id` | string | ID of the session that produced this handoff |
| `status`     | string | Outcome status (see below)                   |
| `summary`    | string | Human-readable summary of what was done      |

### Optional Fields

| Field           | Type     | Description                                    |
| --------------- | -------- | ---------------------------------------------- |
| `task_key`      | string   | Task key for cross-referencing (e.g., PLA-201) |
| `branch`        | string   | Git branch name                                |
| `commit`        | string   | Git commit hash                                |
| `changed_files` | string[] | Files modified in this work                    |
| `risks`         | string[] | Known risks or concerns                        |
| `next_actions`  | string[] | Suggested next actions                         |
| `evidence`      | object[] | Test/build/lint evidence (see below)           |

## Status Values

| Status        | Meaning                                           | Loop Transition (if active)       |
| ------------- | ------------------------------------------------- | --------------------------------- |
| `completed`   | Work is done, ready for verification              | → observing (+ auto-tick advance) |
| `blocked`     | Cannot proceed without external input             | → blocked                         |
| `needs_human` | Requires human decision or review                 | → needs_human                     |
| `failed`      | Work failed (e.g., tests don't pass, infra broke) | → failed                          |

"Active" means the loop is currently in `running`, `observing`, `verifying`, `needs_human`, `blocked`, or `stale` status.

When a `completed` handoff is recorded on a non-terminal loop, the system automatically triggers auto-advance (up to 10 ticks) so the recipe progresses through `handoff.wait` → subsequent steps without requiring a manual tick. Auto-advance stops at `human.wait`, terminal, intervention-required states, or when the current step does not advance (waiting for external input).

For loops in `draft` or terminal states (`completed_unreviewed`, `cancelled`, `approved`,
`merged`, `cleaned`), the handoff artifact and event are recorded but no loop status
transition occurs.

## Evidence

Evidence records what the agent observed or ran as proof of work.

```json
{
  "kind": "test",
  "name": "cargo test -p planeai-core",
  "result": "pass",
  "source": "direct",
  "output_path": ".planeai/loops/loop_.../verifier/cargo-test.log"
}
```

### Evidence Fields

| Field         | Required | Description                                       |
| ------------- | -------- | ------------------------------------------------- |
| `kind`        | yes      | Category (freeform, see recommended values below) |
| `name`        | yes      | Human-readable name or command                    |
| `result`      | yes      | Outcome (freeform, see recommended values below)  |
| `source`      | yes      | How evidence was obtained (typed enum, see below) |
| `output_path` | no       | Path to detailed output log                       |

**Recommended `kind` values:** `test`, `lint`, `build`, `typecheck`

Other values are preserved as-is — tools may emit arbitrary kinds (e.g., `security_scan`, `benchmark`).

**Recommended `result` values:** `pass`, `fail`, `error`, `skip`

Other values are preserved as-is.

### Evidence Source Semantics

The `source` field indicates the **trust level** of the evidence. Verifiers
should not treat `claimed` evidence as proof — it signals the agent asserts a
result without direct observation.

| Source    | Meaning                                                      | Trust Level |
| --------- | ------------------------------------------------------------ | ----------- |
| `direct`  | Agent directly ran the command and observed the result       | High        |
| `proxy`   | Another tool or agent reported the result (e.g., CI webhook) | Medium      |
| `claimed` | Agent asserts the result without direct observation          | Low         |
| `blocked` | Evidence could not be obtained (e.g., test infra was down)   | None        |

**Design principle**: the system preserves source labels as-is. A verifier session
can later re-run `claimed` evidence items with `source: "direct"` to upgrade trust.

## File Location Convention

The `handoff path` command resolves the expected path:

```
<worktree or project root>/.planeai/loops/<loop_id>/sessions/<session_id>/handoff.json
```

Agents should always use `planeai-cli axi loop handoff path` to discover the
path rather than hardcoding it.

## Security

- The `--path` argument to `handoff record` is canonicalized and validated.
- Paths outside the session's worktree or project root are rejected.
- This prevents agents from asking PlaneAI to ingest arbitrary local files.

## AXI Command Reference

### `planeai-cli axi loop handoff path`

Prints the expected handoff file location and next steps.

```bash
planeai-cli axi loop handoff path --loop <LOOP_ID> --session <SESSION_ID>
```

Output (TOON format):

```
handoff_path:
  loop_id: loop_01...
  session_id: 4f3a91c2-...
  role: maker
  path: /repo/.planeai/loops/loop_01.../sessions/4f3a91c2/handoff.json
  exists: false
next_actions[2]:
  - write a planeai.handoff.v1 JSON file to path
  - run `planeai-cli axi loop handoff record --loop loop_01... --session 4f3a91c2 --path <path>`
```

### `planeai-cli axi loop handoff record`

Records a handoff file as a loop artifact and triggers state transitions.

```bash
planeai-cli axi loop handoff record --loop <LOOP_ID> --session <SESSION_ID> --path <PATH>
```

Output on success:

```
handoff_recorded:
  loop_id: loop_01...
  session_id: 4f3a91c2-...
  artifact_id: 9c1b...
  event_id: 42
  schema: planeai.handoff.v1
  status: completed
  loop_status: observing
  session_status: completed
  state_changed: true
  path: /repo/.planeai/loops/loop_01.../sessions/4f3a91c2/handoff.json
next_actions[1]:
  - run verifier gates or `planeai-cli axi loop tick loop_01...`
```

Output on validation error:

```
error: invalid handoff file
path: /repo/.planeai/loops/loop_01.../sessions/4f3a91c2/handoff.json
details[2]:
  - missing required field: summary
  - schema must be planeai.handoff.v1
help[1]:
  - run `planeai-cli axi loop handoff path --loop <id> --session <id>` for the expected location
```

## Scope & Non-Goals (v1)

- No polling or background scheduler (handoffs are push-only, but recording a completed handoff triggers auto-advance through immediately-executable recipe steps)
- No retry policy
- No auto-merge
- No session kill/stop
- Agents are not required to produce a handoff yet (future enforcement)

## Duplicate Policy

Repeated `handoff record` for the same loop/session/path creates a new artifact
and event each time. The event ledger is append-only — latest event wins for
observation. This is intentional: an agent may re-record after fixing issues.

## Validation Rules

Required string fields (`loop_id`, `session_id`, `summary`) must be non-empty
and non-whitespace. A blank summary is rejected the same way a missing one is.

Evidence `kind` and `result` are freeform strings (not validated against a fixed
set) to allow extensibility. The `source` field is the only evidence field
validated against a typed enum.
