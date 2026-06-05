/// Background session cleanup — runs after soft-delete, off the main thread.

/// Data needed by background cleanup (gathered while locks are held).
pub struct CleanupContext {
    pub backend: String,
    pub tmux_name: Option<String>,
    pub worktree_path: Option<String>,
    pub project_path: Option<String>,
}

/// Operations that cleanup can perform (injectable for testing).
pub struct CleanupOps {
    pub kill_tmux: Box<dyn Fn(&str) -> Result<(), String>>,
    pub remove_worktree: Box<dyn Fn(&str, &str) -> Result<(), String>>,
    pub remove_dir: Box<dyn Fn(&str) -> Result<(), String>>,
}

/// Run background cleanup for a destroyed session. Returns collected errors.
pub fn run_cleanup(ctx: &CleanupContext, ops: &CleanupOps) -> Vec<String> {
    let mut errors = vec![];

    // Kill tmux session if tmux-backed
    if ctx.backend == "tmux" {
        if let Some(ref name) = ctx.tmux_name {
            if let Err(e) = (ops.kill_tmux)(name) {
                errors.push(format!("tmux kill: {e}"));
            }
        }
    }

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
    }

    errors
}

/// Production operations that call real tmux/git/fs commands.
pub fn real_ops() -> CleanupOps {
    CleanupOps {
        kill_tmux: Box::new(|name| {
            #[cfg(not(windows))]
            { crate::tmux::kill_session(name) }
            #[cfg(windows)]
            { Ok(()) }
        }),
        remove_worktree: Box::new(|repo, wt| crate::git::worktree_remove(repo, wt)),
        remove_dir: Box::new(|path| {
            std::fs::remove_dir_all(path).map_err(|e| e.to_string())
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn noop_ops() -> CleanupOps {
        CleanupOps {
            kill_tmux: Box::new(|_| Ok(())),
            remove_worktree: Box::new(|_, _| Ok(())),
            remove_dir: Box::new(|_| Ok(())),
        }
    }

    #[test]
    fn cleanup_does_nothing_for_direct_session() {
        let ctx = CleanupContext {
            backend: "direct".to_string(),
            tmux_name: None,
            worktree_path: None,
            project_path: None,
        };
        let errors = run_cleanup(&ctx, &noop_ops());
        assert!(errors.is_empty());
    }

    #[test]
    fn cleanup_kills_tmux_for_tmux_backend() {
        thread_local! {
            static KILLED: RefCell<Vec<String>> = RefCell::new(vec![]);
        }
        let ops = CleanupOps {
            kill_tmux: Box::new(|name| {
                KILLED.with(|k| k.borrow_mut().push(name.to_string()));
                Ok(())
            }),
            remove_worktree: Box::new(|_, _| Ok(())),
            remove_dir: Box::new(|_| Ok(())),
        };
        let ctx = CleanupContext {
            backend: "tmux".to_string(),
            tmux_name: Some("planeai-myapp-abc".to_string()),
            worktree_path: None,
            project_path: None,
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
            static WT_REMOVED: RefCell<Vec<(String, String)>> = RefCell::new(vec![]);
            static DIR_REMOVED: RefCell<Vec<String>> = RefCell::new(vec![]);
        }
        let ops = CleanupOps {
            kill_tmux: Box::new(|_| Ok(())),
            remove_worktree: Box::new(|repo, wt| {
                WT_REMOVED.with(|v| v.borrow_mut().push((repo.to_string(), wt.to_string())));
                Ok(())
            }),
            remove_dir: Box::new(|path| {
                DIR_REMOVED.with(|v| v.borrow_mut().push(path.to_string()));
                Ok(())
            }),
        };
        let ctx = CleanupContext {
            backend: "tmux".to_string(),
            tmux_name: Some("planeai-myapp-abc".to_string()),
            worktree_path: Some("/tmp/wt/abc".to_string()),
            project_path: Some("/tmp/myapp".to_string()),
        };
        let errors = run_cleanup(&ctx, &ops);
        assert!(errors.is_empty());
        WT_REMOVED.with(|v| {
            assert_eq!(v.borrow().as_slice(), &[("/tmp/myapp".to_string(), "/tmp/wt/abc".to_string())]);
        });
        DIR_REMOVED.with(|v| {
            assert_eq!(v.borrow().as_slice(), &["/tmp/wt/abc"]);
        });
    }

    #[test]
    fn cleanup_collects_errors_from_failed_ops() {
        let ops = CleanupOps {
            kill_tmux: Box::new(|_| Err("tmux not found".to_string())),
            remove_worktree: Box::new(|_, _| Err("locked".to_string())),
            remove_dir: Box::new(|_| Err("permission denied".to_string())),
        };
        let ctx = CleanupContext {
            backend: "tmux".to_string(),
            tmux_name: Some("planeai-myapp-abc".to_string()),
            worktree_path: Some("/tmp/wt/abc".to_string()),
            project_path: Some("/tmp/myapp".to_string()),
        };
        let errors = run_cleanup(&ctx, &ops);
        assert_eq!(errors.len(), 3);
        assert!(errors[0].contains("tmux"));
        assert!(errors[1].contains("locked"));
        assert!(errors[2].contains("permission denied"));
    }
}

