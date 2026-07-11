//! Durable loop data model — domain types for loop runs, sessions, events,
//! artifacts, and verifier runs.
//!
//! Also contains the **transition table** — the declared state machine for loop
//! status transitions. See [`apply`] for the pure transition function and
//! [`LoopTrigger`] for the event vocabulary.

use serde::{Deserialize, Serialize};

use crate::handoff::HandoffStatus;

// ─── Loop Status ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopStatus {
    Draft,
    Running,
    Observing,
    Verifying,
    CompletedUnreviewed,
    Blocked,
    NeedsHuman,
    Stale,
    Failed,
    Cancelled,
    Approved,
    Merged,
    Cleaned,
}

impl LoopStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Running => "running",
            Self::Observing => "observing",
            Self::Verifying => "verifying",
            Self::CompletedUnreviewed => "completed_unreviewed",
            Self::Blocked => "blocked",
            Self::NeedsHuman => "needs_human",
            Self::Stale => "stale",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Approved => "approved",
            Self::Merged => "merged",
            Self::Cleaned => "cleaned",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "running" => Some(Self::Running),
            "observing" => Some(Self::Observing),
            "verifying" => Some(Self::Verifying),
            "completed_unreviewed" => Some(Self::CompletedUnreviewed),
            "blocked" => Some(Self::Blocked),
            "needs_human" => Some(Self::NeedsHuman),
            "stale" => Some(Self::Stale),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            "approved" => Some(Self::Approved),
            "merged" => Some(Self::Merged),
            "cleaned" => Some(Self::Cleaned),
            _ => None,
        }
    }

    /// The executor has finished producing a reviewable result (or failed/was cancelled).
    /// These are the statuses that set `executor_finished_at`.
    pub fn is_executor_terminal(&self) -> bool {
        matches!(
            self,
            Self::CompletedUnreviewed | Self::Failed | Self::Cancelled
        )
    }

    /// The loop is paused and requires human or external intervention to proceed.
    /// Do not use `executor_finished_at IS NULL` alone to detect active loops —
    /// these statuses are not terminal but the executor is not actively running.
    pub fn is_intervention_required(&self) -> bool {
        matches!(self, Self::Blocked | Self::NeedsHuman | Self::Stale)
    }

    /// The loop has completed its full lifecycle (approved, merged, cleaned).
    /// These are past executor-terminal — no further transitions are valid
    /// except forward through the lifecycle chain.
    pub fn is_lifecycle_terminal(&self) -> bool {
        matches!(self, Self::Approved | Self::Merged | Self::Cleaned)
    }
}

// ─── Loop Strategy ───────────────────────────────────────────────────────────

/// Freeform strategy identifier. Semantics are defined by the future executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopStrategy(pub String);

impl LoopStrategy {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ─── Loop Run ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRun {
    pub id: String,
    pub project_id: String,
    pub task_key: Option<String>,
    pub created_by_session_id: Option<String>,
    pub strategy: LoopStrategy,
    pub goal: String,
    pub status: LoopStatus,
    pub max_rounds: i64,
    pub created_at: String,
    pub updated_at: String,
    pub executor_finished_at: Option<String>,
    pub policy_json: Option<serde_json::Value>,
    pub budget_json: Option<serde_json::Value>,
}

// ─── Loop Session ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopSession {
    pub loop_id: String,
    pub session_id: String,
    pub role: String,
    pub round: i64,
    pub provider: Option<String>,
    pub status: String,
    pub created_at: String,
}

// ─── Loop Event ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopEvent {
    pub id: i64,
    pub loop_id: String,
    pub ts: String,
    pub kind: String,
    pub payload_json: serde_json::Value,
}

// ─── Loop Artifact ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopArtifact {
    pub id: String,
    pub loop_id: String,
    pub session_id: Option<String>,
    pub kind: String,
    pub path: Option<String>,
    pub content_json: Option<serde_json::Value>,
    pub created_at: String,
}

