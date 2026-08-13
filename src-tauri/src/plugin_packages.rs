use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::plugins::{PluginManifest, PluginSourceKind};

pub const MANIFEST_FILE: &str = "planeai-plugin.json";

#[derive(Debug, Clone)]
pub struct ImportedLocalPackage {
    pub manifest: PluginManifest,
    pub backend_entrypoint: String,
    pub content_hash: String,
    pub package_dir: PathBuf,
    pub original_display_path: String,
}

pub fn current_platform_key() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "macos-arm64",
        ("macos", "x86_64") => "macos-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("windows", "x86_64") => "windows-x64",
        (os, arch) => Box::leak(format!("{os}-{arch}").into_boxed_str()),
    }
}

pub fn import_local_package(
    app_data_dir: &Path,
    source: &Path,
) -> Result<ImportedLocalPackage, String> {
    let source = source
        .canonicalize()
        .map_err(|error| format!("failed to resolve selected plugin directory: {error}"))?;
    if !source.is_dir() {
        return Err("selected plugin package must be a directory".to_string());
    }

    let manifest_path = source.join(MANIFEST_FILE);
    let manifest: PluginManifest = serde_json::from_reader(
        fs::File::open(&manifest_path)
            .map_err(|error| format!("failed to read plugin manifest: {error}"))?,
    )
    .map_err(|error| format!("failed to parse plugin manifest: {error}"))?;
    manifest.validate()?;
    if manifest.source_kind != PluginSourceKind::Local {
        return Err("a locally installed package must declare source_kind \"local\"".to_string());
    }

    let platform = current_platform_key();
    let backend_entrypoint = manifest
        .backend_entrypoints
        .get(platform)
        .cloned()
        .ok_or_else(|| format!("plugin {} has no backend for {platform}", manifest.id))?;
    validate_package_path(&backend_entrypoint, "backend entrypoint")?;
    if let Some(entrypoint) = &manifest.ui_entrypoint {
        validate_package_path(entrypoint, "UI entrypoint")?;
    }

    let backend_path = source.join(&backend_entrypoint);
    if !backend_path.is_file() {
        return Err(format!(
            "plugin backend for {platform} is missing: {backend_entrypoint}"
        ));
    }
    if !is_executable(&backend_path)? {
        return Err(format!(
            "plugin backend for {platform} is not executable: {backend_entrypoint}"
        ));
    }
    if let Some(entrypoint) = &manifest.ui_entrypoint {
        if !source.join(entrypoint).is_file() {
            return Err(format!("plugin UI entrypoint is missing: {entrypoint}"));
        }
    }

    let packages_dir = app_data_dir.join("plugins").join("packages").join("sha256");
    let staging = packages_dir.join(format!(".staging-{}", Uuid::new_v4()));
    fs::create_dir_all(&staging)
        .map_err(|error| format!("failed to create package staging directory: {error}"))?;
    let copy_result = copy_tree(&source, &staging, &mut HashSet::new());
    if let Err(error) = copy_result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    let content_hash = hash_tree(&staging)?;
    let package_dir = packages_dir.join(&content_hash);
    if package_dir.exists() {
        fs::remove_dir_all(&staging).map_err(|error| {
            format!("failed to discard existing package staging directory: {error}")
        })?;
    } else {
        fs::rename(&staging, &package_dir)
            .map_err(|error| format!("failed to publish imported plugin package: {error}"))?;
    }

    Ok(ImportedLocalPackage {
        manifest,
        backend_entrypoint,
        content_hash,
        package_dir,
        original_display_path: source.display().to_string(),
    })
}

pub fn validate_package_path(path: &str, label: &str) -> Result<(), String> {
    let value = Path::new(path);
    if value.as_os_str().is_empty()
        || value.is_absolute()
        || value.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "plugin {label} must be a safe package-relative path: {path}"
        ));
    }
    Ok(())
}

