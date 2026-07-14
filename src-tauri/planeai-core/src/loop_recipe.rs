//! Loop recipe data model — declarative definitions for loop-engineering workflows.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const RECIPE_SCHEMA_V1: &str = "planeai.loop.recipe.v1";

// Supported trigger kinds for v1 execution
pub const TRIGGER_MANUAL: &str = "manual";

// Supported step kinds for v1 execution
pub const STEP_LOOP_EVENT: &str = "loop.event";
pub const STEP_LOOP_STATUS: &str = "loop.status";
pub const STEP_SESSION_CREATE: &str = "session.create";
pub const STEP_SESSION_PROMPT: &str = "session.prompt";
pub const STEP_HANDOFF_WAIT: &str = "handoff.wait";
pub const STEP_HUMAN_WAIT: &str = "human.wait";
pub const STEP_ROUND_NEXT: &str = "round.next";
pub const STEP_GATES_RUN: &str = "gates.run";
pub const STEP_CANDIDATES_CREATE: &str = "candidates.create";
pub const STEP_CANDIDATES_WAIT: &str = "candidates.wait";
pub const STEP_ARBITER_RANK: &str = "arbiter.rank";

// Recognized but not executable step kinds
pub const STEP_PR_FEEDBACK_WAIT: &str = "pr.feedback.wait";
pub const STEP_TASK_CREATE: &str = "task.create";
pub const STEP_CONNECTOR_CALL: &str = "connector.call";

// Supported role modes
pub const MODE_WRITE: &str = "write";
pub const MODE_REVIEW: &str = "review";
pub const MODE_READONLY: &str = "readonly";

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

/// Supported input types for recipe inputs.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InputType {
    #[default]
    Text,
    Textarea,
    Branch,
    Task,
    Boolean,
    Select,
    Number,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeInput {
    #[serde(default)]
    pub required: bool,
    /// Input type: text, textarea, branch, task, boolean, select, number.
    /// Defaults to `InputType::Text` when not specified.
    #[serde(default, rename = "type")]
    pub input_type: InputType,
    /// Human-readable label for the input field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Description/help text shown below the input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Default value. Type depends on `input_type`: string, bool, or number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    /// Options for `select` type inputs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SelectOption>,
}

/// An option for select-type recipe inputs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
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
    #[serde(default = "default_auto_approve")]
    pub auto_approve: bool,
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

fn default_auto_approve() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeStep {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    /// Optional branch override for session.create steps. When present, the session
    /// checks out this existing branch instead of creating a loop-managed one.
    /// Supports template rendering (e.g., `{{ inputs.branch }}`).
    #[serde(default)]
    pub branch: Option<String>,
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
    #[serde(default)]
    pub gates: Vec<RecipeGate>,
    /// Template-rendered comma-separated list of provider names (for candidates.create).
    #[serde(default)]
    pub providers: Option<String>,
}

/// Inline gate declaration for gates.run steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeGate {
    pub name: String,
    pub command: String,
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
    STEP_ROUND_NEXT,
    STEP_GATES_RUN,
    STEP_CANDIDATES_CREATE,
    STEP_CANDIDATES_WAIT,
    STEP_ARBITER_RANK,
];

/// Recognized but not yet executable step kinds.
const FUTURE_STEP_KINDS: &[&str] = &[
    STEP_PR_FEEDBACK_WAIT,
    STEP_TASK_CREATE,
    STEP_CONNECTOR_CALL,
];

/// V1-executable trigger kinds.
const V1_TRIGGER_KINDS: &[&str] = &[TRIGGER_MANUAL];

