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
    pub sources: HashMap<String, JiraSyncSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JiraSyncSource {
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
    fn round_trip_jira_config_with_sources() {
        let mut sources = HashMap::new();
        sources.insert(
            "peng-support".to_string(),
            JiraSyncSource {
                jql: "project = PENG AND assignee = currentUser()".to_string(),
                status_map: HashMap::from([
                    ("In Progress".to_string(), "in_progress".to_string()),
                    ("Done".to_string(), "done".to_string()),
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
            sources,
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: JiraConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn source_has_no_jira_project_field() {
        let json = r#"{
            "site": "https://test.atlassian.net",
            "sources": {
                "my-source": {
                    "jql": "project = PENG AND assignee = currentUser()",
                    "status_map": {"Acknowledged": "in_progress"},
                    "writeback": {"on_start": "In Progress", "on_complete": "Done", "comment": true}
                }
            }
        }"#;
        let config: JiraConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.sources.len(), 1);
        let source = config.sources.get("my-source").unwrap();
        assert_eq!(source.jql, "project = PENG AND assignee = currentUser()");
        assert_eq!(
            source.status_map.get("Acknowledged"),
            Some(&"in_progress".to_string())
        );
        assert_eq!(
            source.writeback.as_ref().unwrap().on_start,
            Some("In Progress".to_string())
        );
    }

    #[test]
    fn defaults_applied_for_missing_optional_fields() {
        let json = r#"{"site": "https://x.atlassian.net"}"#;
        let config: JiraConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.sync_interval_ms, 60_000);
        assert!(config.sources.is_empty());
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
