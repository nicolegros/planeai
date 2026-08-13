use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use url::Url;

include!(concat!(env!("OUT_DIR"), "/oauth_credentials.rs"));

const PLUGIN_ID: &str = "jira";
const PLUGIN_NAME: &str = "Jira";
const PLUGIN_VERSION: &str = "0.1.0";
const HOST_API_VERSION: &str = "planeai.plugin-host.v1";
const MAX_RPC_FRAME_BYTES: u64 = 64 * 1024;
const AUTH_URL: &str = "https://auth.atlassian.com/authorize";
const TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
const RESOURCES_URL: &str = "https://api.atlassian.com/oauth/token/accessible-resources";
const SCOPES: &str = "read:jira-work write:jira-work offline_access";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(120);
const CALLBACK_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CALLBACK_REQUEST_LINE_BYTES: u64 = 8 * 1024;
const OAUTH_NETWORK_TIMEOUT: Duration = Duration::from_secs(20);
const CALLBACK_ADDRESS: &str = "127.0.0.1:19287";
const REDIRECT_URI: &str = "http://localhost:19287/callback";

#[derive(Deserialize)]
struct Request {
    jsonrpc: String,
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i64,
    message: String,
}

#[derive(Debug, thiserror::Error)]
enum AuthError {
    #[error("Jira site URL must be an https URL with a host")]
    InvalidSite,
    #[error("OAuth state mismatch")]
    StateMismatch,
    #[error("Jira authorization failed: {0}")]
    ProviderError(String),
    #[error("Cloud ID not found for site: {0}")]
    CloudIdNotFound(String),
    #[error("Timed out waiting for browser callback")]
    Timeout,
    #[error("OAuth callback could not start: {0}")]
    CallbackStart(String),
    #[error("OAuth request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("OAuth callback failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("OAuth URL could not be built: {0}")]
    Url(#[from] url::ParseError),
    #[error("Jira did not return a refresh token")]
    MissingRefreshToken,
    #[error("Jira plugin secrets are unavailable: {0}")]
    Secrets(String),
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AccessibleResource {
    id: String,
    url: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct Credentials {
    refresh_token: String,
    cloud_id: String,
    site: String,
}

struct PendingAuth {
    site: String,
    verifier: String,
    state: String,
    listener: TcpListener,
}

struct JiraPlugin {
    data_dir: PathBuf,
    secrets_dir: PathBuf,
    pending_auth: Option<PendingAuth>,
    completion: Option<tokio::task::JoinHandle<Result<(), AuthError>>>,
    authorization_error: Option<String>,
    client: Client,
    token_url: String,
    resources_url: String,
}

impl JiraPlugin {
    fn from_environment() -> Result<Self, String> {
        let data_dir = std::env::var_os("PLANEAI_PLUGIN_DATA_DIR")
            .map(PathBuf::from)
            .ok_or("PLANEAI_PLUGIN_DATA_DIR was not provided by the host")?;
        let secrets_dir = std::env::var_os("PLANEAI_PLUGIN_SECRETS_DIR")
            .map(PathBuf::from)
            .ok_or("PLANEAI_PLUGIN_SECRETS_DIR was not provided by the host")?;
        std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&secrets_dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        std::fs::set_permissions(&secrets_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            data_dir,
            secrets_dir,
            pending_auth: None,
            completion: None,
            authorization_error: None,
            client: Client::builder()
                .timeout(OAUTH_NETWORK_TIMEOUT)
                .build()
                .map_err(|error| error.to_string())?,
            token_url: TOKEN_URL.to_string(),
            resources_url: RESOURCES_URL.to_string(),
        })
    }

    fn settings(&self) -> Result<Value, String> {
        let path = self.data_dir.join("settings.json");
        if !path.exists() {
            return Ok(json!({ "site": "", "sync_interval_ms": 60000 }));
        }
        let value: Value = serde_json::from_reader(
            std::fs::File::open(path).map_err(|e| format!("failed to read settings: {e}"))?,
        )
        .map_err(|e| format!("failed to parse settings: {e}"))?;
        if !value.is_object() {
            return Err("plugin settings must be a JSON object".to_string());
        }
        Ok(value)
    }

    fn site(&self) -> Result<String, AuthError> {
        configured_site(&self.data_dir)
    }

    fn credentials_path(&self) -> PathBuf {
        self.secrets_dir.join("credentials.json")
    }

    fn read_credentials(&self) -> Result<Credentials, AuthError> {
        serde_json::from_reader(
            std::fs::File::open(self.credentials_path())
                .map_err(|error| AuthError::Secrets(error.to_string()))?,
        )
        .map_err(|error| AuthError::Secrets(error.to_string()))
    }

    fn delete_credentials(&self) -> Result<(), AuthError> {
        match std::fs::remove_file(self.credentials_path()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AuthError::Secrets(error.to_string())),
        }
    }

    async fn connect_start(&mut self) -> Result<Value, AuthError> {
        if self.pending_auth.is_some() || self.completion.is_some() {
            return Err(AuthError::CallbackStart(
                "an authorization flow is already waiting for a callback".to_string(),
            ));
        }
        let site = self.site()?;
        let listener = TcpListener::bind(CALLBACK_ADDRESS).await.map_err(|e| {
            AuthError::CallbackStart(format!(
                "failed to bind OAuth callback port 19287: {e}. Is another PlaneAI instance running?"
            ))
        })?;
        let (verifier, challenge) = generate_pkce();
        let state = generate_state();
        let authorization_url = build_auth_url(REDIRECT_URI, &challenge, &state)?;
        self.authorization_error = None;
        self.pending_auth = Some(PendingAuth {
            site,
            verifier,
            state,
            listener,
        });
        Ok(json!({ "authorization_url": authorization_url.to_string() }))
    }

    async fn connect_complete(&mut self) -> Result<Value, AuthError> {
        let pending = self.pending_auth.take().ok_or_else(|| {
            AuthError::CallbackStart("start authorization before completing it".to_string())
        })?;
        let client = self.client.clone();
        let token_url = self.token_url.clone();
        let resources_url = self.resources_url.clone();
        let secrets_dir = self.secrets_dir.clone();
        let data_dir = self.data_dir.clone();
        self.completion = Some(tokio::spawn(async move {
            finish_authorization(
                pending,
                &client,
                &token_url,
                &resources_url,
                &data_dir,
                &secrets_dir,
            )
            .await
        }));
        Ok(json!({ "authorizing": true }))
    }

    async fn status(&mut self) -> Value {
        self.reap_completion().await;
        let credentials = self.read_credentials().ok();
        json!({
            "plugin_id": PLUGIN_ID,
            "plugin_name": PLUGIN_NAME,
            "plugin_version": PLUGIN_VERSION,
            "host_api_version": HOST_API_VERSION,
            "runtime_state": "running",
            "last_error": self.authorization_error,
            "connected": credentials.is_some(),
            "authorizing": self.completion.is_some(),
            "site": credentials.map(|credentials| credentials.site),
        })
    }

    async fn reap_completion(&mut self) {
        if self
            .completion
            .as_ref()
            .is_some_and(|completion| completion.is_finished())
        {
            let result = self
                .completion
                .take()
                .expect("completion was checked above")
                .await;
            self.authorization_error = result
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()))
                .err();
        }
    }

    fn cancel_authorization(&mut self) {
        self.pending_auth = None;
        if let Some(completion) = self.completion.take() {
            completion.abort();
        }
    }

    fn connect_cancel(&mut self) {
        self.cancel_authorization();
    }

    fn disconnect(&mut self) -> Result<Value, AuthError> {
        self.cancel_authorization();
        self.delete_credentials()?;
        Ok(json!({ "connected": false }))
    }
}

