//! Loop recipe data model — declarative definitions for loop-engineering workflows.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const RECIPE_SCHEMA_V1: &str = "planeai.loop.recipe.v1";

// Supported trigger kinds for v1 execution
pub const TRIGGER_MANUAL: &str = "manual";

// Recognized but not yet executable trigger kinds
pub const TRIGGER_SCHEDULE: &str = "schedule";
pub const TRIGGER_GITHUB_EVENT: &str = "github_event";
pub const TRIGGER_TASK_EVENT: &str = "task_event";
pub const TRIGGER_PR_FEEDBACK: &str = "pr_feedback";
pub const TRIGGER_CI_FAILURE: &str = "ci_failure";

// Supported step kinds for v1 execution
pub const STEP_LOOP_EVENT: &str = "loop.event";
pub const STEP_LOOP_STATUS: &str = "loop.status";
pub const STEP_SESSION_CREATE: &str = "session.create";
pub const STEP_SESSION_PROMPT: &str = "session.prompt";
pub const STEP_HANDOFF_WAIT: &str = "handoff.wait";
pub const STEP_HUMAN_WAIT: &str = "human.wait";

// Recognized but not executable step kinds
pub const STEP_GATES_RUN: &str = "gates.run";
pub const STEP_PR_FEEDBACK_WAIT: &str = "pr.feedback.wait";
pub const STEP_ARBITER_RANK: &str = "arbiter.rank";
pub const STEP_TASK_CREATE: &str = "task.create";
pub const STEP_CONNECTOR_CALL: &str = "connector.call";

// Supported role modes
pub const MODE_WRITE: &str = "write";
pub const MODE_REVIEW: &str = "review";
pub const MODE_READONLY: &str = "readonly";
pub const MODE_PLAN: &str = "plan";
pub const MODE_TRIAGE: &str = "triage";
pub const MODE_ARBITER: &str = "arbiter";

// Supported isolation values
pub const ISOLATION_WORKTREE: &str = "worktree";
pub const ISOLATION_PROJECT: &str = "project";
pub const ISOLATION_READONLY: &str = "readonly";

// ─── Structs ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRecipe {
    pub schema: String,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub trigger: RecipeTrigger,
    #[serde(default)]
    pub inputs: BTreeMap<String, RecipeInput>,
    #[serde(default)]
    pub knowledge: RecipeKnowledge,
    #[serde(default)]
    pub tools: RecipeTools,
    pub roles: BTreeMap<String, RecipeRole>,
    pub policy: RecipePolicy,
    pub steps: Vec<RecipeStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeTrigger {
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeInput {
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecipeKnowledge {
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub instructions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecipeTools {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeRole {
    #[serde(default = "default_provider")]
    pub provider: String,
    pub mode: String,
    #[serde(default = "default_isolation")]
    pub isolation: String,
    #[serde(default)]
    pub instructions: Option<String>,
}

fn default_provider() -> String {
    "default".to_string()
}

fn default_isolation() -> String {
    "worktree".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipePolicy {
    #[serde(default = "default_max_rounds")]
    pub max_rounds: u32,
    #[serde(default = "default_max_ticks")]
    pub max_ticks: u32,
    #[serde(default = "default_max_sessions")]
    pub max_sessions: u32,
    #[serde(default)]
    pub stale_after_ms: Option<u64>,
    #[serde(default = "default_merge_policy")]
    pub merge_policy: String,
}

fn default_max_rounds() -> u32 {
    3
}
fn default_max_ticks() -> u32 {
    50
}
fn default_max_sessions() -> u32 {
    5
}
fn default_merge_policy() -> String {
    "human".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeStep {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub on: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub select: Option<String>,
    #[serde(default)]
    pub event_kind: Option<String>,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// V1-executable step kinds.
const V1_STEP_KINDS: &[&str] = &[
    STEP_LOOP_EVENT,
    STEP_LOOP_STATUS,
    STEP_SESSION_CREATE,
    STEP_SESSION_PROMPT,
    STEP_HANDOFF_WAIT,
    STEP_HUMAN_WAIT,
];

/// Recognized but not yet executable step kinds.
const FUTURE_STEP_KINDS: &[&str] = &[
    STEP_GATES_RUN,
    STEP_PR_FEEDBACK_WAIT,
    STEP_ARBITER_RANK,
    STEP_TASK_CREATE,
    STEP_CONNECTOR_CALL,
];

/// V1-executable trigger kinds.
const V1_TRIGGER_KINDS: &[&str] = &[TRIGGER_MANUAL];

/// Recognized but not yet executable trigger kinds.
const FUTURE_TRIGGER_KINDS: &[&str] = &[
    TRIGGER_SCHEDULE,
    TRIGGER_GITHUB_EVENT,
    TRIGGER_TASK_EVENT,
    TRIGGER_PR_FEEDBACK,
    TRIGGER_CI_FAILURE,
];

impl RecipeStep {
    /// Returns true if this step kind is executable in v1.
    pub fn is_v1_executable(&self) -> bool {
        V1_STEP_KINDS.contains(&self.kind.as_str())
    }

    /// Returns true if this step kind is recognized (v1 + future).
    pub fn is_recognized(&self) -> bool {
        V1_STEP_KINDS.contains(&self.kind.as_str())
            || FUTURE_STEP_KINDS.contains(&self.kind.as_str())
    }
}

impl RecipeTrigger {
    /// Returns true if this trigger kind is executable in v1.
    pub fn is_v1_executable(&self) -> bool {
        V1_TRIGGER_KINDS.contains(&self.kind.as_str())
    }

    /// Returns true if this trigger kind is recognized (v1 + future).
    pub fn is_recognized(&self) -> bool {
        V1_TRIGGER_KINDS.contains(&self.kind.as_str())
            || FUTURE_TRIGGER_KINDS.contains(&self.kind.as_str())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_v1_executable() {
        let step = RecipeStep {
            id: "s1".into(),
            kind: STEP_SESSION_CREATE.into(),
            role: None,
            prompt: None,
            from: None,
            on: None,
            status: None,
            next: None,
            select: None,
            event_kind: None,
        };
        assert!(step.is_v1_executable());
        assert!(step.is_recognized());
    }

    #[test]
    fn step_future_recognized() {
        let step = RecipeStep {
            id: "s2".into(),
            kind: STEP_GATES_RUN.into(),
            role: None,
            prompt: None,
            from: None,
            on: None,
            status: None,
            next: None,
            select: None,
            event_kind: None,
        };
        assert!(!step.is_v1_executable());
        assert!(step.is_recognized());
    }

    #[test]
    fn step_unknown_kind() {
        let step = RecipeStep {
            id: "s3".into(),
            kind: "totally.unknown".into(),
            role: None,
            prompt: None,
            from: None,
            on: None,
            status: None,
            next: None,
            select: None,
            event_kind: None,
        };
        assert!(!step.is_v1_executable());
        assert!(!step.is_recognized());
    }

    #[test]
    fn trigger_manual_v1() {
        let trigger = RecipeTrigger {
            kind: TRIGGER_MANUAL.into(),
        };
        assert!(trigger.is_v1_executable());
        assert!(trigger.is_recognized());
    }

    #[test]
    fn trigger_schedule_future() {
        let trigger = RecipeTrigger {
            kind: TRIGGER_SCHEDULE.into(),
        };
        assert!(!trigger.is_v1_executable());
        assert!(trigger.is_recognized());
    }

    #[test]
    fn trigger_unknown() {
        let trigger = RecipeTrigger {
            kind: "webhook".into(),
        };
        assert!(!trigger.is_v1_executable());
        assert!(!trigger.is_recognized());
    }
}
