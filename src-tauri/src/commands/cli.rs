use std::path::{Path, PathBuf};

use tauri::State;

use crate::state::ConfigState;

const CLI_BIN: &str = "planeai-cli";
const CLI_LINK: &str = "/usr/local/bin/planeai-cli";

fn bundle_cli() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let name = format!("{CLI_BIN}{}", std::env::consts::EXE_SUFFIX);
    crate::paths::sibling_binary(&exe, &name)
        .ok_or_else(|| format!("binary not found: {name} next to {}", exe.display()))
}

/// Whether the first `planeai-cli` on `path_var` is, after following symlinks, `bundle_cli`.
pub fn cli_points_into(bundle_cli: &Path, path_var: &str) -> bool {
    let Some(name) = bundle_cli.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let found = crate::config::find_executable_in(name, path_var)
        .and_then(|found| std::fs::canonicalize(found).ok());
    found.is_some() && found == std::fs::canonicalize(bundle_cli).ok() && is_executable(bundle_cli)
}

#[tauri::command]
pub async fn check_cli_installed(state: State<'_, ConfigState>) -> Result<bool, String> {
    let extra_path_dirs = state
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .resolved_extra_path_dirs();
    crate::commands::blocking(move || {
        let path_var = planeai_core::command::augmented_path(&extra_path_dirs);
        Ok(bundle_cli().is_ok_and(|cli| cli_points_into(&cli, &path_var)))
    })
    .await
}

#[tauri::command]
pub async fn install_cli() -> Result<(), String> {
    crate::commands::blocking(|| symlink_binary(&bundle_cli()?, Path::new(CLI_LINK))).await
}

fn symlink_binary(source: &Path, target: &Path) -> Result<(), String> {
    if target.symlink_metadata().is_ok() {
        std::fs::remove_file(target).map_err(|e| {
            format!(
                "failed to remove existing symlink at {}: {e}",
                target.display()
            )
        })?;
    }
    #[cfg(unix)]
    {
        ensure_executable(source)?;
        std::os::unix::fs::symlink(source, target)
            .map_err(|e| format!("failed to create symlink {}: {e}", target.display()))
    }
    #[cfg(windows)]
    {
        let _ = source;
        Err(format!(
            "symlink not supported on Windows for {}",
            target.display()
        ))
    }
}

#[cfg(unix)]
fn ensure_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("failed to read metadata for {}: {e}", path.display()))?;
    let mut perms = metadata.permissions();
    let mode = perms.mode();
    if mode & 0o111 == 0 {
        perms.set_mode(mode | 0o111);
        std::fs::set_permissions(path, perms).map_err(|e| {
            format!(
                "failed to set executable permissions on {}: {e}",
                path.display()
            )
        })?;
    }
    Ok(())
}

/// Whether `path` points to an executable file (follows symlinks).
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    struct Fixture {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        bundle_cli: PathBuf,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let bundle_cli = root.join("planeai.app/Contents/MacOS/planeai-cli");
        write_executable(&bundle_cli);
        for dir in ["homebrew/bin", "usr-local/bin", "empty/bin"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        Fixture {
            _tmp: tmp,
            root,
            bundle_cli,
        }
    }

    fn write_executable(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn path_var(f: &Fixture, dirs: &[&str]) -> String {
        dirs.iter()
            .map(|dir| f.root.join(dir).display().to_string())
            .collect::<Vec<_>>()
            .join(":")
    }

    #[test]
    fn homebrew_symlink_into_the_bundle_counts_as_installed() {
        let f = fixture();
        symlink(&f.bundle_cli, f.root.join("homebrew/bin/planeai-cli")).unwrap();

        assert!(cli_points_into(
            &f.bundle_cli,
            &path_var(&f, &["empty/bin", "homebrew/bin"])
        ));
    }

    #[test]
    fn legacy_symlink_chained_through_another_link_counts_as_installed() {
        let f = fixture();
        symlink(&f.bundle_cli, f.root.join("homebrew/bin/planeai-cli")).unwrap();
        symlink(
            f.root.join("homebrew/bin/planeai-cli"),
            f.root.join("usr-local/bin/planeai-cli"),
        )
        .unwrap();

        assert!(cli_points_into(
            &f.bundle_cli,
            &path_var(&f, &["usr-local/bin"])
        ));
    }

    #[test]
    fn missing_from_path_is_not_installed() {
        let f = fixture();

        assert!(!cli_points_into(
            &f.bundle_cli,
            &path_var(&f, &["empty/bin", "homebrew/bin"])
        ));
    }

    #[test]
    fn a_different_cli_first_on_path_is_not_installed() {
        let f = fixture();
        write_executable(&f.root.join("usr-local/bin/planeai-cli"));
        symlink(&f.bundle_cli, f.root.join("homebrew/bin/planeai-cli")).unwrap();

        assert!(!cli_points_into(
            &f.bundle_cli,
            &path_var(&f, &["usr-local/bin", "homebrew/bin"])
        ));
    }

    #[test]
    fn a_dangling_symlink_is_not_installed() {
        let f = fixture();
        symlink(
            f.root.join("Old.app/Contents/MacOS/planeai-cli"),
            f.root.join("usr-local/bin/planeai-cli"),
        )
        .unwrap();

        assert!(!cli_points_into(
            &f.bundle_cli,
            &path_var(&f, &["usr-local/bin"])
        ));
    }

    #[test]
    fn install_replaces_a_stale_link_with_one_into_the_bundle() {
        let f = fixture();
        let link = f.root.join("usr-local/bin/planeai-cli");
        symlink(f.root.join("Old.app/Contents/MacOS/planeai-cli"), &link).unwrap();

        symlink_binary(&f.bundle_cli, &link).unwrap();

        assert_eq!(std::fs::read_link(&link).unwrap(), f.bundle_cli);
    }
}