fn configured_site(data_dir: &std::path::Path) -> Result<String, AuthError> {
    let value: Value = serde_json::from_reader(
        std::fs::File::open(data_dir.join("settings.json"))
            .map_err(|error| AuthError::Secrets(error.to_string()))?,
    )
    .map_err(|error| AuthError::Secrets(error.to_string()))?;
    let site = value
        .get("site")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|site| !site.is_empty())
        .ok_or(AuthError::InvalidSite)?;
    let parsed = Url::parse(site).map_err(|_| AuthError::InvalidSite)?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() || parsed.query().is_some() {
        return Err(AuthError::InvalidSite);
    }
    Ok(site.trim_end_matches('/').to_string())
}

fn write_credentials_to(
    secrets_dir: &std::path::Path,
    credentials: &Credentials,
) -> Result<(), AuthError> {
    let content =
        serde_json::to_vec(credentials).map_err(|error| AuthError::Secrets(error.to_string()))?;
    let temporary = secrets_dir.join(format!(".credentials-{}.tmp", generate_state()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&temporary)
        .map_err(|error| AuthError::Secrets(error.to_string()))?;
    if let Err(error) = file.write_all(&content).and_then(|()| file.sync_all()) {
        let _ = std::fs::remove_file(&temporary);
        return Err(AuthError::Secrets(error.to_string()));
    }
    std::fs::rename(&temporary, secrets_dir.join("credentials.json"))
        .map_err(|error| AuthError::Secrets(error.to_string()))
}

async fn finish_authorization(
    pending: PendingAuth,
    client: &Client,
    token_url: &str,
    resources_url: &str,
    data_dir: &std::path::Path,
    secrets_dir: &std::path::Path,
) -> Result<(), AuthError> {
    let code = tokio::time::timeout(
        CALLBACK_TIMEOUT,
        wait_for_callback(&pending.listener, &pending.state),
    )
    .await
    .map_err(|_| AuthError::Timeout)??;
    let token = exchange_code(client, token_url, &code, &pending.verifier).await?;
    let refresh_token = token.refresh_token.ok_or(AuthError::MissingRefreshToken)?;
    let cloud_id =
        fetch_cloud_id(client, resources_url, &pending.site, &token.access_token).await?;
    if configured_site(data_dir)? != pending.site {
        return Err(AuthError::CallbackStart(
            "Jira site changed while authorization was in progress; reconnect for the new site"
                .to_string(),
        ));
    }
    write_credentials_to(
        secrets_dir,
        &Credentials {
            refresh_token,
            cloud_id,
            site: pending.site,
        },
    )
}

async fn exchange_code(
    client: &Client,
    token_url: &str,
    code: &str,
    verifier: &str,
) -> Result<TokenResponse, AuthError> {
    client
        .post(token_url)
        .json(&json!({
            "grant_type": "authorization_code",
            "client_id": CLIENT_ID,
            "client_secret": CLIENT_SECRET,
            "code": code,
            "redirect_uri": REDIRECT_URI,
            "code_verifier": verifier,
        }))
        .send()
        .await?
        .error_for_status()?
        .json::<TokenResponse>()
        .await
        .map_err(AuthError::from)
}

async fn fetch_cloud_id(
    client: &Client,
    resources_url: &str,
    site: &str,
    access_token: &str,
) -> Result<String, AuthError> {
    let resources = client
        .get(resources_url)
        .bearer_auth(access_token)
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<AccessibleResource>>()
        .await?;
    resources
        .iter()
        .find(|resource| resource.url.trim_end_matches('/') == site)
        .map(|resource| resource.id.clone())
        .ok_or_else(|| AuthError::CloudIdNotFound(site.to_string()))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut plugin = JiraPlugin::from_environment().map_err(std::io::Error::other)?;
    let mut input = BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    while let Some(line) = read_json_rpc_frame(&mut input).await? {
        let request = match serde_json::from_str::<Request>(&line) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("malformed JSON-RPC request: {error}");
                continue;
            }
        };
        let should_shutdown = is_valid_shutdown_request(&request);
        let response = dispatch(&mut plugin, request).await;
        output
            .write_all(encode_response_frame(response)?.as_bytes())
            .await?;
        output.write_all(b"\n").await?;
        output.flush().await?;
        if should_shutdown {
            break;
        }
    }
    Ok(())
}

