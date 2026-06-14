use crate::model::{CreateParams, ListFilter, Task, UpdateParams};

/// Repository trait for task persistence.
/// Implementations can back onto SQLite, an external CLI, or any other storage.
pub trait TaskProvider {
    fn create(&self, params: CreateParams) -> Result<Task, Error>;
    fn get(&self, key: &str) -> Result<Task, Error>;
    fn list(&self, filter: ListFilter) -> Result<Vec<Task>, Error>;
    fn update(&self, key: &str, params: UpdateParams) -> Result<Task, Error>;
    fn delete(&self, key: &str) -> Result<(), Error>;
}

#[derive(Debug)]
pub enum Error {
    NotFound,
    InvalidStatus(String),
    Storage(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "task not found"),
            Self::InvalidStatus(s) => write!(f, "invalid status: {s}"),
            Self::Storage(s) => write!(f, "storage error: {s}"),
        }
    }
}

impl std::error::Error for Error {}
