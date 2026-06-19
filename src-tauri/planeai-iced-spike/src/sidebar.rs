//! Sidebar module — project/task/session navigation panel.
//!
//! Mirrors the behaviour of the Svelte UnifiedSidebar component.
//! Uses the same codepath as the Tauri app:
//! - Projects via `ProjectService::list_active()`
//! - Sessions via `SessionService::list_active()`
//! - Tasks via `SqliteRepository::open(db_path, prefix).list()`

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use iced::widget::{column, container, scrollable, text};
use iced::{Color, Element, Font, Length, Theme};
use rusqlite::Connection;

use planeai_core::services::{self, ProjectService, SessionRecord, SessionService};
use planeai_tasks::model::ListFilter;
use planeai_tasks::provider::TaskProvider;
use planeai_tasks::sqlite::{derive_prefix, SqliteRepository};

// ─── Nav item types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum NavItem {
    ProjectHeader { project_id: String, name: String },
    OrphanSession { session_id: String, name: String, status: String },
    StatusHeader { project_path: String, status: String, count: usize },
    Task { key: String, title: String, status: String, linked_session_id: Option<String> },
}

// ─── Actions returned to the parent ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SidebarAction {
    None,
    SwitchSession(String),
    FocusTerminal,
}

// ─── Status helpers ──────────────────────────────────────────────────────────

fn status_label(status: &str) -> &str {
    match status {
        "in_progress" => "In Progress",
        "in_review" => "In Review",
        "todo" => "Todo",
        "done" => "Done",
        _ => status,
    }
}

fn status_color(status: &str) -> Color {
    match status {
        "todo" => Color::from_rgb8(59, 130, 246),
        "in_progress" => Color::from_rgb8(245, 158, 11),
        "in_review" => Color::from_rgb8(34, 197, 94),
        "done" => Color::from_rgb8(168, 85, 247),
        _ => Color::from_rgb8(150, 150, 150),
    }
}

// ─── State ───────────────────────────────────────────────────────────────────

pub struct SidebarState {
    flat_nav: Vec<NavItem>,
    selected_index: usize,
    collapsed: HashSet<String>,
    db_path: PathBuf,
    // Cached data from last refresh
    cached_projects: Vec<services::Project>,
    cached_sessions: Vec<SessionRecord>,
    cached_tasks_by_project: HashMap<String, Vec<planeai_tasks::model::Task>>,
}

impl SidebarState {
    pub fn new(conn: &Connection, db_path: &Path) -> Self {
        let mut state = Self {
            flat_nav: Vec::new(),
            selected_index: 0,
            collapsed: HashSet::new(),
            db_path: db_path.to_path_buf(),
            cached_projects: Vec::new(),
            cached_sessions: Vec::new(),
            cached_tasks_by_project: HashMap::new(),
        };
        state.refresh(conn);
        state
    }

    pub fn refresh(&mut self, conn: &Connection) {
        // Same as Tauri: ProjectService::list_active()
        self.cached_projects = ProjectService::list_active(conn).unwrap_or_default();
        // Same as Tauri: SessionService::list_active()
        self.cached_sessions = SessionService::list_active(conn).unwrap_or_default();

        // Same as Svelte task-store: load all tasks per project via SqliteRepository
        self.cached_tasks_by_project.clear();
        for project in &self.cached_projects {
            let prefix = derive_prefix(&project.name);
            if let Ok(repo) = SqliteRepository::open(self.db_path.to_str().unwrap_or(""), &prefix) {
                if let Ok(tasks) = repo.list(ListFilter::default()) {
                    self.cached_tasks_by_project.insert(project.path.clone(), tasks);
                }
            }
        }

        self.rebuild_flat_nav();
    }

