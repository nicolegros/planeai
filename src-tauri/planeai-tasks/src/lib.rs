pub mod model;
pub mod provider;
pub mod sqlite;

use model::{ListFilter, Status, Task, UpdateParams};
use provider::TaskProvider;

/// If the task has a parent and all siblings are Done, mark the parent Done.
/// Returns the parent key if auto-completed, None otherwise.
pub fn try_auto_complete_parent(provider: &dyn TaskProvider, task: &Task) -> Option<String> {
    let parent_key = task.parent_key.as_ref()?;
    // Skip if parent is already done
    if let Ok(parent) = provider.get(parent_key) {
        if parent.status == Status::Done {
            return None;
        }
    }
    let siblings = match provider.list(ListFilter {
        parent_key: Some(Some(parent_key.clone())),
        ..Default::default()
    }) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(parent_key, error = %e, "failed to list siblings for auto-complete");
            return None;
        }
    };
    if siblings.is_empty() || siblings.iter().any(|t| t.status != Status::Done) {
        return None;
    }
    if let Err(e) = provider.update(
        parent_key,
        UpdateParams {
            status: Some(Status::Done),
            ..Default::default()
        },
    ) {
        tracing::warn!(parent_key, error = %e, "failed to auto-complete parent");
        return None;
    }
    Some(parent_key.clone())
}
