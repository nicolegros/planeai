//! Background session cleanup — runs after soft-delete, off the main thread.

type OpResult = Result<(), String>;
type Op1 = Box<dyn Fn(&str) -> OpResult>;
type Op2 = Box<dyn Fn(&str, &str) -> OpResult>;
type ListOp = Box<dyn Fn() -> Vec<String>>;

/// Data needed by background cleanup (gathered while locks are held).
pub struct CleanupContext {
    pub backend: String,
    pub tmux_name: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_owned: bool,
    pub project_path: Option<String>,
    pub branch: Option<String>,
    pub session_id: Option<String>,
}

/// Operations to kill a backend session (injectable for testing).
pub struct KillOps {
    pub kill_tmux: Op1,
    /// Ids of the sessions the daemon currently hosts, agent and shell tabs alike.
    pub list_daemon_sessions: ListOp,
    pub kill_daemon_session: Op1,
    pub kill_rmux_session: Op1,
}

/// Operations that cleanup can perform (injectable for testing).
pub struct CleanupOps {
    pub kill: KillOps,
    pub remove_worktree: Op2,
    pub remove_dir: Op1,
    pub delete_branch: Op2,
}

/// Kill the backend process (tmux, daemon, or rmux) for a session. Returns collected errors.
pub fn kill_backend(
    backend: &str,
    tmux_name: Option<&str>,
    session_id: Option<&str>,
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
        "daemon" => {
            if let Some(id) = session_id {
                if let Err(e) = (ops.kill_daemon_session)(id) {
                    errors.push(format!("daemon kill: {e}"));
                }
                // Shell tabs are `{id}:{index}` with sparse indices, so ask the
                // daemon which ones are alive rather than guessing a range.
                let shell_prefix = format!("{id}:");
                for tab_id in (ops.list_daemon_sessions)() {
                    if !tab_id.starts_with(&shell_prefix) {
                        continue;
                    }
                    if let Err(e) = (ops.kill_daemon_session)(&tab_id) {
                        errors.push(format!("daemon kill tab {tab_id}: {e}"));
                    }
                }
            }
        }
        planeai_rmux::BACKEND => {
            if let Some(id) = session_id {
                // Closes this session's own panes — agent and shell tabs — and
                // leaves the task workspace for any sibling agents. The workspace
                // itself is removed only when its task is deleted (ADR-0012).
                if let Err(e) = (ops.kill_rmux_session)(id) {
                    errors.push(format!("rmux close: {e}"));
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
        ctx.session_id.as_deref(),
        &ops.kill,
    );

    // Remove worktree if applicable — only for loop-managed branches (loop/...)
    // that the session created. User branches (e.g. pla-238/bdccd0c1) are never
    // owned by a loop session and must not be deleted.
    let is_loop_managed_branch = ctx
        .branch
        .as_ref()
        .map(|b| b.starts_with("loop/"))
        .unwrap_or(false);

    if let Some(ref wt_path) = ctx.worktree_path {
        if is_loop_managed_branch && ctx.worktree_owned {
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
        list_daemon_sessions: Box::new(|| list_daemon_session_ids().unwrap_or_default()),
        kill_daemon_session: Box::new(|session_id| {
            use std::io::{Read, Write};
            let app_dir = planeai_paths::app_data_dir();
            let mut stream = match planeai_ipc::connect(planeai_ipc::Channel::Daemon, &app_dir) {
                Ok(s) => s,
                Err(_) => return Ok(()), // Daemon not running — nothing to kill
            };
            stream.write_all(&[0x00]).map_err(|e| e.to_string())?;
            let req = serde_json::json!({"cmd": "kill", "session_id": session_id});
            stream
                .write_all(format!("{}\n", req).as_bytes())
                .map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; 1024];
            let _ = stream.read(&mut buf);
            Ok(())
        }),
        kill_rmux_session: Box::new(crate::rmux_ops::close_session_resources),
    }
}

/// Ask the daemon which sessions it hosts. `None` when it is unreachable.
fn list_daemon_session_ids() -> Option<Vec<String>> {
    use std::io::{BufRead, Write};
    let app_dir = planeai_paths::app_data_dir();
    let mut stream = planeai_ipc::connect(planeai_ipc::Channel::Daemon, &app_dir).ok()?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .ok()?;
    stream.write_all(&[0x00]).ok()?;
    let req = serde_json::json!({"cmd": "list"});
    stream.write_all(format!("{}\n", req).as_bytes()).ok()?;
    let mut line = String::new();
    std::io::BufReader::new(stream).read_line(&mut line).ok()?;
    let response: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let ids = response
        .get("sessions")?
        .as_array()?
        .iter()
        .filter_map(|session| session.get("session_id")?.as_str().map(String::from))
        .collect();
    Some(ids)
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
            let mut cmd = std::process::Command::new(crate::command::resolve("git"));
            cmd.args(["branch", "-D", branch]).current_dir(repo);
            cmd.env("PATH", planeai_core::command::augmented_path(&[]));
            planeai_core::command::no_window(&mut cmd);
            let output = cmd
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
            list_daemon_sessions: Box::new(Vec::new),
            kill_daemon_session: Box::new(|_| Ok(())),
            kill_rmux_session: Box::new(|_| Ok(())),
        };
        let errors = kill_backend("tmux", Some("planeai-abc"), None, &ops);
        assert!(errors.is_empty());
        KILLED.with(|k| {
            assert_eq!(k.borrow().as_slice(), &["planeai-abc"]);
        });
    }

    #[test]
    fn kill_backend_rmux_kills_the_session_by_planeai_id() {
        thread_local! {
            static KILLED: RefCell<Vec<String>> = const { RefCell::new(vec![]) };
        }
        let ops = KillOps {
            kill_tmux: Box::new(|_| Ok(())),
            list_daemon_sessions: Box::new(Vec::new),
            kill_daemon_session: Box::new(|_| {
                panic!("an rmux session must not be killed through the daemon")
            }),
            kill_rmux_session: Box::new(|id| {
                KILLED.with(|killed| killed.borrow_mut().push(id.to_string()));
                Ok(())
            }),
        };

        // One rmux session owns the whole PlaneAI session, shell tabs included.
        let errors = kill_backend(planeai_rmux::BACKEND, None, Some("sess-abc"), &ops);

        assert!(errors.is_empty());
        KILLED.with(|killed| assert_eq!(killed.borrow().as_slice(), &["sess-abc"]));
    }

    #[test]
    fn kill_backend_rmux_reports_failures() {
        let ops = KillOps {
            kill_tmux: Box::new(|_| Ok(())),
            list_daemon_sessions: Box::new(Vec::new),
            kill_daemon_session: Box::new(|_| Ok(())),
            kill_rmux_session: Box::new(|_| Err("daemon refused".to_string())),
        };

        let errors = kill_backend(planeai_rmux::BACKEND, None, Some("sess-abc"), &ops);

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("rmux close"));
    }

    #[test]
    fn kill_backend_local_is_noop() {
        let ops = KillOps {
            kill_tmux: Box::new(|_| Ok(())),
            list_daemon_sessions: Box::new(Vec::new),
            kill_daemon_session: Box::new(|_| Ok(())),
            kill_rmux_session: Box::new(|_| Ok(())),
        };
        let errors = kill_backend("local", None, None, &ops);
        assert!(errors.is_empty());
    }

    #[test]
    fn kill_backend_kills_every_live_shell_tab_for_daemon_backend() {
        thread_local! {
            static KILLED: RefCell<Vec<String>> = const { RefCell::new(vec![]) };
        }
        let ops = KillOps {
            kill_tmux: Box::new(|_| Ok(())),
            kill_rmux_session: Box::new(|_| Ok(())),
            // Shell 1 was closed earlier, so the live indices are sparse.
            list_daemon_sessions: Box::new(|| {
                [
                    "sess-abc",
                    "sess-abc:2",
                    "sess-abc:5",
                    "sess-other:2",
                    "sess-abcd:1",
                ]
                .map(String::from)
                .to_vec()
            }),
            kill_daemon_session: Box::new(|id| {
                KILLED.with(|k| k.borrow_mut().push(id.to_string()));
                Ok(())
            }),
        };
        let errors = kill_backend("daemon", None, Some("sess-abc"), &ops);
        assert!(errors.is_empty());
        KILLED.with(|k| {
            assert_eq!(
                k.borrow().as_slice(),
                &["sess-abc", "sess-abc:2", "sess-abc:5"]
            );
        });
    }

    #[test]
    fn kill_backend_without_shell_tabs_only_kills_agent() {
        thread_local! {
            static KILLED: RefCell<Vec<String>> = const { RefCell::new(vec![]) };
        }
        let ops = KillOps {
            kill_tmux: Box::new(|_| Ok(())),
            kill_rmux_session: Box::new(|_| Ok(())),
            list_daemon_sessions: Box::new(Vec::new),
            kill_daemon_session: Box::new(|id| {
                KILLED.with(|k| k.borrow_mut().push(id.to_string()));
                Ok(())
            }),
        };
        let errors = kill_backend("daemon", None, Some("sess-abc"), &ops);
        assert!(errors.is_empty());
        KILLED.with(|k| {
            let killed = k.borrow();
            assert_eq!(killed.len(), 1);
            assert_eq!(killed[0], "sess-abc");
        });
    }

    #[test]
    fn cleanup_kills_daemon_session_for_daemon_backend() {
        thread_local! {
            static KILLED: RefCell<Vec<String>> = const { RefCell::new(vec![]) };
        }
        let ops = CleanupOps {
            kill: KillOps {
                kill_tmux: Box::new(|_| Ok(())),
                kill_rmux_session: Box::new(|_| Ok(())),
                list_daemon_sessions: Box::new(Vec::new),
                kill_daemon_session: Box::new(|id| {
                    KILLED.with(|k| k.borrow_mut().push(id.to_string()));
                    Ok(())
                }),
            },
            remove_worktree: Box::new(|_, _| Ok(())),
            remove_dir: Box::new(|_| Ok(())),
            delete_branch: Box::new(|_, _| Ok(())),
        };
        let ctx = CleanupContext {
            backend: "daemon".to_string(),
            tmux_name: None,
            worktree_path: None,
            worktree_owned: false,
            project_path: None,
            branch: None,
            session_id: Some("sess-123".to_string()),
        };
        let errors = run_cleanup(&ctx, &ops);
        assert!(errors.is_empty());
        KILLED.with(|k| {
            assert_eq!(k.borrow().as_slice(), &["sess-123"]);
        });
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
                list_daemon_sessions: Box::new(Vec::new),
                kill_daemon_session: Box::new(|_| Ok(())),
                kill_rmux_session: Box::new(|_| Ok(())),
            },
            remove_worktree: Box::new(|_, _| Ok(())),
            remove_dir: Box::new(|_| Ok(())),
            delete_branch: Box::new(|_, _| Ok(())),
        };
        let ctx = CleanupContext {
            backend: "tmux".to_string(),
            tmux_name: Some("planeai-myapp-abc".to_string()),
            worktree_path: None,
            worktree_owned: false,
            project_path: None,
            branch: None,
            session_id: None,
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
                list_daemon_sessions: Box::new(Vec::new),
                kill_daemon_session: Box::new(|_| Ok(())),
                kill_rmux_session: Box::new(|_| Ok(())),
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
            worktree_owned: true,
            project_path: Some("/tmp/myapp".to_string()),
            branch: Some("loop/abcd1234/test-iv".to_string()),
            session_id: None,
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
                &[(
                    "/tmp/myapp".to_string(),
                    "loop/abcd1234/test-iv".to_string()
                )]
            );
        });
    }

    #[test]
    fn cleanup_does_not_remove_unowned_worktree() {
        let ops = CleanupOps {
            kill: KillOps {
                kill_tmux: Box::new(|_| Ok(())),
                list_daemon_sessions: Box::new(Vec::new),
                kill_daemon_session: Box::new(|_| Ok(())),
                kill_rmux_session: Box::new(|_| Ok(())),
            },
            remove_worktree: Box::new(|_, _| Err("must not remove worktree".to_string())),
            remove_dir: Box::new(|_| Err("must not remove directory".to_string())),
            delete_branch: Box::new(|_, _| Err("must not delete branch".to_string())),
        };
        let ctx = CleanupContext {
            backend: "tmux".to_string(),
            tmux_name: None,
            worktree_path: Some("/tmp/shared-worktree".to_string()),
            worktree_owned: false,
            project_path: Some("/tmp/project".to_string()),
            branch: Some("loop/shared".to_string()),
            session_id: None,
        };

        assert!(run_cleanup(&ctx, &ops).is_empty());
    }

    #[test]
    fn cleanup_collects_errors_from_failed_ops() {
        let ops = CleanupOps {
            kill: KillOps {
                kill_tmux: Box::new(|_| Err("tmux not found".to_string())),
                list_daemon_sessions: Box::new(Vec::new),
                kill_daemon_session: Box::new(|_| Ok(())),
                kill_rmux_session: Box::new(|_| Ok(())),
            },
            remove_worktree: Box::new(|_, _| Err("locked".to_string())),
            remove_dir: Box::new(|_| Err("permission denied".to_string())),
            delete_branch: Box::new(|_, _| Err("branch in use".to_string())),
        };
        let ctx = CleanupContext {
            backend: "tmux".to_string(),
            tmux_name: Some("planeai-myapp-abc".to_string()),
            worktree_path: Some("/tmp/wt/abc".to_string()),
            worktree_owned: true,
            project_path: Some("/tmp/myapp".to_string()),
            branch: Some("loop/abcd1234/feat-x".to_string()),
            session_id: None,
        };
        let errors = run_cleanup(&ctx, &ops);
        assert_eq!(errors.len(), 4);
        assert!(errors[0].contains("tmux"));
        assert!(errors[1].contains("locked"));
        assert!(errors[2].contains("permission denied"));
        assert!(errors[3].contains("branch in use"));
    }
}