    pub fn flat_nav(&self) -> &[NavItem] {
        &self.flat_nav
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn toggle_section(&mut self, key: &str) {
        if self.collapsed.contains(key) {
            self.collapsed.remove(key);
        } else {
            self.collapsed.insert(key.to_string());
        }
    }

    /// Handle a key press. Returns a SidebarAction if the key triggered one.
    pub fn handle_key(&mut self, key: &str) -> SidebarAction {
        match key {
            "j" | "ArrowDown" => {
                if !self.flat_nav.is_empty() {
                    self.selected_index = (self.selected_index + 1).min(self.flat_nav.len() - 1);
                }
                SidebarAction::None
            }
            "k" | "ArrowUp" => {
                self.selected_index = self.selected_index.saturating_sub(1);
                SidebarAction::None
            }
            "h" | "ArrowLeft" => {
                self.collapse_current();
                SidebarAction::None
            }
            "l" | "ArrowRight" => {
                self.expand_current();
                SidebarAction::None
            }
            "Enter" => self.select_current(),
            "Escape" => SidebarAction::FocusTerminal,
            _ => SidebarAction::None,
        }
    }

    fn select_current(&mut self) -> SidebarAction {
        match self.flat_nav.get(self.selected_index).cloned() {
            Some(NavItem::ProjectHeader { project_id, .. }) => {
                self.toggle_section(&format!("project:{}", project_id));
                self.rebuild_flat_nav();
                SidebarAction::None
            }
            Some(NavItem::StatusHeader { project_path, status, .. }) => {
                self.toggle_section(&format!("{}:{}", project_path, status));
                self.rebuild_flat_nav();
                SidebarAction::None
            }
            Some(NavItem::OrphanSession { session_id, .. }) => {
                SidebarAction::SwitchSession(session_id)
            }
            Some(NavItem::Task { linked_session_id: Some(sid), .. }) => {
                SidebarAction::SwitchSession(sid)
            }
            Some(NavItem::Task { linked_session_id: None, .. }) => SidebarAction::None,
            None => SidebarAction::None,
        }
    }

    fn collapse_current(&mut self) {
        if let Some(key) = self.section_key_for_current() {
            self.collapsed.insert(key);
            self.rebuild_flat_nav();
        }
    }

    fn expand_current(&mut self) {
        if let Some(key) = self.section_key_for_current() {
            self.collapsed.remove(&key);
            self.rebuild_flat_nav();
        }
    }

    fn section_key_for_current(&self) -> Option<String> {
        match self.flat_nav.get(self.selected_index) {
            Some(NavItem::ProjectHeader { project_id, .. }) => Some(format!("project:{}", project_id)),
            Some(NavItem::StatusHeader { project_path, status, .. }) => Some(format!("{}:{}", project_path, status)),
            _ => None,
        }
    }

    /// Render the sidebar as an iced Element.
    pub fn view<'a, M: 'a>(&self, focused: bool) -> Element<'a, M> {
        let mut items = column![].spacing(1);

        for (i, item) in self.flat_nav.iter().enumerate() {
            let is_selected = focused && i == self.selected_index;
            let label = match item {
                NavItem::ProjectHeader { name, .. } => {
                    let arrow = if self.collapsed.contains(&format!("project:{}", match item { NavItem::ProjectHeader { project_id, .. } => project_id, _ => unreachable!() })) { "▶" } else { "▼" };
                    format!("{} {}", arrow, name.to_uppercase())
                }
                NavItem::OrphanSession { name, status, .. } => {
                    let icon = if status == "active" { "●" } else { "○" };
                    format!("  {} {}", icon, name)
                }
                NavItem::StatusHeader { status, count, .. } => {
                    let arrow = if self.collapsed.contains(&match item { NavItem::StatusHeader { project_path, status, .. } => format!("{}:{}", project_path, status), _ => unreachable!() }) { "▶" } else { "▼" };
                    format!("  {} {} ({})", arrow, status_label(status), count)
                }
                NavItem::Task { title, linked_session_id, .. } => {
                    let icon = if linked_session_id.is_some() { "●" } else { "○" };
                    format!("    {} {}", icon, title)
                }
            };

            let color = match item {
                NavItem::ProjectHeader { .. } => Color::from_rgb8(150, 150, 150),
                NavItem::OrphanSession { status, .. } if status == "active" => Color::from_rgb8(180, 180, 180),
                NavItem::OrphanSession { .. } => Color::from_rgb8(120, 120, 120),
                NavItem::StatusHeader { status, .. } => status_color(status),
                NavItem::Task { linked_session_id: Some(_), .. } => Color::from_rgb8(100, 200, 255),
                NavItem::Task { .. } => Color::from_rgb8(180, 180, 180),
            };

            let bg = if is_selected {
                Some(Color::from_rgba8(59, 130, 246, 0.15))
            } else {
                None
            };

            let txt = text(label).size(12).color(color).font(Font::MONOSPACE);
            let item_container = container(txt)
                .width(Length::Fill)
                .padding([2, 4])
                .style(move |_: &Theme| container::Style {
                    background: bg.map(|c| c.into()),
                    ..Default::default()
                });
            items = items.push(item_container);
        }

        let sidebar_content = scrollable(items).width(Length::Fill).height(Length::Fill);

        let border_color = if focused {
            Color::from_rgba8(59, 130, 246, 0.3)
        } else {
            Color::from_rgb8(40, 40, 40)
        };

        container(sidebar_content)
            .width(Length::Fixed(200.0))
            .height(Length::Fill)
            .padding(4)
            .style(move |_: &Theme| container::Style {
                background: Some(Color::from_rgb8(20, 20, 20).into()),
                border: iced::Border {
                    color: border_color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    fn rebuild_flat_nav(&mut self) {
        self.flat_nav.clear();

        let all_task_keys: HashSet<&str> = self.cached_tasks_by_project
            .values()
            .flat_map(|tasks| tasks.iter().map(|t| t.key.as_str()))
            .collect();

        for project in self.cached_projects.clone() {
            let project_key = format!("project:{}", project.id);
            self.flat_nav.push(NavItem::ProjectHeader {
                project_id: project.id.clone(),
                name: project.name.clone(),
            });

            if self.collapsed.contains(&project_key) {
                continue;
            }

            // Orphan sessions
            let orphans: Vec<_> = self.cached_sessions.iter()
                .filter(|s| s.project_id == project.id)
                .filter(|s| s.task_key.is_none() || !all_task_keys.contains(s.task_key.as_deref().unwrap_or("")))
                .collect();
            for s in &orphans {
                self.flat_nav.push(NavItem::OrphanSession {
                    session_id: s.id.clone(),
                    name: if s.name.is_empty() { s.branch.clone() } else { s.name.clone() },
                    status: s.status.clone(),
                });
            }

            // Tasks grouped by status
            let project_tasks = self.cached_tasks_by_project.get(&project.path).cloned().unwrap_or_default();
            let status_order = ["in_progress", "in_review", "todo", "done"];

            for status in status_order {
                let mut group: Vec<_> = project_tasks.iter()
                    .filter(|t| t.status.as_str() == status)
                    .collect();
                if group.is_empty() {
                    continue;
                }
                group.sort_by(|a, b| b.priority.cmp(&a.priority));

                let section_key = format!("{}:{}", project.path, status);
                self.flat_nav.push(NavItem::StatusHeader {
                    project_path: project.path.clone(),
                    status: status.to_string(),
                    count: group.len(),
                });

                if self.collapsed.contains(&section_key) {
                    continue;
                }

                for task in &group {
                    let linked_session_id = self.cached_sessions.iter()
                        .find(|s| s.task_key.as_deref() == Some(&task.key))
                        .map(|s| s.id.clone());

                    self.flat_nav.push(NavItem::Task {
                        key: task.key.clone(),
                        title: task.title.clone(),
                        status: status.to_string(),
                        linked_session_id,
                    });
                }
            }
        }

        // Clamp selected index
        if !self.flat_nav.is_empty() && self.selected_index >= self.flat_nav.len() {
            self.selected_index = self.flat_nav.len() - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use planeai_tasks::model::CreateParams;
    use rusqlite::Connection;
    use tempfile::NamedTempFile;

    /// Set up a file-backed DB (needed because SqliteRepository::open takes a path)
    fn setup_db() -> (Connection, NamedTempFile) {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        let conn = Connection::open(path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;").unwrap();
        services::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        (conn, tmp)
    }

    #[test]
    fn new_loads_projects_as_headers_in_flat_nav() {
        let (conn, tmp) = setup_db();
        ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();
        ProjectService::create(&conn, "other", "/tmp/other").unwrap();

        let sidebar = SidebarState::new(&conn, tmp.path());
        let nav = sidebar.flat_nav();

        let headers: Vec<_> = nav.iter().filter(|item| matches!(item, NavItem::ProjectHeader { .. })).collect();
        assert_eq!(headers.len(), 2);
        assert!(matches!(&headers[0], NavItem::ProjectHeader { name, .. } if name == "myapp"));
        assert!(matches!(&headers[1], NavItem::ProjectHeader { name, .. } if name == "other"));
    }

    #[test]
    fn flat_nav_shows_orphan_sessions_and_tasks_grouped_by_status() {
        let (conn, tmp) = setup_db();
        let project = ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();

        // Create an orphan session (no task_key) — same as Tauri db::create_session_with_id
        SessionService::create(&conn, &services::CreateSessionParams {
            id: "sess-1".to_string(),
            project_id: project.id.clone(),
            name: "my session".to_string(),
            backend: "daemon".to_string(),
            branch: "main".to_string(),
            ..Default::default()
        }).unwrap();

        // Create tasks via SqliteRepository — same codepath as Tauri commands/tasks.rs
        let prefix = derive_prefix(&project.name);
        let repo = SqliteRepository::open(tmp.path().to_str().unwrap(), &prefix).unwrap();
        repo.create(CreateParams {
            title: "Fix bug".to_string(),
            priority: 0,
            ..Default::default()
        }).unwrap();
        repo.create(CreateParams {
            title: "Add feature".to_string(),
            priority: 1,
            ..Default::default()
        }).unwrap();
        drop(repo);

        // Move second task to in_progress (same as Tauri move_task_item)
        let repo = SqliteRepository::open(tmp.path().to_str().unwrap(), &prefix).unwrap();
        let tasks = repo.list(ListFilter::default()).unwrap();
        let feat_task = tasks.iter().find(|t| t.title == "Add feature").unwrap();
        repo.update(&feat_task.key, planeai_tasks::model::UpdateParams {
            status: Some(planeai_tasks::model::Status::InProgress),
            ..Default::default()
        }).unwrap();
        drop(repo);

        let sidebar = SidebarState::new(&conn, tmp.path());
        let nav = sidebar.flat_nav();

        // Expected: ProjectHeader, OrphanSession, StatusHeader(in_progress), Task, StatusHeader(todo), Task
        assert!(matches!(&nav[0], NavItem::ProjectHeader { name, .. } if name == "myapp"));
        assert!(matches!(&nav[1], NavItem::OrphanSession { name, .. } if name == "my session"));
        assert!(matches!(&nav[2], NavItem::StatusHeader { status, count, .. } if status == "in_progress" && *count == 1));
        assert!(matches!(&nav[3], NavItem::Task { title, .. } if title == "Add feature"));
        assert!(matches!(&nav[4], NavItem::StatusHeader { status, count, .. } if status == "todo" && *count == 1));
        assert!(matches!(&nav[5], NavItem::Task { title, .. } if title == "Fix bug"));
    }

    #[test]
    fn collapsing_project_removes_children_from_flat_nav() {
        let (conn, tmp) = setup_db();
        let project = ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();

        SessionService::create(&conn, &services::CreateSessionParams {
            id: "sess-1".to_string(),
            project_id: project.id.clone(),
            name: "orphan".to_string(),
            backend: "daemon".to_string(),
            branch: "main".to_string(),
            ..Default::default()
        }).unwrap();

        let prefix = derive_prefix(&project.name);
        let repo = SqliteRepository::open(tmp.path().to_str().unwrap(), &prefix).unwrap();
        repo.create(CreateParams { title: "Task A".to_string(), ..Default::default() }).unwrap();
        drop(repo);

        let mut sidebar = SidebarState::new(&conn, tmp.path());
        assert!(sidebar.flat_nav().len() > 1); // header + children

        // Collapse the project
        sidebar.toggle_section(&format!("project:{}", project.id));
        sidebar.refresh(&conn);

        // Only the project header should remain
        assert_eq!(sidebar.flat_nav().len(), 1);
        assert!(matches!(&sidebar.flat_nav()[0], NavItem::ProjectHeader { name, .. } if name == "myapp"));
    }

    #[test]
    fn j_k_moves_selected_index_within_bounds() {
        let (conn, tmp) = setup_db();
        let project = ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();
        SessionService::create(&conn, &services::CreateSessionParams {
            id: "s1".to_string(),
            project_id: project.id.clone(),
            name: "sess1".to_string(),
            backend: "daemon".to_string(),
            branch: "main".to_string(),
            ..Default::default()
        }).unwrap();
        SessionService::create(&conn, &services::CreateSessionParams {
            id: "s2".to_string(),
            project_id: project.id.clone(),
            name: "sess2".to_string(),
            backend: "daemon".to_string(),
            branch: "feat".to_string(),
            ..Default::default()
        }).unwrap();

        let mut sidebar = SidebarState::new(&conn, tmp.path());
        // flat_nav: [ProjectHeader, OrphanSession(s1), OrphanSession(s2)]
        assert_eq!(sidebar.selected_index(), 0);

        // j moves down
        sidebar.handle_key("j");
        assert_eq!(sidebar.selected_index(), 1);
        sidebar.handle_key("j");
        assert_eq!(sidebar.selected_index(), 2);

        // j at bottom stays at bottom
        sidebar.handle_key("j");
        assert_eq!(sidebar.selected_index(), 2);

        // k moves up
        sidebar.handle_key("k");
        assert_eq!(sidebar.selected_index(), 1);
        sidebar.handle_key("k");
        assert_eq!(sidebar.selected_index(), 0);

        // k at top stays at top
        sidebar.handle_key("k");
        assert_eq!(sidebar.selected_index(), 0);
    }

    #[test]
    fn h_collapses_current_project_l_expands() {
        let (conn, tmp) = setup_db();
        let project = ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();
        SessionService::create(&conn, &services::CreateSessionParams {
            id: "s1".to_string(),
            project_id: project.id.clone(),
            name: "orphan".to_string(),
            backend: "daemon".to_string(),
            branch: "main".to_string(),
            ..Default::default()
        }).unwrap();

        let mut sidebar = SidebarState::new(&conn, tmp.path());
        let initial_len = sidebar.flat_nav().len();
        assert!(initial_len > 1);

        // Selected is on project header (index 0), h collapses
        sidebar.handle_key("h");
        assert_eq!(sidebar.flat_nav().len(), 1); // only header remains

        // l expands it back
        sidebar.handle_key("l");
        assert_eq!(sidebar.flat_nav().len(), initial_len);
    }

    #[test]
    fn enter_on_orphan_session_returns_switch_session() {
        let (conn, tmp) = setup_db();
        let project = ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();
        SessionService::create(&conn, &services::CreateSessionParams {
            id: "sess-abc".to_string(),
            project_id: project.id.clone(),
            name: "my orphan".to_string(),
            backend: "daemon".to_string(),
            branch: "main".to_string(),
            ..Default::default()
        }).unwrap();

        let mut sidebar = SidebarState::new(&conn, tmp.path());
        // nav[0] = ProjectHeader, nav[1] = OrphanSession
        sidebar.handle_key("j"); // move to orphan
        let action = sidebar.handle_key("Enter");
        assert_eq!(action, SidebarAction::SwitchSession("sess-abc".to_string()));
    }

    #[test]
    fn enter_on_task_with_linked_session_returns_switch_session() {
        let (conn, tmp) = setup_db();
        let project = ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();

        // Create a task
        let prefix = derive_prefix(&project.name);
        let repo = SqliteRepository::open(tmp.path().to_str().unwrap(), &prefix).unwrap();
        let task = repo.create(CreateParams {
            title: "Fix bug".to_string(),
            ..Default::default()
        }).unwrap();
        drop(repo);

        // Create a session linked to that task
        SessionService::create(&conn, &services::CreateSessionParams {
            id: "sess-linked".to_string(),
            project_id: project.id.clone(),
            name: "task session".to_string(),
            backend: "daemon".to_string(),
            branch: "feat".to_string(),
            task_key: Some(task.key.clone()),
            ..Default::default()
        }).unwrap();

        let mut sidebar = SidebarState::new(&conn, tmp.path());
        // nav: [ProjectHeader, StatusHeader(todo), Task(Fix bug)]
        // The session is linked so it won't appear as orphan
        sidebar.handle_key("j"); // StatusHeader
        sidebar.handle_key("j"); // Task
        let action = sidebar.handle_key("Enter");
        assert_eq!(action, SidebarAction::SwitchSession("sess-linked".to_string()));
    }

    #[test]
    fn escape_returns_focus_terminal() {
        let (conn, tmp) = setup_db();
        ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();

        let mut sidebar = SidebarState::new(&conn, tmp.path());
        let action = sidebar.handle_key("Escape");
        assert_eq!(action, SidebarAction::FocusTerminal);
    }
}
