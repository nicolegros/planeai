use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

const KEYRING_SERVICE: &str = "planeai-jira";
const KEYRING_USER: &str = "oauth_tokens";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub client_id: String,
    pub cloud_id: String,
    pub scopes: Vec<String>,
    pub redirect_port: u16,
}

impl Default for OAuthConfig {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            cloud_id: String::new(),
            scopes: vec![
                "read:jira-work".to_string(),
                "write:jira-work".to_string(),
                "offline_access".to_string(),
            ],
            redirect_port: 19847,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

/// Manages token persistence via OS keychain.
pub struct TokenStore;

impl TokenStore {
    pub fn save(tokens: &TokenPair) -> Result<(), String> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .map_err(|e| format!("keyring entry error: {e}"))?;
        let json = serde_json::to_string(tokens).map_err(|e| e.to_string())?;
        entry
            .set_password(&json)
            .map_err(|e| format!("keyring save error: {e}"))
    }

    pub fn load() -> Result<Option<TokenPair>, String> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .map_err(|e| format!("keyring entry error: {e}"))?;
        match entry.get_password() {
            Ok(json) => {
                let tokens: TokenPair =
                    serde_json::from_str(&json).map_err(|e| e.to_string())?;
                Ok(Some(tokens))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("keyring load error: {e}")),
        }
    }

    pub fn delete() -> Result<(), String> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .map_err(|e| format!("keyring entry error: {e}"))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("keyring delete error: {e}")),
        }
    }
}

/// Handles the OAuth 2.0 3LO + PKCE flow with localhost redirect.
pub struct OAuthFlow {
    config: OAuthConfig,
}

impl OAuthFlow {
    pub fn new(config: OAuthConfig) -> Self {
        Self { config }
    }

    fn generate_pkce() -> (String, String) {
        let mut rng = rand::thread_rng();
        let verifier_bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
        let verifier = URL_SAFE_NO_PAD.encode(&verifier_bytes);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        (verifier, challenge)
    }

    fn generate_state() -> String {
        let mut rng = rand::thread_rng();
        let bytes: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
        URL_SAFE_NO_PAD.encode(&bytes)
    }

    /// Returns the authorization URL and starts a localhost listener.
    /// Blocks until the redirect is received, then exchanges the code for tokens.
    pub async fn authorize(&self) -> Result<TokenPair, String> {
        let (verifier, challenge) = Self::generate_pkce();
        let state = Self::generate_state();
        let redirect_uri = format!("http://localhost:{}/callback", self.config.redirect_port);

        let scopes = self.config.scopes.join(" ");
        let auth_url = format!(
            "https://auth.atlassian.com/authorize?audience=api.atlassian.com&client_id={}&scope={}&redirect_uri={}&state={}&response_type=code&prompt=consent&code_challenge={}&code_challenge_method=S256",
            urlencoding(&self.config.client_id),
            urlencoding(&scopes),
            urlencoding(&redirect_uri),
            urlencoding(&state),
            urlencoding(&challenge),
        );

        // Open browser
        let _ = open::that(&auth_url);
        tracing::info!("opened browser for Jira OAuth");

        // Listen for redirect
        let code = tokio::task::spawn_blocking({
            let port = self.config.redirect_port;
            let expected_state = state.clone();
            move || listen_for_code(port, &expected_state)
        })
        .await
        .map_err(|e| format!("join error: {e}"))??;

        // Exchange code for tokens
        let tokens = self.exchange_code(&code, &verifier, &redirect_uri).await?;
        TokenStore::save(&tokens)?;
        Ok(tokens)
    }

    async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenPair, String> {
        let client = reqwest::Client::new();
        let resp = client
            .post("https://auth.atlassian.com/oauth/token")
            .json(&serde_json::json!({
                "grant_type": "authorization_code",
                "client_id": self.config.client_id,
                "code": code,
                "redirect_uri": redirect_uri,
                "code_verifier": verifier,
            }))
            .send()
            .await
            .map_err(|e| format!("token exchange request failed: {e}"))?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("token exchange failed: {text}"));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
        Ok(TokenPair {
            access_token: body["access_token"]
                .as_str()
                .ok_or("missing access_token")?
                .to_string(),
            refresh_token: body["refresh_token"]
                .as_str()
                .ok_or("missing refresh_token")?
                .to_string(),
            expires_at: chrono::Utc::now().timestamp() + expires_in,
        })
    }

    pub async fn refresh(config: &OAuthConfig) -> Result<TokenPair, String> {
        let existing = TokenStore::load()?.ok_or("no stored tokens to refresh")?;
        let client = reqwest::Client::new();
        let resp = client
            .post("https://auth.atlassian.com/oauth/token")
            .json(&serde_json::json!({
                "grant_type": "refresh_token",
                "client_id": config.client_id,
                "refresh_token": existing.refresh_token,
            }))
            .send()
            .await
            .map_err(|e| format!("refresh request failed: {e}"))?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("token refresh failed: {text}"));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
        let tokens = TokenPair {
            access_token: body["access_token"]
                .as_str()
                .ok_or("missing access_token")?
                .to_string(),
            refresh_token: body["refresh_token"]
                .as_str()
                .unwrap_or(&existing.refresh_token)
                .to_string(),
            expires_at: chrono::Utc::now().timestamp() + expires_in,
        };
        TokenStore::save(&tokens)?;
        Ok(tokens)
    }
}

fn urlencoding(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn listen_for_code(port: u16, expected_state: &str) -> Result<String, String> {
    let listener =
        TcpListener::bind(format!("127.0.0.1:{port}")).map_err(|e| format!("bind failed: {e}"))?;

    let (mut stream, _) = listener.accept().map_err(|e| format!("accept failed: {e}"))?;
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|e| format!("read failed: {e}"))?;

    // Parse GET /callback?code=...&state=...
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or("malformed request")?;
    let query = path.split('?').nth(1).unwrap_or("");
    let params: std::collections::HashMap<_, _> = url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    let state = params.get("state").ok_or("missing state parameter")?;
    if state != expected_state {
        return Err("state mismatch".to_string());
    }

    let code = params
        .get("code")
        .ok_or("missing code parameter")?
        .to_string();

    // Send success response
    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h2>Connected to Jira!</h2><p>You can close this tab.</p></body></html>";
    let _ = stream.write_all(response.as_bytes());

    Ok(code)
}
