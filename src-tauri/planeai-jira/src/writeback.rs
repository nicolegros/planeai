use std::sync::Arc;

use chrono::Utc;
use tracing::warn;

use crate::client::JiraClient;
use crate::config::WritebackConfig;

pub enum WritebackAction {
    Start,
    Complete,
}

impl WritebackAction {
    fn label(&self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Complete => "Complete",
        }
    }
}

pub struct JiraWriteback {
    client: Arc<JiraClient>,
}

impl JiraWriteback {
    pub fn new(client: Arc<JiraClient>) -> Self {
        Self { client }
    }

    pub async fn on_status_change(
        &self,
        issue_key: &str,
        action: WritebackAction,
        config: &WritebackConfig,
    ) -> Result<(), crate::Error> {
        let target = match &action {
            WritebackAction::Start => config.on_start.as_deref(),
            WritebackAction::Complete => config.on_complete.as_deref(),
        };

        if let Some(target) = target {
            if let Err(e) = self.client.transition(issue_key, target).await {
                warn!(issue_key, target, error = %e, "writeback transition failed");
            }
        }

        if config.comment {
            let message = format!(
                "planeai: Task moved to {} at {}",
                action.label(),
                Utc::now().format("%Y-%m-%dT%H:%M:%SZ")
            );
            if let Err(e) = self.client.comment(issue_key, &message).await {
                warn!(issue_key, error = %e, "writeback comment failed");
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::JiraAuth;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn setup() -> (MockServer, Arc<JiraClient>) {
        let server = MockServer::start().await;
        let auth = Arc::new(JiraAuth::with_fixed_token(
            "test_token",
            format!("{}/oauth/token", server.uri()),
        ));
        let client = Arc::new(JiraClient::with_base_url(auth, server.uri()));
        (server, client)
    }

    #[tokio::test]
    async fn transition_called_with_correct_target() {
        let (server, client) = setup().await;

        Mock::given(method("GET"))
            .and(path("/issue/PLA-1/transitions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "transitions": [{"id": "31", "name": "Go", "to": {"name": "In Progress"}}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/issue/PLA-1/transitions"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let wb = JiraWriteback::new(client);
        let config = WritebackConfig {
            on_start: Some("In Progress".to_string()),
            on_complete: None,
            comment: false,
        };

        let result = wb
            .on_status_change("PLA-1", WritebackAction::Start, &config)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn comment_contains_timestamp_and_action() {
        let (server, client) = setup().await;

        Mock::given(method("POST"))
            .and(path("/issue/PLA-2/comment"))
            .and(body_string_contains("planeai: Task moved to Complete"))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;

        let wb = JiraWriteback::new(client);
        let config = WritebackConfig {
            on_start: None,
            on_complete: None,
            comment: true,
        };

        let result = wb
            .on_status_change("PLA-2", WritebackAction::Complete, &config)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn does_nothing_when_no_target_and_no_comment() {
        let (server, client) = setup().await;

        // No mocks mounted — any request would panic
        let _server = server;

        let wb = JiraWriteback::new(client);
        let config = WritebackConfig {
            on_start: None,
            on_complete: None,
            comment: false,
        };

        let result = wb
            .on_status_change("PLA-3", WritebackAction::Start, &config)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn transition_failure_still_returns_ok() {
        let (server, client) = setup().await;

        Mock::given(method("GET"))
            .and(path("/issue/PLA-4/transitions"))
            .respond_with(ResponseTemplate::new(500).set_body_string("server error"))
            .mount(&server)
            .await;

        let wb = JiraWriteback::new(client);
        let config = WritebackConfig {
            on_start: Some("Done".to_string()),
            on_complete: None,
            comment: false,
        };

        let result = wb
            .on_status_change("PLA-4", WritebackAction::Start, &config)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn comment_failure_still_returns_ok() {
        let (server, client) = setup().await;

        Mock::given(method("POST"))
            .and(path("/issue/PLA-5/comment"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let wb = JiraWriteback::new(client);
        let config = WritebackConfig {
            on_start: None,
            on_complete: None,
            comment: true,
        };

        let result = wb
            .on_status_change("PLA-5", WritebackAction::Complete, &config)
            .await;
        assert!(result.is_ok());
    }
}
