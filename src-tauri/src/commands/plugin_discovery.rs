use std::path::Path;
use std::time::Duration;

use planeai_core::command::{augmented_path, no_window_tokio};
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::Semaphore;
use tokio::time::timeout;

use crate::plugin_remote::{
    check_integrity, fetch_remote_repo, replacement, version_mismatch, ArchiveFormat,
    DiscoveryCardState, Downloader, RepoFullName, StagedManifest, StagedPackage,
    StagedPluginInstall, StagingStore, EXPIRED_STAGING,
};
use crate::plugins::{PluginInventory, PluginRuntimeHandle, RemoteOrigin};

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(30);
const DISCOVERY_ARGS: &[&str] = &[
    "search",
    "repos",
    "--topic",
    "planeai",
    "--visibility",
    "public",
    "--archived=false",
    "--include-forks=false",
    "--sort",
    "stars",
    "--order",
    "desc",
    "--limit",
    "100",
    "--json",
    "fullName,description,url,updatedAt,stargazersCount,language,isArchived,isFork",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginDiscoveryCandidate {
    pub full_name: String,
    pub description: Option<String>,
    pub url: String,
    pub updated_at: String,
    pub stargazers_count: u64,
    pub language: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GithubRepository {
    full_name: String,
    description: Option<String>,
    url: String,
    updated_at: String,
    stargazers_count: u64,
    language: Option<String>,
    is_archived: bool,
    is_fork: bool,
}

#[tauri::command]
pub async fn discover_plugins() -> Result<Vec<PluginDiscoveryCandidate>, String> {
    let mut command = tokio::process::Command::new(crate::command::resolve("gh"));
    no_window_tokio(&mut command);
    command.env("PATH", augmented_path(&[]));
    command.args(DISCOVERY_ARGS);

    let output = timeout(DISCOVERY_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            "GitHub plugin discovery timed out after 30 seconds. Please retry.".to_string()
        })?
        .map_err(|error| format!("failed to start GitHub CLI for plugin discovery: {error}"))?;

    if !output.status.success() {
        let diagnostic = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if diagnostic.is_empty() {
            format!(
                "GitHub plugin discovery failed with status {}.",
                output.status
            )
        } else {
            format!("GitHub plugin discovery failed: {diagnostic}")
        });
    }

    parse_candidates(&output.stdout)
}

fn parse_candidates(output: &[u8]) -> Result<Vec<PluginDiscoveryCandidate>, String> {
    let mut candidates = serde_json::from_slice::<Vec<GithubRepository>>(output)
        .map_err(|error| format!("GitHub plugin discovery returned invalid JSON: {error}"))?
        .into_iter()
        // Keep the UI safe even if a future gh version stops applying a filter.
        .filter(|repository| !repository.is_archived && !repository.is_fork)
        .map(|repository| PluginDiscoveryCandidate {
            full_name: repository.full_name,
            description: repository.description,
            url: repository.url,
            updated_at: repository.updated_at,
            stargazers_count: repository.stargazers_count,
            language: repository.language,
        })
        .collect::<Vec<_>>();

    // GitHub receives the same sort request, but preserve the product contract
    // if a response is reordered by a CLI or API implementation.
    candidates.sort_by(|left, right| {
        right
            .stargazers_count
            .cmp(&left.stargazers_count)
            .then_with(|| left.full_name.cmp(&right.full_name))
    });
    Ok(candidates)
}

/// Discovery resolves every card at once; this caps the concurrent `gh` processes.
const RESOLVE_CONCURRENCY: usize = 8;

/// Staging and the shared download client for Discovery installs (ADR-0018).
pub struct PluginRemoteState {
    pub staging: StagingStore,
    resolve_permits: Semaphore,
    downloader: Downloader,
}

impl PluginRemoteState {
    pub fn new(app_data_dir: &Path) -> Result<Self, String> {
        let staging = StagingStore::new(app_data_dir);
        if let Err(error) = staging.reset() {
            tracing::warn!(%error, "failed to clear stale plugin staging");
        }
        Ok(Self {
            staging,
            resolve_permits: Semaphore::new(RESOLVE_CONCURRENCY),
            downloader: Downloader::new()?,
        })
    }
}

#[tauri::command]
pub async fn resolve_plugin_candidate(
    repo: String,
    remote: State<'_, PluginRemoteState>,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<DiscoveryCardState, String> {
    let repo = RepoFullName::parse(&repo)?;
    let _permit = remote
        .resolve_permits
        .acquire()
        .await
        .map_err(|error| error.to_string())?;
    let remote_repo = fetch_remote_repo(repo).await?;
    let inventory = runtime.0.list().await?;
    Ok(remote_repo.card_state(&inventory, crate::plugin_packages::current_platform_key()))
}

#[tauri::command]
pub async fn stage_remote_plugin(
    repo: String,
    remote: State<'_, PluginRemoteState>,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<StagedPluginInstall, String> {
    let repo = RepoFullName::parse(&repo)?;
    let remote_repo = fetch_remote_repo(repo.clone()).await?;
    let platform = crate::plugin_packages::current_platform_key();
    let source = remote_repo.select_source(platform)?;
    let format = ArchiveFormat::from_url(&source.url)?;
    let (staging_id, dir) = remote.staging.create_dir()?;
    let staged = stage_into(&remote, &runtime, &remote_repo, &source, format, &dir).await;
    match staged {
        Ok((package, preview)) => {
            remote.staging.insert(staging_id.clone(), package)?;
            Ok(StagedPluginInstall {
                staging_id,
                ..preview
            })
        }
        Err(error) => {
            let _ = crate::commands::blocking(move || crate::plugin_remote::remove_dir(&dir)).await;
            Err(error)
        }
    }
}

