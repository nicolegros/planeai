use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngExt;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use url::Url;

include!(concat!(env!("OUT_DIR"), "/oauth_credentials.rs"));
const AUTH_URL: &str = "https://auth.atlassian.com/authorize";
const TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
const RESOURCES_URL: &str = "https://api.atlassian.com/oauth/token/accessible-resources";
const SCOPES: &str = "read:jira-work write:jira-work offline_access";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECTION_CLEARED_KEY: &str = "connection_cleared";

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
    #[error("Token not found: {0}")]
    TokenNotFound(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),
    #[error("Timed out waiting for browser callback")]
    Timeout,
    #[error("OAuth token request failed: {0}")]
    TokenRequestFailed(String),
    #[error("OAuth refresh token was rejected; reconnect to Jira")]
    RefreshTokenRejected,
    #[error("Jira OAuth credentials could not be durably cleared; repair local credential storage before reconnecting")]
    ConnectionStateNotDurablyCleared,
    #[error("Jira OAuth connection is disconnected; reconnect to Jira")]
    ConnectionCleared,
    #[error("Jira OAuth authorization is already active or in progress")]
    ConnectionAlreadyActive,
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
        match std::fs::read_to_string(self.dir.join(key)) {
            Ok(value) => Ok(value.trim().to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(Error::TokenNotFound(key.to_string()))
            }
            Err(error) => Err(Error::Keyring(error.to_string())),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), Error> {
        let path = self.dir.join(key);
        let (temporary_path, mut temporary) =
            create_token_temp_file(&self.dir, key).map_err(|e| Error::Keyring(e.to_string()))?;
        if let Err(error) = temporary
            .write_all(value.as_bytes())
            .and_then(|()| temporary.sync_all())
        {
            let _ = std::fs::remove_file(&temporary_path);
            return Err(Error::Keyring(error.to_string()));
        }
        drop(temporary);
        if let Err(error) = replace_token_file(&temporary_path, &path) {
            let _ = std::fs::remove_file(&temporary_path);
            return Err(Error::Keyring(error.to_string()));
        }
        sync_token_directory(&self.dir).map_err(|e| Error::Keyring(e.to_string()))?;
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), Error> {
        match std::fs::remove_file(self.dir.join(key)) {
            Ok(()) => {
                sync_token_directory(&self.dir).map_err(|e| Error::Keyring(e.to_string()))?;
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::Keyring(e.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
struct TokenState {
    access_token: String,
    expires_at: std::time::Instant,
}

fn persisted_connection_is_healthy(store: &dyn TokenStore) -> bool {
    let marker_is_healthy = match store.get(CONNECTION_CLEARED_KEY) {
        Ok(state) => state == "false",
        Err(Error::TokenNotFound(_)) => true,
        Err(_) => false,
    };
    marker_is_healthy && store.get("refresh_token").is_ok() && store.get("cloud_id").is_ok()
}

fn create_token_temp_file(
    dir: &std::path::Path,
    key: &str,
) -> std::io::Result<(std::path::PathBuf, std::fs::File)> {
    for _ in 0..10 {
        let path = dir.join(format!(".{key}.{:016x}.tmp", rand::rng().random::<u64>()));
        #[cfg(unix)]
        let file = {
            use std::fs::OpenOptions;
            use std::os::unix::fs::OpenOptionsExt;

            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
        };
        #[cfg(not(unix))]
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path);
        match file {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate unique Jira token temporary file",
    ))
}

fn sync_token_directory(dir: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    std::fs::File::open(dir)?.sync_all()?;
    Ok(())
}

#[cfg(not(windows))]
fn replace_token_file(
    temporary_path: &std::path::Path,
    path: &std::path::Path,
) -> std::io::Result<()> {
    std::fs::rename(temporary_path, path)
}

#[cfg(windows)]
fn replace_token_file(
    temporary_path: &std::path::Path,
    path: &std::path::Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    if !path.exists() {
        return std::fs::rename(temporary_path, path);
    }

    extern "system" {
        fn ReplaceFileW(
            replaced_file_name: *const u16,
            replacement_file_name: *const u16,
            backup_file_name: *const u16,
            replace_flags: u32,
            exclude: *mut core::ffi::c_void,
            reserved: *mut core::ffi::c_void,
        ) -> i32;
    }

    let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let temporary_wide: Vec<u16> = temporary_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let replaced = unsafe {
        ReplaceFileW(
            path_wide.as_ptr(),
            temporary_wide.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if replaced == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

type ConnectionStateListener = std::sync::Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct TokenErrorResponse {
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AccessibleResource {
    id: String,
    url: String,
}

pub struct JiraAuth {
    site: String,
    token_state: Mutex<Option<TokenState>>,
    refresh_lock: Mutex<()>,
    store: std::sync::Arc<dyn TokenStore>,
    client: Client,
    token_url: String,
    resources_url: String,
    connection_cleared: AtomicBool,
    connection_generation: std::sync::atomic::AtomicU64,
    sync_cancellation: std::sync::Mutex<Option<CancellationToken>>,
    // Carries the attempt generation so a stale connect exit cannot clear a newer attempt.
    connect_cancellation: std::sync::Mutex<Option<(u64, CancellationToken)>>,
    connection_state_listeners: std::sync::Mutex<Vec<ConnectionStateListener>>,
}

impl JiraAuth {
    pub fn new(site: &str, token_dir: std::path::PathBuf) -> Self {
        Self::with_store(site, Box::new(FileStore::new(token_dir)))
    }

    pub fn with_store(site: &str, store: Box<dyn TokenStore>) -> Self {
        let connection_cleared = !persisted_connection_is_healthy(store.as_ref());
        Self {
            site: site.to_string(),
            token_state: Mutex::new(None),
            refresh_lock: Mutex::new(()),
            store: store.into(),
            client: Client::new(),
            token_url: TOKEN_URL.to_string(),
            resources_url: RESOURCES_URL.to_string(),
            connection_cleared: AtomicBool::new(connection_cleared),
            connection_generation: std::sync::atomic::AtomicU64::new(0),
            sync_cancellation: std::sync::Mutex::new(None),
            connect_cancellation: std::sync::Mutex::new(None),
            connection_state_listeners: std::sync::Mutex::new(Vec::new()),
        }
    }

    #[cfg(test)]
    fn with_test_config(
        site: &str,
        store: Box<dyn TokenStore>,
        token_url: String,
        resources_url: String,
    ) -> Self {
        let connection_cleared = !persisted_connection_is_healthy(store.as_ref());
        Self {
            site: site.to_string(),
            token_state: Mutex::new(None),
            refresh_lock: Mutex::new(()),
            store: store.into(),
            client: Client::new(),
            token_url,
            resources_url,
            connection_cleared: AtomicBool::new(connection_cleared),
            connection_generation: std::sync::atomic::AtomicU64::new(0),
            sync_cancellation: std::sync::Mutex::new(None),
            connect_cancellation: std::sync::Mutex::new(None),
            connection_state_listeners: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Create a JiraAuth with a pre-loaded access token for integration/client tests.
    #[cfg(test)]
    pub(crate) fn with_fixed_token(token: &str, token_url: String) -> Self {
        let store = crate::test_support::MemStore::with_entries(vec![
            ("refresh_token", "test_refresh"),
            ("cloud_id", "test-cloud"),
            (CONNECTION_CLEARED_KEY, "false"),
        ]);

        let token_state = Some(TokenState {
            access_token: token.to_string(),
            expires_at: std::time::Instant::now() + std::time::Duration::from_secs(3600),
        });

        Self {
            site: "https://test.atlassian.net".to_string(),
            token_state: Mutex::new(token_state),
            refresh_lock: Mutex::new(()),
            store: std::sync::Arc::new(store),
            client: Client::new(),
            token_url,
            resources_url: String::new(),
            connection_cleared: AtomicBool::new(false),
            connection_generation: std::sync::atomic::AtomicU64::new(0),
            sync_cancellation: std::sync::Mutex::new(None),
            connect_cancellation: std::sync::Mutex::new(None),
            connection_state_listeners: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Associate the current background sync loop with this authentication state.
    /// A rejected refresh token cancels that loop so the app can show reconnect-required.
    pub fn set_sync_cancellation(&self, cancellation: CancellationToken) {
        if let Ok(mut current) = self.sync_cancellation.lock() {
            *current = Some(cancellation);
        }
    }

    /// Notify the host application after OAuth connection state changes.
    /// Multiple consumers need this signal: the UI refreshes its indicator and the app runtime
    /// drops stale sync/writeback state before a reconnect is allowed to activate again.
    pub fn set_connection_state_listener(&self, listener: ConnectionStateListener) {
        if let Ok(mut listeners) = self.connection_state_listeners.lock() {
            listeners.push(listener);
        }
    }

    /// Reserve a connection attempt before binding the callback port or opening a browser.
    /// This makes direct callers safe too; the Tauri command has a matching handle-level guard.
    fn reserve_connection_attempt(&self) -> Result<u64, Error> {
        self.connection_cleared
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::ConnectionAlreadyActive)?;
        Ok(self.connection_generation.load(Ordering::Acquire))
    }

    /// Install cancellation before any browser-visible OAuth work. `disconnect` takes this same
    /// mutex, so it either cancels this token or is observed before the caller continues.
    fn install_connect_cancellation(
        &self,
        connection_generation: u64,
    ) -> Result<CancellationToken, Error> {
        let cancellation = CancellationToken::new();
        let mut pending = self
            .connect_cancellation
            .lock()
            .map_err(|_| Error::ConnectionCleared)?;
        if self.connection_generation.load(Ordering::Acquire) != connection_generation
            || self.connection_cleared.load(Ordering::Acquire)
        {
            return Err(Error::ConnectionCleared);
        }
        *pending = Some((connection_generation, cancellation.clone()));
        Ok(cancellation)
    }

    /// Clear only the cancellation token installed by this attempt. A reconnect may begin after
    /// disconnect while an older attempt is still unwinding, so an unconditional clear would
    /// make the new attempt uncancellable.
    fn clear_connect_cancellation(&self, connection_generation: u64) {
        if let Ok(mut pending) = self.connect_cancellation.lock() {
            if pending
                .as_ref()
                .is_some_and(|(generation, _)| *generation == connection_generation)
            {
                *pending = None;
            }
        }
    }

    fn abandon_connection_attempt(&self, connection_generation: u64) {
        if self.connection_generation.load(Ordering::Acquire) == connection_generation {
            self.connection_cleared.store(true, Ordering::Release);
        }
    }

    pub async fn connect(&self) -> Result<(), Error> {
        let connection_generation = self.reserve_connection_attempt()?;
        let cancellation = match self.install_connect_cancellation(connection_generation) {
            Ok(cancellation) => cancellation,
            Err(error) => {
                self.abandon_connection_attempt(connection_generation);
                return Err(error);
            }
        };

        let result = async {
            let listener = TcpListener::bind("127.0.0.1:19287").await.map_err(|e| {
                Error::Io(std::io::Error::new(
                    e.kind(),
                    format!(
                        "failed to bind OAuth callback port 19287: {e}. \
                         Is another instance of planeai already running?"
                    ),
                ))
            })?;
            self.ensure_connection_generation(connection_generation)?;
            if cancellation.is_cancelled() {
                return Err(Error::ConnectionCleared);
            }
            let redirect_uri = "http://localhost:19287/callback".to_string();

            let (verifier, challenge) = generate_pkce();
            let state = generate_state();
            let auth_url = build_auth_url(&redirect_uri, &challenge, &state)?;
            if cancellation.is_cancelled() {
                return Err(Error::ConnectionCleared);
            }
            if open::that(auth_url.as_str()).is_err() {
                tracing::warn!("failed to open browser. Visit: {auth_url}");
            }

            let code = tokio::select! {
                _ = cancellation.cancelled() => Err(Error::ConnectionCleared),
                result = tokio::time::timeout(CALLBACK_TIMEOUT, wait_for_callback(&listener, &state)) => {
                    result.map_err(|_| Error::Timeout)?
                }
            }?;
            let token_resp = tokio::select! {
                _ = cancellation.cancelled() => Err(Error::ConnectionCleared),
                result = self.exchange_code(&code, &redirect_uri, &verifier) => result,
            }?;
            tokio::select! {
                _ = cancellation.cancelled() => Err(Error::ConnectionCleared),
                result = self.complete_connection(&token_resp, connection_generation) => result,
            }
        }
        .await;

        if result.is_err() {
            self.abandon_connection_attempt(connection_generation);
        }
        // Do not clear before exchange/complete: disconnect must cancel every await in the
        // connect flow. Generation matching preserves a newer reconnect's cancellation token.
        self.clear_connect_cancellation(connection_generation);
        result
    }

    pub async fn disconnect(&self) -> Result<(), Error> {
        self.begin_connection_clear();
        let _refresh = self.refresh_lock.lock().await;
        self.clear_connection_locked().await
    }

    /// Complete initial authorization only after the selected Jira cloud is known.
    async fn complete_connection(
        &self,
        token_resp: &TokenResponse,
        connection_generation: u64,
    ) -> Result<(), Error> {
        {
            let _refresh = self.refresh_lock.lock().await;
            self.ensure_connection_generation(connection_generation)?;
        }
        let cloud_id = self.fetch_cloud_id(&token_resp.access_token).await?;
        let _refresh = self.refresh_lock.lock().await;
        self.ensure_connection_generation(connection_generation)?;
        self.set_token("cloud_id", cloud_id).await?;
        self.ensure_connection_generation(connection_generation)?;
        self.store_tokens(token_resp, connection_generation).await
    }

    fn ensure_connection_generation(&self, connection_generation: u64) -> Result<(), Error> {
        (self.connection_generation.load(Ordering::Acquire) == connection_generation)
            .then_some(())
            .ok_or(Error::ConnectionCleared)
    }

    fn begin_connection_clear(&self) {
        self.connection_generation.fetch_add(1, Ordering::AcqRel);
        self.connection_cleared.store(true, Ordering::Release);
        if let Ok(mut cancellation) = self.sync_cancellation.lock() {
            if let Some(cancellation) = cancellation.take() {
                cancellation.cancel();
            }
        }
        if let Ok(mut cancellation) = self.connect_cancellation.lock() {
            if let Some((_, cancellation)) = cancellation.take() {
                cancellation.cancel();
            }
        }
        let listeners = self
            .connection_state_listeners
            .lock()
            .map(|listeners| listeners.clone())
            .unwrap_or_default();
        for listener in listeners {
            listener();
        }
    }

    /// Remove OAuth connection state without touching Jira settings, cache, or task links.
    /// The caller holds refresh_lock so an in-flight refresh cannot restore credentials afterward.
    async fn clear_connection_locked(&self) -> Result<(), Error> {
        let marker_persisted = match self
            .set_token(CONNECTION_CLEARED_KEY, "true".to_string())
            .await
        {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, "failed to persist disconnected Jira OAuth state");
                false
            }
        };
        let mut credentials_removed = true;
        for key in ["refresh_token", "cloud_id"] {
            if let Err(error) = self.delete_token(key).await {
                credentials_removed = false;
                tracing::warn!(%error, key, "failed to remove Jira OAuth state from disk");
            }
        }
        *self.token_state.lock().await = None;

        (marker_persisted && credentials_removed)
            .then_some(())
            .ok_or(Error::ConnectionStateNotDurablyCleared)
    }

    pub async fn access_token(&self) -> Result<String, Error> {
        if self.connection_cleared.load(Ordering::Acquire) {
            return Err(Error::ConnectionCleared);
        }
        if let Some(token) = self.valid_cached_token().await {
            return Ok(token);
        }

        // Refresh-token rotation invalidates the previous token. Serialize refreshes and
        // recheck the cache after acquiring the lock so concurrent callers share one grant.
        let _refresh = self.refresh_lock.lock().await;
        if self.connection_cleared.load(Ordering::Acquire) {
            return Err(Error::ConnectionCleared);
        }
        if let Some(token) = self.valid_cached_token().await {
            return Ok(token);
        }
        if !self.is_connected() {
            return Err(Error::ConnectionCleared);
        }

        let connection_generation = self.connection_generation.load(Ordering::Acquire);
        self.refresh(connection_generation).await
    }

    async fn valid_cached_token(&self) -> Option<String> {
        let state = self.token_state.lock().await;
        state.as_ref().and_then(|token| {
            (token.expires_at > std::time::Instant::now() + Duration::from_secs(60))
                .then(|| token.access_token.clone())
        })
    }

    /// Clear a cached token only when it is the token that received a 401 response.
    /// This prevents a delayed 401 from invalidating a token refreshed by another request.
    pub async fn invalidate_token_if_matches(&self, failed_token: &str) {
        let mut state = self.token_state.lock().await;
        if state
            .as_ref()
            .is_some_and(|token| token.access_token == failed_token)
        {
            *state = None;
        }
    }

    pub fn is_connected(&self) -> bool {
        !self.connection_cleared.load(Ordering::Acquire)
            && persisted_connection_is_healthy(self.store.as_ref())
    }

    /// Return whether this auth instance remains locally active without reading token storage.
    pub fn is_connection_active(&self) -> bool {
        !self.connection_cleared.load(Ordering::Acquire)
    }

    /// Jira Cloud site this auth instance was constructed to authorize.
    pub fn site(&self) -> &str {
        &self.site
    }

    async fn set_token(&self, key: &'static str, value: String) -> Result<(), Error> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.set(key, &value))
            .await
            .map_err(|error| Error::Keyring(format!("token storage task failed: {error}")))?
    }

    async fn delete_token(&self, key: &'static str) -> Result<(), Error> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.delete(key))
            .await
            .map_err(|error| Error::Keyring(format!("token cleanup task failed: {error}")))?
    }

    pub fn cloud_id(&self) -> Result<String, Error> {
        if self.connection_cleared.load(Ordering::Acquire) {
            return Err(Error::ConnectionCleared);
        }
        self.store.get("cloud_id")
    }

    async fn refresh(&self, connection_generation: u64) -> Result<String, Error> {
        let refresh_token = self.store.get("refresh_token")?;

        let body = serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": CLIENT_ID,
            "client_secret": CLIENT_SECRET,
            "refresh_token": refresh_token,
        });

        let response = self.client.post(&self.token_url).json(&body).send().await?;
        let resp = match decode_token_response(response).await {
            Err(Error::TokenRequestFailed(error)) if error == "invalid_grant" => {
                self.begin_connection_clear();
                self.clear_connection_locked().await?;
                return Err(Error::RefreshTokenRejected);
            }
            result => result?,
        };

        if self.connection_cleared.load(Ordering::Acquire) {
            return Err(Error::ConnectionCleared);
        }
        self.store_tokens(&resp, connection_generation).await?;
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

        let response = self.client.post(&self.token_url).json(&body).send().await?;
        decode_token_response(response).await
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

    async fn store_tokens(
        &self,
        resp: &TokenResponse,
        connection_generation: u64,
    ) -> Result<(), Error> {
        self.ensure_connection_generation(connection_generation)?;
        if let Some(rt) = &resp.refresh_token {
            self.set_token("refresh_token", rt.clone()).await?;
            self.ensure_connection_generation(connection_generation)?;
            self.set_token(CONNECTION_CLEARED_KEY, "false".to_string())
                .await?;
            self.ensure_connection_generation(connection_generation)?;
            self.connection_cleared.store(false, Ordering::Release);
        }
        self.ensure_connection_generation(connection_generation)?;
        let ts = TokenState {
            access_token: resp.access_token.clone(),
            expires_at: std::time::Instant::now() + Duration::from_secs(resp.expires_in),
        };
        *self.token_state.lock().await = Some(ts);
        Ok(())
    }
}

async fn decode_token_response(response: reqwest::Response) -> Result<TokenResponse, Error> {
    if response.status().is_success() {
        return response.json::<TokenResponse>().await.map_err(Error::from);
    }

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let error = serde_json::from_str::<TokenErrorResponse>(&body)
        .ok()
        .and_then(|response| response.error)
        .filter(|error| !error.is_empty())
        .unwrap_or_else(|| {
            if body.is_empty() {
                status.to_string()
            } else {
                body
            }
        });

    Err(Error::TokenRequestFailed(error))
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

    struct FailingDeleteStore {
        entries: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
        fail_refresh_token_delete: bool,
        fail_cloud_id_delete: bool,
        fail_connection_marker_write: bool,
    }

    impl FailingDeleteStore {
        fn new(
            entries: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
            fail_refresh_token_delete: bool,
            fail_cloud_id_delete: bool,
            fail_connection_marker_write: bool,
        ) -> Self {
            Self {
                entries,
                fail_refresh_token_delete,
                fail_cloud_id_delete,
                fail_connection_marker_write,
            }
        }
    }

    impl TokenStore for FailingDeleteStore {
        fn get(&self, key: &str) -> Result<String, Error> {
            self.entries
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .ok_or_else(|| Error::TokenNotFound(key.to_string()))
        }

        fn set(&self, key: &str, value: &str) -> Result<(), Error> {
            if self.fail_connection_marker_write && key == CONNECTION_CLEARED_KEY {
                return Err(Error::Keyring("simulated marker write failure".to_string()));
            }
            self.entries
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), Error> {
            if (self.fail_refresh_token_delete && key == "refresh_token")
                || (self.fail_cloud_id_delete && key == "cloud_id")
            {
                return Err(Error::Keyring("simulated delete failure".to_string()));
            }
            self.entries.lock().unwrap().remove(key);
            Ok(())
        }
    }
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
        store.set(CONNECTION_CLEARED_KEY, "false").unwrap();
        store.set("cloud_id", "cloud-123").unwrap();

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
    async fn concurrent_access_token_calls_share_one_refresh() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("refresh_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "refreshed_access",
                "refresh_token": "new_refresh",
                "expires_in": 3600
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let store = MemStore::new();
        store.set("refresh_token", "old_refresh").unwrap();
        store.set(CONNECTION_CLEARED_KEY, "false").unwrap();
        store.set("cloud_id", "cloud-123").unwrap();
        let auth = JiraAuth::with_test_config(
            "https://mysite.atlassian.net",
            Box::new(store),
            format!("{}/oauth/token", mock_server.uri()),
            String::new(),
        );

        let (first, second) = tokio::join!(auth.access_token(), auth.access_token());
        assert_eq!(first.unwrap(), "refreshed_access");
        assert_eq!(second.unwrap(), "refreshed_access");
        mock_server.verify().await;
    }

    #[tokio::test]
    async fn rejected_refresh_clears_connection_for_reconnect() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("refresh_token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant"
            })))
            .mount(&mock_server)
            .await;

        let store = MemStore::with_entries(vec![
            ("refresh_token", "revoked_refresh"),
            ("cloud_id", "cloud-123"),
            (CONNECTION_CLEARED_KEY, "false"),
        ]);
        let auth = JiraAuth::with_test_config(
            "https://mysite.atlassian.net",
            Box::new(store),
            format!("{}/oauth/token", mock_server.uri()),
            String::new(),
        );

        let cancellation = CancellationToken::new();
        auth.set_sync_cancellation(cancellation.clone());
        let state_changed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let state_changed_listener = state_changed.clone();
        auth.set_connection_state_listener(std::sync::Arc::new(move || {
            state_changed_listener.store(true, std::sync::atomic::Ordering::SeqCst);
        }));

        assert!(matches!(
            auth.access_token().await,
            Err(Error::RefreshTokenRejected)
        ));
        assert!(!auth.is_connected());
        assert!(auth.cloud_id().is_err());
        assert!(cancellation.is_cancelled());
        assert!(state_changed.load(std::sync::atomic::Ordering::SeqCst));
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
    async fn is_connected_requires_an_explicit_healthy_marker() {
        let store = MemStore::with_entries(vec![
            ("refresh_token", "tok"),
            ("cloud_id", "cloud-123"),
            (CONNECTION_CLEARED_KEY, "false"),
        ]);
        let auth = JiraAuth::with_test_config(
            "https://x.atlassian.net",
            Box::new(store),
            String::new(),
            String::new(),
        );
        assert!(auth.is_connected());

        let legacy_store = MemStore::with_entries(vec![
            ("refresh_token", "legacy-refresh"),
            ("cloud_id", "legacy-cloud"),
        ]);
        let legacy_auth = JiraAuth::with_test_config(
            "https://x.atlassian.net",
            Box::new(legacy_store),
            String::new(),
            String::new(),
        );
        assert!(legacy_auth.is_connected());

        let corrupt_marker = MemStore::with_entries(vec![
            ("refresh_token", "tok"),
            ("cloud_id", "cloud-123"),
            (CONNECTION_CLEARED_KEY, "corrupt"),
        ]);
        let auth_with_corrupt_marker = JiraAuth::with_test_config(
            "https://x.atlassian.net",
            Box::new(corrupt_marker),
            String::new(),
            String::new(),
        );
        assert!(!auth_with_corrupt_marker.is_connected());
        assert!(matches!(
            auth_with_corrupt_marker.access_token().await,
            Err(Error::ConnectionCleared)
        ));
    }

    #[tokio::test]
    async fn stale_token_invalidation_keeps_a_newer_cached_token() {
        let auth = JiraAuth::with_fixed_token("new_access", String::new());

        auth.invalidate_token_if_matches("stale_access").await;

        assert_eq!(auth.access_token().await.unwrap(), "new_access");
    }

    #[test]
    fn file_store_delete_reports_non_not_found_errors() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("refresh_token")).unwrap();
        let store = FileStore::new(temp.path().to_path_buf());

        assert!(store.delete("refresh_token").is_err());
    }

    #[tokio::test]
    async fn rejected_refresh_stays_disconnected_when_token_delete_fails() {
        let entries = std::sync::Arc::new(std::sync::Mutex::new(
            [
                ("refresh_token".to_string(), "revoked_refresh".to_string()),
                ("cloud_id".to_string(), "cloud-123".to_string()),
                (CONNECTION_CLEARED_KEY.to_string(), "false".to_string()),
            ]
            .into_iter()
            .collect(),
        ));
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant"
            })))
            .mount(&mock_server)
            .await;

        let auth = JiraAuth::with_test_config(
            "https://mysite.atlassian.net",
            Box::new(FailingDeleteStore::new(entries.clone(), true, false, true)),
            format!("{}/oauth/token", mock_server.uri()),
            String::new(),
        );
        assert!(matches!(
            auth.access_token().await,
            Err(Error::ConnectionStateNotDurablyCleared)
        ));
        assert!(!auth.is_connected());
        assert!(matches!(
            auth.access_token().await,
            Err(Error::ConnectionCleared)
        ));

        let restarted = JiraAuth::with_test_config(
            "https://mysite.atlassian.net",
            Box::new(FailingDeleteStore::new(entries, false, false, false)),
            String::new(),
            String::new(),
        );
        assert!(!restarted.is_connected());
        assert!(matches!(
            restarted.access_token().await,
            Err(Error::ConnectionCleared)
        ));
    }

    #[tokio::test]
    async fn rejected_refresh_reports_when_credentials_cannot_be_durably_cleared() {
        let entries = std::sync::Arc::new(std::sync::Mutex::new(
            [
                ("refresh_token".to_string(), "revoked_refresh".to_string()),
                ("cloud_id".to_string(), "cloud-123".to_string()),
                (CONNECTION_CLEARED_KEY.to_string(), "false".to_string()),
            ]
            .into_iter()
            .collect(),
        ));
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant"
            })))
            .mount(&mock_server)
            .await;

        let auth = JiraAuth::with_test_config(
            "https://mysite.atlassian.net",
            Box::new(FailingDeleteStore::new(entries, true, true, true)),
            format!("{}/oauth/token", mock_server.uri()),
            String::new(),
        );

        assert!(matches!(
            auth.access_token().await,
            Err(Error::ConnectionStateNotDurablyCleared)
        ));
        assert!(!auth.is_connected());
    }

    #[tokio::test]
    async fn disconnect_prevents_an_older_oauth_completion_from_persisting_tokens() {
        let auth = JiraAuth::with_test_config(
            "https://mysite.atlassian.net",
            Box::new(MemStore::new()),
            String::new(),
            String::new(),
        );
        let token = TokenResponse {
            access_token: "access".to_string(),
            refresh_token: Some("refresh".to_string()),
            expires_in: 3600,
        };

        auth.disconnect().await.unwrap();

        assert!(matches!(
            auth.complete_connection(&token, 0).await,
            Err(Error::ConnectionCleared)
        ));
        assert!(auth.store.get("refresh_token").is_err());
        assert!(auth.store.get("cloud_id").is_err());
    }

    #[tokio::test]
    async fn cloud_id_failure_does_not_store_refresh_token() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/resources"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;
        let auth = JiraAuth::with_test_config(
            "https://mysite.atlassian.net",
            Box::new(MemStore::new()),
            String::new(),
            format!("{}/resources", mock_server.uri()),
        );
        let token = TokenResponse {
            access_token: "access".to_string(),
            refresh_token: Some("refresh".to_string()),
            expires_in: 3600,
        };

        assert!(matches!(
            auth.complete_connection(&token, 0).await,
            Err(Error::CloudIdNotFound(_))
        ));
        assert!(auth.store.get("refresh_token").is_err());
    }

    #[tokio::test]
    async fn active_connection_attempt_is_rejected_before_oauth_work() {
        let auth = JiraAuth::with_fixed_token("access", String::new());

        assert!(matches!(
            auth.connect().await,
            Err(Error::ConnectionAlreadyActive)
        ));
    }

    #[tokio::test]
    async fn disconnect_cancels_a_callback_waiter_installed_before_oauth_work() {
        let auth = JiraAuth::with_test_config(
            "https://mysite.atlassian.net",
            Box::new(MemStore::new()),
            String::new(),
            String::new(),
        );
        let generation = auth.reserve_connection_attempt().unwrap();
        let cancellation = auth.install_connect_cancellation(generation).unwrap();

        auth.disconnect().await.unwrap();

        tokio::time::timeout(Duration::from_millis(100), cancellation.cancelled())
            .await
            .expect("disconnect must cancel a pending OAuth callback promptly");
    }

    #[tokio::test]
    async fn stale_oauth_exit_does_not_clear_a_reconnect_cancellation() {
        let auth = JiraAuth::with_test_config(
            "https://mysite.atlassian.net",
            Box::new(MemStore::new()),
            String::new(),
            String::new(),
        );
        let first_generation = auth.reserve_connection_attempt().unwrap();
        let first_cancellation = auth.install_connect_cancellation(first_generation).unwrap();

        auth.disconnect().await.unwrap();
        assert!(first_cancellation.is_cancelled());

        let reconnect_generation = auth.reserve_connection_attempt().unwrap();
        let reconnect_cancellation = auth
            .install_connect_cancellation(reconnect_generation)
            .unwrap();
        // The first attempt unwinds after a reconnect starts. Its cleanup must retain the new
        // token so disconnect can still cancel reconnect's exchange and completion awaits.
        auth.clear_connect_cancellation(first_generation);

        assert!(auth.connect_cancellation.lock().unwrap().is_some());
        auth.disconnect().await.unwrap();
        assert!(reconnect_cancellation.is_cancelled());
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