/// Recognized but not yet executable trigger kinds.
const FUTURE_TRIGGER_KINDS: &[&str] = &[
    "schedule",
    "github_event",
    "task_event",
    "pr_feedback",
    "ci_failure",
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

    /// Validate that the correct fields are present for the declared step kind.
    /// Returns a list of problems (empty = valid).
    pub fn validate_for_kind(&self) -> Vec<String> {
        let mut problems = Vec::new();
        match self.kind.as_str() {
            STEP_SESSION_CREATE if self.role.is_none() => {
                problems.push(format!(
                    "step '{}': session.create requires 'role'",
                    self.id
                ));
            }
            STEP_SESSION_PROMPT if self.role.is_none() => {
                problems.push(format!(
                    "step '{}': session.prompt requires 'role'",
                    self.id
                ));
            }
            STEP_HANDOFF_WAIT => {
                if self.from.is_none() {
                    problems.push(format!("step '{}': handoff.wait requires 'from'", self.id));
                }
                if self.on.is_none() {
                    problems.push(format!(
                        "step '{}': handoff.wait requires 'on' mapping",
                        self.id
                    ));
                }
            }
            STEP_LOOP_STATUS if self.status.is_none() => {
                problems.push(format!("step '{}': loop.status requires 'status'", self.id));
            }
            STEP_CANDIDATES_CREATE => {
                if self.role.is_none() {
                    problems.push(format!(
                        "step '{}': candidates.create requires 'role'",
                        self.id
                    ));
                }
                if self.providers.is_none() {
                    problems.push(format!(
                        "step '{}': candidates.create requires 'providers'",
                        self.id
                    ));
                }
            }
            STEP_CANDIDATES_WAIT => {
                if self.from.is_none() {
                    problems.push(format!(
                        "step '{}': candidates.wait requires 'from'",
                        self.id
                    ));
                }
            }
            STEP_ARBITER_RANK => {
                if self.role.is_none() {
                    problems.push(format!(
                        "step '{}': arbiter.rank requires 'role'",
                        self.id
                    ));
                }
                if self.prompt.is_none() {
                    problems.push(format!(
                        "step '{}': arbiter.rank requires 'prompt'",
                        self.id
                    ));
                }
            }
            _ => {}
        }
        problems
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
    fn recipe_input_deserializes_with_all_fields() {
        let yaml = r#"
            required: true
            type: select
            label: "Merge strategy"
            description: "How to merge the PR"
            default: "squash"
            options:
              - value: squash
                label: "Squash merge"
              - value: rebase
                label: "Rebase"
        "#;
        let input: RecipeInput = serde_yml::from_str(yaml).unwrap();
        assert!(input.required);
        assert_eq!(input.input_type, InputType::Select);
        assert_eq!(input.label, Some("Merge strategy".to_string()));
        assert_eq!(input.description, Some("How to merge the PR".to_string()));
        assert_eq!(
            input.default,
            Some(serde_json::Value::String("squash".to_string()))
        );
        assert_eq!(input.options.len(), 2);
        assert_eq!(input.options[0].value, "squash");
        assert_eq!(input.options[0].label, "Squash merge");
        assert_eq!(input.options[1].value, "rebase");
        assert_eq!(input.options[1].label, "Rebase");
    }

    #[test]
    fn recipe_input_defaults_type_to_text() {
        let yaml = r#"
            required: true
        "#;
        let input: RecipeInput = serde_yml::from_str(yaml).unwrap();
        assert!(input.required);
        assert_eq!(input.input_type, InputType::Text);
        assert_eq!(input.label, None);
        assert_eq!(input.description, None);
        assert_eq!(input.default, None);
        assert!(input.options.is_empty());
    }

    #[test]
    fn recipe_input_boolean_default() {
        let yaml = r#"
            type: boolean
            label: "Draft PR"
            default: true
        "#;
        let input: RecipeInput = serde_yml::from_str(yaml).unwrap();
        assert_eq!(input.input_type, InputType::Boolean);
        assert_eq!(input.default, Some(serde_json::Value::Bool(true)));
    }

    #[test]
    fn recipe_input_number_default() {
        let yaml = r#"
            type: number
            label: "Max retries"
            default: 5
        "#;
        let input: RecipeInput = serde_yml::from_str(yaml).unwrap();
        assert_eq!(input.input_type, InputType::Number);
        assert_eq!(input.default, Some(serde_json::json!(5)));
    }

    #[test]
    fn recipe_inputs_sorted_alphabetically() {
        let yaml = r#"
            schema: planeai.loop.recipe.v1
            id: test
            name: Test
            trigger:
              kind: manual
            inputs:
              goal:
                required: true
                type: textarea
              branch:
                required: true
                type: branch
              gate_command:
                required: false
                type: text
            roles:
              maker:
                mode: write
            policy:
              max_rounds: 3
            steps:
              - id: start
                kind: loop.event
                event_kind: started
        "#;
        let recipe: LoopRecipe = serde_yml::from_str(yaml).unwrap();
        let keys: Vec<&String> = recipe.inputs.keys().collect();
        // BTreeMap sorts alphabetically
        assert_eq!(keys, vec!["branch", "gate_command", "goal"]);
    }

    #[test]
    fn step_v1_executable() {
        let step = RecipeStep {
            id: "s1".into(),
            kind: STEP_SESSION_CREATE.into(),
            role: None,
            prompt: None,
            branch: None,
            from: None,
            on: None,
            status: None,
            next: None,
            select: None,
            event_kind: None,
            gates: vec![],
            providers: None,
        };
        assert!(step.is_v1_executable());
        assert!(step.is_recognized());
    }

    #[test]
    fn step_future_recognized() {
        let step = RecipeStep {
            id: "s2".into(),
            kind: STEP_PR_FEEDBACK_WAIT.into(),
            role: None,
            prompt: None,
            branch: None,
            from: None,
            on: None,
            status: None,
            next: None,
            select: None,
            event_kind: None,
            gates: vec![],
            providers: None,
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
            branch: None,
            from: None,
            on: None,
            status: None,
            next: None,
            select: None,
            event_kind: None,
            gates: vec![],
            providers: None,
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
            kind: "schedule".into(),
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

    #[test]
    fn step_session_create_with_branch_field() {
        let yaml = r#"
            id: create_gatekeeper
            kind: session.create
            role: gatekeeper
            branch: "{{ inputs.branch }}"
            prompt: "Fix the issues"
        "#;
        let step: RecipeStep = serde_yml::from_str(yaml).unwrap();
        assert_eq!(step.id, "create_gatekeeper");
        assert_eq!(step.branch, Some("{{ inputs.branch }}".to_string()));
        assert_eq!(step.role, Some("gatekeeper".to_string()));
    }

    #[test]
    fn step_without_branch_field_defaults_to_none() {
        let yaml = r#"
            id: create_maker
            kind: session.create
            role: maker
            prompt: "Implement the feature"
        "#;
        let step: RecipeStep = serde_yml::from_str(yaml).unwrap();
        assert_eq!(step.branch, None);
    }
}
