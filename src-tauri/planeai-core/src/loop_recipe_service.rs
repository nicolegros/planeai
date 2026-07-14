//! Loop recipe service — discovery, loading, validation, and snapshot creation.
//!
//! Recipes are discovered from three locations with project > user > builtin precedence.
//! No database connection required — purely file-based.
//!
//! # I/O safety
//!
//! All file I/O in this module is synchronous (`std::fs`). This is safe because
//! the service is only called from the CLI binary (`planeai-cli`), never from
//! Tauri IPC handlers. If recipe operations are ever exposed as Tauri commands,
//! they must be wrapped with `commands::blocking(|| { ... }).await`.

use crate::loop_recipe::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

// ─── Built-in recipes (embedded at compile time) ─────────────────────────────

const BUILTIN_MAKER_VERIFIER: &str = include_str!("../resources/recipes/maker-verifier.yaml");

// ─── Types ───────────────────────────────────────────────────────────────────

/// Source where a recipe was discovered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeSource {
    Builtin,
    User,
    Project,
    Path,
}

impl RecipeSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::User => "user",
            Self::Project => "project",
            Self::Path => "path",
        }
    }
}

/// A discovered recipe with its source and optional path.
#[derive(Debug, Clone)]
pub struct DiscoveredRecipe {
    pub recipe: LoopRecipe,
    pub source: RecipeSource,
    pub path: Option<PathBuf>,
}

/// Validation result.
#[derive(Debug, Clone)]
pub struct RecipeValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Runtime snapshot stored in policy_json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeSnapshot {
    pub recipe_schema: String,
    pub recipe_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_description: Option<String>,
    pub recipe_source: String,
    pub recipe_path: Option<String>,
    pub inputs: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input_defs: BTreeMap<String, RecipeInput>,
    pub runtime: RecipeRuntime,
    pub policy: SnapshotPolicy,
    pub roles: BTreeMap<String, RecipeRole>,
    pub steps: Vec<RecipeStep>,
    pub knowledge: RecipeKnowledge,
    pub tools: RecipeTools,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeRuntime {
    pub current_step: String,
    pub tick_count: u32,
    pub round: u32,
    pub created_session_ids: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub last_error: Option<String>,
    /// Timestamp of the last consumed handoff — used to ignore stale handoffs
    /// from previous rounds when checking handoff.wait.
    #[serde(default)]
    pub last_handoff_consumed_at: Option<String>,
    /// When set, overrides the step-kind derivation for LoopStatus.
    /// Used by steps that block execution conditionally (e.g., `human.wait` → NeedsHuman,
    /// `round.next` at max_rounds → Blocked). Cleared when the loop resumes.
    #[serde(default)]
    pub status_override: Option<crate::loop_run::LoopStatus>,
    #[serde(default)]
    pub last_activity_at: Option<String>,
    /// Per-session observation state for stale/heartbeat detection.
    #[serde(default)]
    pub session_observations: BTreeMap<String, SessionObservation>,
}

/// Tracks the last-known observation cursor for a loop-owned session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionObservation {
    /// Event ID cursor — events with id > this value are considered new.
    /// `None` means never observed (first-tick seeding needed).
    #[serde(default)]
    pub last_cursor: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotPolicy {
    pub max_rounds: u32,
    pub max_ticks: u32,
    pub max_sessions: u32,
    #[serde(default)]
    pub stale_after_ms: Option<u64>,
    pub merge_policy: String,
    #[serde(default = "default_auto_approve")]
    pub auto_approve: bool,
}

fn default_auto_approve() -> bool {
    true
}

// ─── Service ─────────────────────────────────────────────────────────────────

pub struct RecipeService;

impl RecipeService {
    /// Load the built-in recipes (embedded in binary).
    pub fn builtin_recipes() -> Vec<DiscoveredRecipe> {
        let mut recipes = Vec::new();
        if let Ok(recipe) = Self::parse_yaml(BUILTIN_MAKER_VERIFIER) {
            recipes.push(DiscoveredRecipe {
                recipe,
                source: RecipeSource::Builtin,
                path: None,
            });
        }
        recipes
    }

