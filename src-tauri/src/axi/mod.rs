//! AXI subcommand implementations — TOON output for agent consumption.

mod helpers;
mod home;
mod loop_cmds;
mod project;
mod session;
mod task;

// Re-export all public functions to maintain the same public API.
pub(crate) use helpers::emit_error;
pub use home::home;
pub use loop_cmds::{
    loop_create, loop_handoff_path, loop_handoff_record, loop_observe, loop_stop, loop_tick,
    loop_tree, loop_verify,
};
pub use project::project_ls;
pub use session::{
    session_children, session_create_output, session_ls, session_prompt,
    session_read_cursor_output, session_read_output, session_tree,
};
pub use task::{
    task_add, task_add_with_lifecycle, task_ls, task_move, task_move_with_lifecycle, task_show,
};

// ─── Recipe Commands (delegated to axi_recipe module) ────────────────────────

pub use crate::axi_recipe::{recipe_ls, recipe_show, recipe_validate};

#[cfg(test)]
mod tests;
