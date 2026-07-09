//! AXI recipe subcommands — list, show, validate loop recipes.

use crate::axi::emit_error;
use planeai_toon::{field, int_val, render, str_val, Value};

pub fn recipe_ls(cwd: &str) -> (String, i32) {
    use planeai_core::loop_recipe_service::RecipeService;

    let project_root = resolve_project_root(cwd);
    let recipes = RecipeService::discover_all(Some(&project_root));

    if recipes.is_empty() {
        let fields = vec![
            field("recipes", str_val("0 recipes found")),
            field(
                "help",
                Value::List(vec![
                    "Add recipes to .planeai/loops/*.yaml or ~/.config/planeai/loops/*.yaml".into(),
                ]),
            ),
        ];
        return (render(&fields), 0);
    }

    let rows: Vec<Vec<String>> = recipes
        .iter()
        .map(|dr| {
            vec![
                dr.recipe.id.clone(),
                dr.recipe.name.clone(),
                dr.source.as_str().to_string(),
                dr.path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            ]
        })
        .collect();

    let fields = vec![field(
        "recipes",
        Value::Table {
            columns: vec!["id".into(), "name".into(), "source".into(), "path".into()],
            rows,
        },
    )];
    (render(&fields), 0)
}

pub fn recipe_show(id_or_path: &str, cwd: &str) -> (String, i32) {
    use planeai_core::loop_recipe_service::RecipeService;

    let project_root = resolve_project_root(cwd);
    let discovered = match RecipeService::resolve(id_or_path, Some(&project_root)) {
        Ok(d) => d,
        Err(e) => {
            return (
                emit_error(
                    &e,
                    &["Run `planeai-cli axi loop recipe ls` to see available recipes".into()],
                ),
                1,
            )
        }
    };

    let recipe = &discovered.recipe;
    let validation = RecipeService::validate(recipe, Some(&project_root));

    let mut recipe_fields = vec![
        field("id", str_val(&recipe.id)),
        field("name", str_val(&recipe.name)),
        field("schema", str_val(&recipe.schema)),
        field("source", str_val(discovered.source.as_str())),
        field("trigger", str_val(&recipe.trigger.kind)),
        field("valid", Value::Bool(validation.valid)),
    ];
    if let Some(ref desc) = recipe.description {
        recipe_fields.push(field("description", str_val(desc)));
    }

    let mut fields = vec![field("recipe", Value::Object(recipe_fields))];

    // Knowledge
    if !recipe.knowledge.files.is_empty() {
        fields.push(field(
            "knowledge",
            Value::Array(recipe.knowledge.files.clone()),
        ));
    }

    // Tools
    if !recipe.tools.required.is_empty() || !recipe.tools.optional.is_empty() {
        let mut tools_fields = Vec::new();
        if !recipe.tools.required.is_empty() {
            tools_fields.push(field(
                "required",
                Value::Array(recipe.tools.required.clone()),
            ));
        }
        if !recipe.tools.optional.is_empty() {
            tools_fields.push(field(
                "optional",
                Value::Array(recipe.tools.optional.clone()),
            ));
        }
        fields.push(field("tools", Value::Object(tools_fields)));
    }

    // Roles
    let role_rows: Vec<Vec<String>> = recipe
        .roles
        .iter()
        .map(|(id, r)| {
            vec![
                id.clone(),
                r.provider.clone(),
                r.mode.clone(),
                r.isolation.clone(),
            ]
        })
        .collect();
    fields.push(field(
        "roles",
        Value::Table {
            columns: vec![
                "id".into(),
                "provider".into(),
                "mode".into(),
                "isolation".into(),
            ],
            rows: role_rows,
        },
    ));

    // Steps
    let step_rows: Vec<Vec<String>> = recipe
        .steps
        .iter()
        .map(|s| vec![s.id.clone(), s.kind.clone()])
        .collect();
    fields.push(field(
        "steps",
        Value::Table {
            columns: vec!["id".into(), "kind".into()],
            rows: step_rows,
        },
    ));

    // Policy
    fields.push(field(
        "policy",
        Value::Object(vec![
            field("max_rounds", int_val(recipe.policy.max_rounds as i64)),
            field("max_ticks", int_val(recipe.policy.max_ticks as i64)),
            field("max_sessions", int_val(recipe.policy.max_sessions as i64)),
            field("merge_policy", str_val(&recipe.policy.merge_policy)),
        ]),
    ));

    (render(&fields), 0)
}

pub fn recipe_validate(id_or_path: &str, cwd: &str) -> (String, i32) {
    use planeai_core::loop_recipe_service::RecipeService;

    let project_root = resolve_project_root(cwd);
    let discovered = match RecipeService::resolve(id_or_path, Some(&project_root)) {
        Ok(d) => d,
        Err(e) => {
            return (
                emit_error(
                    &format!("invalid loop recipe: {}", e),
                    &["use schema planeai.loop.recipe.v1".into()],
                ),
                1,
            )
        }
    };

    let recipe = &discovered.recipe;
    let result = RecipeService::validate(recipe, Some(&project_root));

    if !result.valid {
        let details: Vec<String> = result.errors.clone();
        let fields = vec![
            field("error", str_val("invalid loop recipe")),
            field("path", str_val(id_or_path)),
            field("details", Value::List(details)),
            field(
                "help",
                Value::List(vec!["use schema planeai.loop.recipe.v1".into()]),
            ),
        ];
        return (render(&fields), 1);
    }

    let mut fields = vec![field(
        "recipe_validation",
        Value::Object(vec![
            field("id", str_val(&recipe.id)),
            field("valid", Value::Bool(true)),
            field("source", str_val(discovered.source.as_str())),
        ]),
    )];

    if !result.warnings.is_empty() {
        fields.push(field("warnings", Value::List(result.warnings.clone())));
    }

    (render(&fields), 0)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Walk up from cwd to find the project root (directory containing .git or .planeai).
/// Falls back to cwd if neither is found.
fn resolve_project_root(cwd: &str) -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from(cwd);
    loop {
        if path.join(".git").exists() || path.join(".planeai").exists() {
            return path;
        }
        if !path.pop() {
            return std::path::PathBuf::from(cwd);
        }
    }
}