    /// Discover user recipes from ~/.config/planeai/loops/*.yaml
    pub fn user_recipes() -> Vec<DiscoveredRecipe> {
        let dir = Self::user_recipes_dir();
        Self::load_from_dir(&dir, RecipeSource::User)
    }

    /// Discover project recipes from <project_root>/.planeai/loops/*.yaml
    pub fn project_recipes(project_root: &Path) -> Vec<DiscoveredRecipe> {
        let dir = project_root.join(".planeai").join("loops");
        Self::load_from_dir(&dir, RecipeSource::Project)
    }

    /// Load a recipe from a specific file path.
    pub fn load_from_path(path: &Path) -> Result<DiscoveredRecipe, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
        let recipe = Self::parse_yaml(&content)?;
        Ok(DiscoveredRecipe {
            recipe,
            source: RecipeSource::Path,
            path: Some(path.to_path_buf()),
        })
    }

    /// Discover all recipes with precedence: project > user > builtin.
    ///
    /// Later sources override earlier ones when IDs collide.
    pub fn discover_all(project_root: Option<&Path>) -> Vec<DiscoveredRecipe> {
        let mut by_id: BTreeMap<String, DiscoveredRecipe> = BTreeMap::new();

        // Lowest precedence first
        for dr in Self::builtin_recipes() {
            by_id.insert(dr.recipe.id.clone(), dr);
        }
        for dr in Self::user_recipes() {
            by_id.insert(dr.recipe.id.clone(), dr);
        }
        if let Some(root) = project_root {
            for dr in Self::project_recipes(root) {
                by_id.insert(dr.recipe.id.clone(), dr);
            }
        }

        by_id.into_values().collect()
    }

    /// Resolve a recipe by ID or path.
    pub fn resolve(
        id_or_path: &str,
        project_root: Option<&Path>,
    ) -> Result<DiscoveredRecipe, String> {
        // If it looks like a path, try loading directly
        let as_path = Path::new(id_or_path);
        if as_path.extension().is_some() || id_or_path.contains('/') || id_or_path.contains('\\') {
            return Self::load_from_path(as_path);
        }

        // Otherwise search by ID
        let all = Self::discover_all(project_root);
        all.into_iter()
            .find(|dr| dr.recipe.id == id_or_path)
            .ok_or_else(|| format!("recipe not found: {}", id_or_path))
    }

