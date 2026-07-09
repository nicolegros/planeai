//! Structured handoff schema for loop engineering.
//!
//! Provides a versioned, validated schema for agents to report loop completion,
//! blockers, risks, tests, and evidence in a machine-readable format.

use serde::{Deserialize, Serialize};

// ─── Schema Version ──────────────────────────────────────────────────────────

pub const SCHEMA_V1: &str = "planeai.handoff.v1";

// ─── Handoff Status ──────────────────────────────────────────────────────────

/// The outcome status reported by the agent in the handoff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffStatus {
    Completed,
    Blocked,
    NeedsHuman,
    Failed,
}

impl HandoffStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::NeedsHuman => "needs_human",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "completed" => Some(Self::Completed),
            "blocked" => Some(Self::Blocked),
            "needs_human" => Some(Self::NeedsHuman),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

// ─── Evidence Source ─────────────────────────────────────────────────────────

/// How the evidence was obtained. Verifiers should not trust `claimed` evidence
/// as proof — it indicates the agent asserts the result without direct observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    /// The agent directly ran the command and observed the result.
    Direct,
    /// Another tool or agent reported the result (e.g., CI webhook).
    Proxy,
    /// The agent claims the result without direct observation.
    Claimed,
    /// The evidence could not be obtained (e.g., test infra was down).
    Blocked,
}

impl EvidenceSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Proxy => "proxy",
            Self::Claimed => "claimed",
            Self::Blocked => "blocked",
        }
    }
}

// ─── Evidence ────────────────────────────────────────────────────────────────

/// A single piece of evidence (e.g., a test run, a lint pass, a build result).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// The category of evidence (e.g., "test", "lint", "build", "typecheck").
    pub kind: String,
    /// Human-readable name or command that was run.
    pub name: String,
    /// The result of the evidence (e.g., "pass", "fail", "error").
    pub result: String,
    /// How the evidence was obtained.
    pub source: EvidenceSource,
    /// Optional path to detailed output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
}

// ─── Handoff V1 ──────────────────────────────────────────────────────────────

/// The structured handoff document (v1 schema).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffV1 {
    /// Must be "planeai.handoff.v1".
    pub schema: String,
    /// The loop this handoff belongs to.
    pub loop_id: String,
    /// The session that produced this handoff.
    pub session_id: String,
    /// Optional task key for cross-referencing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_key: Option<String>,
    /// The outcome status.
    pub status: HandoffStatus,
    /// Git branch name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Git commit hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Files changed in this work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_files: Vec<String>,
    /// Human-readable summary of what changed.
    pub summary: String,
    /// Known risks or concerns.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risks: Vec<String>,
    /// Suggested next actions for the loop orchestrator or next session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<String>,
    /// Evidence items (tests, lints, builds).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Evidence>,
}

// ─── Parser + Validator ──────────────────────────────────────────────────────

/// Errors that can occur when parsing or validating a handoff file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffError {
    /// JSON parsing failed.
    ParseError(String),
    /// Schema field is missing or has an unknown version.
    UnknownSchema(String),
    /// A required field is missing or invalid.
    MissingField(String),
    /// The loop_id in the file does not match the expected loop_id.
    LoopIdMismatch { expected: String, actual: String },
    /// The session_id in the file does not match the expected session_id.
    SessionIdMismatch { expected: String, actual: String },
    /// The handoff status is invalid.
    InvalidStatus(String),
}

impl std::fmt::Display for HandoffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseError(e) => write!(f, "failed to parse handoff JSON: {e}"),
            Self::UnknownSchema(s) => {
                write!(f, "unknown handoff schema: {s} (expected {SCHEMA_V1})")
            }
            Self::MissingField(field) => write!(f, "missing required field: {field}"),
            Self::LoopIdMismatch { expected, actual } => {
                write!(f, "loop_id mismatch: expected {expected}, got {actual}")
            }
            Self::SessionIdMismatch { expected, actual } => {
                write!(f, "session_id mismatch: expected {expected}, got {actual}")
            }
            Self::InvalidStatus(s) => write!(f, "invalid handoff status: {s}"),
        }
    }
}

