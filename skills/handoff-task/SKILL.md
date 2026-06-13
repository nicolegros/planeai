---
name: handoff-task
description: Compact the current conversation into a handoff document for another agent to pick up as a task.
argument-hint: "What will the next session be used for?"
---

Write a handoff document summarising the current conversation so a fresh agent can continue the work. Save it as a task via `planeai-cli task add`.

Spare no detail in the description, the next agent will rely on it to understand the context and pick up the work. Include any relevant information, decisions made, and next steps.

Assume the next agent is weaker, so be explicit and thorough in your explanations (What? Why? How?).

Suggest the skills to be used, if any, by the next session.

Do not duplicate content already captured in other artifacts (PRDs, plans, ADRs, issues, commits, diffs). Reference them by path or URL instead.

If the user passed arguments, treat them as a description of what the next session will focus on and tailor the doc accordingly.

## CLI Reference

```
planeai-cli task add "Title" [flags]

Flags:
  --desc <string>         Task description
  --priority <int>        Priority (1 = highest, 0 = default)
  --tags <a,b,c>          Comma-separated tags
  --blocked-by <K1,K2>    Comma-separated blocker keys
  --parent <KEY>          Parent task key
  --project <name>        Target project (default: resolve from CWD)
  --pretty                Human-readable output

planeai-cli task show <key>
planeai-cli task edit <key> [--title ...] [--desc ...] [--priority ...] [--tags ...] [--blocked-by ...]
```
