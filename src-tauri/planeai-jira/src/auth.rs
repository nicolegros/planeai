use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngExt;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use url::Url;

include!(concat!(env!("OUT_DIR"), "/oauth_credentials.rs"));
const AUTH_URL: &str = "https://auth.atlassian.com/authorize";
const TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
const RESOURCES_URL: &str = "https://api.atlassian.com/oauth/token/accessible-resources";
const SCOPES: &str = "read:jira-work write:jira-work offline_access";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("OAuth state mismatch")]
    StateMismatch,
    #[error("No authorization code received")]
    NoCode,
    #[error("Cloud ID not found for site: {0}")]
    CloudIdNotFound(String),
    #[error("Keyring error: {0}")]
    Keyring(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),
    #[error("Timed out waiting for browser callback")]
    Timeout,
}

/// Abstraction over secret storage (OS keychain in production, in-memory for tests).
pub trait TokenStore: Send + Sync {
    fn get(&self, key: &str) -> Result<String, Error>;
    fn set(&self, key: &str, value: &str) -> Result<(), Error>;
    fn delete(&self, key: &str) -> Result<(), Error>;
}

/// Production token store backed by a file in the app data directory.
/// Falls back from OS keychain to avoid entitlement issues in dev builds.
pub struct FileStore {
    dir: std::path::PathBuf,
}

impl FileStore {
    pub fn new(dir: std::path::PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Self { dir }
    }
}

impl TokenStore for FileStore {
    fn get(&self, key: &str) -> Result<String, Error> {
        std::fs::read_to_string(self.dir.join(key))
            .map(|s| s.trim().to_string())
            .map_err(|e| Error::Keyring(e.to_string()))
    }

    fn set(&self, key: &str, value: &str) -> Result<(), Error> {
        let path = self.dir.join(key);
        std::fs::write(&path, value).map_err(|e| Error::Keyring(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| Error::Keyring(e.to_string()))?;
        }
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), Error> {
        let _ = std::fs::remove_file(self.dir.join(key));
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct TokenState {
    access_token: String,
    expires_at: std::time::Instant,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct AccessibleResource {
    id: String,
    url: String,
}

pub struct JiraAuth {
    site: String,
    token_state: Mutex<Option<TokenState>>,
    store: Box<dyn TokenStore>,
    client: Client,
    token_url: String,
    resources_url: String,
}

impl JiraAuth {
    pub fn new(site: &str, token_dir: std::path::PathBuf) -> Self {
        Self::with_store(site, Box::new(FileStore::new(token_dir)))
    }

    pub fn with_store(site: &str, store: Box<dyn TokenStore>) -> Self {
        Self {
            site: site.to_string(),
            token_state: Mutex::new(None),
            store,
            client: Client::new(),
            token_url: TOKEN_URL.to_string(),
            resources_url: RESOURCES_URL.to_string(),
        }
    }

    #[cfg(test)]
    fn with_test_config(
        site: &str,
        store: Box<dyn TokenStore>,
        token_url: String,
        resources_url: String,
    ) -> Self {
        Self {
            site: site.to_string(),
            token_state: Mutex::new(None),
            store,
            client: Client::new(),
            token_url,
            resources_url,
        }
    }

    /// Create a JiraAuth with a pre-loaded access token for integration/client tests.
    #[cfg(test)]
    pub(crate) fn with_fixed_token(token: &str, token_url: String) -> Self {
        let store =
            crate::test_support::MemStore::with_entries(vec![("refresh_token", "test_refresh")]);

        let token_state = Some(TokenState {
            access_token: token.to_string(),
            expires_at: std::time::Instant::now() + std::time::Duration::from_secs(3600),
        });

        Self {
            site: "https://test.atlassian.net".to_string(),
            token_state: Mutex::new(token_state),
            store: Box::new(store),
            client: Client::new(),
            token_url,
            resources_url: String::new(),
        }
    }

    pub async fn connect(&self) -> Result<(), Error> {
        let listener = TcpListener::bind("127.0.0.1:19287").await.map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "failed to bind OAuth callback port 19287: {e}. \
                     Is another instance of planeai already running?"
                ),
            ))
        })?;
        let redirect_uri = "http://localhost:19287/callback".to_string();

        let (verifier, challenge) = generate_pkce();
        let state = generate_state();

        let auth_url = build_auth_url(&redirect_uri, &challenge, &state)?;
        if open::that(auth_url.as_str()).is_err() {
            tracing::warn!("failed to open browser. Visit: {auth_url}");
        }

        let code = tokio::time::timeout(CALLBACK_TIMEOUT, wait_for_callback(&listener, &state))
            .await
            .map_err(|_| Error::Timeout)??;

        let token_resp = self.exchange_code(&code, &redirect_uri, &verifier).await?;
        self.store_tokens(&token_resp).await?;

        let cloud_id = self.fetch_cloud_id(&token_resp.access_token).await?;
        self.store.set("cloud_id", &cloud_id)?;

        Ok(())
    }

    pub async fn disconnect(&self) -> Result<(), Error> {
        self.store.delete("refresh_token")?;
        self.store.delete("cloud_id")?;
        *self.token_state.lock().await = None;
        Ok(())
    }

    pub async fn access_token(&self) -> Result<String, Error> {
        {
            let state = self.token_state.lock().await;
            if let Some(ts) = state.as_ref() {
                if ts.expires_at > std::time::Instant::now() + Duration::from_secs(60) {
                    return Ok(ts.access_token.clone());
                }
            }
        }
        self.refresh().await
    }

    /// Clear the cached token so the next access_token() call triggers a refresh.
    pub async fn invalidate_token(&self) {
        *self.token_state.lock().await = None;
    }

    pub fn is_connected(&self) -> bool {
        self.store.get("refresh_token").is_ok()
    }

    pub fn cloud_id(&self) -> Result<String, Error> {
        self.store.get("cloud_id")
    }

    async fn refresh(&self) -> Result<String, Error> {
        let refresh_token = self.store.get("refresh_token")?;

        let body = serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": CLIENT_ID,
            "client_secret": CLIENT_SECRET,
            "refresh_token": refresh_token,
        });

        let resp = self
            .client
            .post(&self.token_url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<TokenResponse>()
            .await?;

        self.store_tokens(&resp).await?;
        Ok(resp.access_token)
    }

    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        verifier: &str,
    ) -> Result<TokenResponse, Error> {
        let body = serde_json::json!({
            "grant_type": "authorization_code",
            "client_id": CLIENT_ID,
            "client_secret": CLIENT_SECRET,
            "code": code,
            "redirect_uri": redirect_uri,
            "code_verifier": verifier,
        });

        self.client
            .post(&self.token_url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<TokenResponse>()
            .await
            .map_err(Error::from)
    }

    async fn fetch_cloud_id(&self, access_token: &str) -> Result<String, Error> {
        let resources: Vec<AccessibleResource> = self
            .client
            .get(&self.resources_url)
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let site_url = self.site.trim_end_matches('/');
        resources
            .iter()
            .find(|r| r.url.trim_end_matches('/') == site_url)
            .map(|r| r.id.clone())
            .ok_or_else(|| Error::CloudIdNotFound(self.site.clone()))
    }

    async fn store_tokens(&self, resp: &TokenResponse) -> Result<(), Error> {
        if let Some(rt) = &resp.refresh_token {
            self.store.set("refresh_token", rt)?;
        }
        let ts = TokenState {
            access_token: resp.access_token.clone(),
            expires_at: std::time::Instant::now() + Duration::from_secs(resp.expires_in),
        };
        *self.token_state.lock().await = Some(ts);
        Ok(())
    }
}

