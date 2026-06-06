use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

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
}

pub struct WatcherManager {
    watchers: HashMap<String, RecommendedWatcher>,
}

impl WatcherManager {
    pub fn new() -> Self {
        Self { watchers: HashMap::new() }
    }

    pub fn watch(
        &mut self,
        session_id: &str,
        path: &str,
        sender: mpsc::Sender<FsEvent>,
    ) -> Result<(), String> {
        let sid = session_id.to_string();
        let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                for p in event.paths {
                    let _ = sender.send(FsEvent {
                        session_id: sid.clone(),
                        path: p.to_string_lossy().into_owned(),
                    });
                }
            }
        }).map_err(|e| e.to_string())?;

        watcher.watch(Path::new(path), RecursiveMode::NonRecursive)
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
    use tempfile::TempDir;
    use std::fs;
    use std::sync::mpsc;

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

        assert_eq!(names, vec!["alpha", "zeta", "apple.txt", "banana.txt", "cherry.txt"]);

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
        ).unwrap();

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
        let (tx, rx) = mpsc::channel();

        let mut manager = WatcherManager::new();
        manager.watch("session-1", root.to_str().unwrap(), tx).unwrap();

        // Give watcher time to set up
        std::thread::sleep(std::time::Duration::from_millis(100));

        fs::write(root.join("watched.txt"), "hello").unwrap();

        // Wait for event (with timeout)
        let event = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        assert_eq!(event.session_id, "session-1");
        assert!(event.path.contains("watched.txt"));

        manager.unwatch("session-1");
    }
}
