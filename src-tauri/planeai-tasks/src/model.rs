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
    #[serde(default = "default_base_branch")]
    pub base_branch: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// When the user last looked at the task.
    #[serde(default)]
    pub last_visited_at: Option<DateTime<Utc>>,
    /// When an agent working on the task last finished a turn, asked for attention, or exited.
    #[serde(default)]
    pub last_activity_at: Option<DateTime<Utc>>,
    #[serde(flatten, default)]
    pub snooze: Snooze,
    /// Bumped by every write to `snooze`; see `StoredSnooze`.
    #[serde(default)]
    pub snooze_rev: i64,
}

impl Task {
    /// Hidden from the active lists until it wakes; a Done task is never snoozed.
    pub fn is_snoozed(&self, now: DateTime<Utc>) -> bool {
        self.status != Status::Done && self.snooze.is_snoozed(now)
    }
}

/// When a snoozed task wakes. The user's snooze writes `snoozed_until` and `snoozed_at`; an agent
/// asking for attention or exiting writes `woke_at`; waking it by hand, or Done, clears all three.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snooze {
    #[serde(default)]
    pub snoozed_until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub snoozed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub woke_at: Option<DateTime<Utc>>,
}

/// A snooze as stored, with its revision: every write to the snooze bumps `snooze_rev`, so a reader
/// holding two copies of a task's snooze keeps the one with the higher revision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct StoredSnooze {
    #[serde(flatten)]
    pub snooze: Snooze,
    pub snooze_rev: i64,
}

impl Snooze {
    pub fn is_snoozed(&self, now: DateTime<Utc>) -> bool {
        self.woke_at.is_none() && self.snoozed_until.is_some_and(|until| now < until)
    }
}

pub const DEFAULT_BASE_BRANCH: &str = "main";

fn default_base_branch() -> String {
    DEFAULT_BASE_BRANCH.to_string()
}

#[derive(Debug, Clone)]
pub struct CreateParams {
    pub key: Option<String>,
    pub title: String,
    pub description: String,
    pub status: Option<Status>,
    pub priority: i32,
    pub parent_key: Option<String>,
    pub blocked_by: Vec<String>,
    pub tags: Vec<String>,
    pub base_branch: String,
}

impl Default for CreateParams {
    fn default() -> Self {
        Self {
            key: None,
            title: String::new(),
            description: String::new(),
            status: None,
            priority: 0,
            parent_key: None,
            blocked_by: Vec::new(),
            tags: Vec::new(),
            base_branch: DEFAULT_BASE_BRANCH.to_string(),
        }
    }
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
    pub base_branch: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ListFilter {
    pub status: Option<Status>,
    pub exclude_status: Option<Status>,
    pub tags: Vec<String>,
    pub parent_key: Option<Option<String>>,
}