fn generate_pkce() -> (String, String) {
    let mut rng = rand::rng();
    let verifier_bytes: Vec<u8> = (0..32).map(|_| rng.random()).collect();
    let verifier = URL_SAFE_NO_PAD.encode(&verifier_bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn generate_state() -> String {
    let bytes: Vec<u8> = (0..16).map(|_| rand::rng().random()).collect();
    URL_SAFE_NO_PAD.encode(&bytes)
}

fn build_auth_url(redirect_uri: &str, challenge: &str, state: &str) -> Result<Url, Error> {
    let mut url = Url::parse(AUTH_URL)?;
    url.query_pairs_mut()
        .append_pair("audience", "api.atlassian.com")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("scope", SCOPES)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("prompt", "consent");
    Ok(url)
}

async fn wait_for_callback(listener: &TcpListener, expected_state: &str) -> Result<String, Error> {
    let (stream, _) = listener.accept().await?;
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);

    let mut request_line = String::new();
    buf_reader.read_line(&mut request_line).await?;

    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or(Error::NoCode)?;

    let full_url = format!("http://localhost{path}");
    let parsed = Url::parse(&full_url)?;
    let params: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();

    let state = params.get("state").cloned().unwrap_or_default();
    if state != expected_state {
        let body = "Authentication failed: state mismatch.";
        let response = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let _ = writer.write_all(response.as_bytes()).await;
        return Err(Error::StateMismatch);
    }

    let code = params.get("code").cloned().ok_or(Error::NoCode)?;

    let body = "Authentication successful! You can close this tab.";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = writer.write_all(response.as_bytes()).await;

    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MemStore;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn pkce_verifier_length_and_charset() {
        let (verifier, challenge) = generate_pkce();
        assert!(verifier.len() >= 43 && verifier.len() <= 128);
        assert!(verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert!(!verifier.contains('='));
        assert!(!challenge.contains('='));
        // Verify S256 relationship
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, expected);
    }

    #[test]
    fn state_uniqueness() {
        assert_ne!(generate_state(), generate_state());
    }

    #[test]
    fn auth_url_contains_required_params() {
        let url = build_auth_url("http://localhost:9999/callback", "challenge", "mystate").unwrap();
        let s = url.to_string();
        assert!(s.contains("audience=api.atlassian.com"));
        assert!(s.contains("client_id="));
        assert!(s.contains("scope="));
        assert!(s.contains("redirect_uri="));
        assert!(s.contains("response_type=code"));
        assert!(s.contains("code_challenge=challenge"));
        assert!(s.contains("code_challenge_method=S256"));
        assert!(s.contains("state=mystate"));
    }

    #[tokio::test]
    async fn state_mismatch_rejected() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = tokio::spawn(async move { wait_for_callback(&listener, "expected").await });

        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        stream
            .write_all(b"GET /callback?code=abc&state=wrong HTTP/1.1\r\n\r\n")
            .await
            .unwrap();

        assert!(matches!(handle.await.unwrap(), Err(Error::StateMismatch)));
    }

    #[tokio::test]
    async fn valid_callback_returns_code() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = tokio::spawn(async move { wait_for_callback(&listener, "goodstate").await });

        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        stream
            .write_all(b"GET /callback?code=mycode123&state=goodstate HTTP/1.1\r\n\r\n")
            .await
            .unwrap();

        assert_eq!(handle.await.unwrap().unwrap(), "mycode123");
    }

    #[tokio::test]
    async fn token_exchange_with_mock_server() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("authorization_code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "test_access",
                "refresh_token": "test_refresh",
                "expires_in": 3600
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/oauth/token/accessible-resources"))
            .and(header("Authorization", "Bearer test_access"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"id": "cloud-123", "url": "https://mysite.atlassian.net", "name": "My Site"}
            ])))
            .mount(&mock_server)
            .await;

        let auth = JiraAuth::with_test_config(
            "https://mysite.atlassian.net",
            Box::new(MemStore::new()),
            format!("{}/oauth/token", mock_server.uri()),
            format!("{}/oauth/token/accessible-resources", mock_server.uri()),
        );

        let resp = auth
            .exchange_code("testcode", "http://localhost:1234/callback", "verifier")
            .await
            .unwrap();

        assert_eq!(resp.access_token, "test_access");
        assert_eq!(resp.refresh_token.as_deref(), Some("test_refresh"));

        let cloud_id = auth.fetch_cloud_id(&resp.access_token).await.unwrap();
        assert_eq!(cloud_id, "cloud-123");
    }

    #[tokio::test]
    async fn token_refresh_uses_store() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("refresh_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "refreshed_access",
                "refresh_token": "new_refresh",
                "expires_in": 3600
            })))
            .mount(&mock_server)
            .await;

        let store = MemStore::new();
        store.set("refresh_token", "old_refresh").unwrap();

        let auth = JiraAuth::with_test_config(
            "https://mysite.atlassian.net",
            Box::new(store),
            format!("{}/oauth/token", mock_server.uri()),
            String::new(),
        );

        // Set expired token state
        *auth.token_state.lock().await = Some(TokenState {
            access_token: "old_token".to_string(),
            expires_at: std::time::Instant::now(),
        });

        let token = auth.access_token().await.unwrap();
        assert_eq!(token, "refreshed_access");

        // Verify new refresh token was stored
        assert_eq!(auth.store.get("refresh_token").unwrap(), "new_refresh");
    }

    #[tokio::test]
    async fn disconnect_clears_state() {
        let store = MemStore::new();
        store.set("refresh_token", "rt").unwrap();
        store.set("cloud_id", "cid").unwrap();

        let auth = JiraAuth::with_test_config(
            "https://mysite.atlassian.net",
            Box::new(store),
            String::new(),
            String::new(),
        );

        *auth.token_state.lock().await = Some(TokenState {
            access_token: "tok".to_string(),
            expires_at: std::time::Instant::now() + Duration::from_secs(3600),
        });

        auth.disconnect().await.unwrap();

        assert!(auth.token_state.lock().await.is_none());
        assert!(!auth.is_connected());
        assert!(auth.cloud_id().is_err());
    }

    #[tokio::test]
    async fn is_connected_reflects_store() {
        let store = MemStore::new();
        let auth = JiraAuth::with_test_config(
            "https://x.atlassian.net",
            Box::new(store),
            String::new(),
            String::new(),
        );

        assert!(!auth.is_connected());
        auth.store.set("refresh_token", "tok").unwrap();
        assert!(auth.is_connected());
    }

    #[tokio::test]
    async fn cloud_id_not_found_for_wrong_site() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/resources"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"id": "other-123", "url": "https://other.atlassian.net", "name": "Other"}
            ])))
            .mount(&mock_server)
            .await;

        let auth = JiraAuth::with_test_config(
            "https://mysite.atlassian.net",
            Box::new(MemStore::new()),
            String::new(),
            format!("{}/resources", mock_server.uri()),
        );

        assert!(matches!(
            auth.fetch_cloud_id("token").await,
            Err(Error::CloudIdNotFound(_))
        ));
    }
}