// ─── Verifier Run ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierRun {
    pub id: String,
    pub loop_id: String,
    pub session_id: Option<String>,
    pub verifier_type: String,
    pub name: String,
    pub command: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub output_path: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

// ─── Transition Table ────────────────────────────────────────────────────────

/// Events that trigger loop status transitions. Callers declare what happened;
/// the transition table ([`apply`]) decides the resulting state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum LoopTrigger {
    /// Draft → Running (user starts the loop)
    Start,
    /// Any non-terminal → Cancelled
    Cancel,
    /// Running → Observing (handoff.wait step, no handoff found yet)
    HandoffWaiting,
    /// Observing → Running (handoff.wait step found an existing handoff)
    HandoffConsumed,
    /// Active → Observing|Blocked|NeedsHuman|Failed (external handoff record)
    HandoffReceived(HandoffStatus),
    /// Running → Verifying (gates.run step started)
    GatesStarted,
    /// Verifying → Running (gates.run step completed)
    GatesCompleted,
    /// Running → Blocked (max_rounds reached)
    RoundBlocked,
    /// Running → NeedsHuman (max_sessions reached)
    SessionLimitReached,
    /// Running → Failed (max_ticks exceeded)
    MaxTicksExceeded,
    /// Running → NeedsHuman (human.wait step)
    HumanWaitReached,
    /// Running → {allow-listed targets} (recipe loop.status step)
    RecipeSetStatus(LoopStatus),
    /// CompletedUnreviewed → Approved (human approves)
    Approve,
    /// Approved → Merged (PR merged)
    MarkMerged,
    /// Merged → Cleaned (worktree cleaned up)
    MarkCleaned,
}

impl LoopTrigger {
    /// Short string name for Display impls and error messages.
    /// For structured logging/audit, use serde serialization which preserves payloads.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Cancel => "Cancel",
            Self::HandoffWaiting => "HandoffWaiting",
            Self::HandoffConsumed => "HandoffConsumed",
            Self::HandoffReceived(_) => "HandoffReceived",
            Self::GatesStarted => "GatesStarted",
            Self::GatesCompleted => "GatesCompleted",
            Self::RoundBlocked => "RoundBlocked",
            Self::SessionLimitReached => "SessionLimitReached",
            Self::MaxTicksExceeded => "MaxTicksExceeded",
            Self::HumanWaitReached => "HumanWaitReached",
            Self::RecipeSetStatus(_) => "RecipeSetStatus",
            Self::Approve => "Approve",
            Self::MarkMerged => "MarkMerged",
            Self::MarkCleaned => "MarkCleaned",
        }
    }
}

/// Outcome of applying a trigger to a status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionResult {
    /// Status changed to the contained value.
    Changed(LoopStatus),
    /// Trigger was valid but produced no state change (from == to).
    Unchanged,
}

/// Error returned when a trigger is not valid from the current status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: LoopStatus,
    pub trigger: LoopTrigger,
}

impl std::fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid transition: cannot apply {:?} from status '{}'",
            self.trigger,
            self.from.as_str()
        )
    }
}

impl std::error::Error for InvalidTransition {}

