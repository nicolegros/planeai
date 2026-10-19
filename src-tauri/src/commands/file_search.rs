use std::sync::{Arc, Mutex};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};
use planeai_core::command::{augmented_path, no_window_tokio};
use tauri::State;

/// Tracked files of the repo the file picker last opened, so keystrokes skip `git ls-files`.
#[derive(Default)]
pub struct FileSearchState(Mutex<Option<FileIndex>>);

struct FileIndex {
    repo_path: String,
    files: Arc<Vec<String>>,
}

async fn list_tracked_files(repo_path: &str) -> Result<Vec<String>, String> {
    let mut cmd = tokio::process::Command::new(crate::command::resolve("git"));
    no_window_tokio(&mut cmd);
    cmd.env("PATH", augmented_path(&[]));
    // -z keeps non-ASCII paths verbatim instead of C-quoting them
    cmd.args(["ls-files", "-z"]).current_dir(repo_path);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to run git ls-files: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(output
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect())
}

async fn rebuild_index(
    state: &FileSearchState,
    repo_path: String,
) -> Result<Arc<Vec<String>>, String> {
    let files = Arc::new(list_tracked_files(&repo_path).await?);
    *state.0.lock().unwrap() = Some(FileIndex {
        repo_path,
        files: files.clone(),
    });
    Ok(files)
}

/// Best `limit` matches for `query`, best first. An empty query keeps `git ls-files` order.
fn search(files: &[String], query: &str, limit: usize) -> Vec<String> {
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    pattern
        .match_list(files, &mut matcher)
        .into_iter()
        .take(limit)
        .map(|(file, _)| file.clone())
        .collect()
}

/// Snapshot the repo's tracked files. Called each time the file picker opens.
#[tauri::command]
pub async fn index_files(
    repo_path: String,
    state: State<'_, FileSearchState>,
) -> Result<(), String> {
    rebuild_index(&state, repo_path).await.map(|_| ())
}

#[tauri::command]
pub async fn search_files(
    repo_path: String,
    query: String,
    limit: usize,
    state: State<'_, FileSearchState>,
) -> Result<Vec<String>, String> {
    let cached = state
        .0
        .lock()
        .unwrap()
        .as_ref()
        .filter(|index| index.repo_path == repo_path)
        .map(|index| index.files.clone());
    let files = match cached {
        Some(files) => files,
        None => rebuild_index(&state, repo_path).await?,
    };
    super::blocking(move || Ok(search(&files, &query, limit))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn empty_query_keeps_listing_order_up_to_limit() {
        let all = files(&["b.rs", "a.rs", "c.rs"]);
        assert_eq!(search(&all, "", 2), files(&["b.rs", "a.rs"]));
    }

    #[test]
    fn fuzzy_query_ranks_the_closest_path_first() {
        let all = files(&[
            "src/lib/api.ts",
            "src/components/CommandMenu.svelte",
            "src/components/__tests__/CommandMenu.test.ts",
            "docs/commands.md",
        ]);
        assert_eq!(
            search(&all, "cmdmenu.svelte", 10),
            files(&["src/components/CommandMenu.svelte"])
        );
    }

    #[test]
    fn query_without_match_returns_nothing() {
        let all = files(&["src/main.rs"]);
        assert_eq!(search(&all, "zzz", 10), Vec::<String>::new());
    }

    #[tokio::test]
    async fn lists_tracked_files_with_non_ascii_names() {
        let dir = tempfile::TempDir::new().unwrap();
        let repo = dir.path();
        std::fs::write(repo.join("café.txt"), "").unwrap();
        std::fs::write(repo.join("untracked.txt"), "").unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .status()
                .unwrap();
            assert!(status.success());
        };
        git(&["init", "-q"]);
        git(&["add", "café.txt"]);

        let listed = list_tracked_files(repo.to_str().unwrap()).await.unwrap();

        assert_eq!(listed, files(&["café.txt"]));
    }
}
