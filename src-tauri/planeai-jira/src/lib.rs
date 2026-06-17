pub mod auth;
pub mod client;
pub mod config;
pub mod db;
pub mod model;
pub mod repository;

#[derive(Debug)]
pub enum Error {
    Storage(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(s) => write!(f, "jira storage error: {s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Storage(e.to_string())
    }
}
