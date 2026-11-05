//! Remote plugin packages: resolving a Discovery repository to one archive for this platform,
//! downloading and verifying it, extracting it into staging, and committing or discarding
//! the staged package (ADR-0018).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use planeai_core::command::{augmented_path, no_window_tokio};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::time::timeout;
use uuid::Uuid;

use crate::plugins::{
    PluginHostCapability, PluginInventory, PluginManifest, PluginSourceKind, PluginTool,
    PluginUiContribution, RemoteOrigin,
};

const GH_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);
const DOWNLOAD_LIMIT_BYTES: u64 = 100 * 1024 * 1024;
const EXTRACT_LIMIT_BYTES: u64 = 500 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
pub const EXPIRED_STAGING: &str = "This install review expired. Start again.";

/// A GitHub `owner/name`, validated at the command boundary so commands never take URLs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFullName(String);

impl RepoFullName {
    pub fn parse(value: &str) -> Result<Self, String> {
        let invalid = || format!("invalid GitHub repository name: {value}");
        let (owner, name) = value.split_once('/').ok_or_else(invalid)?;
        let owner_ok =
            !owner.is_empty() && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        let name_ok = !name.is_empty()
            && name != "."
            && name != ".."
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !owner_ok || !name_ok {
            return Err(invalid());
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn name(&self) -> &str {
        self.0.split_once('/').map(|(_, name)| name).unwrap_or("")
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiscoveryCardState {
    Unavailable {
        reason: String,
    },
    Install {
        latest_version: Option<String>,
    },
    UpToDate {
        installed_version: String,
    },
    Update {
        installed_version: String,
        latest_version: String,
    },
    Reinstall {
        installed_version: String,
    },
    InstalledLocally {
        plugin_id: String,
        installed_version: String,
        latest_version: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PackageIntegrity {
    Verified { sha256: String },
    Unverified { sha256: String },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StagedReplacement {
    None,
    Repo {
        installed_version: String,
    },
    Local {
        installed_version: String,
        original_display_path: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VersionMismatch {
    pub release_tag: String,
    pub manifest_version: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StagedManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub host_api_version: String,
    pub capabilities: Vec<PluginHostCapability>,
    pub ui_contributions: Vec<PluginUiContribution>,
    pub tools: Vec<PluginTool>,
}

impl From<&PluginManifest> for StagedManifest {
    fn from(manifest: &PluginManifest) -> Self {
        Self {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            host_api_version: manifest.host_api_version.clone(),
            capabilities: manifest.capabilities.clone(),
            ui_contributions: manifest.effective_ui_contributions(),
            tools: manifest.tools.clone(),
        }
    }
}

/// What the user reviews before a staged package is committed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StagedPluginInstall {
    pub staging_id: String,
    pub repo: String,
    pub source_url: String,
    pub release_tag: Option<String>,
    pub integrity: PackageIntegrity,
    pub manifest: StagedManifest,
    pub replaces: StagedReplacement,
    pub version_mismatch: Option<VersionMismatch>,
}

/// The default-branch manifest, read leniently because HEAD may target a newer host.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
struct HeadManifest {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    downloads: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct Release {
    tag_name: String,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    /// `sha256:<hex>` as the GitHub API reports it.
    #[serde(default)]
    digest: Option<String>,
}

impl ReleaseAsset {
    fn sha256(&self) -> Option<String> {
        self.digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .map(str::to_ascii_lowercase)
    }
}

/// Everything resolution learned about one Discovery repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRepo {
    repo: RepoFullName,
    head: Option<HeadManifest>,
    latest_release: Option<Release>,
}

/// The one archive to download for this platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSource {
    pub url: String,
    /// Set when the archive is an asset of the latest release.
    pub release_tag: Option<String>,
    pub expected_sha256: Option<String>,
    pub format: ArchiveFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    TarGz,
    Zip,
}

impl ArchiveFormat {
    pub fn from_url(url: &str) -> Result<Self, String> {
        let path = url.split(['?', '#']).next().unwrap_or(url);
        if path.ends_with(".tar.gz") || path.ends_with(".tgz") {
            Ok(Self::TarGz)
        } else if path.ends_with(".zip") {
            Ok(Self::Zip)
        } else {
            Err(format!(
                "plugin download must be a .tar.gz or .zip archive: {url}"
            ))
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Self::TarGz => "package.tar.gz",
            Self::Zip => "package.zip",
        }
    }
}

/// The release tag without a leading `v`, when it is semver.
fn release_version(tag: &str) -> Option<semver::Version> {
    semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

impl RemoteRepo {
    fn latest_version(&self) -> Option<semver::Version> {
        self.latest_release
            .as_ref()
            .and_then(|release| release_version(&release.tag_name))
    }

    fn release_asset_for_url(&self, url: &str) -> Option<&ReleaseAsset> {
        let release = self.latest_release.as_ref()?;
        let prefix = format!("https://github.com/{}/releases/", self.repo.as_str());
        let rest = url.strip_prefix(&prefix)?;
        let asset_name = rest
            .strip_prefix("latest/download/")
            .or_else(|| rest.strip_prefix(&format!("download/{}/", release.tag_name)))?;
        release.assets.iter().find(|asset| asset.name == asset_name)
    }

    fn fallback_asset(&self, platform: &str) -> Option<(&ReleaseAsset, ArchiveFormat)> {
        let release = self.latest_release.as_ref()?;
        let stem = format!("{}-{platform}", self.repo.name());
        release.assets.iter().find_map(|asset| {
            let format = if asset.name == format!("{stem}.tar.gz") {
                ArchiveFormat::TarGz
            } else if asset.name == format!("{stem}.zip") {
                ArchiveFormat::Zip
            } else {
                return None;
            };
            Some((asset, format))
        })
    }

    pub fn select_source(&self, platform: &str) -> Result<PackageSource, String> {
        if let Some(url) = self
            .head
            .as_ref()
            .and_then(|head| head.downloads.get(platform))
        {
            if !url.starts_with("https://") {
                return Err(format!(
                    "Not available for {platform}: the repository manifest download is not an https URL."
                ));
            }
            let format = ArchiveFormat::from_url(url)?;
            return Ok(match self.release_asset_for_url(url) {
                Some(asset) => PackageSource {
                    url: url.clone(),
                    release_tag: self
                        .latest_release
                        .as_ref()
                        .map(|release| release.tag_name.clone()),
                    expected_sha256: asset.sha256(),
                    format,
                },
                None => PackageSource {
                    url: url.clone(),
                    release_tag: None,
                    expected_sha256: None,
                    format,
                },
            });
        }
        let stem = format!("{}-{platform}", self.repo.name());
        match (&self.latest_release, self.fallback_asset(platform)) {
            (Some(release), Some((asset, format))) => Ok(PackageSource {
                url: asset.browser_download_url.clone(),
                release_tag: Some(release.tag_name.clone()),
                expected_sha256: asset.sha256(),
                format,
            }),
            (Some(release), None) => Err(format!(
                "Not available for {platform}: the repository manifest lists no download and release {} has no {stem}.tar.gz or {stem}.zip asset.",
                release.tag_name
            )),
            (None, _) => Err(format!(
                "Not available for {platform}: the repository manifest lists no download and the repository has no release."
            )),
        }
    }

    /// The installed plugin this repository stands for: by provenance first, then by HEAD id.
    fn installed<'a>(&self, inventory: &'a [PluginInventory]) -> Option<&'a PluginInventory> {
        inventory
            .iter()
            .find(|plugin| {
                plugin
                    .remote_origin
                    .as_ref()
                    .is_some_and(|origin| origin.repo == self.repo.as_str())
            })
            .or_else(|| {
                let id = self.head.as_ref()?.id.as_deref()?;
                inventory.iter().find(|plugin| plugin.id == id)
            })
    }

    pub fn card_state(&self, inventory: &[PluginInventory], platform: &str) -> DiscoveryCardState {
        let latest_version = self.latest_version();
        let latest_label = latest_version.as_ref().map(ToString::to_string);
        let Some(installed) = self.installed(inventory) else {
            return match self.select_source(platform) {
                Ok(_) => DiscoveryCardState::Install {
                    latest_version: latest_label,
                },
                Err(reason) => DiscoveryCardState::Unavailable { reason },
            };
        };
        if installed.source_kind != PluginSourceKind::Local {
            return DiscoveryCardState::Unavailable {
                reason: format!(
                    "Plugin id {} is reserved by a bundled plugin.",
                    installed.id
                ),
            };
        }
        if let Err(reason) = self.select_source(platform) {
            return DiscoveryCardState::Unavailable { reason };
        }
        let installed_version = installed.version.clone();
        if installed.remote_origin.is_none() {
            return DiscoveryCardState::InstalledLocally {
                plugin_id: installed.id.clone(),
                installed_version,
                latest_version: latest_label,
            };
        }
        match (
            semver::Version::parse(&installed_version).ok(),
            latest_version,
        ) {
            (Some(current), Some(latest)) if latest > current => DiscoveryCardState::Update {
                installed_version,
                latest_version: latest.to_string(),
            },
            (Some(_), Some(_)) => DiscoveryCardState::UpToDate { installed_version },
            _ => DiscoveryCardState::Reinstall { installed_version },
        }
    }
}

/// What a staged package replaces, from the installed plugin with the staged manifest's id.
pub fn replacement(
    inventory: &[PluginInventory],
    manifest_id: &str,
) -> Result<StagedReplacement, String> {
    let Some(installed) = inventory.iter().find(|plugin| plugin.id == manifest_id) else {
        return Ok(StagedReplacement::None);
    };
    if installed.source_kind != PluginSourceKind::Local {
        return Err(format!(
            "plugin id is reserved by a bundled plugin: {manifest_id}"
        ));
    }
    Ok(match installed.remote_origin {
        Some(_) => StagedReplacement::Repo {
            installed_version: installed.version.clone(),
        },
        None => StagedReplacement::Local {
            installed_version: installed.version.clone(),
            original_display_path: installed.original_display_path.clone(),
        },
    })
}

pub fn version_mismatch(
    release_tag: Option<&str>,
    manifest_version: &str,
) -> Option<VersionMismatch> {
    let tag = release_tag?;
    let tag_version = tag.strip_prefix('v').unwrap_or(tag);
    (tag_version != manifest_version).then(|| VersionMismatch {
        release_tag: tag.to_string(),
        manifest_version: manifest_version.to_string(),
    })
}

// ─── GitHub metadata through gh ──────────────────────────────────────────────

pub(crate) enum GhError {
    Timeout,
    Spawn(std::io::Error),
    Failed {
        status: std::process::ExitStatus,
        diagnostic: String,
    },
}

impl GhError {
    /// The user-facing message; `what` names the request, such as "GitHub plugin discovery".
    pub(crate) fn describe(&self, what: &str) -> String {
        match self {
            Self::Timeout => format!(
                "{what} timed out after {} seconds. Please retry.",
                GH_TIMEOUT.as_secs()
            ),
            Self::Spawn(error) => format!("failed to start GitHub CLI: {error}"),
            Self::Failed { status, diagnostic } if diagnostic.is_empty() => {
                format!("{what} failed with status {status}.")
            }
            Self::Failed { diagnostic, .. } => format!("{what} failed: {diagnostic}"),
        }
    }
}

/// Runs `gh` with `args` off the main thread and returns its stdout.
pub(crate) async fn run_gh(args: &[&str]) -> Result<Vec<u8>, GhError> {
    let mut command = tokio::process::Command::new(crate::command::resolve("gh"));
    no_window_tokio(&mut command);
    command.env("PATH", augmented_path(&[]));
    command.args(args);
    let output = timeout(GH_TIMEOUT, command.output())
        .await
        .map_err(|_| GhError::Timeout)?
        .map_err(GhError::Spawn)?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    Err(GhError::Failed {
        status: output.status,
        diagnostic: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

/// `gh api` with a 404 read as absence.
async fn gh_api(args: &[&str]) -> Result<Option<Vec<u8>>, String> {
    let mut full = vec!["api"];
    full.extend_from_slice(args);
    match run_gh(&full).await {
        Ok(stdout) => Ok(Some(stdout)),
        Err(GhError::Failed { diagnostic, .. }) if diagnostic.contains("HTTP 404") => Ok(None),
        Err(error) => Err(error.describe("GitHub request")),
    }
}

pub async fn fetch_remote_repo(repo: RepoFullName) -> Result<RemoteRepo, String> {
    let manifest_path = format!("repos/{}/contents/planeai-plugin.json", repo.as_str());
    let release_path = format!("repos/{}/releases/latest", repo.as_str());
    let manifest_args = [
        manifest_path.as_str(),
        "-H",
        "Accept: application/vnd.github.raw",
    ];
    let release_args = [release_path.as_str()];
    let (head, release) = tokio::join!(gh_api(&manifest_args), gh_api(&release_args));
    let head = head?.and_then(|raw| match serde_json::from_slice::<HeadManifest>(&raw) {
        Ok(manifest) => Some(manifest),
        Err(error) => {
            tracing::warn!(repo = repo.as_str(), %error, "ignoring unparseable HEAD plugin manifest");
            None
        }
    });
    let latest_release = match release? {
        Some(raw) => Some(
            serde_json::from_slice::<Release>(&raw)
                .map_err(|error| format!("GitHub release response was invalid JSON: {error}"))?,
        ),
        None => None,
    };
    Ok(RemoteRepo {
        repo,
        head,
        latest_release,
    })
}

// ─── Download ─────────────────────────────────────────────────────────────────

pub struct Downloader {
    client: reqwest::Client,
}

impl Downloader {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(DOWNLOAD_TIMEOUT)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if attempt.url().scheme() != "https" {
                    attempt.error("redirected to a non-https URL")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|error| format!("failed to build plugin download client: {error}"))?;
        Ok(Self { client })
    }

    /// Plain-http client for a local test server; production stays https-only.
    #[cfg(test)]
    pub fn insecure_for_tests() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(DOWNLOAD_TIMEOUT)
                .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
                .build()
                .unwrap(),
        }
    }

    /// Streams `url` into `destination`, returning the archive's SHA-256.
    pub async fn download(&self, url: &str, destination: &Path) -> Result<String, String> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| format!("plugin download failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "plugin download failed with HTTP {}: {url}",
                response.status().as_u16()
            ));
        }
        let over_limit = || {
            format!(
                "plugin download exceeds the {} MB limit",
                DOWNLOAD_LIMIT_BYTES / (1024 * 1024)
            )
        };
        if response
            .content_length()
            .is_some_and(|length| length > DOWNLOAD_LIMIT_BYTES)
        {
            return Err(over_limit());
        }
        let mut file = tokio::fs::File::create(destination)
            .await
            .map_err(|error| format!("failed to create plugin download file: {error}"))?;
        let mut hasher = Sha256::new();
        let mut received: u64 = 0;
        let mut response = response;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| format!("plugin download failed: {error}"))?
        {
            received += chunk.len() as u64;
            if received > DOWNLOAD_LIMIT_BYTES {
                return Err(over_limit());
            }
            hasher.update(&chunk);
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                .await
                .map_err(|error| format!("failed to write plugin download: {error}"))?;
        }
        tokio::io::AsyncWriteExt::flush(&mut file)
            .await
            .map_err(|error| format!("failed to write plugin download: {error}"))?;
        Ok(hex(&hasher.finalize()))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Fails closed when the downloaded bytes differ from the release asset digest.
pub fn check_integrity(sha256: String, expected: Option<&str>) -> Result<PackageIntegrity, String> {
    match expected {
        Some(expected) if expected.eq_ignore_ascii_case(&sha256) => {
            Ok(PackageIntegrity::Verified { sha256 })
        }
        Some(expected) => Err(format!(
            "plugin archive SHA-256 {sha256} does not match the release asset digest {expected}"
        )),
        None => Ok(PackageIntegrity::Unverified { sha256 }),
    }
}

// ─── Extraction ───────────────────────────────────────────────────────────────

/// A relative archive entry path with no absolute, root, prefix, or parent components.
fn safe_entry_path(raw: &str) -> Result<PathBuf, String> {
    let escapes = || format!("plugin archive entry escapes the package: {raw}");
    let mut safe = PathBuf::new();
    for component in Path::new(raw).components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(escapes())
            }
        }
    }
    if raw.starts_with('/') || raw.starts_with('\\') || safe.as_os_str().is_empty() {
        return Err(escapes());
    }
    Ok(safe)
}

/// Extracts the archive under `destination` and returns its single top-level package directory.
pub fn extract_archive(
    archive: &Path,
    format: ArchiveFormat,
    destination: &Path,
) -> Result<PathBuf, String> {
    extract_archive_within(archive, format, destination, EXTRACT_LIMIT_BYTES)
}

fn extract_archive_within(
    archive: &Path,
    format: ArchiveFormat,
    destination: &Path,
    limit: u64,
) -> Result<PathBuf, String> {
    fs::create_dir_all(destination)
        .map_err(|error| format!("failed to create plugin extraction directory: {error}"))?;
    let mut budget = ExtractionBudget {
        limit,
        remaining: limit,
    };
    match format {
        ArchiveFormat::TarGz => extract_tar_gz(archive, destination, &mut budget)?,
        ArchiveFormat::Zip => extract_zip(archive, destination, &mut budget)?,
    }
    package_root(destination)
}

/// Total extracted bytes allowed across every entry, so a small archive cannot fill the disk.
struct ExtractionBudget {
    limit: u64,
    remaining: u64,
}

impl ExtractionBudget {
    fn charge(&mut self, bytes: u64) -> Result<(), String> {
        self.remaining = self.remaining.checked_sub(bytes).ok_or_else(|| {
            format!(
                "plugin archive expands beyond the {} MB limit",
                self.limit / (1024 * 1024)
            )
        })?;
        Ok(())
    }
}

fn extract_tar_gz(
    archive: &Path,
    destination: &Path,
    budget: &mut ExtractionBudget,
) -> Result<(), String> {
    let file = fs::File::open(archive)
        .map_err(|error| format!("failed to open plugin archive: {error}"))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let entries = tar
        .entries()
        .map_err(|error| format!("failed to read plugin archive: {error}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|error| format!("failed to read plugin archive: {error}"))?;
        let kind = entry.header().entry_type();
        let raw = entry
            .path()
            .map_err(|error| format!("plugin archive entry has an invalid path: {error}"))?
            .to_string_lossy()
            .into_owned();
        if kind.is_symlink() || kind.is_hard_link() {
            return Err(format!("plugin archive contains a link: {raw}"));
        }
        if kind.is_pax_global_extensions() {
            continue;
        }
        if !kind.is_dir() && !kind.is_file() {
            return Err(format!(
                "plugin archive contains an unsupported entry: {raw}"
            ));
        }
        let target = destination.join(safe_entry_path(&raw)?);
        let entry_error = |error: std::io::Error| {
            format!("failed to extract plugin archive entry {raw}: {error}")
        };
        // Unpack does not create parents, and a tar may omit directory entries entirely.
        if kind.is_dir() {
            fs::create_dir_all(&target).map_err(entry_error)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(entry_error)?;
        }
        // The tar reader yields exactly the header size, so charging it up front is exact.
        budget.charge(entry.size())?;
        entry.unpack(&target).map_err(entry_error)?;
    }
    Ok(())
}

fn extract_zip(
    archive: &Path,
    destination: &Path,
    budget: &mut ExtractionBudget,
) -> Result<(), String> {
    let file = fs::File::open(archive)
        .map_err(|error| format!("failed to open plugin archive: {error}"))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|error| format!("invalid plugin zip: {error}"))?;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| format!("failed to read plugin zip entry: {error}"))?;
        let raw = entry
            .name()
            .map_err(|error| format!("plugin archive entry has an invalid name: {error}"))?
            .into_owned();
        if entry.is_symlink() {
            return Err(format!("plugin archive contains a link: {raw}"));
        }
        let target = destination.join(safe_entry_path(&raw)?);
        let entry_error = |error: std::io::Error| {
            format!("failed to extract plugin archive entry {raw}: {error}")
        };
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(entry_error)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(entry_error)?;
        }
        let mut output = fs::File::create(&target).map_err(entry_error)?;
        // Zip headers can understate the size, so count what actually inflates.
        let written = std::io::copy(
            &mut (&mut entry).take(budget.remaining.saturating_add(1)),
            &mut output,
        )
        .map_err(entry_error)?;
        budget.charge(written)?;
        output.flush().map_err(entry_error)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(mode & 0o777)).map_err(
                |error| format!("failed to set mode on plugin archive entry {raw}: {error}"),
            )?;
        }
    }
    Ok(())
}