async fn read_json_rpc_frame<R>(reader: &mut R) -> Result<Option<String>, std::io::Error>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    let mut frame = Vec::new();
    let bytes_read = reader
        .take(MAX_RPC_FRAME_BYTES)
        .read_until(b'\n', &mut frame)
        .await?;
    if bytes_read == 0 {
        return Ok(None);
    }
    if !frame.ends_with(b"\n") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "JSON-RPC request exceeded the frame limit or was not newline terminated",
        ));
    }
    String::from_utf8(frame)
        .map(Some)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

async fn dispatch(plugin: &mut JiraPlugin, request: Request) -> Response {
    if !matches!(
        request.id,
        Value::Null | Value::String(_) | Value::Number(_)
    ) {
        return error(Value::Null, -32600, "expected a scalar JSON-RPC id");
    }
    if request.jsonrpc != "2.0" {
        return error(request.id, -32600, "expected jsonrpc 2.0");
    }
    let result = match request.method.as_str() {
        "plugin.handshake" => match request
            .params
            .get("host_api_version")
            .and_then(Value::as_str)
        {
            Some(HOST_API_VERSION) => Ok(json!({
                "plugin_id": PLUGIN_ID,
                "plugin_name": PLUGIN_NAME,
                "plugin_version": PLUGIN_VERSION,
                "host_api_version": HOST_API_VERSION,
            })),
            _ => Err("unsupported plugin host API version".to_string()),
        },
        "jira.status" => Ok(plugin.status().await),
        "jira.settings.get" => plugin.settings(),
        "jira.connect.start" => plugin
            .connect_start()
            .await
            .map_err(|error| error.to_string()),
        "jira.connect.complete" => plugin
            .connect_complete()
            .await
            .map_err(|error| error.to_string()),
        "jira.connect.cancel" => {
            plugin.connect_cancel();
            Ok(json!({ "cancelled": true }))
        }
        "jira.disconnect" => plugin.disconnect().map_err(|error| error.to_string()),
        "plugin.shutdown" => Ok(json!({ "stopping": true })),
        _ => Err("method not found".to_string()),
    };
    match result {
        Ok(value) => success(request.id, value),
        Err(message) => error(request.id, -32000, &message),
    }
}

