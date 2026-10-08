use std::time::Duration;

use planeai_core::command::{augmented_path, no_window_tokio};
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

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