pub fn remove_local_artifacts(
    app_data_dir: &Path,
    plugin_id: &str,
    package_path: Option<&Path>,
) -> Result<(), String> {
    let state = state_root(app_data_dir, plugin_id);
    if state.exists() {
        fs::remove_dir_all(&state)
            .map_err(|error| format!("failed to delete plugin state: {error}"))?;
    }
    if let Some(package) = package_path.filter(|path| path.exists()) {
        fs::remove_dir_all(package)
            .map_err(|error| format!("failed to delete imported plugin package: {error}"))?;
    }
    Ok(())
}

pub fn state_root(app_data_dir: &Path, plugin_id: &str) -> PathBuf {
    app_data_dir.join("plugins").join("state").join(plugin_id)
}

fn copy_tree(
    source: &Path,
    destination: &Path,
    visited_directories: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    let metadata = fs::metadata(source).map_err(|error| {
        format!(
            "failed to read plugin package entry {}: {error}",
            source.display()
        )
    })?;
    if metadata.is_dir() {
        let canonical = source.canonicalize().map_err(|error| {
            format!(
                "failed to resolve plugin package directory {}: {error}",
                source.display()
            )
        })?;
        if !visited_directories.insert(canonical) {
            return Err(format!(
                "plugin package contains a directory link cycle at {}",
                source.display()
            ));
        }
        fs::create_dir_all(destination).map_err(|error| {
            format!(
                "failed to create imported package directory {}: {error}",
                destination.display()
            )
        })?;
        let mut entries = fs::read_dir(source)
            .map_err(|error| {
                format!(
                    "failed to read plugin package directory {}: {error}",
                    source.display()
                )
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                format!(
                    "failed to enumerate plugin package directory {}: {error}",
                    source.display()
                )
            })?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_tree(
                &entry.path(),
                &destination.join(entry.file_name()),
                visited_directories,
            )?;
        }
        visited_directories.remove(&source.canonicalize().unwrap_or_default());
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(format!(
            "plugin package contains an unsupported entry: {}",
            source.display()
        ));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create imported package parent: {error}"))?;
    }
    let mut input = fs::File::open(source).map_err(|error| {
        format!(
            "failed to open plugin package file {}: {error}",
            source.display()
        )
    })?;
    let mut output = fs::File::create(destination).map_err(|error| {
        format!(
            "failed to create imported plugin file {}: {error}",
            destination.display()
        )
    })?;
    std::io::copy(&mut input, &mut output).map_err(|error| {
        format!(
            "failed to copy plugin package file {}: {error}",
            source.display()
        )
    })?;
    preserve_mode(&metadata, destination)?;
    Ok(())
}

fn hash_tree(root: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for relative in files {
        let path = root.join(&relative);
        let metadata = fs::metadata(&path).map_err(|error| {
            format!(
                "failed to inspect imported plugin file {}: {error}",
                path.display()
            )
        })?;
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        hasher.update(executable_mode(&metadata).to_le_bytes());
        hasher.update(metadata.len().to_le_bytes());
        let mut file = fs::File::open(&path).map_err(|error| {
            format!(
                "failed to hash imported plugin file {}: {error}",
                path.display()
            )
        })?;
        let mut buffer = [0_u8; 8192];
        loop {
            let read = file.read(&mut buffer).map_err(|error| {
                format!(
                    "failed to hash imported plugin file {}: {error}",
                    path.display()
                )
            })?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_files(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| {
            format!(
                "failed to read imported plugin directory {}: {error}",
                directory.display()
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            format!(
                "failed to enumerate imported plugin directory {}: {error}",
                directory.display()
            )
        })?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, files)?;
        } else if path.is_file() {
            files.push(
                path.strip_prefix(root)
                    .expect("imported path remains under root")
                    .to_path_buf(),
            );
        }
    }
    Ok(())
}

#[cfg(unix)]
fn preserve_mode(metadata: &fs::Metadata, destination: &Path) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    fs::set_permissions(destination, fs::Permissions::from_mode(metadata.mode()))
        .map_err(|error| format!("failed to preserve plugin executable mode: {error}"))
}

