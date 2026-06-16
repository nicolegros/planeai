use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_sync_interval_ms() -> u64 {
    60_000
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JiraConfig {
    pub site: String,
    #[serde(default = "default_sync_interval_ms")]
    pub sync_interval_ms: u64,
    #[serde(default)]
    pub projects: HashMap<String, JiraProjectMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JiraProjectMapping {
    pub jira_project: String,
    pub jql: String,
    #[serde(default)]
    pub status_map: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writeback: Option<WritebackConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WritebackConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_complete: Option<String>,
    #[serde(default)]
    pub comment: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_jira_config() {
        let mut projects = HashMap::new();
        projects.insert(
            "myproject".to_string(),
            JiraProjectMapping {
                jira_project: "MP".to_string(),
                jql: "project = MP AND status != Done".to_string(),
                status_map: HashMap::from([
                    ("In Progress".to_string(), "active".to_string()),
                    ("Done".to_string(), "completed".to_string()),
                ]),
                writeback: Some(WritebackConfig {
                    on_start: Some("In Progress".to_string()),
                    on_complete: Some("Done".to_string()),
                    comment: true,
                }),
            },
        );

        let config = JiraConfig {
            site: "https://mycompany.atlassian.net".to_string(),
            sync_interval_ms: 30_000,
            projects,
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: JiraConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn defaults_applied_for_missing_optional_fields() {
        let json = r#"{"site": "https://x.atlassian.net"}"#;
        let config: JiraConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.sync_interval_ms, 60_000);
        assert!(config.projects.is_empty());
    }

    #[test]
    fn writeback_defaults() {
        let json = r#"{"on_start": null, "on_complete": null, "comment": false}"#;
        let wb: WritebackConfig = serde_json::from_str(json).unwrap();
        assert_eq!(wb.on_start, None);
        assert_eq!(wb.on_complete, None);
        assert!(!wb.comment);
    }

    #[test]
    fn writeback_comment_defaults_to_false() {
        let json = r#"{}"#;
        let wb: WritebackConfig = serde_json::from_str(json).unwrap();
        assert!(!wb.comment);
    }
}