impl std::error::Error for HandoffError {}

/// Parse a handoff JSON string into a validated HandoffV1 struct.
///
/// This performs structural validation only. It does NOT check whether the
/// loop_id/session_id exist in the database — that is the caller's responsibility.
pub fn parse_handoff(json: &str) -> Result<HandoffV1, Vec<HandoffError>> {
    // First, check if the schema field is present and correct before full deserialization.
    // This gives a better error for unknown schemas vs. a generic parse error.
    let raw: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => return Err(vec![HandoffError::ParseError(e.to_string())]),
    };

    let mut errors = Vec::new();

    // Validate schema version
    match raw.get("schema").and_then(|v| v.as_str()) {
        Some(SCHEMA_V1) => {}
        Some(other) => errors.push(HandoffError::UnknownSchema(other.to_string())),
        None => errors.push(HandoffError::MissingField("schema".to_string())),
    }

    // Validate required fields exist and are non-blank
    let is_present_and_nonempty = |key: &str| -> bool {
        raw.get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty())
    };

    if !is_present_and_nonempty("loop_id") {
        errors.push(HandoffError::MissingField("loop_id".to_string()));
    }
    if !is_present_and_nonempty("session_id") {
        errors.push(HandoffError::MissingField("session_id".to_string()));
    }
    if !is_present_and_nonempty("summary") {
        errors.push(HandoffError::MissingField("summary".to_string()));
    }

    // Validate status field
    match raw.get("status").and_then(|v| v.as_str()) {
        Some(s) if HandoffStatus::parse(s).is_some() => {}
        Some(s) => errors.push(HandoffError::InvalidStatus(s.to_string())),
        None => errors.push(HandoffError::MissingField("status".to_string())),
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    // If structural validation passes, attempt full deserialization
    match serde_json::from_value::<HandoffV1>(raw) {
        Ok(handoff) => Ok(handoff),
        Err(e) => Err(vec![HandoffError::ParseError(e.to_string())]),
    }
}

