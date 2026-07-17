---
title: Writing Your First Loop
description: Create a minimal loop recipe, run it, and understand how agents coordinate through handoffs.
---

:::caution[Experimental]
Loops are under active development. Behavior, recipe schema, and CLI commands may change between releases.
:::

By the end of this tutorial you'll have written a loop recipe from scratch and watched two AI agents coordinate through it. We'll keep it minimal — no gates, no retries — to focus on the core mechanics.

## Prerequisites

- planeai installed and running ([Getting Started](/planeai/tutorials/first-session/))
- At least one provider configured (e.g., Claude, Kiro, Copilot)
- A registered project in planeai
- **Session backend set to tmux or daemon** — loops require persistent sessions that survive across steps. The local backend won't work because sessions must stay alive while the loop runner waits for handoffs.

:::tip
If you haven't set up a project yet, follow the [Your First Session](/planeai/tutorials/first-session/) tutorial first.
:::

## Step 1: Create the recipe file

In your project root, create the file `.planeai/loops/hello-world.yaml`:

```yaml
schema: planeai.loop.recipe.v1
id: hello-world
name: Hello World
description: Two agents greet each other through a handoff.

trigger:
  kind: manual

inputs:
  greeting:
    type: text
    label: Greeting message
    required: true
    default: "Hello, I'm agent A!"

knowledge:
  files: []
  instructions: []

tools:
  required:
    - git
  optional: []

roles:
  greeter:
    provider: default
    mode: write
    isolation: worktree
    instructions: |
      You are the greeter. Your only job is to say the greeting message,
      then record a handoff with status: completed.
  responder:
    provider: default
    mode: write
    isolation: worktree
    instructions: |
      You are the responder. Read the greeter's handoff message
      and respond with a friendly reply, then record a handoff with status: completed.

policy:
  max_rounds: 1
  max_ticks: 15
  max_sessions: 2
  merge_policy: human

steps:
  - id: create_greeter
    kind: session.create
    role: greeter
    prompt: |
      Say this greeting "{{ inputs.greeting }}" to your fellow ai agent by recording a handoff with status: completed.
      Handoff protocol:
      1. Session id: $PLANEAI_SESSION_ID
      2. Path: planeai-cli axi loop handoff path --loop {{ loop_run.id }} --session "$PLANEAI_SESSION_ID"
      3. Write JSON with schema planeai.handoff.v1
      4. Record: planeai-cli axi loop handoff record --loop {{ loop_run.id }} --session "$PLANEAI_SESSION_ID" --path <handoff-path>

  - id: wait_greeter
    kind: handoff.wait
    from: greeter
    on:
      completed: create_responder
      blocked: done_blocked
      needs_human: done_blocked
      failed: done_blocked

  - id: create_responder
    kind: session.create
    role: responder
    prompt: |
      The greeter recorded a handoff for you. Read its content to see what they said,
      then respond with a friendly reply and record your own handoff with status: completed.

      Handoff protocol:
      1. Session id: $PLANEAI_SESSION_ID
      2. Path: planeai-cli axi loop handoff path --loop {{ loop_run.id }} --session "$PLANEAI_SESSION_ID"
      3. Write JSON with schema planeai.handoff.v1
      4. Record: planeai-cli axi loop handoff record --loop {{ loop_run.id }} --session "$PLANEAI_SESSION_ID" --path <handoff-path>

  - id: wait_responder
    kind: handoff.wait
    from: responder
    on:
      completed: done
      blocked: done_blocked
      needs_human: done_blocked
      failed: done_blocked

  - id: done
    kind: loop.status
    status: completed_unreviewed

  - id: done_blocked
    kind: loop.status
    status: blocked
```

Let's break this down:

- **`trigger: manual`** — the loop only runs when you explicitly start it
- **`inputs`** — one text input for the greeting message, with a default value
- **`roles`** — two roles (greeter and responder), each getting their own worktree
- **`policy`** — limits to prevent runaway execution (1 round, 15 ticks, 2 sessions)
- **`steps`** — the state machine: create a session, wait for its handoff, create the next session, wait again, then mark done

## Step 2: Validate the recipe

Run the recipe validator to catch any schema errors before launching:

```bash
planeai-cli axi loop recipe validate .planeai/loops/hello-world.yaml
```

You should see output confirming the recipe is valid:

```
✓ hello-world: valid
```

If you see errors, double-check your YAML indentation and field names against the recipe above.

## Step 3: Verify it's discovered

planeai scans your project's `.planeai/loops/` directory for recipes. Confirm discovery:

```bash
planeai-cli axi loop recipe ls
```

You should see `hello-world` listed with `source: project`:

```
ID            Name          Source    Trigger
hello-world   Hello World   project   manual
```

## Step 4: Launch the loop

1. Press `Cmd+N` (`Ctrl+N` on Linux/Windows) then `L` to open the loop form
2. Select your project
3. Pick the **Hello World** recipe from the dropdown
4. Fill in the **Greeting message** field — e.g. "Hello, I'm agent A!"
5. Click **Start loop** (`Cmd+Enter`)

Here's what happens behind the scenes:

1. planeai creates a new loop run from the `hello-world` recipe
2. Auto-advance executes the first step (`create_greeter`) — spinning up an agent session with the greeter role
3. The greeter receives the prompt with your greeting message
4. The runner parks at `wait_greeter`, observing the greeter's session for a handoff

You'll see the loop appear in the **Loop Runs** panel in the sidebar.

## Step 5: Watch it run

Click on the loop in the **Loop Runs** sidebar panel to open the loop dashboard. You'll see the steps advance in real time.

The greeter agent says the greeting and records a handoff with status `completed`. When that handoff lands, auto-advance kicks in:

1. `wait_greeter` sees `completed` → transitions to `create_responder`
2. A new agent session spins up with the responder role
3. The responder reads the greeting, replies, and records its own handoff
4. `wait_responder` sees `completed` → transitions to `done`
5. The loop reaches `completed_unreviewed`

The dashboard shows the current step, round number, and final status. You can also click into each session from the loop panel to see the agent's terminal output.

:::note
The whole loop typically completes in under a minute, depending on your provider's response time.
:::

## What just happened

Your recipe defined two roles and five steps. The loop runner executed steps sequentially, parking at each `handoff.wait` until the corresponding agent delivered. When an agent recorded a handoff, the runner matched the status against the `on` map and advanced to the next step.

**Handoffs are the coordination primitive.** Agents don't talk to each other directly — they communicate through structured handoffs that the loop runner observes and routes.

## Next steps

Now that you understand the mechanics, explore:

- The [Loops guide](/planeai/guides/loops/) for the full maker-verifier strategy with gates, rounds, and retries
- The [Loops Reference](/planeai/reference/loops/) for the complete recipe schema and all available step kinds