#[cfg(not(unix))]
fn preserve_mode(_metadata: &fs::Metadata, _destination: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn executable_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    metadata.mode() & 0o111
}

#[cfg(not(unix))]
fn executable_mode(_metadata: &fs::Metadata) -> u32 {
    0
}

#[cfg(unix)]
fn is_executable(path: &Path) -> Result<bool, String> {
    use std::os::unix::fs::MetadataExt;
    Ok(fs::metadata(path)
        .map_err(|error| format!("failed to inspect plugin backend: {error}"))?
        .mode()
        & 0o111
        != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> Result<bool, String> {
    Ok(path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn write_package(root: &Path, id: &str) {
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join("bin/plugin"), b"fixture-v1").unwrap();
        #[cfg(unix)]
        fs::set_permissions(root.join("bin/plugin"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(
            root.join(MANIFEST_FILE),
            format!(
                r#"{{
  "schema": "planeai.plugin.v1",
  "id": "{id}",
  "name": "Fixture",
  "version": "1.0.0",
  "host_api_version": "planeai.plugin-host.v1",
  "source_kind": "local",
  "backend_entrypoints": {{ "{}": "bin/plugin" }}
}}"#,
                current_platform_key()
            ),
        )
        .unwrap();
    }

    #[test]
    fn imports_an_immutable_copy_under_the_application_data_root() {
        let temp = TempDir::new().unwrap();
        let app_data_dir = temp.path().join("application-support").join("planeai");
        let source = temp.path().join("source");
        write_package(&source, "fixture");
        fs::write(source.join("data.txt"), b"original").unwrap();
        let imported = import_local_package(&app_data_dir, &source).unwrap();
        assert!(imported
            .package_dir
            .starts_with(app_data_dir.join("plugins").join("packages").join("sha256")));
        fs::write(source.join("data.txt"), b"changed").unwrap();
        fs::remove_dir_all(&source).unwrap();
        assert_eq!(
            fs::read(imported.package_dir.join("data.txt")).unwrap(),
            b"original"
        );
        assert!(imported.package_dir.join("bin/plugin").is_file());
    }

    #[test]
    fn rejects_missing_current_platform_binary_before_import() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join(MANIFEST_FILE),
            r#"{
          "schema": "planeai.plugin.v1", "id": "fixture", "name": "Fixture", "version": "1",
          "host_api_version": "planeai.plugin-host.v1", "source_kind": "local",
          "backend_entrypoints": { "windows-x64": "bin/plugin.exe" }
        }"#,
        )
        .unwrap();
        assert!(import_local_package(temp.path(), &source)
            .unwrap_err()
            .contains("has no backend"));
    }

    #[test]
    fn removal_deletes_only_host_owned_import_and_state() {
        let temp = TempDir::new().unwrap();
        let original = temp.path().join("original-package");
        fs::create_dir_all(&original).unwrap();
        fs::write(original.join("keep.txt"), b"original").unwrap();
        let imported = temp.path().join("plugins/packages/sha256/hash");
        fs::create_dir_all(&imported).unwrap();
        fs::write(imported.join("plugin"), b"imported").unwrap();
        let state = state_root(temp.path(), "fixture");
        fs::create_dir_all(state.join("data")).unwrap();
        fs::write(state.join("data/state.db"), b"state").unwrap();

        remove_local_artifacts(temp.path(), "fixture", Some(&imported)).unwrap();

        assert!(original.join("keep.txt").is_file());
        assert!(!imported.exists());
        assert!(!state.exists());
    }

    #[test]
    fn rejects_unsafe_declared_entrypoints() {
        assert!(validate_package_path("../plugin", "backend").is_err());
        assert!(validate_package_path("/plugin", "backend").is_err());
        assert!(validate_package_path("bin/plugin", "backend").is_ok());
    }
}