/// Pure transition function — the declared state machine for loop status.
///
/// Given the current status and a trigger event, returns the new status
/// (`Changed`), confirms a no-op (`Unchanged`), or rejects the transition
/// (`InvalidTransition`).
pub fn apply(
    from: &LoopStatus,
    trigger: &LoopTrigger,
) -> Result<TransitionResult, InvalidTransition> {
    let reject = || {
        Err(InvalidTransition {
            from: from.clone(),
            trigger: trigger.clone(),
        })
    };

    match trigger {
        LoopTrigger::Start => match from {
            LoopStatus::Draft => Ok(TransitionResult::Changed(LoopStatus::Running)),
            _ => reject(),
        },

        LoopTrigger::Cancel => {
            // Allowed from any non-terminal state (executor-terminal or lifecycle-terminal)
            if from.is_executor_terminal() || from.is_lifecycle_terminal() {
                reject()
            } else {
                Ok(TransitionResult::Changed(LoopStatus::Cancelled))
            }
        }

        LoopTrigger::HandoffWaiting => match from {
            LoopStatus::Running => Ok(TransitionResult::Changed(LoopStatus::Observing)),
            LoopStatus::Observing => Ok(TransitionResult::Unchanged),
            _ => reject(),
        },

        LoopTrigger::HandoffConsumed => match from {
            LoopStatus::Observing => Ok(TransitionResult::Changed(LoopStatus::Running)),
            LoopStatus::Running => Ok(TransitionResult::Unchanged),
            _ => reject(),
        },

        LoopTrigger::HandoffReceived(handoff_status) => {
            // Valid from any "active" state (not terminal, not draft)
            let is_active = matches!(
                from,
                LoopStatus::Running
                    | LoopStatus::Observing
                    | LoopStatus::Verifying
                    | LoopStatus::NeedsHuman
                    | LoopStatus::Blocked
                    | LoopStatus::Stale
            );
            if !is_active {
                return reject();
            }
            let target = match handoff_status {
                HandoffStatus::Completed => LoopStatus::Observing,
                HandoffStatus::Blocked => LoopStatus::Blocked,
                HandoffStatus::NeedsHuman => LoopStatus::NeedsHuman,
                HandoffStatus::Failed => LoopStatus::Failed,
            };
            if &target == from {
                Ok(TransitionResult::Unchanged)
            } else {
                Ok(TransitionResult::Changed(target))
            }
        }

        LoopTrigger::GatesStarted => match from {
            LoopStatus::Running => Ok(TransitionResult::Changed(LoopStatus::Verifying)),
            _ => reject(),
        },

        LoopTrigger::GatesCompleted => match from {
            LoopStatus::Verifying => Ok(TransitionResult::Changed(LoopStatus::Running)),
            _ => reject(),
        },

        LoopTrigger::RoundBlocked => match from {
            LoopStatus::Running => Ok(TransitionResult::Changed(LoopStatus::Blocked)),
            _ => reject(),
        },

        LoopTrigger::SessionLimitReached => match from {
            LoopStatus::Running => Ok(TransitionResult::Changed(LoopStatus::NeedsHuman)),
            _ => reject(),
        },

        LoopTrigger::MaxTicksExceeded => match from {
            LoopStatus::Running => Ok(TransitionResult::Changed(LoopStatus::Failed)),
            _ => reject(),
        },

        LoopTrigger::HumanWaitReached => match from {
            LoopStatus::Running => Ok(TransitionResult::Changed(LoopStatus::NeedsHuman)),
            _ => reject(),
        },

        LoopTrigger::RecipeSetStatus(target) => {
            // Only allowed from Running, and only to the allow-listed targets
            if from != &LoopStatus::Running {
                return reject();
            }
            const ALLOWED: &[LoopStatus] = &[
                LoopStatus::Observing,
                LoopStatus::Verifying,
                LoopStatus::CompletedUnreviewed,
                LoopStatus::Approved,
                LoopStatus::Blocked,
                LoopStatus::NeedsHuman,
                LoopStatus::Failed,
                LoopStatus::Cancelled,
            ];
            if ALLOWED.contains(target) {
                Ok(TransitionResult::Changed(target.clone()))
            } else {
                reject()
            }
        }

        LoopTrigger::Approve => match from {
            LoopStatus::CompletedUnreviewed => Ok(TransitionResult::Changed(LoopStatus::Approved)),
            _ => reject(),
        },

        LoopTrigger::MarkMerged => match from {
            LoopStatus::Approved => Ok(TransitionResult::Changed(LoopStatus::Merged)),
            _ => reject(),
        },

        LoopTrigger::MarkCleaned => match from {
            LoopStatus::Merged => Ok(TransitionResult::Changed(LoopStatus::Cleaned)),
            _ => reject(),
        },
    }
}

