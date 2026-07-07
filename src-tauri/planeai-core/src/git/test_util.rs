//! Shared test utilities for git module tests.

use std::process::Command;

pub fn git(path: &std::path::Path, args: &[&str]) {
    Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
}

pub fn configure_git_identity(path: &std::path::Path) {
    git(path, &["config", "user.email", "test@test.com"]);
    git(path, &["config", "user.name", "Test"]);
}
