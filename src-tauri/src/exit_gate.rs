//! Who owns the plugin shutdown as the app exits or installs an update, so a quit is never lost
//! and plugins are never started once the app is going away.

use std::sync::Mutex;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Running,
    /// An update install stopped the plugins and may still give them back.
    UpdateHold {
        quit_requested: bool,
    },
    /// The update installed; the requested restart is the exit.
    Restarting,
    ShuttingDown,
    ExitPermitted,
}

/// What `ExitRequested` must do.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExitRequest {
    Allow,
    /// Prevent this exit; the owner of the shutdown exits the app later.
    Prevent,
    /// Prevent this exit, stop the plugins, then exit.
    StartShutdown,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FailedUpdate {
    Restore,
    /// A quit arrived during the install: stop the plugins and exit instead.
    Quit,
    /// The hold already ended, or the app is exiting.
    NotHeld,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FinishedUpdate {
    Restart,
    /// A quit arrived during the install: stop the plugins and exit instead.
    Quit,
    /// The hold already ended, or the app is exiting.
    NotHeld,
}

#[derive(Default)]
pub(crate) struct ExitGate(Mutex<State>);

impl ExitGate {
    pub(crate) fn is_running(&self) -> bool {
        *self.lock() == State::Running
    }

    pub(crate) fn request_exit(&self) -> ExitRequest {
        let mut state = self.lock();
        match *state {
            State::Running => {
                *state = State::ShuttingDown;
                ExitRequest::StartShutdown
            }
            State::UpdateHold { .. } => {
                *state = State::UpdateHold {
                    quit_requested: true,
                };
                ExitRequest::Prevent
            }
            State::Restarting | State::ShuttingDown => ExitRequest::Prevent,
            State::ExitPermitted => ExitRequest::Allow,
        }
    }

    /// For `RunEvent::Exit`, which cannot be prevented: whether plugins still need stopping.
    pub(crate) fn begin_final_exit(&self) -> bool {
        let mut state = self.lock();
        if *state == State::ExitPermitted {
            return false;
        }
        *state = State::ShuttingDown;
        true
    }

    pub(crate) fn begin_update(&self) -> bool {
        let mut state = self.lock();
        if *state != State::Running {
            return false;
        }
        *state = State::UpdateHold {
            quit_requested: false,
        };
        true
    }

    pub(crate) fn fail_update(&self) -> FailedUpdate {
        let mut state = self.lock();
        let State::UpdateHold { quit_requested } = *state else {
            return FailedUpdate::NotHeld;
        };
        if quit_requested {
            *state = State::ShuttingDown;
            return FailedUpdate::Quit;
        }
        *state = State::Running;
        FailedUpdate::Restore
    }

    pub(crate) fn finish_update(&self) -> FinishedUpdate {
        let mut state = self.lock();
        let State::UpdateHold { quit_requested } = *state else {
            return FinishedUpdate::NotHeld;
        };
        if quit_requested {
            *state = State::ShuttingDown;
            return FinishedUpdate::Quit;
        }
        *state = State::Restarting;
        FinishedUpdate::Restart
    }

    pub(crate) fn permit_exit(&self) {
        *self.lock() = State::ExitPermitted;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quit_starts_one_shutdown_and_exits_once_permitted() {
        let gate = ExitGate::default();

        assert_eq!(gate.request_exit(), ExitRequest::StartShutdown);
        assert_eq!(gate.request_exit(), ExitRequest::Prevent);
        assert!(!gate.is_running());
        assert!(!gate.begin_update(), "no update may start during a quit");
        gate.permit_exit();
        assert_eq!(gate.request_exit(), ExitRequest::Allow);
        assert!(!gate.begin_final_exit());
    }

    #[test]
    fn a_quit_during_a_failed_update_exits_instead_of_restoring() {
        let gate = ExitGate::default();
        assert!(gate.begin_update());

        assert_eq!(gate.request_exit(), ExitRequest::Prevent);
        assert_eq!(gate.fail_update(), FailedUpdate::Quit);
        assert!(!gate.is_running());
    }

    #[test]
    fn a_quit_during_a_successful_update_exits_instead_of_restarting() {
        let gate = ExitGate::default();
        assert!(gate.begin_update());

        assert_eq!(gate.request_exit(), ExitRequest::Prevent);
        assert_eq!(gate.finish_update(), FinishedUpdate::Quit);
    }

    #[test]
    fn a_failed_update_without_a_quit_gives_the_plugins_back() {
        let gate = ExitGate::default();
        assert!(gate.begin_update());
        assert!(!gate.begin_update());

        assert_eq!(gate.fail_update(), FailedUpdate::Restore);
        assert!(gate.is_running());
        assert_eq!(gate.fail_update(), FailedUpdate::NotHeld);
    }

    #[test]
    fn a_successful_update_turns_a_later_quit_into_the_restart() {
        let gate = ExitGate::default();
        assert!(gate.begin_update());

        assert_eq!(gate.finish_update(), FinishedUpdate::Restart);
        assert!(!gate.is_running());
        assert_eq!(gate.request_exit(), ExitRequest::Prevent);
        assert_eq!(gate.fail_update(), FailedUpdate::NotHeld);
        assert!(gate.begin_final_exit());
    }

    #[test]
    fn the_final_exit_ends_an_update_hold_and_refuses_later_updates() {
        let gate = ExitGate::default();
        assert!(gate.begin_update());

        assert!(gate.begin_final_exit());
        assert_eq!(gate.fail_update(), FailedUpdate::NotHeld);
        assert_eq!(gate.finish_update(), FinishedUpdate::NotHeld);
        assert!(!gate.begin_update());
    }
}
