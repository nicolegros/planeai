use super::git_cmd;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CommitEntry {
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
}

/// List the last N commits on the current branch.
pub fn list_commits(repo_path: &str, limit: u32) -> Result<Vec<CommitEntry>, String> {
    let output = git_cmd()
        .args(["log", "--format=%H %h %s", &format!("-{limit}")])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let mut commits = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        // Format: full_sha short_sha subject (subject may contain spaces)
        let mut parts = line.splitn(3, ' ');
        let sha = parts.next().unwrap_or("").to_string();
        let short_sha = parts.next().unwrap_or("").to_string();
        let subject = parts.next().unwrap_or("").to_string();
        if !sha.is_empty() {
            commits.push(CommitEntry {
                sha,
                short_sha,
                subject,
            });
        }
    }

    Ok(commits)
}
