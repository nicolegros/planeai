pub mod auth;
pub mod client;
pub mod config;
pub mod db;
pub mod model;
pub mod repository;
pub mod sync;
pub mod writeback;

#[cfg(test)]
pub(crate) mod test_support;

pub use sync::{JiraSync, SyncListener, SyncResult};
pub use writeback::{JiraWriteback, WritebackAction};

#[derive(Debug)]
pub enum Error {
    Storage(String),
    Client(String),
    TaskProvider(String),
    Cancelled,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(s) => write!(f, "jira storage error: {s}"),
            Self::Client(s) => write!(f, "jira client error: {s}"),
            Self::TaskProvider(s) => write!(f, "task provider error: {s}"),
            Self::Cancelled => write!(f, "jira sync cancelled"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Storage(e.to_string())
    }
}

impl From<crate::client::Error> for Error {
    fn from(e: crate::client::Error) -> Self {
        Self::Client(e.to_string())
    }
}

impl From<planeai_tasks::provider::Error> for Error {
    fn from(e: planeai_tasks::provider::Error) -> Self {
        Self::TaskProvider(e.to_string())
    }
}