    /// Validate a recipe, returning errors and warnings.
    pub fn validate(recipe: &LoopRecipe, project_root: Option<&Path>) -> RecipeValidationResult {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Schema
        if recipe.schema != RECIPE_SCHEMA_V1 {
            errors.push(format!(
                "unsupported schema: expected '{}', got '{}'",
                RECIPE_SCHEMA_V1, recipe.schema
            ));
        }

        // ID
        if recipe.id.is_empty() {
            errors.push("id is missing or empty".to_string());
        }

        // Name
        if recipe.name.is_empty() {
            errors.push("name is missing or empty".to_string());
        }

        // Trigger
        if recipe.trigger.kind.is_empty() {
            errors.push("trigger.kind is missing".to_string());
        } else if !recipe.trigger.is_recognized() {
            errors.push(format!(
                "trigger.kind '{}' is not recognized",
                recipe.trigger.kind
            ));
        } else if !recipe.trigger.is_v1_executable() {
            warnings.push(format!(
                "trigger.kind '{}' is recognized but not executable in v1",
                recipe.trigger.kind
            ));
        }

        // Roles
        if recipe.roles.is_empty() {
            errors.push("roles must not be empty".to_string());
        }

        // Check for duplicate role ids (BTreeMap handles this implicitly, but
        // we check referenced roles below)
        let role_ids: HashSet<&str> = recipe.roles.keys().map(|s| s.as_str()).collect();

        // Steps
        if recipe.steps.is_empty() {
            errors.push("steps must not be empty".to_string());
        }

        // Duplicate step IDs
        let mut step_ids: HashSet<String> = HashSet::new();
        let all_step_ids: HashSet<&str> = recipe.steps.iter().map(|s| s.id.as_str()).collect();
        for step in &recipe.steps {
            if !step_ids.insert(step.id.clone()) {
                errors.push(format!("duplicate step id: '{}'", step.id));
            }
        }

        // Step validation
        let role_bearing_kinds: HashSet<&str> =
            [STEP_SESSION_CREATE, STEP_SESSION_PROMPT, STEP_HANDOFF_WAIT]
                .into_iter()
                .collect();

        let mut referenced_roles: HashSet<String> = HashSet::new();

        for step in &recipe.steps {
            // Role reference check
            if role_bearing_kinds.contains(step.kind.as_str()) {
                let role_ref = step.role.as_deref().or(step.from.as_deref());
                if let Some(r) = role_ref {
                    if !role_ids.contains(r) {
                        errors.push(format!(
                            "step '{}' references unknown role '{}'",
                            step.id, r
                        ));
                    }
                    referenced_roles.insert(r.to_string());
                }
            }

            // Next step reference check
            if let Some(ref next) = step.next {
                if !all_step_ids.contains(next.as_str()) {
                    errors.push(format!(
                        "step '{}' references unknown next step '{}'",
                        step.id, next
                    ));
                }
            }

            // On mapping — values are step IDs
            if let Some(ref on_map) = step.on {
                for (key, target) in on_map {
                    if !all_step_ids.contains(target.as_str()) {
                        errors.push(format!(
                            "step '{}' on.{} references unknown step '{}'",
                            step.id, key, target
                        ));
                    }
                }
            }

            // Future step kind warning
            if !step.is_v1_executable() && step.is_recognized() {
                warnings.push(format!(
                    "step '{}' uses kind '{}' which is recognized but not executable in v1",
                    step.id, step.kind
                ));
            }

            // Per-kind field validation
            for problem in step.validate_for_kind() {
                errors.push(problem);
            }
        }

        // Policy validation
        if recipe.policy.max_rounds < 1 {
            errors.push("policy.max_rounds must be >= 1".to_string());
        }
        if recipe.policy.max_sessions < 1 {
            errors.push("policy.max_sessions must be >= 1".to_string());
        }
        if recipe.policy.max_ticks < 1 {
            errors.push("policy.max_ticks must be >= 1".to_string());
        }
        if recipe.policy.merge_policy != "human" {
            errors.push(format!(
                "policy.merge_policy must be 'human', got '{}'",
                recipe.policy.merge_policy
            ));
        }

        // Warnings: unreferenced roles
        for role_id in &role_ids {
            if !referenced_roles.contains(*role_id) {
                warnings.push(format!(
                    "role '{}' is declared but not referenced in any step",
                    role_id
                ));
            }
        }

        // Warnings: knowledge files missing (only with project_root)
        if let Some(root) = project_root {
            for file in &recipe.knowledge.files {
                let file_path = root.join(file);
                if !file_path.exists() {
                    warnings.push(format!("knowledge file '{}' not found in project", file));
                }
            }
        }

        // Warnings: write role with non-worktree isolation
        for (role_id, role) in &recipe.roles {
            if role.mode == MODE_WRITE && role.isolation != ISOLATION_WORKTREE {
                warnings.push(format!(
                    "role '{}' has mode 'write' but isolation '{}' (expected 'worktree')",
                    role_id, role.isolation
                ));
            }
        }

        // Warnings: tools.required not available (basic check — we only check
        // that common CLI tools exist on PATH)
        for tool in &recipe.tools.required {
            // Only warn for tools we can reasonably check (CLI tools on PATH)
            if matches!(tool.as_str(), "git" | "gh" | "jira")
                && std::process::Command::new(tool)
                    .arg("--version")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_err()
            {
                warnings.push(format!("tools.required '{}' not found on PATH", tool));
            }
        }

        let valid = errors.is_empty();
        RecipeValidationResult {
            valid,
            errors,
            warnings,
        }
    }

