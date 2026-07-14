# ADR-0010: Loop status derivation from step pointer

**Status:** Accepted  
**Date:** 2026-07-14  
**Context:** PLA-232 — eliminate loop status desync between recipe step pointer and status column

## Problem

Recipe-driven loops had two independent authorities for status: the `status` column (set by explicit `LoopTrigger` transitions) and the recipe step pointer (`snapshot.runtime.current_step`). Because these were updated at different points — and by different code paths — they could diverge, leaving the loop in an inconsistent state (e.g., status says "running" but the step pointer sits at `handoff.wait`).

## Decision

**Derive `LoopStatus` from the step pointer at write time, not from trigger-driven transitions.**

1. `persist_snapshot` is the single choke point for snapshot persistence. It serializes the snapshot, derives status from the current step kind (via `derive_status_from_step`), and writes both atomically.
2. `status_override` (a field on `RecipeRuntime`) wins over step-kind derivation for cases where the blocking condition is contextual, not inherent to the step kind (e.g., `round.next` when max_rounds is reached → Blocked).
3. Lifecycle transitions (`Start`, `Cancel`, `Approve`, `HandoffReceived`) still use `transition_loop` since they operate outside the recipe tick and represent external user/system events.
4. Recipe-tick triggers (†) — `HandoffWaiting`, `HandoffConsumed`, `GatesStarted`, `GatesCompleted`, `RoundBlocked`, `SessionLimitReached`, `MaxTicksExceeded`, `HumanWaitReached` — are rejected at runtime if fired against recipe-driven loops. They exist only for the transition table's reference semantics and non-recipe loops.
5. `current_round` column is deprecated (kept for schema compat). Round state lives exclusively in `snapshot.runtime.round`.

## Consequences

- **Status can never desync from step pointer** for recipe-driven loops — they share a single atomic write.
- **Fewer code paths** modify status: `persist_snapshot` for recipe ticks, `transition_loop` for lifecycle events.
- **New step kinds** must be registered in `derive_status_from_step` or the loop is marked `Stale` to surface the gap.
- **Frontend** no longer shows `current_round/max_rounds` since the column is deprecated — it shows only the max_rounds limit.
- **Non-recipe loops** are unaffected; they continue using the trigger-based transition model.
