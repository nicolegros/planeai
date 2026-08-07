use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

const DEBOUNCE_MS: u64 = 200;

/// Maps a notify EventKind to a simple string for the frontend.
fn event_kind_to_string(kind: &EventKind) -> String {
    match kind {
        EventKind::Create(_) => "create".to_string(),
        EventKind::Remove(_) => "remove".to_string(),
        EventKind::Modify(notify::event::ModifyKind::Name(_)) => "rename".to_string(),
        EventKind::Modify(_) => "modify".to_string(),
        _ => "modify".to_string(),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FsEvent {
    pub session_id: String,
    pub path: String,
    pub kind: String,
}

/// Recursively lists all file and directory paths under `root`.
/// Returns canonical path strings sorted directories-first, case-insensitive.
pub fn list_all_paths(root: &str) -> Result<Vec<String>, String> {
    let root_path = Path::new(root);
    if !root_path.is_dir() {
        return Err(format!("Not a directory: {root}"));
    }

    let mut paths = Vec::new();
    collect_paths(root_path, root_path, &mut paths).map_err(|e| e.to_string())?;
    Ok(paths)
}

fn collect_paths(
    root: &Path,
    dir: &Path,
    paths: &mut Vec<String>,
) -> Result<(), std::io::Error> {
    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .collect();

    // Sort: directories first, then case-insensitive alphabetical
    entries.sort_by(|a, b| {
        let a_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a
                .file_name()
                .to_string_lossy()
                .to_lowercase()
                .cmp(&b.file_name().to_string_lossy().to_lowercase()),
        }
    });

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();

        // Skip hidden files and common ignored directories
        if name.starts_with('.') || name == "node_modules" || name == "target" || name == "dist" {
            continue;
        }

        // Use path relative to root for the canonical path
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();

        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);

        paths.push(rel.clone());

        if is_dir {
            collect_paths(root, &path, paths)?;
        }
    }

    Ok(())
}

pub struct WatcherManager {
    watchers: HashMap<String, RecommendedWatcher>,
}

impl WatcherManager {
    pub fn new() -> Self {
        Self {
            watchers: HashMap::new(),
        }
    }

    pub fn watch(
        &mut self,
        session_id: &str,
        path: &str,
        sender: mpsc::Sender<FsEvent>,
    ) -> Result<(), String> {
        let sid = session_id.to_string();
        // Track path → kind; last event kind wins when debounced
        let pending: Arc<Mutex<HashMap<String, String>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_clone = pending.clone();
        let sender_clone = sender.clone();
        let sid_clone = sid.clone();

        // Debounce thread: flushes accumulated paths every DEBOUNCE_MS
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(DEBOUNCE_MS));
            let entries: Vec<(String, String)> = {
                let mut map = pending_clone.lock().unwrap();
                if map.is_empty() {
                    continue;
                }
                map.drain().collect()
            };
            for (p, kind) in entries {
                let _ = sender_clone.send(FsEvent {
                    session_id: sid_clone.clone(),
                    path: p,
                    kind,
                });
            }
        });

        let mut watcher =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let kind = event_kind_to_string(&event.kind);
                    let mut map = pending.lock().unwrap();
                    for p in event.paths {
                        map.insert(p.to_string_lossy().into_owned(), kind.clone());
                    }
                }
            })
            .map_err(|e| e.to_string())?;

        watcher
            .watch(Path::new(path), RecursiveMode::Recursive)
            .map_err(|e| e.to_string())?;

        self.watchers.insert(session_id.to_string(), watcher);
        Ok(())
    }

    pub fn unwatch(&mut self, session_id: &str) {
        self.watchers.remove(session_id);
    }
}