    /// Create a snapshot for storing in policy_json.
    pub fn create_snapshot(
        discovered: &DiscoveredRecipe,
        inputs: BTreeMap<String, serde_json::Value>,
    ) -> RecipeSnapshot {
        let recipe = &discovered.recipe;
        let first_step_id = recipe
            .steps
            .first()
            .map(|s| s.id.clone())
            .unwrap_or_default();

        RecipeSnapshot {
            recipe_schema: recipe.schema.clone(),
            recipe_id: recipe.id.clone(),
            recipe_name: Some(recipe.name.clone()),
            recipe_description: recipe.description.clone(),
            recipe_source: discovered.source.as_str().to_string(),
            recipe_path: discovered.path.as_ref().map(|p| p.display().to_string()),
            inputs,
            input_defs: recipe.inputs.clone(),
            runtime: RecipeRuntime {
                current_step: first_step_id,
                tick_count: 0,
                round: 1,
                created_session_ids: BTreeMap::new(),
                last_error: None,
                last_handoff_consumed_at: None,
                status_override: None,
                last_activity_at: None,
                session_observations: BTreeMap::new(),
            },
            policy: SnapshotPolicy {
                max_rounds: recipe.policy.max_rounds,
                max_ticks: recipe.policy.max_ticks,
                max_sessions: recipe.policy.max_sessions,
                stale_after_ms: recipe.policy.stale_after_ms,
                merge_policy: recipe.policy.merge_policy.clone(),
                auto_approve: recipe.policy.auto_approve,
            },
            roles: recipe.roles.clone(),
            steps: recipe.steps.clone(),
            knowledge: recipe.knowledge.clone(),
            tools: recipe.tools.clone(),
        }
    }

