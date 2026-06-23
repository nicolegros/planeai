//! Background session cleanup — runs after soft-delete, off the main thread.

type OpResult = Result<(), String>;
type Op1 = Box<dyn Fn(&str) -> OpResult>;
type Op2 = Box<dyn Fn(&str, &str) -> OpResult>;

/// Data needed by background cleanup (gathered while locks are held).
pub struct CleanupContext {
    pub backend: String,
    pub tmux_name: Option<String>,
    pub worktree_path: Option<String>,
    pub project_path: Option<String>,
    pub branch: Option<String>,
    pub session_id: Option<String>,
    pub tab_count: i64,
}

/// Operations to kill a backend session (injectable for testing).
pub struct KillOps {
    pub kill_tmux: Op1,
}

/// Operations that cleanup can perform (injectable for testing).
pub struct CleanupOps {
    pub kill: KillOps,
    pub remove_worktree: Op2,
    pub remove_dir: Op1,
    pub delete_branch: Op2,
}

/// Kill the backend process (tmux or daemon) for a session. Returns collected errors.
/// For daemon-backend sessions with tab_count > 1, also kills shell tabs
/// ({session_id}:1, {session_id}:2, ...).
pub fn kill_backend(
    backend: &str,
    tmux_name: Option<&str>,
    ops: &KillOps,
) -> Vec<String> {
    let mut errors = vec![];
    match backend {
        "tmux" => {
            if let Some(name) = tmux_name {
                if let Err(e) = (ops.kill_tmux)(name) {
                    errors.push(format!("tmux kill: {e}"));
                }
            }
        }
        _ => {}
    }
    errors
}

/// Run background cleanup for a destroyed session. Returns collected errors.
pub fn run_cleanup(ctx: &CleanupContext, ops: &CleanupOps) -> Vec<String> {
    let mut errors = kill_backend(
        &ctx.backend,
        ctx.tmux_name.as_deref(),
        &ops.kill,
    );

    // Remove worktree if applicable
    if let Some(ref wt_path) = ctx.worktree_path {
        if let Some(ref project_path) = ctx.project_path {
            if let Err(e) = (ops.remove_worktree)(project_path, wt_path) {
                errors.push(format!("worktree remove: {e}"));
            }
        }
        if let Err(e) = (ops.remove_dir)(wt_path) {
            errors.push(format!("remove dir: {e}"));
        }
        // Delete the branch that was created with the worktree
        if let (Some(ref project_path), Some(ref branch)) = (&ctx.project_path, &ctx.branch) {
            if let Err(e) = (ops.delete_branch)(project_path, branch) {
                errors.push(format!("branch delete: {e}"));
            }
        }
    }

    errors
}

/// Production kill operations.
pub fn real_kill_ops() -> KillOps {
    KillOps {
        kill_tmux: Box::new(|_name| {
            #[cfg(not(windows))]
            {
                crate::tmux::kill_session(_name)
            }
            #[cfg(windows)]
            {
                Ok(())
            }
        }),
    }
}