async fn stage_into(
    remote: &PluginRemoteState,
    runtime: &PluginRuntimeHandle,
    remote_repo: &crate::plugin_remote::RemoteRepo,
    source: &crate::plugin_remote::PackageSource,
    format: ArchiveFormat,
    dir: &Path,
) -> Result<(StagedPackage, StagedPluginInstall), String> {
    let archive = dir.join(format.file_name());
    let sha256 = remote.downloader.download(&source.url, &archive).await?;
    let integrity = check_integrity(sha256.clone(), source.expected_sha256.as_deref())?;
    let extracted = dir.join("extracted");
    let inspected = crate::commands::blocking(move || {
        let root = crate::plugin_remote::extract_archive(&archive, format, &extracted)?;
        crate::plugin_packages::inspect_package(&root)
    })
    .await?;
    let inventory = runtime.0.list().await?;
    let replaces = replacement(&inventory, &inspected.manifest.id)?;
    let latest_tag = remote_repo
        .latest_release
        .as_ref()
        .map(|release| release.tag_name.as_str());
    let preview = StagedPluginInstall {
        staging_id: String::new(),
        repo: remote_repo.repo.as_str().to_string(),
        source_url: source.url.clone(),
        release_tag: source.release_tag.clone(),
        integrity,
        manifest: StagedManifest::from(&inspected.manifest),
        replaces,
        version_mismatch: version_mismatch(latest_tag, &inspected.manifest.version),
    };
    let package = StagedPackage {
        dir: dir.to_path_buf(),
        package_root: inspected.source,
        origin: RemoteOrigin {
            repo: remote_repo.repo.as_str().to_string(),
            release_tag: source.release_tag.clone(),
            source_url: source.url.clone(),
            archive_sha256: sha256,
        },
    };
    Ok((package, preview))
}

#[tauri::command]
pub async fn commit_staged_plugin(
    staging_id: String,
    remote: State<'_, PluginRemoteState>,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<PluginInventory, String> {
    let staged = remote
        .staging
        .take(&staging_id)?
        .ok_or_else(|| EXPIRED_STAGING.to_string())?;
    let result = runtime
        .0
        .install_remote(staged.package_root, staged.origin)
        .await;
    // The import copied the package, so staging is removable whether or not it succeeded.
    if let Err(error) =
        crate::commands::blocking(move || crate::plugin_remote::remove_dir(&staged.dir)).await
    {
        tracing::warn!(%error, "failed to remove committed plugin staging directory");
    }
    result
}

#[tauri::command]
pub async fn discard_staged_plugin(
    staging_id: String,
    remote: State<'_, PluginRemoteState>,
) -> Result<(), String> {
    let staged = remote.staging.take(&staging_id)?;
    match staged {
        Some(staged) => {
            crate::commands::blocking(move || crate::plugin_remote::remove_dir(&staged.dir)).await
        }
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_the_fixed_public_planeai_catalog() {
        assert_eq!(
            DISCOVERY_ARGS,
            [
                "search",
                "repos",
                "--topic",
                "planeai",
                "--visibility",
                "public",
                "--archived=false",
                "--include-forks=false",
                "--sort",
                "stars",
                "--order",
                "desc",
                "--limit",
                "100",
                "--json",
                "fullName,description,url,updatedAt,stargazersCount,language,isArchived,isFork",
            ]
        );
    }

    #[test]
    fn filters_non_candidates_and_sorts_by_stars() {
        let candidates = parse_candidates(
            br#"[
              {"fullName":"planeai/low","description":null,"url":"https://github.com/planeai/low","updatedAt":"2026-09-20T00:00:00Z","stargazersCount":1,"language":null,"isArchived":false,"isFork":false},
              {"fullName":"planeai/fork","description":"copy","url":"https://github.com/planeai/fork","updatedAt":"2026-09-20T00:00:00Z","stargazersCount":100,"language":"Rust","isArchived":false,"isFork":true},
              {"fullName":"planeai/archived","description":"old","url":"https://github.com/planeai/archived","updatedAt":"2026-09-20T00:00:00Z","stargazersCount":99,"language":"Rust","isArchived":true,"isFork":false},
              {"fullName":"planeai/high","description":"active","url":"https://github.com/planeai/high","updatedAt":"2026-09-21T00:00:00Z","stargazersCount":3,"language":"Rust","isArchived":false,"isFork":false}
            ]"#,
        )
        .unwrap();

        assert_eq!(
            candidates,
            vec![
                PluginDiscoveryCandidate {
                    full_name: "planeai/high".to_string(),
                    description: Some("active".to_string()),
                    url: "https://github.com/planeai/high".to_string(),
                    updated_at: "2026-09-21T00:00:00Z".to_string(),
                    stargazers_count: 3,
                    language: Some("Rust".to_string()),
                },
                PluginDiscoveryCandidate {
                    full_name: "planeai/low".to_string(),
                    description: None,
                    url: "https://github.com/planeai/low".to_string(),
                    updated_at: "2026-09-20T00:00:00Z".to_string(),
                    stargazers_count: 1,
                    language: None,
                },
            ]
        );
    }

    #[test]
    fn rejects_invalid_cli_json() {
        assert!(parse_candidates(b"not json").is_err());
    }
}
