use std::path::{Path, PathBuf};

fn daemon_bin_name() -> String {
    format!("planeai-daemon{}", std::env::consts::EXE_SUFFIX)
}

/// The bundled binary `name` shipped beside `exe`, or in a macOS bundle's `Contents/Resources`.
/// `exe` is canonicalized first so a symlinked launcher such as Homebrew's
/// `/opt/homebrew/bin/planeai-cli` still finds its siblings inside the app bundle.
pub fn sibling_binary(exe: &Path, name: &str) -> Option<PathBuf> {
    let real = std::fs::canonicalize(exe).ok()?;
    let dir = real.parent()?;
    let resources = dir
        .parent()
        .map(|contents| contents.join("Resources").join(name));
    std::iter::once(dir.join(name))
        .chain(resources)
        .find(|candidate| candidate.is_file())
}

fn current_exe_sibling(name: &str) -> Option<PathBuf> {
    sibling_binary(&std::env::current_exe().ok()?, name)
}

/// Resolve the daemon binary path. In production, use the bundled resource.
/// In development, use the workspace target directory.
pub fn resolve_daemon_binary(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager as _;

    let bin_name = daemon_bin_name();
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled = resource_dir.join(&bin_name);
        if bundled.exists() {
            return bundled;
        }
    }
    current_exe_sibling(&bin_name).unwrap_or_else(|| {
        tracing::warn!("daemon binary not found alongside executable, falling back to PATH");
        PathBuf::from(bin_name)
    })
}

/// Resolve the daemon binary without an AppHandle, as the CLI and sync contexts must.
pub fn resolve_daemon_binary_fallback() -> PathBuf {
    let bin_name = daemon_bin_name();
    current_exe_sibling(&bin_name).unwrap_or_else(|| PathBuf::from(bin_name))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn sibling_binary_follows_a_symlinked_exe_into_the_app_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        touch(&root.join("planeai.app/Contents/MacOS/planeai-cli"));
        touch(&root.join("planeai.app/Contents/MacOS/planeai-daemon"));
        // A stale daemon beside the symlink must not shadow the bundle's own.
        touch(&root.join("bin/planeai-daemon"));
        symlink(
            root.join("planeai.app/Contents/MacOS/planeai-cli"),
            root.join("bin/planeai-cli"),
        )
        .unwrap();

        assert_eq!(
            sibling_binary(&root.join("bin/planeai-cli"), "planeai-daemon"),
            Some(root.join("planeai.app/Contents/MacOS/planeai-daemon"))
        );
    }

    #[test]
    fn sibling_binary_falls_back_to_bundle_resources() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        touch(&root.join("planeai.app/Contents/MacOS/planeai"));
        touch(&root.join("planeai.app/Contents/Resources/planeai-daemon"));

        assert_eq!(
            sibling_binary(
                &root.join("planeai.app/Contents/MacOS/planeai"),
                "planeai-daemon"
            ),
            Some(root.join("planeai.app/Contents/Resources/planeai-daemon"))
        );
    }

    #[test]
    fn sibling_binary_is_none_when_no_sibling_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        touch(&root.join("target/debug/planeai"));

        assert_eq!(
            sibling_binary(&root.join("target/debug/planeai"), "planeai-daemon"),
            None
        );
    }

    #[test]
    fn sibling_binary_is_none_for_a_missing_exe() {
        assert_eq!(
            sibling_binary(Path::new("/nonexistent/planeai-cli"), "planeai-daemon"),
            None
        );
    }
}