/// Validate that the handoff's loop_id and session_id match the expected values.
pub fn validate_ids(
    handoff: &HandoffV1,
    expected_loop_id: &str,
    expected_session_id: &str,
) -> Result<(), Vec<HandoffError>> {
    let mut errors = Vec::new();

    if handoff.loop_id != expected_loop_id {
        errors.push(HandoffError::LoopIdMismatch {
            expected: expected_loop_id.to_string(),
            actual: handoff.loop_id.clone(),
        });
    }
    if handoff.session_id != expected_session_id {
        errors.push(HandoffError::SessionIdMismatch {
            expected: expected_session_id.to_string(),
            actual: handoff.session_id.clone(),
        });
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_handoff_json() -> String {
        serde_json::json!({
            "schema": "planeai.handoff.v1",
            "loop_id": "loop_abc123",
            "session_id": "4f3a91c2-0000-0000-0000-000000000000",
            "task_key": "PLA-201",
            "status": "completed",
            "branch": "pla-201/shortid",
            "commit": "abc123def456",
            "changed_files": ["src/main.rs", "src/lib.rs"],
            "summary": "Implemented the feature",
            "risks": ["May break on Windows"],
            "next_actions": ["Run integration tests"],
            "evidence": [{
                "kind": "test",
                "name": "cargo test -p planeai-core",
                "result": "pass",
                "source": "direct",
                "output_path": ".planeai/loops/loop_abc123/verifier/cargo-test.log"
            }]
        })
        .to_string()
    }

    #[test]
    fn parse_valid_handoff() {
        let json = valid_handoff_json();
        let handoff = parse_handoff(&json).unwrap();

        assert_eq!(handoff.schema, SCHEMA_V1);
        assert_eq!(handoff.loop_id, "loop_abc123");
        assert_eq!(handoff.session_id, "4f3a91c2-0000-0000-0000-000000000000");
        assert_eq!(handoff.task_key, Some("PLA-201".to_string()));
        assert_eq!(handoff.status, HandoffStatus::Completed);
        assert_eq!(handoff.branch, Some("pla-201/shortid".to_string()));
        assert_eq!(handoff.commit, Some("abc123def456".to_string()));
        assert_eq!(handoff.changed_files, vec!["src/main.rs", "src/lib.rs"]);
        assert_eq!(handoff.summary, "Implemented the feature");
        assert_eq!(handoff.risks, vec!["May break on Windows"]);
        assert_eq!(handoff.next_actions, vec!["Run integration tests"]);
        assert_eq!(handoff.evidence.len(), 1);
        assert_eq!(handoff.evidence[0].kind, "test");
        assert_eq!(handoff.evidence[0].source, EvidenceSource::Direct);
    }

    #[test]
    fn parse_minimal_valid_handoff() {
        let json = serde_json::json!({
            "schema": "planeai.handoff.v1",
            "loop_id": "loop_minimal",
            "session_id": "sess-1",
            "status": "blocked",
            "summary": "Cannot proceed"
        })
        .to_string();

        let handoff = parse_handoff(&json).unwrap();
        assert_eq!(handoff.status, HandoffStatus::Blocked);
        assert_eq!(handoff.summary, "Cannot proceed");
        assert!(handoff.changed_files.is_empty());
        assert!(handoff.evidence.is_empty());
        assert!(handoff.risks.is_empty());
        assert!(handoff.next_actions.is_empty());
        assert_eq!(handoff.branch, None);
        assert_eq!(handoff.commit, None);
        assert_eq!(handoff.task_key, None);
    }

    #[test]
    fn parse_missing_required_field_summary() {
        let json = serde_json::json!({
            "schema": "planeai.handoff.v1",
            "loop_id": "loop_1",
            "session_id": "sess-1",
            "status": "completed"
        })
        .to_string();

        let errors = parse_handoff(&json).unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, HandoffError::MissingField(f) if f == "summary")));
    }

    #[test]
    fn parse_missing_multiple_required_fields() {
        let json = serde_json::json!({
            "schema": "planeai.handoff.v1"
        })
        .to_string();

        let errors = parse_handoff(&json).unwrap_err();
        assert!(errors.len() >= 3); // loop_id, session_id, summary, status
    }

    #[test]
    fn parse_unknown_schema_version() {
        let json = serde_json::json!({
            "schema": "planeai.handoff.v99",
            "loop_id": "loop_1",
            "session_id": "sess-1",
            "status": "completed",
            "summary": "Done"
        })
        .to_string();

        let errors = parse_handoff(&json).unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, HandoffError::UnknownSchema(s) if s == "planeai.handoff.v99")));
    }

    #[test]
    fn parse_missing_schema_field() {
        let json = serde_json::json!({
            "loop_id": "loop_1",
            "session_id": "sess-1",
            "status": "completed",
            "summary": "Done"
        })
        .to_string();

        let errors = parse_handoff(&json).unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, HandoffError::MissingField(f) if f == "schema")));
    }

    #[test]
    fn parse_invalid_status() {
        let json = serde_json::json!({
            "schema": "planeai.handoff.v1",
            "loop_id": "loop_1",
            "session_id": "sess-1",
            "status": "done",
            "summary": "Done"
        })
        .to_string();

        let errors = parse_handoff(&json).unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, HandoffError::InvalidStatus(s) if s == "done")));
    }

    #[test]
    fn parse_invalid_json() {
        let errors = parse_handoff("not json at all").unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, HandoffError::ParseError(_))));
    }

    #[test]
    fn validate_ids_match() {
        let json = valid_handoff_json();
        let handoff = parse_handoff(&json).unwrap();
        validate_ids(
            &handoff,
            "loop_abc123",
            "4f3a91c2-0000-0000-0000-000000000000",
        )
        .unwrap();
    }

    #[test]
    fn validate_ids_loop_mismatch() {
        let json = valid_handoff_json();
        let handoff = parse_handoff(&json).unwrap();
        let errors = validate_ids(
            &handoff,
            "loop_different",
            "4f3a91c2-0000-0000-0000-000000000000",
        )
        .unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, HandoffError::LoopIdMismatch { .. })));
    }

    #[test]
    fn validate_ids_session_mismatch() {
        let json = valid_handoff_json();
        let handoff = parse_handoff(&json).unwrap();
        let errors = validate_ids(&handoff, "loop_abc123", "wrong-session-id").unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, HandoffError::SessionIdMismatch { .. })));
    }

    #[test]
    fn validate_ids_both_mismatch() {
        let json = valid_handoff_json();
        let handoff = parse_handoff(&json).unwrap();
        let errors = validate_ids(&handoff, "wrong-loop", "wrong-session").unwrap_err();
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn all_handoff_statuses_roundtrip() {
        for status in &["completed", "blocked", "needs_human", "failed"] {
            let json = serde_json::json!({
                "schema": "planeai.handoff.v1",
                "loop_id": "loop_1",
                "session_id": "sess-1",
                "status": status,
                "summary": "test"
            })
            .to_string();
            let handoff = parse_handoff(&json).unwrap();
            assert_eq!(handoff.status.as_str(), *status);
        }
    }

    #[test]
    fn all_evidence_sources_roundtrip() {
        for source in &["direct", "proxy", "claimed", "blocked"] {
            let json = serde_json::json!({
                "schema": "planeai.handoff.v1",
                "loop_id": "loop_1",
                "session_id": "sess-1",
                "status": "completed",
                "summary": "test",
                "evidence": [{
                    "kind": "test",
                    "name": "run",
                    "result": "pass",
                    "source": source
                }]
            })
            .to_string();
            let handoff = parse_handoff(&json).unwrap();
            assert_eq!(handoff.evidence[0].source.as_str(), *source);
        }
    }

    #[test]
    fn serialize_and_deserialize_roundtrip() {
        let handoff = HandoffV1 {
            schema: SCHEMA_V1.to_string(),
            loop_id: "loop_rt".to_string(),
            session_id: "sess-rt".to_string(),
            task_key: Some("PLA-99".to_string()),
            status: HandoffStatus::NeedsHuman,
            branch: Some("feat/test".to_string()),
            commit: Some("deadbeef".to_string()),
            changed_files: vec!["a.rs".to_string()],
            summary: "Needs review".to_string(),
            risks: vec!["Risky".to_string()],
            next_actions: vec!["Review".to_string()],
            evidence: vec![Evidence {
                kind: "lint".to_string(),
                name: "clippy".to_string(),
                result: "pass".to_string(),
                source: EvidenceSource::Proxy,
                output_path: None,
            }],
        };

        let json = serde_json::to_string(&handoff).unwrap();
        let parsed = parse_handoff(&json).unwrap();
        assert_eq!(parsed, handoff);
    }

    #[test]
    fn parse_rejects_blank_summary() {
        let json = serde_json::json!({
            "schema": "planeai.handoff.v1",
            "loop_id": "loop_1",
            "session_id": "sess-1",
            "status": "completed",
            "summary": "   "
        })
        .to_string();

        let errors = parse_handoff(&json).unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, HandoffError::MissingField(f) if f == "summary")));
    }

    #[test]
    fn parse_rejects_empty_loop_id() {
        let json = serde_json::json!({
            "schema": "planeai.handoff.v1",
            "loop_id": "",
            "session_id": "sess-1",
            "status": "completed",
            "summary": "Done"
        })
        .to_string();

        let errors = parse_handoff(&json).unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, HandoffError::MissingField(f) if f == "loop_id")));
    }

    #[test]
    fn parse_rejects_whitespace_session_id() {
        let json = serde_json::json!({
            "schema": "planeai.handoff.v1",
            "loop_id": "loop_1",
            "session_id": "\t\n",
            "status": "completed",
            "summary": "Done"
        })
        .to_string();

        let errors = parse_handoff(&json).unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, HandoffError::MissingField(f) if f == "session_id")));
    }
}
