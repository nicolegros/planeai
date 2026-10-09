//! Paths persisted for a project are stored relative to its root when they live under it,
//! so moving the project folder does not orphan them. Paths outside the root stay absolute.

use std::path::Path;

/// The stored form of `path`: relative to `project_root` when under it, else unchanged.
pub fn encode(project_root: &str, path: &str) -> String {
    let path = Path::new(path);
    if !path.is_absolute() {
        return path.to_string_lossy().into_owned();
    }
    let relative = strip(Path::new(project_root), path).or_else(|| {
        let root = std::fs::canonicalize(project_root).ok()?;
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        strip(&root, &path)
    });
    relative.unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn strip(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    Some(if relative.as_os_str().is_empty() {
        ".".to_string()
    } else {
        relative.to_string_lossy().into_owned()
    })
}

/// The absolute form of a stored path. Absolute stored paths are returned unchanged.
pub fn resolve(project_root: &str, stored: &str) -> String {
    if stored == "." {
        return project_root.to_string();
    }
    Path::new(project_root)
        .join(stored)
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_under_the_project_are_stored_relative() {
        assert_eq!(
            encode("/repos/app", "/repos/app/.planeai/loops/l1/v.log"),
            ".planeai/loops/l1/v.log"
        );
        assert_eq!(encode("/repos/app", "/repos/app"), ".");
    }

    #[test]
    fn paths_outside_the_project_stay_absolute() {
        assert_eq!(
            encode(
                "/repos/app",
                "/home/u/.planeai/worktrees/app/ab12/handoff.json"
            ),
            "/home/u/.planeai/worktrees/app/ab12/handoff.json"
        );
        assert_eq!(
            encode("/repos/app", "/repos/application/x"),
            "/repos/application/x"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_root_still_matches_its_canonical_form() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir_all(real.join("sub")).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let canonical_file = std::fs::canonicalize(real.join("sub")).unwrap();

        assert_eq!(
            encode(link.to_str().unwrap(), canonical_file.to_str().unwrap()),
            "sub"
        );
    }

    #[test]
    fn stored_paths_resolve_against_the_current_root() {
        assert_eq!(
            resolve("/moved/app", ".planeai/v.log"),
            "/moved/app/.planeai/v.log"
        );
        assert_eq!(
            resolve("/moved/app", "/elsewhere/h.json"),
            "/elsewhere/h.json"
        );
        assert_eq!(resolve("/moved/app", "."), "/moved/app");
    }
}