fn package_root(destination: &Path) -> Result<PathBuf, String> {
    let entries = fs::read_dir(destination)
        .map_err(|error| format!("failed to read extracted plugin archive: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to read extracted plugin archive: {error}"))?;
    let [entry] = entries.as_slice() else {
        return Err(format!(
            "plugin archive must contain exactly one top-level directory, found {} entries",
            entries.len()
        ));
    };
    let root = entry.path();
    if !root.is_dir() || !root.join(crate::plugin_packages::MANIFEST_FILE).is_file() {
        return Err(
            "plugin archive must contain one top-level directory holding planeai-plugin.json"
                .to_string(),
        );
    }
    Ok(root)
}

// ─── Staging ──────────────────────────────────────────────────────────────────

/// A downloaded, verified and extracted package waiting for the user's decision.
#[derive(Debug)]
pub struct StagedPackage {
    pub dir: PathBuf,
    pub package_root: PathBuf,
    pub origin: RemoteOrigin,
}

#[derive(Debug)]
pub struct StagingStore {
    root: PathBuf,
    entries: Mutex<HashMap<String, StagedPackage>>,
}

impl StagingStore {
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            root: app_data_dir.join("plugins").join("staging"),
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Wipes every staged package; a crash mid-stage leaves nothing behind.
    pub fn reset(&self) -> Result<(), String> {
        if self.root.exists() {
            fs::remove_dir_all(&self.root)
                .map_err(|error| format!("failed to clear plugin staging: {error}"))?;
        }
        self.entries.lock().map_err(|e| e.to_string())?.clear();
        Ok(())
    }

