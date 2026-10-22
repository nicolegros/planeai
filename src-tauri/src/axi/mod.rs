//! AXI subcommand implementations — TOON output for agent consumption.

mod helpers;
mod home;
mod project;
mod session;
mod task;

// Re-export all public functions to maintain the same public API.
pub use home::home;
pub use project::project_ls;
pub use session::{
    session_children, session_create_output, session_ls, session_prompt,
    session_read_cursor_output, session_read_output, session_tree,
};
pub use task::{
    task_add, task_add_with_lifecycle, task_ls, task_move, task_move_with_lifecycle, task_show,
};

#[cfg(test)]
mod tests;
