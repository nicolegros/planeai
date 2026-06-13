---
name: implement-from-task
description: Pick up a task by key, move it to in_progress, read project documentation (agents.md, readme.md, context.md), and implement it. Use when user provides a task key (e.g., "PLA-1", "NOM-3") and wants to start working on it, says "pick up task", "work on", or "implement" a task.
---

## Steps

1. **Read the task**: Run `planeai-cli task show <key>` to get the task details.
2. **Move to in progress**: Run `planeai-cli task move <key> in_progress`.
3. **Read project documentation**: Read `AGENTS.md`, `README.md`, and `CONTEXT.md` in the repository root to understand conventions, architecture, and constraints.
4. **Implement**: Complete the work described in the task, following the project's documented guidelines (/tdd when possible).
5. **Validate**: Make sure the acceptance criteria are met and tests pass. If there are issues, iterate on the implementation until they are resolved.
6. **Signal completion**: When done, inform the user the task is ready for review. Do NOT move the task to another status without user confirmation.
