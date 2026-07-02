use reqwest::{Client, Response, StatusCode};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{debug, instrument, warn};

use crate::auth::JiraAuth;

const SEARCH_FIELDS: &str = "summary,description,status,priority,labels,parent,issuelinks";
const PAGE_SIZE: u64 = 50;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unauthorized — token refresh failed")]
    Unauthorized,
    #[error("rate limited — retry after {0}s")]
    RateLimited(u64),
    #[error("not found")]
    NotFound,
    #[error("API error: {0}")]
    ApiError(String),
    #[error("auth error: {0}")]
    Auth(#[from] crate::auth::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

/// Wire type returned by the Jira search API. Callers map this to `model::JiraIssue`
/// by supplying `jira_project`, `sync_status`, and `last_synced_at`.
#[derive(Debug, Clone)]
pub struct FetchedIssue {
    pub issue_key: String,
    pub summary: String,
    pub description: String,
    pub status: String,
    pub status_category: Option<String>,
    pub priority: Option<String>,
    pub labels: Vec<String>,
}

pub struct JiraClient {
    auth: Arc<JiraAuth>,
    cloud_id: String,
    client: Client,
    #[cfg(test)]
    base_url_override: Option<String>,
}

impl JiraClient {
    pub fn new(auth: Arc<JiraAuth>, cloud_id: String) -> Self {
        Self {
            auth,
            cloud_id,
            client: Client::new(),
            #[cfg(test)]
            base_url_override: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_base_url(auth: Arc<JiraAuth>, base_url: String) -> Self {
        Self {
            auth,
            cloud_id: String::new(),
            client: Client::new(),
            base_url_override: Some(base_url),
        }
    }

    fn base_url(&self) -> String {
        #[cfg(test)]
        if let Some(url) = &self.base_url_override {
            return url.clone();
        }
        format!(
            "https://api.atlassian.com/ex/jira/{}/rest/api/3",
            self.cloud_id
        )
    }

    async fn send_with_retry(
        &self,
        build: impl Fn(&str) -> reqwest::RequestBuilder,
    ) -> Result<Response, Error> {
        let token = self.auth.access_token().await?;
        let resp = build(&token).send().await?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            warn!("received 401, refreshing token and retrying");
            self.auth.invalidate_token().await;
            let new_token = self.auth.access_token().await?;
            let retry = build(&new_token).send().await?;
            if retry.status() == StatusCode::UNAUTHORIZED {
                warn!("retry after refresh still 401");
                return Err(Error::Unauthorized);
            }
            return Self::check_status(retry).await;
        }

        Self::check_status(resp).await
    }

    async fn check_status(resp: Response) -> Result<Response, Error> {
        match resp.status() {
            s if s.is_success() => Ok(resp),
            StatusCode::NOT_FOUND => Err(Error::NotFound),
            StatusCode::TOO_MANY_REQUESTS => {
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(60);
                warn!(retry_after, "jira rate limited");
                Err(Error::RateLimited(retry_after))
            }
            status => {
                let text = resp.text().await.unwrap_or_default();
                warn!(%status, body = %text, "jira API error");
                Err(Error::ApiError(text))
            }
        }
    }

    #[instrument(skip(self), fields(jql))]
    pub async fn search(&self, jql: &str) -> Result<Vec<FetchedIssue>, Error> {
        let mut issues = Vec::new();
        let mut next_page_token: Option<String> = None;
        let page_size_str = PAGE_SIZE.to_string();

        loop {
            debug!(?next_page_token, "fetching search page");
            let url = format!("{}/search/jql", self.base_url());
            let token_ref = next_page_token.clone();

            let page: SearchResponse = self
                .send_with_retry(|token| {
                    let mut req = self.client.get(&url).bearer_auth(token).query(&[
                        ("jql", jql),
                        ("fields", SEARCH_FIELDS),
                        ("maxResults", page_size_str.as_str()),
                    ]);
                    if let Some(ref pt) = token_ref {
                        req = req.query(&[("nextPageToken", pt.as_str())]);
                    }
                    req
                })
                .await?
                .json()
                .await?;

            parse_page(&page, &mut issues);
            match page.next_page_token {
                Some(token) => next_page_token = Some(token),
                None => break,
            }
        }

        debug!(count = issues.len(), "search complete");
        Ok(issues)
    }

    #[instrument(skip(self))]
    pub async fn transition(&self, issue_key: &str, transition_name: &str) -> Result<(), Error> {
        let url = format!("{}/issue/{}/transitions", self.base_url(), issue_key);

        let resp = self
            .send_with_retry(|token| self.client.get(&url).bearer_auth(token))
            .await?;
        let data: TransitionsResponse = resp.json().await?;

        let found = data.transitions.iter().find(|t| {
            t.to.as_ref()
                .map(|to| to.name.eq_ignore_ascii_case(transition_name))
                .unwrap_or(false)
        });

        let transition = found.ok_or_else(|| {
            let available: Vec<_> = data
                .transitions
                .iter()
                .filter_map(|t| t.to.as_ref().map(|to| to.name.as_str()))
                .collect();
            warn!(
                issue_key,
                transition_name,
                ?available,
                "transition not found"
            );
            Error::ApiError(format!(
                "transition '{}' not found. Available: {:?}",
                transition_name, available
            ))
        })?;

        let body = serde_json::json!({"transition": {"id": transition.id}});
        self.send_with_retry(|token| self.client.post(&url).bearer_auth(token).json(&body))
            .await?;

        debug!(issue_key, transition_name, "transition complete");
        Ok(())
    }

    #[instrument(skip(self, body))]
    pub async fn comment(&self, issue_key: &str, body: &str) -> Result<(), Error> {
        let url = format!("{}/issue/{}/comment", self.base_url(), issue_key);
        let adf_body = serde_json::json!({
            "body": {
                "type": "doc",
                "version": 1,
                "content": [{"type": "paragraph", "content": [{"type": "text", "text": body}]}]
            }
        });

        self.send_with_retry(|token| self.client.post(&url).bearer_auth(token).json(&adf_body))
            .await?;

        debug!(issue_key, "comment posted");
        Ok(())
    }
}

// --- Internal response types ---

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    issues: Vec<RawIssue>,
    #[serde(default, rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct RawIssue {
    key: String,
    fields: RawFields,
}

#[derive(Deserialize)]
struct RawFields {
    summary: Option<String>,
    description: Option<serde_json::Value>,
    status: Option<StatusField>,
    priority: Option<PriorityField>,
    labels: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct StatusField {
    name: String,
    #[serde(rename = "statusCategory")]
    status_category: Option<StatusCategoryField>,
}

#[derive(Deserialize)]
struct StatusCategoryField {
    name: String,
}

#[derive(Deserialize)]
struct PriorityField {
    name: String,
}

#[derive(Deserialize)]
struct TransitionsResponse {
    transitions: Vec<Transition>,
}

#[derive(Deserialize)]
struct Transition {
    id: String,
    to: Option<TransitionTarget>,
}

#[derive(Deserialize)]
struct TransitionTarget {
    name: String,
}

fn extract_plain_text(adf: &serde_json::Value) -> String {
    match adf {
        serde_json::Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(|t| t.as_str()) {
                return text.to_string();
            }
            if let Some(content) = map.get("content").and_then(|c| c.as_array()) {
                return content
                    .iter()
                    .map(extract_plain_text)
                    .collect::<Vec<_>>()
                    .join("");
            }
            String::new()
        }
        _ => String::new(),
    }
}

fn parse_page(page: &SearchResponse, issues: &mut Vec<FetchedIssue>) {
    for raw in &page.issues {
        let description = raw
            .fields
            .description
            .as_ref()
            .map(extract_plain_text)
            .unwrap_or_default();

        issues.push(FetchedIssue {
            issue_key: raw.key.clone(),
            summary: raw.fields.summary.clone().unwrap_or_default(),
            description,
            status: raw
                .fields
                .status
                .as_ref()
                .map(|s| s.name.clone())
                .unwrap_or_default(),
            status_category: raw
                .fields
                .status
                .as_ref()
                .and_then(|s| s.status_category.as_ref())
                .map(|c| c.name.clone()),
            priority: raw.fields.priority.as_ref().map(|p| p.name.clone()),
            labels: raw.fields.labels.clone().unwrap_or_default(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn test_client(mock_server: &MockServer) -> JiraClient {
        let auth = Arc::new(JiraAuth::with_fixed_token(
            "test_token",
            format!("{}/oauth/token", mock_server.uri()),
        ));
        JiraClient::with_base_url(auth, mock_server.uri())
    }

    fn issue_json(key: &str, summary: &str, status: &str) -> serde_json::Value {
        serde_json::json!({
            "key": key,
            "fields": {
                "summary": summary,
                "description": {"type": "doc", "version": 1, "content": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "desc text"}]}
                ]},
                "status": {"name": status},
                "priority": {"name": "High"},
                "labels": ["bug", "backend"]
            }
        })
    }

    #[tokio::test]
    async fn search_returns_parsed_issues() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .and(query_param("jql", "project = TEST"))
            .and(header("Authorization", "Bearer test_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issues": [
                    issue_json("TEST-1", "First issue", "To Do"),
                    issue_json("TEST-2", "Second issue", "In Progress"),
                ]
            })))
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let issues = client.search("project = TEST").await.unwrap();

        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].issue_key, "TEST-1");
        assert_eq!(issues[0].summary, "First issue");
        assert_eq!(issues[0].status, "To Do");
        assert_eq!(issues[0].priority.as_deref(), Some("High"));
        assert_eq!(issues[0].labels, vec!["bug", "backend"]);
        assert_eq!(issues[0].description, "desc text");
        assert_eq!(issues[1].issue_key, "TEST-2");
    }

    #[tokio::test]
    async fn search_paginates_multiple_pages() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .and(query_param("nextPageToken", "page2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issues": [
                    issue_json("T-50", "Issue 50", "Open"),
                    issue_json("T-51", "Issue 51", "Open"),
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        // This matches any request to /search/jql (including the first page without nextPageToken)
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "nextPageToken": "page2",
                "issues": (0..50).map(|i| issue_json(&format!("T-{i}"), &format!("Issue {i}"), "Open")).collect::<Vec<_>>()
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let issues = client.search("project = T").await.unwrap();

        assert_eq!(issues.len(), 52);
        assert_eq!(issues[51].issue_key, "T-51");
    }

    #[tokio::test]
    async fn transition_finds_and_executes() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/issue/TEST-1/transitions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "transitions": [
                    {"id": "11", "name": "Start", "to": {"name": "In Progress"}},
                    {"id": "21", "name": "Done", "to": {"name": "Done"}},
                ]
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/issue/TEST-1/transitions"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        client.transition("TEST-1", "In Progress").await.unwrap();
    }

    #[tokio::test]
    async fn transition_case_insensitive() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/issue/TEST-1/transitions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "transitions": [
                    {"id": "11", "name": "Start", "to": {"name": "In Progress"}},
                ]
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/issue/TEST-1/transitions"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        client.transition("TEST-1", "in progress").await.unwrap();
    }

    #[tokio::test]
    async fn transition_errors_when_not_found() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/issue/TEST-1/transitions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "transitions": [
                    {"id": "11", "name": "Start", "to": {"name": "In Progress"}},
                ]
            })))
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let err = client.transition("TEST-1", "Done").await.unwrap_err();
        assert!(matches!(err, Error::ApiError(_)));
        let msg = err.to_string();
        assert!(msg.contains("Done"));
        assert!(msg.contains("In Progress"));
    }

    #[tokio::test]
    async fn comment_posts_adf_body() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/issue/TEST-1/comment"))
            .and(header("Authorization", "Bearer test_token"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        client
            .comment("TEST-1", "Hello from planeai")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn unauthorized_triggers_retry() {
        let server = MockServer::start().await;

        // Token refresh endpoint
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "refreshed_token",
                "refresh_token": "new_refresh",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;

        // First call returns 401, second (after refresh) returns 200
        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .and(header("Authorization", "Bearer test_token"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .and(header("Authorization", "Bearer refreshed_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issues": [issue_json("TEST-1", "Refreshed", "Open")]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let issues = client.search("project = TEST").await.unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].summary, "Refreshed");
    }

    #[tokio::test]
    async fn rate_limited_returns_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/search/jql"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "30"))
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let err = client.search("project = X").await.unwrap_err();
        assert!(matches!(err, Error::RateLimited(30)));
    }

    #[tokio::test]
    async fn not_found_returns_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/issue/NOPE-1/transitions"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let err = client.transition("NOPE-1", "Done").await.unwrap_err();
        assert!(matches!(err, Error::NotFound));
    }
}
