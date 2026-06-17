use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Todo,
    InProgress,
    InReview,
    Done,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::InReview => "in_review",
            Self::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "todo" => Some(Self::Todo),
            "in_progress" => Some(Self::InProgress),
            "in_review" => Some(Self::InReview),
            "done" => Some(Self::Done),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub status: Status,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub parent_key: Option<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct CreateParams {
    pub title: String,
    pub description: String,
    pub status: Option<Status>,
    pub priority: i32,
    pub parent_key: Option<String>,
    pub blocked_by: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateParams {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<Status>,
    pub priority: Option<i32>,
    pub parent_key: Option<Option<String>>,
    pub blocked_by: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct ListFilter {
    pub status: Option<Status>,
    pub exclude_status: Option<Status>,
    pub tags: Vec<String>,
    pub parent_key: Option<Option<String>>,
}