fn success(id: Value, result: Value) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn error(id: Value, code: i64, message: &str) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(RpcError {
            code,
            message: message.to_string(),
        }),
    }
}

fn encode_response_frame(response: Response) -> Result<String, serde_json::Error> {
    let frame = serde_json::to_string(&response)?;
    if frame.len() < MAX_RPC_FRAME_BYTES as usize {
        return Ok(frame);
    }
    serde_json::to_string(&error(
        response.id,
        -32000,
        "plugin response exceeded the maximum frame size",
    ))
}

fn is_valid_shutdown_request(request: &Request) -> bool {
    request.jsonrpc == "2.0"
        && request.method == "plugin.shutdown"
        && matches!(
            request.id,
            Value::Null | Value::String(_) | Value::Number(_)
        )
}

fn generate_pkce() -> (String, String) {
    let verifier_bytes: Vec<u8> = (0..32).map(|_| rand::rng().random()).collect();
    let verifier = URL_SAFE_NO_PAD.encode(&verifier_bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn generate_state() -> String {
    let bytes: Vec<u8> = (0..16).map(|_| rand::rng().random()).collect();
    URL_SAFE_NO_PAD.encode(bytes)
}

fn build_auth_url(redirect_uri: &str, challenge: &str, state: &str) -> Result<Url, AuthError> {
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

async fn wait_for_callback(
    listener: &TcpListener,
    expected_state: &str,
) -> Result<String, AuthError> {
    loop {
        let (stream, _) = listener.accept().await?;
        let (reader, mut writer) = stream.into_split();
        let reader = BufReader::new(reader);
        let mut request_line = Vec::new();
        let bytes_read = match tokio::time::timeout(
            CALLBACK_REQUEST_TIMEOUT,
            reader
                .take(MAX_CALLBACK_REQUEST_LINE_BYTES)
                .read_until(b'\n', &mut request_line),
        )
        .await
        {
            Ok(Ok(bytes_read)) => bytes_read,
            Ok(Err(error)) => return Err(AuthError::Io(error)),
            Err(_) => {
                write_callback_response(&mut writer, "408 Request Timeout", "Request timed out.")
                    .await;
                continue;
            }
        };
        let request_line = match (bytes_read > 0 && request_line.ends_with(b"\n"))
            .then(|| String::from_utf8(request_line))
        {
            Some(Ok(line)) => line,
            _ => {
                write_callback_response(
                    &mut writer,
                    "400 Bad Request",
                    "Invalid callback request.",
                )
                .await;
                continue;
            }
        };
        let mut request_parts = request_line.split_whitespace();
        let (Some("GET"), Some(path)) = (request_parts.next(), request_parts.next()) else {
            write_callback_response(&mut writer, "400 Bad Request", "Invalid callback request.")
                .await;
            continue;
        };
        if !path.starts_with("/callback") {
            write_callback_response(&mut writer, "400 Bad Request", "Invalid callback request.")
                .await;
            continue;
        }
        let parsed = match Url::parse(&format!("http://localhost{path}")) {
            Ok(parsed) => parsed,
            Err(_) => {
                write_callback_response(
                    &mut writer,
                    "400 Bad Request",
                    "Invalid callback request.",
                )
                .await;
                continue;
            }
        };
        let params: HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        if params.get("state").map(String::as_str) != Some(expected_state) {
            write_callback_response(
                &mut writer,
                "400 Bad Request",
                "Authentication failed: state mismatch.",
            )
            .await;
            return Err(AuthError::StateMismatch);
        }
        if let Some(provider_error) = params.get("error") {
            let description = params
                .get("error_description")
                .map(|description| format!(": {description}"))
                .unwrap_or_default();
            write_callback_response(
                &mut writer,
                "400 Bad Request",
                "Authentication failed. You can close this tab.",
            )
            .await;
            return Err(AuthError::ProviderError(format!(
                "{provider_error}{description}"
            )));
        }
        let Some(code) = params.get("code").cloned() else {
            write_callback_response(
                &mut writer,
                "400 Bad Request",
                "No authorization code received.",
            )
            .await;
            continue;
        };
        write_callback_response(
            &mut writer,
            "200 OK",
            "Authentication successful! You can close this tab.",
        )
        .await;
        return Ok(code);
    }
}

async fn write_callback_response(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    status: &str,
    body: &str,
) {
    let _ = writer
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn pkce_is_url_safe_and_uses_s256() {
        let (verifier, challenge) = generate_pkce();
        assert!(verifier.len() >= 43 && verifier.len() <= 128);
        assert!(verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_eq!(
            challenge,
            URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
        );
    }

    #[tokio::test]
    async fn callback_rejects_a_mismatched_state() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let wait = tokio::spawn(async move { wait_for_callback(&listener, "expected").await });
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        stream
            .write_all(b"GET /callback?code=abc&state=wrong HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        assert!(matches!(wait.await.unwrap(), Err(AuthError::StateMismatch)));
    }

    #[tokio::test]
    async fn exchange_and_cloud_lookup_use_the_plugin_backend_client() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("authorization_code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"access_token":"access","refresh_token":"refresh","expires_in":3600}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/resources"))
            .and(header("Authorization", "Bearer access"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!([{ "id":"cloud", "url":"https://example.atlassian.net" }]),
                ),
            )
            .mount(&server)
            .await;
        let root =
            std::env::temp_dir().join(format!("planeai-jira-plugin-test-{}", generate_state()));
        let plugin = JiraPlugin {
            data_dir: root.join("data"),
            secrets_dir: root.join("secrets"),
            pending_auth: None,
            completion: None,
            authorization_error: None,
            client: Client::new(),
            token_url: format!("{}/oauth/token", server.uri()),
            resources_url: format!("{}/resources", server.uri()),
        };
        std::fs::create_dir_all(&plugin.data_dir).unwrap();
        std::fs::create_dir_all(&plugin.secrets_dir).unwrap();
        let token = exchange_code(&plugin.client, &plugin.token_url, "code", "verifier")
            .await
            .unwrap();
        assert_eq!(
            fetch_cloud_id(
                &plugin.client,
                &plugin.resources_url,
                "https://example.atlassian.net",
                &token.access_token,
            )
            .await
            .unwrap(),
            "cloud"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn callback_ignores_malformed_requests_until_a_valid_callback_arrives() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let wait = tokio::spawn(async move { wait_for_callback(&listener, "expected").await });

        let mut malformed = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        malformed.write_all(b"not an HTTP request\n").await.unwrap();
        let mut valid = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        valid
            .write_all(b"GET /callback?code=abc&state=expected HTTP/1.1\r\n\r\n")
            .await
            .unwrap();

        assert_eq!(wait.await.unwrap().unwrap(), "abc");
    }

    #[test]
    fn credentials_are_replaced_atomically_with_private_permissions() {
        let root =
            std::env::temp_dir().join(format!("planeai-jira-plugin-test-{}", generate_state()));
        let plugin = JiraPlugin {
            data_dir: root.join("data"),
            secrets_dir: root.join("secrets"),
            pending_auth: None,
            completion: None,
            authorization_error: None,
            client: Client::new(),
            token_url: TOKEN_URL.to_string(),
            resources_url: RESOURCES_URL.to_string(),
        };
        std::fs::create_dir_all(&plugin.secrets_dir).unwrap();
        write_credentials_to(
            &plugin.secrets_dir,
            &Credentials {
                refresh_token: "first".to_string(),
                cloud_id: "cloud-a".to_string(),
                site: "https://example.atlassian.net".to_string(),
            },
        )
        .unwrap();
        write_credentials_to(
            &plugin.secrets_dir,
            &Credentials {
                refresh_token: "second".to_string(),
                cloud_id: "cloud-b".to_string(),
                site: "https://example.atlassian.net".to_string(),
            },
        )
        .unwrap();

        let credentials = plugin.read_credentials().unwrap();
        assert_eq!(credentials.refresh_token, "second");
        assert_eq!(credentials.cloud_id, "cloud-b");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(plugin.credentials_path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert!(std::fs::read_dir(&plugin.secrets_dir)
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn oversized_responses_are_replaced_with_a_bounded_error_frame() {
        let frame = encode_response_frame(success(
            json!(1),
            json!({ "site": "x".repeat(MAX_RPC_FRAME_BYTES as usize) }),
        ))
        .unwrap();
        assert!(frame.len() < MAX_RPC_FRAME_BYTES as usize);
        assert!(frame.contains("maximum frame size"));
    }

    #[test]
    fn malformed_shutdown_request_does_not_terminate_the_plugin() {
        let request = Request {
            jsonrpc: "2.0".to_string(),
            id: json!(["not-scalar"]),
            method: "plugin.shutdown".to_string(),
            params: Value::Null,
        };
        assert!(!is_valid_shutdown_request(&request));
    }
}
