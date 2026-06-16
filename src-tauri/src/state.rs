use rusqlite::Connection;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::config;
use crate::file_explorer;
use crate::notify;
use crate::pty;
use crate::symphony;

pub struct DbState(pub Arc<Mutex<Connection>>);
pub struct PtyState(pub pty::PtyManager);
pub struct NotifyHandle(pub notify::SharedNotifyState);
pub struct ConfigState(pub Mutex<config::Config>);
pub struct FileExplorerState(pub Mutex<file_explorer::WatcherManager>);
pub struct SymphonyHandle(pub Mutex<symphony::SymphonyState>);
/// Tracks session IDs that are managed by the daemon (not local PTY).
pub struct DaemonSessions(pub Mutex<HashSet<String>>);
