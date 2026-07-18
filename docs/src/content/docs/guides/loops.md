---
title: Loops (Experimental)
description: Multi-agent workflows with declarative recipes — how loops orchestrate AI agents through structured handoffs.
draft: false
---

:::caution[Experimental]
Loops are under active development. Behavior, recipe schema, and CLI commands may change between releases.
:::

Loops let you orchestrate multiple AI agents in a structured workflow. Instead of manually prompting agents and copying context between them, you define a recipe — a YAML declaration of roles, steps, and policies — and let planeai run it. The system handles worktree isolation, handoff detection, gate verification, and retry logic automatically.

:::note[Backend requirement]
Loops require the **tmux** or **daemon** session backend. The local backend won't work — sessions must persist while the loop runner waits for handoffs between steps. See [Configuration](/planeai/guides/configuration/) to set your backend.
:::

## Quick Start

1. Press `Cmd+N` (`Ctrl+N` on Linux/Windows) then `L` to open the loop form
2. Select your project and pick the **Maker + Verifier** recipe
3. Fill in the **Goal** — e.g. "Add pagination to /users"
4. Leave max rounds at 3 and click **Start loop** (`Cmd+Enter`)

That's it. planeai will:

1. Spawn a **maker** session in an isolated git worktree to implement the goal
2. Run automated **gates** (tests, lint) when the maker signals completion
3. Spawn a **verifier** session that reviews the diff in readonly mode
4. If the verifier rejects, the maker retries with the feedback — up to your max rounds

The loop continues until the verifier approves or the round limit is reached. You can monitor progress in the **Loop Runs** panel in the sidebar.

## How Loops Work

Loops follow a **tick model**: the recipe defines an ordered list of steps, and the runner executes one step per tick. When a step completes and the next step is immediately executable (no waiting required), the runner auto-advances without pausing.

### Key Concepts

- **Recipe** — a YAML file declaring the full workflow: roles, steps, gates, and policy
- **Roles** — named agents with a provider, mode (implement/review), and isolation level
- **Steps** — ordered actions the runner executes (prompt, wait for handoff, run gates)
- **Handoffs** — structured signals agents emit when they finish their current step
- **Rounds** — retry iterations when a verifier rejects work
- **Gates** — automated checks (tests, lint, build) that run between steps
- **Policy** — safety bounds: max rounds, timeouts, failure behavior

```
┌──────────┐     ┌──────────┐     ┌──────────┐
│  Recipe  │────▶│  Runner  │────▶│  Steps   │
└──────────┘     └──────────┘     └──────────┘
                      │                 │
                      ▼                 ▼
                 ┌──────────┐     ┌──────────┐
                 │  Roles   │     │  Gates   │
                 └──────────┘     └──────────┘
```

The runner persists state between ticks — if planeai restarts, loops resume from their last completed step.

## The Maker-Verifier Strategy

The builtin `maker-verifier` recipe implements a two-agent feedback loop modeled on code review:

1. The **maker** receives the goal and implements it in an isolated worktree
2. On completion, **gates** run automatically (CI checks, test suite)
3. If gates pass, the **verifier** reviews the diff in readonly mode
4. If the verifier approves → loop completes
5. If the verifier rejects → round increments, maker receives feedback, retries from step 1

The terminal state is `completed_unreviewed` or `approved` — in both cases, a human decides whether to merge.

### State Machine

```
                         ┌─────────────────────────────────────┐
                         │                                     │
                         ▼                                     │
┌─────────┐    ┌────────────────┐    ┌────────────┐    ┌──────┴──────┐
│  start  │───▶│   observing    │───▶│   gates    │───▶│  verifying  │
└─────────┘    └────────────────┘    └────────────┘    └─────────────┘
                       ▲                    │                   │
                       │                    │                   │
                       │              ┌─────▼─────┐      ┌─────▼─────┐
                       │              │  blocked  │      │ approved  │
                       │              └───────────┘      └───────────┘
                       │                                       │
                       │    ┌────────────────────────┐         │
                       └────│  rejection (new round) │◀────────┘
                            └────────────────────────┘

              ┌───────────────────────┐    ┌───────────┐
              │ completed_unreviewed  │    │  failed   │
              └───────────────────────┘    └───────────┘
              (max rounds reached)          (unrecoverable)
```

The maker works in a dedicated worktree branched from `main`. The verifier sees only the diff — it cannot modify code. This separation ensures the feedback loop converges rather than producing conflicting edits.

## Loop States

| State                  | Meaning                                                                |
| ---------------------- | ---------------------------------------------------------------------- |
| `draft`                | Loop created but not yet started                                       |
| `running`              | Actively executing steps (session creation, prompts)                   |
| `observing`            | Waiting for an agent to produce a handoff                              |
| `verifying`            | Running gate commands (tests, lint)                                    |
| `stale`                | No agent activity detected within `stale_after_ms`; needs intervention |
| `completed_unreviewed` | Loop completed successfully; human must review and merge               |
| `approved`             | Human approved the work; ready to merge                                |
| `blocked`              | Max rounds reached or agent declared non-completable                   |
| `needs_human`          | Agent explicitly requested human input                                 |
| `failed`               | Unrecoverable error; loop cannot continue                              |
| `cancelled`            | Manually cancelled by user                                             |
| `merged`               | Work was merged into the target branch                                 |
| `cleaned`              | Worktrees and branches cleaned up after merge or cancellation          |

## Handoffs

Agents communicate completion by writing a structured handoff signal. The runner watches each session for this signal via a `handoff.wait` step.

The handoff follows the `planeai.handoff.v1` schema:

```yaml
schema: planeai.handoff.v1
status: completed # completed | blocked | needs_human | failed
summary: "Added pagination with cursor-based approach"
files_changed: 4
```

When the runner detects a handoff, it reads the status and routes to the next step:

- `completed` → advance to gates or verifier
- `blocked` → transition to `blocked` state
- `needs_human` → pause and notify
- `failed` → transition to `failed` state

Agents don't need to know about this schema — planeai injects the handoff instructions into the agent's prompt automatically.

## Gates

Gates are automated verification steps that run between the maker and verifier. They catch obvious failures before a verifier spends time reviewing.

A `gates.run` step executes the configured checks (typically `make ci` or a custom command) and routes based on the result:

- **Pass** → advance to verifier
- **Fail** → transition to `blocked` or trigger a retry round with gate output as feedback
- **Error** → transition to `failed` (infrastructure problem)

Gate output is stored and included in the retry prompt, so the maker knows exactly what broke.

```yaml
steps:
  - type: gates.run
    commands:
      - make ci
    on_fail: retry # retry | block | fail
```

## Recipe Locations

Recipes are loaded from three locations, checked in order (first match wins):

1. **Project** — `.planeai/loops/*.yaml` in the repository root
2. **User** — `~/.config/planeai/loops/*.yaml`
3. **Builtin** — shipped with planeai (e.g., `maker-verifier`)

Place project-specific recipes in `.planeai/loops/` and commit them with the repo. Use the user directory for personal recipes shared across projects.

## Monitoring Loops

The sidebar **Loop Runs** panel shows all active and recent loops. Click a loop to open the **Loop Dashboard** — a detailed view showing:

- Current step and state
- Round number and elapsed time
- Verifier results and feedback history
- Gate output per round
- Controls to stop or retry

From the dashboard you can also click into individual agent sessions to see their terminal output.

## Next Steps

- [Writing Your First Loop](/planeai/tutorials/first-loop/) — tutorial: build a custom recipe from scratch
- [Loops Reference](/planeai/reference/loops/) — full YAML schema, all step types, policy options