/// Returns true if the loop is in a state where ticking (executing a recipe step) is valid.
pub fn can_tick(status: &LoopStatus) -> bool {
    matches!(
        status,
        LoopStatus::Running | LoopStatus::Observing | LoopStatus::Verifying
    )
}

// ─── Status Derivation from Step Pointer ─────────────────────────────────────

/// Derive the expected `LoopStatus` from the current recipe step and runtime state.
///
/// The recipe step pointer + optional status_override form the single source of
/// truth for loop progression. This function:
/// 1. Checks `status_override` first — if set, it takes precedence (used by
///    steps that block conditionally, like `human.wait` or `round.next` at max).
/// 2. Otherwise derives from `step.kind` (and for `loop.status` steps, the
///    declared target status).
///
/// Returns `None` only if both override is absent AND step kind is unrecognized.
pub fn derive_status_from_step(
    step_kind: &str,
    step_status: Option<&str>,
    status_override: Option<&str>,
) -> Option<LoopStatus> {
    use crate::loop_recipe::{
        STEP_GATES_RUN, STEP_HANDOFF_WAIT, STEP_HUMAN_WAIT, STEP_LOOP_EVENT, STEP_LOOP_STATUS,
        STEP_ROUND_NEXT, STEP_SESSION_CREATE, STEP_SESSION_PROMPT,
    };

    // 1. Check explicit override (set by blocking executors)
    if let Some(ov) = status_override {
        return LoopStatus::parse(ov);
    }

    // 2. Derive from step kind
    match step_kind {
        // Steps that imply the loop is actively executing
        STEP_SESSION_CREATE | STEP_SESSION_PROMPT | STEP_LOOP_EVENT | STEP_ROUND_NEXT => {
            Some(LoopStatus::Running)
        }

        // Waiting for external handoff — loop is observing
        STEP_HANDOFF_WAIT => Some(LoopStatus::Observing),

        // Running verification gates
        STEP_GATES_RUN => Some(LoopStatus::Verifying),

        // Waiting for human intervention
        STEP_HUMAN_WAIT => Some(LoopStatus::NeedsHuman),

        // Explicit status declaration — the step says what status to adopt
        STEP_LOOP_STATUS => {
            let target = step_status.unwrap_or("observing");
            LoopStatus::parse(target)
        }

        // Unrecognized step kind
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── derive_status_from_step: step kind → status mapping ─────────────────

    #[test]
    fn derive_running_from_session_create() {
        assert_eq!(
            derive_status_from_step("session.create", None, None),
            Some(LoopStatus::Running)
        );
    }

    #[test]
    fn derive_running_from_session_prompt() {
        assert_eq!(
            derive_status_from_step("session.prompt", None, None),
            Some(LoopStatus::Running)
        );
    }

    #[test]
    fn derive_running_from_loop_event() {
        assert_eq!(
            derive_status_from_step("loop.event", None, None),
            Some(LoopStatus::Running)
        );
    }

    #[test]
    fn derive_running_from_round_next() {
        assert_eq!(
            derive_status_from_step("round.next", None, None),
            Some(LoopStatus::Running)
        );
    }

    #[test]
    fn derive_observing_from_handoff_wait() {
        assert_eq!(
            derive_status_from_step("handoff.wait", None, None),
            Some(LoopStatus::Observing)
        );
    }

    #[test]
    fn derive_verifying_from_gates_run() {
        assert_eq!(
            derive_status_from_step("gates.run", None, None),
            Some(LoopStatus::Verifying)
        );
    }

    #[test]
    fn derive_needs_human_from_human_wait() {
        assert_eq!(
            derive_status_from_step("human.wait", None, None),
            Some(LoopStatus::NeedsHuman)
        );
    }

    #[test]
    fn derive_from_loop_status_step_uses_step_status_field() {
        assert_eq!(
            derive_status_from_step("loop.status", Some("completed_unreviewed"), None),
            Some(LoopStatus::CompletedUnreviewed)
        );
        assert_eq!(
            derive_status_from_step("loop.status", Some("failed"), None),
            Some(LoopStatus::Failed)
        );
        assert_eq!(
            derive_status_from_step("loop.status", Some("blocked"), None),
            Some(LoopStatus::Blocked)
        );
    }

    #[test]
    fn derive_from_loop_status_step_defaults_to_observing() {
        assert_eq!(
            derive_status_from_step("loop.status", None, None),
            Some(LoopStatus::Observing)
        );
    }

    #[test]
    fn derive_returns_none_for_unknown_step_kind() {
        assert_eq!(derive_status_from_step("unknown.step", None, None), None);
        assert_eq!(derive_status_from_step("", None, None), None);
    }

    // ─── derive_status_from_step: status_override takes precedence ───────────

    #[test]
    fn override_takes_precedence_over_step_kind() {
        // Even though session.create normally derives Running, override wins
        assert_eq!(
            derive_status_from_step("session.create", None, Some("needs_human")),
            Some(LoopStatus::NeedsHuman)
        );
    }

    #[test]
    fn override_blocked_overrides_round_next_running() {
        // round.next at max_rounds: step kind is round.next (→ Running) but override says Blocked
        assert_eq!(
            derive_status_from_step("round.next", None, Some("blocked")),
            Some(LoopStatus::Blocked)
        );
    }

    #[test]
    fn override_failed_overrides_any_step() {
        assert_eq!(
            derive_status_from_step("handoff.wait", None, Some("failed")),
            Some(LoopStatus::Failed)
        );
    }

    #[test]
    fn override_cancelled_overrides_any_step() {
        assert_eq!(
            derive_status_from_step("gates.run", None, Some("cancelled")),
            Some(LoopStatus::Cancelled)
        );
    }

    #[test]
    fn none_override_falls_through_to_step_kind() {
        assert_eq!(
            derive_status_from_step("handoff.wait", None, None),
            Some(LoopStatus::Observing)
        );
    }

    #[test]
    fn invalid_override_value_returns_none() {
        // If someone puts garbage in status_override, parse returns None
        assert_eq!(
            derive_status_from_step("session.create", None, Some("invalid_status")),
            None
        );
    }

    // ─── Status derivation guarantees no desync ──────────────────────────────

    /// The critical invariant: for every V1-executable step kind, derivation
    /// ALWAYS produces a status. There is no path where a V1 step kind returns
    /// None (which would skip the status write and allow desync).
    #[test]
    fn all_v1_step_kinds_produce_a_status() {
        let v1_kinds = [
            "session.create",
            "session.prompt",
            "handoff.wait",
            "loop.status",
            "loop.event",
            "human.wait",
            "round.next",
            "gates.run",
        ];
        for kind in &v1_kinds {
            let result = derive_status_from_step(kind, None, None);
            assert!(
                result.is_some(),
                "step kind '{}' must produce a status but returned None",
                kind
            );
        }
    }

    /// The derivation is deterministic: same inputs always produce same output.
    /// This guarantees that repeated saves don't flip-flop the status.
    #[test]
    fn derivation_is_deterministic() {
        for _ in 0..100 {
            assert_eq!(
                derive_status_from_step("handoff.wait", None, None),
                Some(LoopStatus::Observing)
            );
            assert_eq!(
                derive_status_from_step("round.next", None, Some("blocked")),
                Some(LoopStatus::Blocked)
            );
        }
    }

    /// Override always wins, regardless of step kind. This ensures that when
    /// an executor sets status_override, the derivation cannot produce a
    /// different status than what was explicitly declared.
    #[test]
    fn override_always_wins_regardless_of_step_kind() {
        let all_v1_kinds = [
            "session.create",
            "session.prompt",
            "handoff.wait",
            "loop.status",
            "loop.event",
            "human.wait",
            "round.next",
            "gates.run",
        ];
        for kind in &all_v1_kinds {
            assert_eq!(
                derive_status_from_step(kind, None, Some("needs_human")),
                Some(LoopStatus::NeedsHuman),
                "override must win for step kind '{}'",
                kind
            );
        }
    }
}
