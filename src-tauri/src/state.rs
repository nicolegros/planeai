use rusqlite::Connection;
use std::sync::{Arc, Mutex};

use crate::config;
use crate::daemon_client;
use crate::file_explorer;
use crate::notify;
use crate::pty;
use crate::symphony;

pub struct DbState(pub Arc<Mutex<Connection>>);

#[derive(Default)]
pub struct ProjectOperationState(
    Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
);

impl ProjectOperationState {
    pub fn lock_for(&self, project_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.0.lock().expect("project operation lock poisoned");
        locks
            .entry(project_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }
}

pub struct PtyState(pub pty::PtyManager);
pub struct NotifyHandle(pub notify::SharedNotifyState);
pub struct ConfigState(pub Mutex<config::Config>);
pub struct FileExplorerState(pub Mutex<file_explorer::WatcherManager>);
pub struct SymphonyHandle(pub Mutex<symphony::SymphonyState>);
pub struct DaemonState(pub tokio::sync::Mutex<Option<daemon_client::DaemonClient>>);
