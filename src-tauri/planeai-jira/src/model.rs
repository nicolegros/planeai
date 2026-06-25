use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Synced,
    Stale,
}

impl SyncStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Synced => "synced",
            Self::Stale => "stale",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "synced" => Some(Self::Synced),
            "stale" => Some(Self::Stale),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JiraIssue {
    pub issue_key: String,
    /// The config source name this issue belongs to (stored as `jira_project` in DB for compat).
    pub source_name: String,
    pub summary: String,
    pub description: String,
    pub status: String,
    pub priority: Option<String>,
    pub labels: Vec<String>,
    pub sync_status: SyncStatus,
    pub last_synced_at: DateTime<Utc>,
}
