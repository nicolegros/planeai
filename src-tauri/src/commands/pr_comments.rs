use tauri::State;

use planeai_core::command::augmented_path;

use crate::state::DbState;

use super::pr::{resolve_github_repo, resolve_session_context};

#[tauri::command]
pub async fn get_pr_comments(
    session_id: String,
    db_state: State<'_, DbState>,
) -> Result<usize, String> {
    let ctx = resolve_session_context(&db_state, &session_id)?;

    tracing::debug!(session_id = %session_id, branch = %ctx.branch, "get_pr_comments called");

    let repo = resolve_github_repo(&ctx.cwd).await?;
    let Some(repo) = repo else {
        tracing::debug!("not a GitHub remote, skipping PR comments");
        return Ok(0);
    };

    let (owner, name) = repo
        .split_once('/')
        .ok_or_else(|| format!("invalid repo format: {repo}"))?;

    let query = r#"query($owner:String!,$repo:String!,$branch:String!){repository(owner:$owner,name:$repo){pullRequests(headRefName:$branch,first:1,states:[OPEN]){nodes{comments{totalCount}reviewThreads(first:100){nodes{isResolved}}}}}}"#;
    let jq = r#".data.repository.pullRequests.nodes[0] // null | if . then (.comments.totalCount) + ([.reviewThreads.nodes[] | select(.isResolved == false)] | length) else 0 end"#;

    let mut cmd = tokio::process::Command::new(crate::command::resolve("gh"));
    cmd.args([
        "api",
        "graphql",
        "-f",
        &format!("query={query}"),
        "-f",
        &format!("owner={owner}"),
        "-f",
        &format!("repo={name}"),
        "-f",
        &format!("branch={}", ctx.branch),
        "--jq",
        jq,
    ])
    .current_dir(&ctx.cwd);
    cmd.env("PATH", augmented_path(&[]));
    planeai_core::command::no_window_tokio(&mut cmd);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to run gh: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        tracing::warn!(stderr = %stderr, "gh api graphql failed for pr comments");
        return Ok(0);
    }

    let count: usize = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    tracing::debug!(count, "pr comments fetched");

    Ok(count)
}
