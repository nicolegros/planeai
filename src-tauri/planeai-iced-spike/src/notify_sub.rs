use std::sync::{Mutex, OnceLock};

use planeai_core::notify::{
    AgentState, NotifyEvent, NotifyMessage, NotifyState, SharedNotifyState,
};

pub enum NotifyAction {
    StateChanged {
        session_id: String,
        state: AgentState,
    },
    FireNotification {
        session_id: String,
    },
    RefreshSidebar,
}

pub fn notify_ipc_stream() -> impl iced::futures::Stream<Item = NotifyMessage> {
    use tokio::sync::mpsc;
    static TX: OnceLock<mpsc::UnboundedSender<NotifyMessage>> = OnceLock::new();
    static RX: OnceLock<Mutex<Option<mpsc::UnboundedReceiver<NotifyMessage>>>> = OnceLock::new();

    let _ = TX.get_or_init(|| {
        let (tx, rx) = mpsc::unbounded_channel();
        RX.get_or_init(|| Mutex::new(Some(rx)));
        let tx2 = tx.clone();
        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            let app_dir = planeai_core::app_data_dir();
            let Ok(listener) =
                planeai_ipc::IpcListener::bind(planeai_ipc::Channel::Notify, &app_dir)
            else {
                tracing::warn!("notify: failed to bind IPC listener");
                return;
            };
            tracing::info!("notify: IPC listener started");
            loop {
                let stream = match listener.accept() {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let tx = tx2.clone();
                std::thread::spawn(move || {
                    let reader = BufReader::new(stream);
                    for line in reader.lines().map_while(Result::ok) {
                        let line = line.trim().to_string();
                        if line.is_empty() {
                            continue;
                        }
                        let msg = planeai_core::notify::parse_notify_message(&line);
                        if msg.session_id.is_empty() {
                            continue;
                        }
                        let _ = tx.send(msg);
                    }
                });
            }
        });
        tx
    });

    let rx = RX
        .get()
        .and_then(|m| m.lock().unwrap().take())
        .expect("notify IPC receiver already taken");

    tokio_stream::wrappers::UnboundedReceiverStream::new(rx)
}

pub fn dispatch_ipc_message(ns: &mut NotifyState, msg: &NotifyMessage) -> Vec<NotifyAction> {
    let mut actions = Vec::new();
    match msg.event {
        NotifyEvent::Busy => {
            ns.notify_output(&msg.session_id);
            tracing::debug!(session_id = %msg.session_id, "notify: busy (hook)");
            actions.push(NotifyAction::StateChanged {
                session_id: msg.session_id.clone(),
                state: AgentState::Busy,
            });
        }
        NotifyEvent::Notification => {
            let fired = ns.notify_stop_immediate(&msg.session_id);
            if fired {
                tracing::info!(session_id = %msg.session_id, "notify: idle (immediate)");
                actions.push(NotifyAction::FireNotification {
                    session_id: msg.session_id.clone(),
                });
                actions.push(NotifyAction::StateChanged {
                    session_id: msg.session_id.clone(),
                    state: AgentState::Idle,
                });
            }
        }
        NotifyEvent::Stop => {
            let hook_enabled = ns.get_meta(&msg.session_id).is_some_and(|m| m.hook_enabled);
            if hook_enabled {
                tracing::debug!(session_id = %msg.session_id, "notify: stop (debouncing)");
                ns.notify_stop_debounced(&msg.session_id);
            } else {
                let fired = ns.notify_stop(&msg.session_id);
                if fired {
                    tracing::info!(session_id = %msg.session_id, "notify: idle (stop, no hook)");
                    actions.push(NotifyAction::FireNotification {
                        session_id: msg.session_id.clone(),
                    });
                    actions.push(NotifyAction::StateChanged {
                        session_id: msg.session_id.clone(),
                        state: AgentState::Idle,
                    });
                }
            }
        }
        NotifyEvent::SessionCreated | NotifyEvent::SessionChanged => {
            tracing::debug!(session_id = %msg.session_id, event = ?msg.event, "notify: session event");
            actions.push(NotifyAction::RefreshSidebar);
        }
    }
    actions
}

pub fn fire_notification(notify_state: &SharedNotifyState, session_id: &str) {
    let ns = notify_state.lock().unwrap();
    let (title, body) = match ns.get_meta(session_id) {
        Some(meta) => (meta.project_name.clone(), format!("{} is ready", meta.name)),
        None => ("planeai".to_string(), "Agent is ready".to_string()),
    };
    drop(ns);
    let _ = notify_rust::Notification::new()
        .summary(&title)
        .body(&body)
        .show();
}

pub fn check_silence_and_debounce(ns: &mut NotifyState) -> Vec<String> {
    let mut to_notify = Vec::new();
    let busy = ns.busy_sessions();
    for id in busy {
        if ns.check_silence(&id) {
            to_notify.push(id);
        }
    }
    let debounced = ns.debounced_sessions();
    for id in debounced {
        if ns.check_debounce(&id) {
            to_notify.push(id);
        }
    }
    to_notify
}
