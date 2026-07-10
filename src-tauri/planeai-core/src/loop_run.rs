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
    pub current_round: i64,
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Cancel => "Cancel",
            Self::HandoffWaiting => "HandoffWaiting",
            Self::HandoffConsumed => "HandoffConsumed",
            Self::HandoffReceived(_) => "HandoffReceived",
            Self::GatesStarted => "GatesStarted",
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
pub fn apply(from: &LoopStatus, trigger: &LoopTrigger) -> Result<TransitionResult, InvalidTransition> {
    let reject = || Err(InvalidTransition { from: from.clone(), trigger: trigger.clone() });

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
    matches!(status, LoopStatus::Running | LoopStatus::Observing | LoopStatus::Verifying)
}