/// Production operations that call real tmux/git/fs commands.
pub fn real_ops() -> CleanupOps {
    CleanupOps {
        kill: real_kill_ops(),
        remove_worktree: Box::new(crate::git::worktree_remove),
        remove_dir: Box::new(|path| match std::fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }),
        delete_branch: Box::new(|repo, branch| {
            let output = std::process::Command::new("git")
                .args(["branch", "-D", branch])
                .current_dir(repo)
                .output()
                .map_err(|e| format!("failed to run git: {e}"))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.contains("not found") {
                    return Err(stderr.to_string());
                }
            }
            Ok(())
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn kill_backend_tmux() {
        thread_local! {
            static KILLED: RefCell<Vec<String>> = const { RefCell::new(vec![]) };
        }
        let ops = KillOps {
            kill_tmux: Box::new(|name| {
                KILLED.with(|k| k.borrow_mut().push(name.to_string()));
                Ok(())
            }),
        };
        let errors = kill_backend("tmux", Some("planeai-abc"), &ops);
        assert!(errors.is_empty());
        KILLED.with(|k| {
            assert_eq!(k.borrow().as_slice(), &["planeai-abc"]);
        });
    }

    #[test]
    fn kill_backend_local_is_noop() {
        let ops = KillOps {
            kill_tmux: Box::new(|_| Ok(())),
        };
        let errors = kill_backend("local", None, &ops);
        assert!(errors.is_empty());
    }

    #[test]
    fn cleanup_kills_tmux_for_tmux_backend() {
        thread_local! {
            static KILLED: RefCell<Vec<String>> = const { RefCell::new(vec![]) };
        }
        let ops = CleanupOps {
            kill: KillOps {
                kill_tmux: Box::new(|name| {
                    KILLED.with(|k| k.borrow_mut().push(name.to_string()));
                    Ok(())
                }),
            },
            remove_worktree: Box::new(|_, _| Ok(())),
            remove_dir: Box::new(|_| Ok(())),
            delete_branch: Box::new(|_, _| Ok(())),
        };
        let ctx = CleanupContext {
            backend: "tmux".to_string(),
            tmux_name: Some("planeai-myapp-abc".to_string()),
            worktree_path: None,
            project_path: None,
            branch: None,
            session_id: None,
            tab_count: 1,
        };
        let errors = run_cleanup(&ctx, &ops);
        assert!(errors.is_empty());
        KILLED.with(|k| {
            assert_eq!(k.borrow().as_slice(), &["planeai-myapp-abc"]);
        });
    }

    #[test]
    fn cleanup_removes_worktree_and_dir() {
        thread_local! {
            static WT_REMOVED: RefCell<Vec<(String, String)>> = const { RefCell::new(vec![]) };
            static DIR_REMOVED: RefCell<Vec<String>> = const { RefCell::new(vec![]) };
            static BR_DELETED: RefCell<Vec<(String, String)>> = const { RefCell::new(vec![]) };
        }
        let ops = CleanupOps {
            kill: KillOps {
                kill_tmux: Box::new(|_| Ok(())),
            },
            remove_worktree: Box::new(|repo, wt| {
                WT_REMOVED.with(|v| v.borrow_mut().push((repo.to_string(), wt.to_string())));
                Ok(())
            }),
            remove_dir: Box::new(|path| {
                DIR_REMOVED.with(|v| v.borrow_mut().push(path.to_string()));
                Ok(())
            }),
            delete_branch: Box::new(|repo, branch| {
                BR_DELETED.with(|v| v.borrow_mut().push((repo.to_string(), branch.to_string())));
                Ok(())
            }),
        };
        let ctx = CleanupContext {
            backend: "tmux".to_string(),
            tmux_name: Some("planeai-myapp-abc".to_string()),
            worktree_path: Some("/tmp/wt/abc".to_string()),
            project_path: Some("/tmp/myapp".to_string()),
            branch: Some("test-iv".to_string()),
            session_id: None,
            tab_count: 1,
        };
        let errors = run_cleanup(&ctx, &ops);
        assert!(errors.is_empty());
        WT_REMOVED.with(|v| {
            assert_eq!(
                v.borrow().as_slice(),
                &[("/tmp/myapp".to_string(), "/tmp/wt/abc".to_string())]
            );
        });
        DIR_REMOVED.with(|v| {
            assert_eq!(v.borrow().as_slice(), &["/tmp/wt/abc"]);
        });
        BR_DELETED.with(|v| {
            assert_eq!(
                v.borrow().as_slice(),
                &[("/tmp/myapp".to_string(), "test-iv".to_string())]
            );
        });
    }

    #[test]
    fn cleanup_collects_errors_from_failed_ops() {
        let ops = CleanupOps {
            kill: KillOps {
                kill_tmux: Box::new(|_| Err("tmux not found".to_string())),
            },
            remove_worktree: Box::new(|_, _| Err("locked".to_string())),
            remove_dir: Box::new(|_| Err("permission denied".to_string())),
            delete_branch: Box::new(|_, _| Err("branch in use".to_string())),
        };
        let ctx = CleanupContext {
            backend: "tmux".to_string(),
            tmux_name: Some("planeai-myapp-abc".to_string()),
            worktree_path: Some("/tmp/wt/abc".to_string()),
            project_path: Some("/tmp/myapp".to_string()),
            branch: Some("feat-x".to_string()),
            session_id: None,
            tab_count: 1,
        };
        let errors = run_cleanup(&ctx, &ops);
        assert_eq!(errors.len(), 4);
        assert!(errors[0].contains("tmux"));
        assert!(errors[1].contains("locked"));
        assert!(errors[2].contains("permission denied"));
        assert!(errors[3].contains("branch in use"));
    }
}