    pub fn create_dir(&self) -> Result<(String, PathBuf), String> {
        let id = Uuid::new_v4().to_string();
        let dir = self.root.join(&id);
        fs::create_dir_all(&dir)
            .map_err(|error| format!("failed to create plugin staging directory: {error}"))?;
        Ok((id, dir))
    }

    pub fn insert(&self, id: String, staged: StagedPackage) -> Result<(), String> {
        self.entries
            .lock()
            .map_err(|e| e.to_string())?
            .insert(id, staged);
        Ok(())
    }

    pub fn take(&self, id: &str) -> Result<Option<StagedPackage>, String> {
        Ok(self.entries.lock().map_err(|e| e.to_string())?.remove(id))
    }
}

pub fn remove_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        fs::remove_dir_all(dir)
            .map_err(|error| format!("failed to remove plugin staging directory: {error}"))?;
    }
    Ok(())
}

/// Downloads, verifies, extracts and inspects `source` under `dir`: the package to keep in the
/// staging store and the preview the user reviews before committing it.
pub async fn stage_package(
    downloader: &Downloader,
    repo: &RepoFullName,
    source: &PackageSource,
    inventory: &[PluginInventory],
    staging_id: String,
    dir: &Path,
) -> Result<(StagedPackage, StagedPluginInstall), String> {
    let format = source.format;
    let archive = dir.join(format.file_name());
    let sha256 = downloader.download(&source.url, &archive).await?;
    let integrity = check_integrity(sha256.clone(), source.expected_sha256.as_deref())?;
    let extracted = dir.join("extracted");
    let inspected = crate::commands::blocking(move || {
        let root = extract_archive(&archive, format, &extracted)?;
        crate::plugin_packages::inspect_package(&root)
    })
    .await?;
    let preview = StagedPluginInstall {
        staging_id,
        repo: repo.as_str().to_string(),
        source_url: source.url.clone(),
        release_tag: source.release_tag.clone(),
        integrity,
        manifest: StagedManifest::from(&inspected.manifest),
        replaces: replacement(inventory, &inspected.manifest.id)?,
        version_mismatch: version_mismatch(
            source.release_tag.as_deref(),
            &inspected.manifest.version,
        ),
    };
    let package = StagedPackage {
        dir: dir.to_path_buf(),
        package_root: inspected.source,
        origin: RemoteOrigin {
            repo: repo.as_str().to_string(),
            release_tag: source.release_tag.clone(),
            source_url: source.url.clone(),
            archive_sha256: sha256,
        },
    };
    Ok((package, preview))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::{PluginRuntimeState, RemoteOrigin};
    use tempfile::TempDir;

    fn repo() -> RepoFullName {
        RepoFullName::parse("acme/planeai-plugin-acme").unwrap()
    }

    fn asset(name: &str, digest: Option<&str>) -> ReleaseAsset {
        ReleaseAsset {
            name: name.to_string(),
            browser_download_url: format!(
                "https://github.com/acme/planeai-plugin-acme/releases/download/v1.2.0/{name}"
            ),
            digest: digest.map(str::to_string),
        }
    }

    fn release() -> Release {
        Release {
            tag_name: "v1.2.0".to_string(),
            assets: vec![
                asset(
                    "planeai-plugin-acme-macos-arm64.tar.gz",
                    Some("sha256:ABCDEF"),
                ),
                asset("planeai-plugin-acme-linux-x64.zip", None),
            ],
        }
    }

    fn head(downloads: &[(&str, &str)]) -> HeadManifest {
        HeadManifest {
            id: Some("acme".to_string()),
            downloads: downloads
                .iter()
                .map(|(platform, url)| (platform.to_string(), url.to_string()))
                .collect(),
        }
    }

    fn inventory(id: &str, version: &str, remote_origin: Option<RemoteOrigin>) -> PluginInventory {
        PluginInventory {
            id: id.to_string(),
            name: id.to_string(),
            version: version.to_string(),
            host_api_version: "planeai.plugin-host.v6".to_string(),
            source_kind: PluginSourceKind::Local,
            backend_entrypoint: "bin/plugin".to_string(),
            ui_contributions: vec![],
            capabilities: vec![],
            background_service: None,
            providers: vec![],
            tools: vec![],
            installed_hash: Some("hash".to_string()),
            installed_path: Some("/app/plugins/packages/sha256/hash".to_string()),
            original_display_path: remote_origin
                .is_none()
                .then(|| "/Users/me/acme".to_string()),
            remote_origin,
            enabled: false,
            state: PluginRuntimeState::Disabled,
            last_error: None,
            log_path: None,
        }
    }

    fn origin(repo: &str) -> RemoteOrigin {
        RemoteOrigin {
            repo: repo.to_string(),
            release_tag: Some("v1.0.0".to_string()),
            source_url: format!("https://github.com/{repo}/releases/download/v1.0.0/x.tar.gz"),
            archive_sha256: "abc".to_string(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn gh_failures_read_as_one_sentence_per_cause() {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            GhError::Timeout.describe("GitHub plugin discovery"),
            "GitHub plugin discovery timed out after 30 seconds. Please retry."
        );
        assert_eq!(
            GhError::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound, "no gh"))
                .describe("GitHub request"),
            "failed to start GitHub CLI: no gh"
        );
        assert_eq!(
            GhError::Failed {
                status: std::process::ExitStatus::from_raw(256),
                diagnostic: String::new(),
            }
            .describe("GitHub request"),
            "GitHub request failed with status exit status: 1."
        );
        assert_eq!(
            GhError::Failed {
                status: std::process::ExitStatus::from_raw(256),
                diagnostic: "HTTP 403: rate limited".to_string(),
            }
            .describe("GitHub plugin discovery"),
            "GitHub plugin discovery failed: HTTP 403: rate limited"
        );
    }

    #[test]
    fn repo_names_are_owner_slash_name_in_the_github_charset() {
        assert_eq!(repo().name(), "planeai-plugin-acme");
        for invalid in [
            "acme",
            "acme/",
            "/x",
            "acme/x/y",
            "acme/..",
            "acme/x y",
            "a$b/x",
            "https://github.com/acme/x",
        ] {
            assert_eq!(
                RepoFullName::parse(invalid).unwrap_err(),
                format!("invalid GitHub repository name: {invalid}"),
                "{invalid}"
            );
        }
    }

    #[test]
    fn manifest_download_wins_and_is_verified_when_it_is_the_latest_release_asset() {
        let remote = RemoteRepo {
            repo: repo(),
            head: Some(head(&[(
                "macos-arm64",
                "https://github.com/acme/planeai-plugin-acme/releases/latest/download/planeai-plugin-acme-macos-arm64.tar.gz",
            )])),
            latest_release: Some(release()),
        };
        assert_eq!(
            remote.select_source("macos-arm64").unwrap(),
            PackageSource {
                url: "https://github.com/acme/planeai-plugin-acme/releases/latest/download/planeai-plugin-acme-macos-arm64.tar.gz".to_string(),
                release_tag: Some("v1.2.0".to_string()),
                expected_sha256: Some("abcdef".to_string()),
                format: ArchiveFormat::TarGz,
            }
        );

        let tagged = RemoteRepo {
            head: Some(head(&[(
                "macos-arm64",
                "https://github.com/acme/planeai-plugin-acme/releases/download/v1.2.0/planeai-plugin-acme-macos-arm64.tar.gz",
            )])),
            ..remote.clone()
        };
        assert_eq!(
            tagged.select_source("macos-arm64").unwrap().expected_sha256,
            Some("abcdef".to_string())
        );

        let elsewhere = RemoteRepo {
            head: Some(head(&[("macos-arm64", "https://cdn.acme.dev/acme.zip")])),
            ..remote.clone()
        };
        assert_eq!(
            elsewhere.select_source("macos-arm64").unwrap(),
            PackageSource {
                url: "https://cdn.acme.dev/acme.zip".to_string(),
                release_tag: None,
                expected_sha256: None,
                format: ArchiveFormat::Zip,
            }
        );

        let older_tag = RemoteRepo {
            head: Some(head(&[(
                "macos-arm64",
                "https://github.com/acme/planeai-plugin-acme/releases/download/v1.1.0/planeai-plugin-acme-macos-arm64.tar.gz",
            )])),
            ..remote
        };
        assert_eq!(
            older_tag
                .select_source("macos-arm64")
                .unwrap()
                .expected_sha256,
            None
        );
    }

    #[test]
    fn falls_back_to_the_conventional_release_asset_and_explains_when_nothing_fits() {
        let remote = RemoteRepo {
            repo: repo(),
            head: Some(head(&[(
                "windows-x64",
                "https://cdn.acme.dev/acme-win.zip",
            )])),
            latest_release: Some(release()),
        };
        assert_eq!(
            remote.select_source("linux-x64").unwrap(),
            PackageSource {
                url: "https://github.com/acme/planeai-plugin-acme/releases/download/v1.2.0/planeai-plugin-acme-linux-x64.zip".to_string(),
                release_tag: Some("v1.2.0".to_string()),
                expected_sha256: None,
                format: ArchiveFormat::Zip,
            }
        );
        assert_eq!(
            remote.select_source("linux-arm64").unwrap_err(),
            "Not available for linux-arm64: the repository manifest lists no download and release v1.2.0 has no planeai-plugin-acme-linux-arm64.tar.gz or planeai-plugin-acme-linux-arm64.zip asset."
        );
        let no_release = RemoteRepo {
            head: None,
            latest_release: None,
            ..remote.clone()
        };
        assert_eq!(
            no_release.select_source("macos-arm64").unwrap_err(),
            "Not available for macos-arm64: the repository manifest lists no download and the repository has no release."
        );
        let plain_http = RemoteRepo {
            head: Some(head(&[("macos-arm64", "http://cdn.acme.dev/acme.zip")])),
            ..remote
        };
        assert_eq!(
            plain_http.select_source("macos-arm64").unwrap_err(),
            "Not available for macos-arm64: the repository manifest download is not an https URL."
        );
    }

    #[test]
    fn card_state_table() {
        let remote = RemoteRepo {
            repo: repo(),
            head: Some(head(&[])),
            latest_release: Some(release()),
        };
        let cases: Vec<(&str, RemoteRepo, Vec<PluginInventory>, DiscoveryCardState)> = vec![
            (
                "fresh install",
                remote.clone(),
                vec![],
                DiscoveryCardState::Install {
                    latest_version: Some("1.2.0".to_string()),
                },
            ),
            (
                "fresh install without a semver tag",
                RemoteRepo {
                    latest_release: Some(Release {
                        tag_name: "nightly".to_string(),
                        ..release()
                    }),
                    ..remote.clone()
                },
                vec![],
                DiscoveryCardState::Install {
                    latest_version: None,
                },
            ),
            (
                "unavailable platform",
                RemoteRepo {
                    latest_release: None,
                    ..remote.clone()
                },
                vec![],
                DiscoveryCardState::Unavailable {
                    reason: "Not available for macos-arm64: the repository manifest lists no download and the repository has no release.".to_string(),
                },
            ),
            (
                "repo install behind latest, matched by provenance",
                remote.clone(),
                vec![inventory("other-id", "1.0.0", Some(origin("acme/planeai-plugin-acme")))],
                DiscoveryCardState::Update {
                    installed_version: "1.0.0".to_string(),
                    latest_version: "1.2.0".to_string(),
                },
            ),
            (
                "repo install at latest",
                remote.clone(),
                vec![inventory("acme", "1.2.0", Some(origin("acme/planeai-plugin-acme")))],
                DiscoveryCardState::UpToDate {
                    installed_version: "1.2.0".to_string(),
                },
            ),
            (
                "repo install ahead of latest",
                remote.clone(),
                vec![inventory("acme", "2.0.0", Some(origin("acme/planeai-plugin-acme")))],
                DiscoveryCardState::UpToDate {
                    installed_version: "2.0.0".to_string(),
                },
            ),
            (
                "repo install with a non-semver installed version",
                remote.clone(),
                vec![inventory("acme", "build-7", Some(origin("acme/planeai-plugin-acme")))],
                DiscoveryCardState::Reinstall {
                    installed_version: "build-7".to_string(),
                },
            ),
            (
                "repo install with a non-semver release tag",
                RemoteRepo {
                    latest_release: Some(Release {
                        tag_name: "nightly".to_string(),
                        ..release()
                    }),
                    ..remote.clone()
                },
                vec![inventory("acme", "1.0.0", Some(origin("acme/planeai-plugin-acme")))],
                DiscoveryCardState::Reinstall {
                    installed_version: "1.0.0".to_string(),
                },
            ),
            (
                "folder install matched by HEAD id",
                remote.clone(),
                vec![inventory("acme", "0.9.0", None)],
                DiscoveryCardState::InstalledLocally {
                    plugin_id: "acme".to_string(),
                    installed_version: "0.9.0".to_string(),
                    latest_version: Some("1.2.0".to_string()),
                },
            ),
            (
                "provenance from another repo is not this card",
                remote.clone(),
                vec![inventory("someone-else", "1.0.0", Some(origin("other/planeai-plugin-other")))],
                DiscoveryCardState::Install {
                    latest_version: Some("1.2.0".to_string()),
                },
            ),
            (
                "installed but the platform is no longer served",
                RemoteRepo {
                    latest_release: None,
                    ..remote.clone()
                },
                vec![inventory("acme", "1.0.0", Some(origin("acme/planeai-plugin-acme")))],
                DiscoveryCardState::Unavailable {
                    reason: "Not available for macos-arm64: the repository manifest lists no download and the repository has no release.".to_string(),
                },
            ),
            (
                "bundled id",
                RemoteRepo {
                    head: Some(HeadManifest {
                        id: Some("jira".to_string()),
                        downloads: BTreeMap::new(),
                    }),
                    ..remote.clone()
                },
                vec![PluginInventory {
                    source_kind: PluginSourceKind::Builtin,
                    ..inventory("jira", "0.1.0", None)
                }],
                DiscoveryCardState::Unavailable {
                    reason: "Plugin id jira is reserved by a bundled plugin.".to_string(),
                },
            ),
        ];
        for (name, remote, inventory, expected) in cases {
            assert_eq!(
                remote.card_state(&inventory, "macos-arm64"),
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn replacement_is_derived_from_the_staged_manifest_id() {
        let plugins = vec![
            inventory("local-one", "0.5.0", None),
            inventory(
                "remote-one",
                "1.0.0",
                Some(origin("acme/planeai-plugin-acme")),
            ),
            PluginInventory {
                source_kind: PluginSourceKind::Builtin,
                ..inventory("jira", "0.1.0", None)
            },
        ];
        assert_eq!(
            replacement(&plugins, "new").unwrap(),
            StagedReplacement::None
        );
        assert_eq!(
            replacement(&plugins, "local-one").unwrap(),
            StagedReplacement::Local {
                installed_version: "0.5.0".to_string(),
                original_display_path: Some("/Users/me/acme".to_string()),
            }
        );
        assert_eq!(
            replacement(&plugins, "remote-one").unwrap(),
            StagedReplacement::Repo {
                installed_version: "1.0.0".to_string(),
            }
        );
        assert_eq!(
            replacement(&plugins, "jira").unwrap_err(),
            "plugin id is reserved by a bundled plugin: jira"
        );
    }

    #[test]
    fn version_mismatch_compares_the_tag_without_its_v() {
        assert_eq!(version_mismatch(Some("v1.2.0"), "1.2.0"), None);
        assert_eq!(version_mismatch(Some("1.2.0"), "1.2.0"), None);
        assert_eq!(version_mismatch(None, "0.0.0"), None);
        assert_eq!(
            version_mismatch(Some("v1.2.0"), "0.0.0"),
            Some(VersionMismatch {
                release_tag: "v1.2.0".to_string(),
                manifest_version: "0.0.0".to_string(),
            })
        );
    }

    #[test]
    fn integrity_fails_closed_on_a_digest_mismatch() {
        assert_eq!(
            check_integrity("abc".to_string(), Some("ABC")).unwrap(),
            PackageIntegrity::Verified {
                sha256: "abc".to_string()
            }
        );
        assert_eq!(
            check_integrity("abc".to_string(), None).unwrap(),
            PackageIntegrity::Unverified {
                sha256: "abc".to_string()
            }
        );
        assert_eq!(
            check_integrity("abc".to_string(), Some("def")).unwrap_err(),
            "plugin archive SHA-256 abc does not match the release asset digest def"
        );
    }

    #[test]
    fn archive_format_comes_from_the_url_path() {
        assert_eq!(
            ArchiveFormat::from_url("https://x/a-macos-arm64.tar.gz?token=1").unwrap(),
            ArchiveFormat::TarGz
        );
        assert_eq!(
            ArchiveFormat::from_url("https://x/a.zip").unwrap(),
            ArchiveFormat::Zip
        );
        assert_eq!(
            ArchiveFormat::from_url("https://x/a.dmg").unwrap_err(),
            "plugin download must be a .tar.gz or .zip archive: https://x/a.dmg"
        );
    }

    #[test]
    fn entry_paths_must_stay_inside_the_package() {
        assert_eq!(
            safe_entry_path("acme/bin/plugin").unwrap(),
            PathBuf::from("acme/bin/plugin")
        );
        assert_eq!(
            safe_entry_path("./acme/planeai-plugin.json").unwrap(),
            PathBuf::from("acme/planeai-plugin.json")
        );
        for raw in ["../x", "acme/../../x", "/etc/passwd", "", "."] {
            assert_eq!(
                safe_entry_path(raw).unwrap_err(),
                format!("plugin archive entry escapes the package: {raw}"),
                "{raw}"
            );
        }
    }

    fn manifest_json(id: &str) -> String {
        format!(
            r#"{{"schema":"planeai.plugin.v1","id":"{id}","name":"Acme","version":"1.2.0","host_api_version":"planeai.plugin-host.v1","source_kind":"local","backend_entrypoints":{{"{}":"bin/plugin"}}}}"#,
            crate::plugin_packages::current_platform_key()
        )
    }

    fn tar_gz(entries: &[(&str, TarEntry)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (path, entry) in entries {
            // The builder refuses `..` and absolute names, which the attack cases need.
            let mut header = tar::Header::new_gnu();
            header.as_old_mut().name[..path.len()].copy_from_slice(path.as_bytes());
            let (kind, mode, bytes): (tar::EntryType, u32, &[u8]) = match entry {
                TarEntry::Dir => (tar::EntryType::Directory, 0o755, &[]),
                TarEntry::File(bytes, mode) => (tar::EntryType::Regular, *mode, bytes),
                TarEntry::Symlink(target) => {
                    header.set_link_name(target).unwrap();
                    (tar::EntryType::Symlink, 0o777, &[])
                }
                TarEntry::Hardlink(target) => {
                    header.set_link_name(target).unwrap();
                    (tar::EntryType::Link, 0o644, &[])
                }
            };
            header.set_entry_type(kind);
            header.set_mode(mode);
            header.set_size(bytes.len() as u64);
            header.set_cksum();
            builder.append(&header, bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    enum TarEntry {
        Dir,
        File(Vec<u8>, u32),
        Symlink(&'static str),
        Hardlink(&'static str),
    }

    fn good_tar() -> Vec<u8> {
        tar_gz(&[
            ("acme/", TarEntry::Dir),
            (
                "acme/planeai-plugin.json",
                TarEntry::File(manifest_json("acme").into_bytes(), 0o644),
            ),
            ("acme/bin/", TarEntry::Dir),
            (
                "acme/bin/plugin",
                TarEntry::File(b"#!/bin/sh\n".to_vec(), 0o755),
            ),
        ])
    }

    fn zip_bytes(entries: &[(&str, Option<&[u8]>, u32)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (path, bytes, mode) in entries {
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(*mode);
            match bytes {
                Some(bytes) => {
                    writer.start_file(*path, options).unwrap();
                    writer.write_all(bytes).unwrap();
                }
                None => writer.add_directory(*path, options).unwrap(),
            }
        }
        writer.finish().unwrap().into_inner()
    }

    fn write_archive(temp: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = temp.path().join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn extracts_a_valid_tar_gz_and_zip_into_one_package_root() {
        let temp = TempDir::new().unwrap();
        let archive = write_archive(&temp, "a.tar.gz", &good_tar());
        let root =
            extract_archive(&archive, ArchiveFormat::TarGz, &temp.path().join("tar")).unwrap();
        assert_eq!(root, temp.path().join("tar").join("acme"));
        assert_eq!(
            fs::read_to_string(root.join("planeai-plugin.json")).unwrap(),
            manifest_json("acme")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(root.join("bin/plugin"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o755
            );
        }

        let archive = write_archive(
            &temp,
            "a.zip",
            &zip_bytes(&[
                ("acme/", None, 0o755),
                (
                    "acme/planeai-plugin.json",
                    Some(manifest_json("acme").as_bytes()),
                    0o644,
                ),
                ("acme/bin/plugin", Some(b"#!/bin/sh\n"), 0o755),
            ]),
        );
        let root = extract_archive(&archive, ArchiveFormat::Zip, &temp.path().join("zip")).unwrap();
        assert_eq!(root, temp.path().join("zip").join("acme"));
        assert_eq!(fs::read(root.join("bin/plugin")).unwrap(), b"#!/bin/sh\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(root.join("bin/plugin"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o755
            );
        }
    }

    #[test]
    fn extracts_a_tar_gz_that_carries_no_directory_entries() {
        let temp = TempDir::new().unwrap();
        let archive = write_archive(
            &temp,
            "flat.tar.gz",
            &tar_gz(&[
                (
                    "acme/planeai-plugin.json",
                    TarEntry::File(manifest_json("acme").into_bytes(), 0o644),
                ),
                (
                    "acme/bin/plugin",
                    TarEntry::File(b"#!/bin/sh\n".to_vec(), 0o755),
                ),
            ]),
        );
        let root =
            extract_archive(&archive, ArchiveFormat::TarGz, &temp.path().join("flat")).unwrap();
        assert_eq!(root, temp.path().join("flat").join("acme"));
        assert_eq!(fs::read(root.join("bin/plugin")).unwrap(), b"#!/bin/sh\n");
    }

    #[test]
    fn stops_extracting_once_the_archive_expands_past_the_budget() {
        let temp = TempDir::new().unwrap();
        let manifest = manifest_json("acme").into_bytes();
        let zeros = vec![0u8; 2 * 1024 * 1024];
        let one_mb = 1024 * 1024;

        let tar = write_archive(
            &temp,
            "bomb.tar.gz",
            &tar_gz(&[
                ("acme/", TarEntry::Dir),
                (
                    "acme/planeai-plugin.json",
                    TarEntry::File(manifest.clone(), 0o644),
                ),
                ("acme/blob", TarEntry::File(zeros.clone(), 0o644)),
            ]),
        );
        assert_eq!(
            extract_archive_within(&tar, ArchiveFormat::TarGz, &temp.path().join("tar"), one_mb)
                .unwrap_err(),
            "plugin archive expands beyond the 1 MB limit"
        );
        assert!(!temp.path().join("tar/acme/blob").exists());

        let zip = write_archive(
            &temp,
            "bomb.zip",
            &zip_bytes(&[
                ("acme/planeai-plugin.json", Some(&manifest), 0o644),
                ("acme/blob", Some(&zeros), 0o644),
            ]),
        );
        assert!(
            fs::metadata(&zip).unwrap().len() < one_mb / 10,
            "the fixture compresses"
        );
        assert_eq!(
            extract_archive_within(&zip, ArchiveFormat::Zip, &temp.path().join("zip"), one_mb)
                .unwrap_err(),
            "plugin archive expands beyond the 1 MB limit"
        );
        assert!(
            fs::metadata(temp.path().join("zip/acme/blob"))
                .unwrap()
                .len()
                <= one_mb + 1,
            "extraction stops at the budget instead of draining the entry"
        );
    }

    #[test]
    fn rejects_unsafe_and_malformed_archives() {
        let temp = TempDir::new().unwrap();
        let manifest = manifest_json("acme").into_bytes();
        let cases: Vec<(&str, Vec<u8>, &str)> =
            vec![
            (
                "zip slip",
                tar_gz(&[
                    ("acme/", TarEntry::Dir),
                    ("acme/planeai-plugin.json", TarEntry::File(manifest.clone(), 0o644)),
                    ("acme/../evil", TarEntry::File(b"x".to_vec(), 0o644)),
                ]),
                "plugin archive entry escapes the package: acme/../evil",
            ),
            (
                "absolute path",
                tar_gz(&[("/tmp/evil", TarEntry::File(b"x".to_vec(), 0o644))]),
                "plugin archive entry escapes the package: /tmp/evil",
            ),
            (
                "symlink",
                tar_gz(&[
                    ("acme/", TarEntry::Dir),
                    ("acme/planeai-plugin.json", TarEntry::File(manifest.clone(), 0o644)),
                    ("acme/link", TarEntry::Symlink("/etc/passwd")),
                ]),
                "plugin archive contains a link: acme/link",
            ),
            (
                "hardlink",
                tar_gz(&[
                    ("acme/", TarEntry::Dir),
                    ("acme/planeai-plugin.json", TarEntry::File(manifest.clone(), 0o644)),
                    ("acme/hard", TarEntry::Hardlink("acme/planeai-plugin.json")),
                ]),
                "plugin archive contains a link: acme/hard",
            ),
            (
                "two top-level directories",
                tar_gz(&[
                    ("acme/", TarEntry::Dir),
                    ("acme/planeai-plugin.json", TarEntry::File(manifest.clone(), 0o644)),
                    ("other/", TarEntry::Dir),
                    ("other/readme", TarEntry::File(b"x".to_vec(), 0o644)),
                ]),
                "plugin archive must contain exactly one top-level directory, found 2 entries",
            ),
            (
                "manifest at the archive root",
                tar_gz(&[("planeai-plugin.json", TarEntry::File(manifest.clone(), 0o644))]),
                "plugin archive must contain one top-level directory holding planeai-plugin.json",
            ),
            (
                "top-level directory without a manifest",
                tar_gz(&[
                    ("acme/", TarEntry::Dir),
                    ("acme/readme", TarEntry::File(b"x".to_vec(), 0o644)),
                ]),
                "plugin archive must contain one top-level directory holding planeai-plugin.json",
            ),
        ];
        for (name, bytes, expected) in cases {
            let archive = write_archive(&temp, &format!("{name}.tar.gz"), &bytes);
            let destination = temp.path().join(name);
            assert_eq!(
                extract_archive(&archive, ArchiveFormat::TarGz, &destination).unwrap_err(),
                expected,
                "{name}"
            );
            assert!(
                !destination.join("..").join("evil").exists() && !temp.path().join("evil").exists(),
                "{name} wrote outside the destination"
            );
        }

        let zip_slip = write_archive(
            &temp,
            "slip.zip",
            &zip_bytes(&[("acme/../evil", Some(b"x"), 0o644)]),
        );
        assert_eq!(
            extract_archive(&zip_slip, ArchiveFormat::Zip, &temp.path().join("slip")).unwrap_err(),
            "plugin archive entry escapes the package: acme/../evil"
        );
        assert!(!temp.path().join("evil").exists());
    }

    #[test]
    fn staged_packages_are_taken_once_and_removal_is_idempotent() {
        let temp = TempDir::new().unwrap();
        let store = StagingStore::new(temp.path());
        let (id, dir) = store.create_dir().unwrap();
        fs::write(dir.join("package.zip"), b"bytes").unwrap();
        store
            .insert(
                id.clone(),
                StagedPackage {
                    dir: dir.clone(),
                    package_root: dir.join("extracted").join("acme"),
                    origin: origin("acme/planeai-plugin-acme"),
                },
            )
            .unwrap();

        let taken = store.take(&id).unwrap().unwrap();
        assert_eq!(taken.dir, dir);
        assert!(
            store.take(&id).unwrap().is_none(),
            "a commit consumes the staging"
        );

        let (second, second_dir) = store.create_dir().unwrap();
        store
            .insert(
                second.clone(),
                StagedPackage {
                    dir: second_dir.clone(),
                    package_root: second_dir.clone(),
                    origin: origin("acme/planeai-plugin-acme"),
                },
            )
            .unwrap();
        let discarded = store.take(&second).unwrap().unwrap();
        remove_dir(&discarded.dir).unwrap();
        assert!(!second_dir.exists());
        remove_dir(&discarded.dir).unwrap();
        assert!(store.take(&second).unwrap().is_none());
        assert!(store.take("never-staged").unwrap().is_none());

        let (_, leftover) = store.create_dir().unwrap();
        assert!(leftover.exists());
        store.reset().unwrap();
        assert!(!temp.path().join("plugins").join("staging").exists());
    }

    async fn serve(body: Vec<u8>) -> String {
        use axum::routing::get;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = axum::Router::new()
            .route(
                "/acme-macos-arm64.tar.gz",
                get(move || async move { body.clone() }),
            )
            .route(
                "/moved.tar.gz",
                get(|| async { axum::response::Redirect::permanent("/acme-macos-arm64.tar.gz") }),
            );
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn downloads_stream_to_disk_and_report_the_archive_hash() {
        let bytes = good_tar();
        let expected = hex(&Sha256::digest(&bytes));
        let base = serve(bytes.clone()).await;
        let temp = TempDir::new().unwrap();
        let destination = temp.path().join("package.tar.gz");

        let sha256 = Downloader::insecure_for_tests()
            .download(&format!("{base}/moved.tar.gz"), &destination)
            .await
            .unwrap();
        assert_eq!(sha256, expected);
        assert_eq!(fs::read(&destination).unwrap(), bytes);

        assert_eq!(
            Downloader::insecure_for_tests()
                .download(&format!("{base}/missing.tar.gz"), &destination)
                .await
                .unwrap_err(),
            format!("plugin download failed with HTTP 404: {base}/missing.tar.gz")
        );
        assert_eq!(
            Downloader::new()
                .unwrap()
                .download(&format!("{base}/acme-macos-arm64.tar.gz"), &destination)
                .await
                .unwrap_err(),
            format!(
                "plugin download failed: builder error for url ({base}/acme-macos-arm64.tar.gz)"
            )
        );
    }

    #[tokio::test]
    async fn stages_a_served_archive_into_a_reviewable_package() {
        let bytes = good_tar();
        let sha256 = hex(&Sha256::digest(&bytes));
        let base = serve(bytes).await;
        let temp = TempDir::new().unwrap();
        let store = StagingStore::new(temp.path());
        let url = format!("{base}/acme-macos-arm64.tar.gz");
        let source = PackageSource {
            url: url.clone(),
            release_tag: Some("v1.2.0".to_string()),
            expected_sha256: Some(sha256.to_ascii_uppercase()),
            format: ArchiveFormat::TarGz,
        };

        let (id, dir) = store.create_dir().unwrap();
        let (package, preview) = stage_package(
            &Downloader::insecure_for_tests(),
            &repo(),
            &source,
            &[],
            id.clone(),
            &dir,
        )
        .await
        .unwrap();
        assert_eq!(
            preview,
            StagedPluginInstall {
                staging_id: id,
                repo: "acme/planeai-plugin-acme".to_string(),
                source_url: url.clone(),
                release_tag: Some("v1.2.0".to_string()),
                integrity: PackageIntegrity::Verified {
                    sha256: sha256.clone()
                },
                manifest: StagedManifest {
                    id: "acme".to_string(),
                    name: "Acme".to_string(),
                    version: "1.2.0".to_string(),
                    host_api_version: "planeai.plugin-host.v1".to_string(),
                    capabilities: vec![],
                    ui_contributions: vec![],
                    tools: vec![],
                },
                replaces: StagedReplacement::None,
                version_mismatch: None,
            }
        );
        assert_eq!(package.dir, dir);
        assert_eq!(
            package.package_root,
            dir.join("extracted").join("acme").canonicalize().unwrap()
        );
        assert_eq!(
            package.origin,
            RemoteOrigin {
                repo: "acme/planeai-plugin-acme".to_string(),
                release_tag: Some("v1.2.0".to_string()),
                source_url: url.clone(),
                archive_sha256: sha256.clone(),
            }
        );
        assert_eq!(
            fs::read_to_string(package.package_root.join("planeai-plugin.json")).unwrap(),
            manifest_json("acme")
        );

        // The warning compares against the tag the archive came from, not the latest release.
        let (id, dir) = store.create_dir().unwrap();
        let older = PackageSource {
            release_tag: Some("v1.1.0".to_string()),
            expected_sha256: None,
            ..source.clone()
        };
        let (_, preview) = stage_package(
            &Downloader::insecure_for_tests(),
            &repo(),
            &older,
            &[inventory("acme", "0.9.0", None)],
            id,
            &dir,
        )
        .await
        .unwrap();
        assert_eq!(
            preview.integrity,
            PackageIntegrity::Unverified {
                sha256: sha256.clone()
            }
        );
        assert_eq!(
            preview.replaces,
            StagedReplacement::Local {
                installed_version: "0.9.0".to_string(),
                original_display_path: Some("/Users/me/acme".to_string()),
            }
        );
        assert_eq!(
            preview.version_mismatch,
            Some(VersionMismatch {
                release_tag: "v1.1.0".to_string(),
                manifest_version: "1.2.0".to_string(),
            })
        );

        let (id, dir) = store.create_dir().unwrap();
        let tampered = PackageSource {
            expected_sha256: Some("0000".to_string()),
            ..source
        };
        let error = stage_package(
            &Downloader::insecure_for_tests(),
            &repo(),
            &tampered,
            &[],
            id,
            &dir,
        )
        .await
        .unwrap_err();
        assert_eq!(
            error,
            format!("plugin archive SHA-256 {sha256} does not match the release asset digest 0000")
        );
        assert!(
            !dir.join("extracted").exists(),
            "nothing is extracted after a digest mismatch"
        );
    }

    /// `cargo test -- --ignored remote_install_real_catalog` stages the real GitHub plugin.
    #[tokio::test]
    #[ignore]
    async fn remote_install_real_catalog() {
        let repo = RepoFullName::parse("nicolegros/planeai-plugin-github").unwrap();
        let remote = fetch_remote_repo(repo.clone()).await.unwrap();
        let source = remote
            .select_source(crate::plugin_packages::current_platform_key())
            .unwrap();
        let temp = TempDir::new().unwrap();
        let (id, dir) = StagingStore::new(temp.path()).create_dir().unwrap();
        let (package, preview) = stage_package(
            &Downloader::new().unwrap(),
            &repo,
            &source,
            &[],
            id.clone(),
            &dir,
        )
        .await
        .unwrap();
        assert_eq!(preview.staging_id, id);
        assert_eq!(preview.manifest.id, "github");
        let imported = crate::plugin_packages::import_local_package(
            &temp.path().join("app"),
            &package.package_root,
        )
        .unwrap();
        assert_eq!(imported.manifest.id, "github");
        eprintln!(
            "staged {} from {} ({:?})",
            preview.manifest.version, preview.source_url, preview.integrity
        );
    }
}
