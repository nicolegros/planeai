//! Kill a session's backend process (tmux, daemon or rmux); ops are injectable for tests.

type OpResult = Result<(), String>;
type Op1 = Box<dyn Fn(&str) -> OpResult>;
type ListOp = Box<dyn Fn() -> Result<Vec<String>, String>>;

/// Operations to kill a backend session (injectable for testing).
pub struct KillOps {
    pub kill_tmux: Op1,
    /// Ids of the sessions the daemon currently hosts, agent and shell tabs alike.
    pub list_daemon_sessions: ListOp,
    pub kill_daemon_session: Op1,
    pub kill_rmux_session: Op1,
}

/// Kill the backend process (tmux, daemon, or rmux) for a session. Returns collected errors.
pub fn kill_backend(
    backend: &str,
    tmux_name: Option<&str>,
    id: &str,
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
            if let Err(e) = (ops.kill_daemon_session)(id) {
                errors.push(format!("daemon kill: {e}"));
            }
            // Shell tabs are `{id}:{index}` with sparse indices, so ask the
            // daemon which ones are alive rather than guessing a range.
            let shell_prefix = format!("{id}:");
            let live = (ops.list_daemon_sessions)().unwrap_or_else(|e| {
                errors.push(format!("daemon list: {e}"));
                Vec::new()
            });
            for tab_id in live {
                if !tab_id.starts_with(&shell_prefix) {
                    continue;
                }
                if let Err(e) = (ops.kill_daemon_session)(&tab_id) {
                    errors.push(format!("daemon kill tab {tab_id}: {e}"));
                }
            }
        }
        planeai_rmux::BACKEND => {
            // Closes this session's own panes — agent and shell tabs — and
            // leaves the task workspace for any sibling agents. The workspace
            // itself is removed only when its task is deleted (ADR-0012).
            if let Err(e) = (ops.kill_rmux_session)(id) {
                errors.push(format!("rmux close: {e}"));
            }
        }
        _ => {}
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
        list_daemon_sessions: Box::new(|| {
            crate::daemon::list_session_ids()
                .map(|ids| ids.unwrap_or_default().into_iter().collect())
        }),
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
            list_daemon_sessions: Box::new(|| Ok(Vec::new())),
            kill_daemon_session: Box::new(|_| Ok(())),
            kill_rmux_session: Box::new(|_| Ok(())),
        };
        let errors = kill_backend("tmux", Some("planeai-abc"), "sess-abc", &ops);
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
            list_daemon_sessions: Box::new(|| Ok(Vec::new())),
            kill_daemon_session: Box::new(|_| {
                panic!("an rmux session must not be killed through the daemon")
            }),
            kill_rmux_session: Box::new(|id| {
                KILLED.with(|killed| killed.borrow_mut().push(id.to_string()));
                Ok(())
            }),
        };

        // One rmux session owns the whole PlaneAI session, shell tabs included.
        let errors = kill_backend(planeai_rmux::BACKEND, None, "sess-abc", &ops);

        assert!(errors.is_empty());
        KILLED.with(|killed| assert_eq!(killed.borrow().as_slice(), &["sess-abc"]));
    }

    #[test]
    fn kill_backend_rmux_reports_failures() {
        let ops = KillOps {
            kill_tmux: Box::new(|_| Ok(())),
            list_daemon_sessions: Box::new(|| Ok(Vec::new())),
            kill_daemon_session: Box::new(|_| Ok(())),
            kill_rmux_session: Box::new(|_| Err("daemon refused".to_string())),
        };

        let errors = kill_backend(planeai_rmux::BACKEND, None, "sess-abc", &ops);

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("rmux close"));
    }

    #[test]
    fn kill_backend_local_is_noop() {
        let ops = KillOps {
            kill_tmux: Box::new(|_| Ok(())),
            list_daemon_sessions: Box::new(|| Ok(Vec::new())),
            kill_daemon_session: Box::new(|_| Ok(())),
            kill_rmux_session: Box::new(|_| Ok(())),
        };
        let errors = kill_backend("local", None, "sess-abc", &ops);
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
                Ok([
                    "sess-abc",
                    "sess-abc:2",
                    "sess-abc:5",
                    "sess-other:2",
                    "sess-abcd:1",
                ]
                .map(String::from)
                .to_vec())
            }),
            kill_daemon_session: Box::new(|id| {
                KILLED.with(|k| k.borrow_mut().push(id.to_string()));
                Ok(())
            }),
        };
        let errors = kill_backend("daemon", None, "sess-abc", &ops);
        assert!(errors.is_empty());
        KILLED.with(|k| {
            assert_eq!(
                k.borrow().as_slice(),
                &["sess-abc", "sess-abc:2", "sess-abc:5"]
            );
        });
    }

    #[test]
    fn kill_backend_reports_shell_tabs_it_could_not_list() {
        let ops = KillOps {
            kill_tmux: Box::new(|_| Ok(())),
            kill_rmux_session: Box::new(|_| Ok(())),
            list_daemon_sessions: Box::new(|| Err("timed out".to_string())),
            kill_daemon_session: Box::new(|_| Ok(())),
        };
        let errors = kill_backend("daemon", None, "sess-abc", &ops);
        assert_eq!(errors, vec!["daemon list: timed out".to_string()]);
    }

    #[test]
    fn kill_backend_without_shell_tabs_only_kills_agent() {
        thread_local! {
            static KILLED: RefCell<Vec<String>> = const { RefCell::new(vec![]) };
        }
        let ops = KillOps {
            kill_tmux: Box::new(|_| Ok(())),
            kill_rmux_session: Box::new(|_| Ok(())),
            list_daemon_sessions: Box::new(|| Ok(Vec::new())),
            kill_daemon_session: Box::new(|id| {
                KILLED.with(|k| k.borrow_mut().push(id.to_string()));
                Ok(())
            }),
        };
        let errors = kill_backend("daemon", None, "sess-abc", &ops);
        assert!(errors.is_empty());
        KILLED.with(|k| {
            let killed = k.borrow();
            assert_eq!(killed.len(), 1);
            assert_eq!(killed[0], "sess-abc");
        });
    }
}