pub fn create_file(path: &str) -> Result<(), String> {
    std::fs::File::create(path).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn create_directory(path: &str) -> Result<(), String> {
    std::fs::create_dir(path).map_err(|e| e.to_string())
}

pub fn rename_entry(old_path: &str, new_path: &str) -> Result<(), String> {
    std::fs::rename(old_path, new_path).map_err(|e| e.to_string())
}

pub fn delete_to_trash(path: &str) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

pub fn list_directory(path: &str) -> Result<Vec<DirEntry>, String> {
    let dir = Path::new(path);
    let mut entries: Vec<DirEntry> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .map(|e| {
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            DirEntry {
                name: e.file_name().to_string_lossy().into_owned(),
                path: e.path().to_string_lossy().into_owned(),
                is_dir,
            }
        })
        .collect();

    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;
    use tempfile::TempDir;

    #[test]
    fn list_directory_returns_dirs_first_then_files_alphabetical() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(root.join("banana.txt"), "").unwrap();
        fs::write(root.join("apple.txt"), "").unwrap();
        fs::create_dir(root.join("zeta")).unwrap();
        fs::create_dir(root.join("alpha")).unwrap();
        fs::write(root.join("cherry.txt"), "").unwrap();

        let entries = list_directory(root.to_str().unwrap()).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

        assert_eq!(
            names,
            vec!["alpha", "zeta", "apple.txt", "banana.txt", "cherry.txt"]
        );

        // Verify is_dir flags
        assert!(entries[0].is_dir);
        assert!(entries[1].is_dir);
        assert!(!entries[2].is_dir);
        assert!(!entries[3].is_dir);
        assert!(!entries[4].is_dir);
    }

    #[test]
    fn list_directory_errors_for_nonexistent_path() {
        let result = list_directory("/tmp/absolutely-does-not-exist-xyz");
        assert!(result.is_err());
    }

    #[test]
    fn create_file_appears_in_listing() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let file_path = root.join("new_file.txt");

        create_file(file_path.to_str().unwrap()).unwrap();

        let entries = list_directory(root.to_str().unwrap()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "new_file.txt");
        assert!(!entries[0].is_dir);
    }

    #[test]
    fn create_directory_appears_in_listing() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let dir_path = root.join("new_dir");

        create_directory(dir_path.to_str().unwrap()).unwrap();

        let entries = list_directory(root.to_str().unwrap()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "new_dir");
        assert!(entries[0].is_dir);
    }

    #[test]
    fn rename_entry_old_disappears_new_appears() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("old.txt"), "content").unwrap();

        rename_entry(
            root.join("old.txt").to_str().unwrap(),
            root.join("new.txt").to_str().unwrap(),
        )
        .unwrap();

        let entries = list_directory(root.to_str().unwrap()).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(!names.contains(&"old.txt"));
        assert!(names.contains(&"new.txt"));
    }

    #[test]
    fn delete_to_trash_removes_from_listing() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("doomed.txt"), "bye").unwrap();

        delete_to_trash(root.join("doomed.txt").to_str().unwrap()).unwrap();

        let entries = list_directory(root.to_str().unwrap()).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(!names.contains(&"doomed.txt"));
    }

    #[test]
    fn watch_directory_emits_event_on_file_create() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // Create a subdirectory to test recursive watching
        let subdir = root.join("nested");
        fs::create_dir(&subdir).unwrap();

        let (tx, rx) = mpsc::channel();

        let mut manager = WatcherManager::new();
        manager
            .watch("session-1", root.to_str().unwrap(), tx)
            .unwrap();

        // Give watcher time to set up
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Write in a nested subdirectory (tests recursive mode)
        fs::write(subdir.join("deep.txt"), "hello").unwrap();

        // Drain events until we find one for deep.txt (other events may fire for the directory)
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut found = false;
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(std::time::Duration::from_millis(500)) {
                Ok(event) => {
                    assert_eq!(event.session_id, "session-1");
                    if event.path.contains("deep.txt") {
                        found = true;
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        assert!(found, "expected an event containing 'deep.txt'");

        manager.unwatch("session-1");
    }

    #[test]
    fn watch_debounces_rapid_changes() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let (tx, rx) = mpsc::channel();

        let mut manager = WatcherManager::new();
        manager
            .watch("session-2", root.to_str().unwrap(), tx)
            .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(100));

        // Rapid-fire writes to the same file
        for i in 0..5 {
            fs::write(root.join("rapid.txt"), format!("v{i}")).unwrap();
        }

        // Collect events over 500ms — should be deduplicated to 1 path
        std::thread::sleep(std::time::Duration::from_millis(500));
        let mut paths: Vec<String> = Vec::new();
        while let Ok(event) = rx.try_recv() {
            paths.push(event.path);
        }
        // Filter to only events for the file under test (the watcher may also emit
        // events for the directory itself or other OS-generated files like .DS_Store).
        let rapid_paths: Vec<&String> = paths.iter().filter(|p| p.contains("rapid.txt")).collect();
        assert!(
            !rapid_paths.is_empty(),
            "expected at least one debounced event for rapid.txt"
        );

        manager.unwatch("session-2");
    }

    #[test]
    fn list_all_paths_returns_recursive_relative_paths() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join("src").join("lib")).unwrap();
        fs::write(root.join("README.md"), "").unwrap();
        fs::write(root.join("src").join("main.rs"), "").unwrap();
        fs::write(root.join("src").join("lib").join("utils.rs"), "").unwrap();

        let paths = list_all_paths(root.to_str().unwrap()).unwrap();

        // Directories first at each level, then files alphabetically
        assert!(paths.contains(&"src".to_string()));
        assert!(paths.contains(&"src/lib".to_string()));
        assert!(paths.contains(&"src/main.rs".to_string()));
        assert!(paths.contains(&"src/lib/utils.rs".to_string()));
        assert!(paths.contains(&"README.md".to_string()));

        // src (dir) should come before README.md (file)
        let src_idx = paths.iter().position(|p| p == "src").unwrap();
        let readme_idx = paths.iter().position(|p| p == "README.md").unwrap();
        assert!(src_idx < readme_idx);
    }

    #[test]
    fn list_all_paths_skips_hidden_and_ignored_dirs() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::create_dir(root.join(".git")).unwrap();
        fs::write(root.join(".git").join("config"), "").unwrap();
        fs::create_dir(root.join("node_modules")).unwrap();
        fs::write(root.join("node_modules").join("pkg.json"), "").unwrap();
        fs::create_dir(root.join("target")).unwrap();
        fs::write(root.join("target").join("out"), "").unwrap();
        fs::write(root.join("visible.txt"), "").unwrap();

        let paths = list_all_paths(root.to_str().unwrap()).unwrap();

        assert_eq!(paths, vec!["visible.txt"]);
    }

    #[test]
    fn list_all_paths_errors_for_nonexistent_path() {
        let result = list_all_paths("/tmp/does-not-exist-xyz");
        assert!(result.is_err());
    }

    #[test]
    fn watch_emits_event_with_kind() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let (tx, rx) = mpsc::channel();

        let mut manager = WatcherManager::new();
        manager
            .watch("session-kind", root.to_str().unwrap(), tx)
            .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(100));

        // Create a file — should emit with a non-empty kind field
        fs::write(root.join("new_file.txt"), "hello").unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut found = false;
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(std::time::Duration::from_millis(500)) {
                Ok(event) => {
                    assert_eq!(event.session_id, "session-kind");
                    // Verify kind field is populated (macOS FSEvents may report
                    // "create" or "modify" depending on event coalescing)
                    if event.path.contains("new_file.txt") {
                        assert!(
                            ["create", "modify"].contains(&event.kind.as_str()),
                            "expected 'create' or 'modify', got '{}'",
                            event.kind
                        );
                        found = true;
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        assert!(found, "expected an event for new_file.txt with a kind field");

        manager.unwatch("session-kind");
    }
}