    /// Validate inputs against recipe input definitions.
    pub fn validate_inputs(
        inputs: &BTreeMap<String, serde_json::Value>,
        recipe_inputs: &BTreeMap<String, RecipeInput>,
    ) -> Result<(), String> {
        for (key, def) in recipe_inputs {
            let value = inputs.get(key);

            // Check required
            if def.required {
                match value {
                    None => return Err(format!("input '{}' is required", key)),
                    Some(serde_json::Value::String(s)) if s.is_empty() => {
                        return Err(format!("input '{}' is required", key))
                    }
                    Some(serde_json::Value::Null) => {
                        return Err(format!("input '{}' is required", key))
                    }
                    _ => {}
                }
            }

            // Check type if value is present
            if let Some(val) = value {
                if val.is_null() {
                    continue;
                }
                match def.input_type {
                    InputType::Boolean => {
                        if !val.is_boolean() {
                            return Err(format!("input '{}' must be a boolean", key));
                        }
                    }
                    InputType::Number => {
                        if !val.is_number() {
                            return Err(format!("input '{}' must be a number", key));
                        }
                    }
                    InputType::Select => {
                        if let Some(s) = val.as_str() {
                            let valid = def.options.iter().any(|o| o.value == s);
                            if !valid {
                                return Err(format!("input '{}' has invalid option '{}'", key, s));
                            }
                        }
                    }
                    // text, textarea, branch, task: accept any string
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Parse YAML content into a LoopRecipe.
    pub fn parse_yaml(content: &str) -> Result<LoopRecipe, String> {
        serde_yml::from_str(content).map_err(|e| format!("YAML parse error: {}", e))
    }

    // ─── Internal helpers ────────────────────────────────────────────────────

    fn user_recipes_dir() -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home)
                .join(".config")
                .join("planeai")
                .join("loops")
        }
        #[cfg(not(target_os = "macos"))]
        {
            let config = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
                format!("{}/.config", std::env::var("HOME").unwrap_or_default())
            });
            PathBuf::from(config).join("planeai").join("loops")
        }
    }

    fn load_from_dir(dir: &Path, source: RecipeSource) -> Vec<DiscoveredRecipe> {
        let mut recipes = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return recipes,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str());
            if ext == Some("yaml") || ext == Some("yml") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(recipe) = Self::parse_yaml(&content) {
                        recipes.push(DiscoveredRecipe {
                            recipe,
                            source: source.clone(),
                            path: Some(path),
                        });
                    }
                }
            }
        }
        recipes
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_builtin_maker_verifier() {
        let recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER)
            .expect("built-in maker-verifier should parse");
        assert_eq!(recipe.schema, RECIPE_SCHEMA_V1);
        assert_eq!(recipe.id, "maker-verifier");
        assert_eq!(recipe.name, "Maker + Verifier");
        assert_eq!(recipe.roles.len(), 2);
        assert!(recipe.roles.contains_key("maker"));
        assert!(recipe.roles.contains_key("verifier"));
        assert!(!recipe.steps.is_empty());
    }

    #[test]
    fn builtin_recipes_discovered() {
        let recipes = RecipeService::builtin_recipes();
        assert!(!recipes.is_empty());
        assert_eq!(recipes[0].source, RecipeSource::Builtin);
        assert!(recipes[0].path.is_none());
    }

    #[test]
    fn validate_valid_recipe() {
        let recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        let result = RecipeService::validate(&recipe, None);
        assert!(result.valid, "errors: {:?}", result.errors);
    }

    #[test]
    fn validate_empty_id() {
        let mut recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        recipe.id = String::new();
        let result = RecipeService::validate(&recipe, None);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("id")));
    }

    #[test]
    fn validate_empty_name() {
        let mut recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        recipe.name = String::new();
        let result = RecipeService::validate(&recipe, None);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("name")));
    }

    #[test]
    fn validate_bad_schema() {
        let mut recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        recipe.schema = "planeai.loop.recipe.v99".to_string();
        let result = RecipeService::validate(&recipe, None);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("schema")));
    }

    #[test]
    fn validate_unknown_trigger() {
        let mut recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        recipe.trigger.kind = "webhook".to_string();
        let result = RecipeService::validate(&recipe, None);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("trigger")));
    }

    #[test]
    fn validate_future_trigger_warns() {
        let mut recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        recipe.trigger.kind = "schedule".to_string();
        let result = RecipeService::validate(&recipe, None);
        assert!(result.valid);
        assert!(result.warnings.iter().any(|w| w.contains("schedule")));
    }

    #[test]
    fn validate_unknown_role_reference() {
        let mut recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        recipe.steps[0].role = Some("nonexistent".to_string());
        let result = RecipeService::validate(&recipe, None);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("unknown role")));
    }

    #[test]
    fn validate_unknown_next_step() {
        let mut recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        recipe.steps[0].next = Some("nowhere".to_string());
        let result = RecipeService::validate(&recipe, None);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("unknown next step")));
    }

    #[test]
    fn validate_bad_policy() {
        let mut recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        recipe.policy.max_rounds = 0;
        recipe.policy.max_sessions = 0;
        recipe.policy.max_ticks = 0;
        let result = RecipeService::validate(&recipe, None);
        assert!(!result.valid);
        assert!(result.errors.len() >= 3);
    }

    #[test]
    fn validate_bad_merge_policy() {
        let mut recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        recipe.policy.merge_policy = "auto".to_string();
        let result = RecipeService::validate(&recipe, None);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("merge_policy")));
    }

    #[test]
    fn validate_unreferenced_role_warns() {
        let mut recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        // Add an unused role to trigger the warning
        recipe.roles.insert(
            "arbiter".to_string(),
            crate::loop_recipe::RecipeRole {
                provider: "default".to_string(),
                mode: "review".to_string(),
                isolation: "readonly".to_string(),
                instructions: None,
            },
        );
        let result = RecipeService::validate(&recipe, None);
        // arbiter role is declared but never referenced in steps
        assert!(result.warnings.iter().any(|w| w.contains("arbiter")));
    }

    #[test]
    fn validate_duplicate_step_ids() {
        let mut recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        let first_id = recipe.steps[0].id.clone();
        recipe.steps[1].id = first_id.clone();
        let result = RecipeService::validate(&recipe, None);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("duplicate step id")));
    }

    #[test]
    fn snapshot_creation() {
        let recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        let discovered = DiscoveredRecipe {
            recipe,
            source: RecipeSource::Builtin,
            path: None,
        };
        let mut inputs = BTreeMap::new();
        inputs.insert(
            "goal".to_string(),
            serde_json::Value::String("implement feature X".to_string()),
        );

        let snapshot = RecipeService::create_snapshot(&discovered, inputs.clone());
        assert_eq!(snapshot.recipe_schema, RECIPE_SCHEMA_V1);
        assert_eq!(snapshot.recipe_id, "maker-verifier");
        assert_eq!(snapshot.recipe_source, "builtin");
        assert!(snapshot.recipe_path.is_none());
        assert_eq!(snapshot.inputs, inputs);
        assert_eq!(snapshot.runtime.current_step, "create_maker");
        assert_eq!(snapshot.runtime.tick_count, 0);
        assert_eq!(snapshot.runtime.round, 1);
        assert_eq!(snapshot.policy.max_rounds, 3);
        assert_eq!(snapshot.policy.max_ticks, 50);
        assert_eq!(snapshot.policy.max_sessions, 5);
        assert_eq!(snapshot.policy.stale_after_ms, Some(600000));
        assert_eq!(snapshot.policy.merge_policy, "human");

        // recipe_name and recipe_description populated from recipe
        assert_eq!(snapshot.recipe_name, Some("Maker + Verifier".to_string()));
        assert!(snapshot.recipe_description.is_some());

        // input_defs populated from recipe.inputs
        assert!(!snapshot.input_defs.is_empty());
        let goal_def = snapshot.input_defs.get("goal").expect("goal input_def");
        assert!(goal_def.required);
        assert_eq!(goal_def.input_type, InputType::Textarea);
        assert_eq!(goal_def.label, Some("Goal".to_string()));
    }

    #[test]
    fn snapshot_input_defs_match_recipe_inputs() {
        let recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        let discovered = DiscoveredRecipe {
            recipe: recipe.clone(),
            source: RecipeSource::Builtin,
            path: None,
        };
        let inputs = BTreeMap::new();
        let snapshot = RecipeService::create_snapshot(&discovered, inputs);

        // All recipe inputs should appear in snapshot.input_defs
        assert_eq!(snapshot.input_defs.len(), recipe.inputs.len());
        for (key, def) in &recipe.inputs {
            let snap_def = snapshot
                .input_defs
                .get(key)
                .unwrap_or_else(|| panic!("input_defs missing key: {key}"));
            assert_eq!(snap_def.required, def.required);
            assert_eq!(snap_def.input_type, def.input_type);
            assert_eq!(snap_def.label, def.label);
        }
    }

    #[test]
    fn resolve_by_id() {
        let result = RecipeService::resolve("maker-verifier", None);
        assert!(result.is_ok());
        let dr = result.unwrap();
        assert_eq!(dr.recipe.id, "maker-verifier");
        assert_eq!(dr.source, RecipeSource::Builtin);
    }

    #[test]
    fn resolve_unknown_id() {
        let result = RecipeService::resolve("nonexistent-recipe", None);
        assert!(result.is_err());
    }

    #[test]
    fn load_from_nonexistent_path() {
        let result = RecipeService::load_from_path(Path::new("/tmp/nonexistent.yaml"));
        assert!(result.is_err());
    }

    #[test]
    fn snapshot_serializes_to_json() {
        let recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        let discovered = DiscoveredRecipe {
            recipe,
            source: RecipeSource::Builtin,
            path: None,
        };
        let inputs = BTreeMap::new();
        let snapshot = RecipeService::create_snapshot(&discovered, inputs);
        let json = serde_json::to_string(&snapshot).expect("snapshot should serialize to JSON");
        assert!(json.contains("maker-verifier"));
    }

    #[test]
    fn validate_on_references_unknown_step() {
        let mut recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        // Modify the handoff.wait step's on mapping to reference a nonexistent step
        for step in &mut recipe.steps {
            if step.kind == STEP_HANDOFF_WAIT {
                if let Some(ref mut on_map) = step.on {
                    on_map.insert("completed".to_string(), "nonexistent_step".to_string());
                }
                break;
            }
        }
        let result = RecipeService::validate(&recipe, None);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("nonexistent_step")));
    }

    #[test]
    fn write_role_non_worktree_warns() {
        let mut recipe = RecipeService::parse_yaml(BUILTIN_MAKER_VERIFIER).unwrap();
        recipe.roles.get_mut("maker").unwrap().isolation = "project".to_string();
        let result = RecipeService::validate(&recipe, None);
        // Should still be valid, just a warning
        assert!(result.valid);
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("maker") && w.contains("project")));
    }
}
