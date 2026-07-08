//! Durable loop data model — domain types for loop runs, sessions, events,
//! artifacts, and verifier runs.

use serde::{Deserialize, Serialize};

// ─── Loop Status ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopStatus {
    Draft,
    Running,
    Observing,
    Verifying,
    CompletedUnreviewed,
    Blocked,
    NeedsHuman,
    Stale,
    Failed,
    Cancelled,
    Approved,
    Merged,
    Cleaned,
}

impl LoopStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Running => "running",
            Self::Observing => "observing",
            Self::Verifying => "verifying",
            Self::CompletedUnreviewed => "completed_unreviewed",
            Self::Blocked => "blocked",
            Self::NeedsHuman => "needs_human",
            Self::Stale => "stale",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Approved => "approved",
            Self::Merged => "merged",
            Self::Cleaned => "cleaned",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "running" => Some(Self::Running),
            "observing" => Some(Self::Observing),
            "verifying" => Some(Self::Verifying),
            "completed_unreviewed" => Some(Self::CompletedUnreviewed),
            "blocked" => Some(Self::Blocked),
            "needs_human" => Some(Self::NeedsHuman),
            "stale" => Some(Self::Stale),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            "approved" => Some(Self::Approved),
            "merged" => Some(Self::Merged),
            "cleaned" => Some(Self::Cleaned),
            _ => None,
        }
    }
}

// ─── Loop Strategy ───────────────────────────────────────────────────────────

/// Freeform strategy identifier. Semantics are defined by the future executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopStrategy(pub String);

impl LoopStrategy {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ─── Loop Run ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRun {
    pub id: String,
    pub project_id: String,
    pub task_key: Option<String>,
    pub parent_session_id: String,
    pub strategy: LoopStrategy,
    pub goal: String,
    pub status: LoopStatus,
    pub current_round: i64,
    pub max_rounds: i64,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
    pub policy_json: Option<serde_json::Value>,
    pub budget_json: Option<serde_json::Value>,
}

// ─── Loop Session ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopSession {
    pub loop_id: String,
    pub session_id: String,
    pub role: String,
    pub round: i64,
    pub provider: Option<String>,
    pub status: String,
    pub created_at: String,
}

// ─── Loop Event ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopEvent {
    pub id: i64,
    pub loop_id: String,
    pub ts: String,
    pub kind: String,
    pub payload_json: serde_json::Value,
}

// ─── Loop Artifact ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopArtifact {
    pub id: String,
    pub loop_id: String,
    pub session_id: Option<String>,
    pub kind: String,
    pub path: Option<String>,
    pub content_json: Option<serde_json::Value>,
    pub created_at: String,
}

// ─── Verifier Run ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierRun {
    pub id: String,
    pub loop_id: String,
    pub session_id: Option<String>,
    pub verifier_type: String,
    pub name: String,
    pub command: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub output_path: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
}
