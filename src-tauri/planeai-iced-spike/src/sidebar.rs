//! Sidebar module — project/task/session navigation panel.
//!
//! Mirrors the behaviour of the Svelte UnifiedSidebar component.
//! Uses the same codepath as the Tauri app:
//! - Projects via `ProjectService::list_active()`
//! - Sessions via `SessionService::list_active()`
//! - Tasks via `SqliteRepository::open(db_path, prefix).list()`

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, mouse_area, row, scrollable, svg, text};
use iced::{Color, Element, Font, Length, Padding, Theme};
use rusqlite::Connection;

use crate::theme::PlaneAiTheme;
use planeai_core::services::{self, ProjectService, SessionRecord, SessionService};
use planeai_tasks::model::ListFilter;
use planeai_tasks::provider::TaskProvider;
use planeai_tasks::sqlite::{derive_prefix, SqliteRepository};

// ─── Nav item types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum NavItem {
    ProjectHeader {
        project_id: String,
        name: String,
    },
    OrphanSession {
        session_id: String,
        name: String,
        status: String,
        has_worktree: bool,
    },
    StatusHeader {
        project_path: String,
        status: String,
        count: usize,
    },
    Task {
        key: String,
        title: String,
        status: String,
        parent_key: Option<String>,
        is_parent: bool,
        linked_session_id: Option<String>,
    },
}

// ─── Actions returned to the parent ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SidebarAction {
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

// ─── Icons ────────────────────────────────────────────────────────────────────

fn icon_chevron_right<'a, M: 'a>(color: Color) -> Element<'a, M> {
    let handle = svg::Handle::from_memory(include_bytes!("../icons/chevron-right.svg").as_slice());
    svg(handle)
        .width(12)
        .height(12)
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

fn icon_chevron_down<'a, M: 'a>(color: Color) -> Element<'a, M> {
    let handle = svg::Handle::from_memory(include_bytes!("../icons/chevron-down.svg").as_slice());
    svg(handle)
        .width(12)
        .height(12)
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

fn icon_git_fork<'a, M: 'a>(color: Color) -> Element<'a, M> {
    let handle = svg::Handle::from_memory(include_bytes!("../icons/git-fork.svg").as_slice());
    svg(handle)
        .width(12)
        .height(12)
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

// ─── State ───────────────────────────────────────────────────────────────────

pub struct SidebarState {
    flat_nav: Vec<NavItem>,
    selected_index: usize,
    collapsed: HashSet<String>,
    db_path: PathBuf,
    pub width: f32,
    resizing: bool,
    last_cursor_x: f32,
    active_session_id: Option<String>,
    scrollable_id: iced::widget::Id,
    viewport_height: f32,
    scroll_offset_y: f32,
    content_height: f32,
    // Cached data from last refresh
    cached_projects: Vec<services::Project>,
    cached_sessions: Vec<SessionRecord>,
    cached_tasks_by_project: HashMap<String, Vec<planeai_tasks::model::Task>>,
}

const DEFAULT_SIDEBAR_WIDTH: f32 = 224.0;
const MIN_SIDEBAR_WIDTH: f32 = 160.0;

impl SidebarState {
    pub fn new(conn: &Connection, db_path: &Path) -> Self {
        let mut state = Self {
            flat_nav: Vec::new(),
            selected_index: 0,
            collapsed: HashSet::new(),
            db_path: db_path.to_path_buf(),
            width: services::LayoutService::get(conn, "sidebar_width", DEFAULT_SIDEBAR_WIDTH),
            resizing: false,
            last_cursor_x: 0.0,
            active_session_id: None,
            scrollable_id: iced::widget::Id::unique(),
            viewport_height: 0.0,
            scroll_offset_y: 0.0,
            content_height: 0.0,
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
                    self.cached_tasks_by_project
                        .insert(project.path.clone(), tasks);
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

    pub fn set_width(&mut self, width: f32) {
        self.width = width.max(MIN_SIDEBAR_WIDTH);
    }

    /// Call on mouse button press. Returns true if resize started (caller should not forward click).
    pub fn handle_mouse_down(&mut self) -> bool {
        if (self.last_cursor_x - self.width).abs() < 6.0 {
            self.resizing = true;
            true
        } else {
            false
        }
    }

    /// Call on mouse move with cursor x position.
    pub fn handle_mouse_move(&mut self, x: f32) {
        if self.resizing {
            self.set_width(x);
        }
        // Always track for edge detection on next mouse_down
        self.last_cursor_x = x;
    }

    /// Call on mouse button release.
    pub fn handle_mouse_up(&mut self, conn: &Connection) {
        if self.resizing {
            services::LayoutService::set(conn, "sidebar_width", self.width);
        }
        self.resizing = false;
    }

    pub fn is_resizing(&self) -> bool {
        self.resizing
    }

    pub fn set_active_session(&mut self, id: Option<String>) {
        self.active_session_id = id;
    }

    /// Handle a click on a flat_nav item by index. Returns an action if applicable.
    pub fn handle_click(&mut self, index: usize) -> Option<SidebarAction> {
        if index >= self.flat_nav.len() {
            return None;
        }
        self.selected_index = index;
        self.select_current()
    }

    /// Returns a Task that scrolls the selected item into view only when near edges.
    pub fn scroll_to_selected<M: 'static>(&self) -> iced::Task<M> {
        use iced::widget::scrollable::AbsoluteOffset;
        let n = self.flat_nav.len();
        if n == 0 || self.viewport_height == 0.0 {
            return iced::Task::none();
        }
        // Derive item height from actual content height
        let content_height = self.content_height.max(self.viewport_height);
        let item_height = content_height / n as f32;

        let item_top = self.selected_index as f32 * item_height;
        let item_bottom = item_top + item_height;
        let view_top = self.scroll_offset_y;
        let view_bottom = view_top + self.viewport_height;

        if item_bottom > view_bottom {
            iced::widget::operation::scroll_to(
                self.scrollable_id.clone(),
                AbsoluteOffset {
                    x: 0.0,
                    y: item_bottom - self.viewport_height,
                },
            )
        } else if item_top < view_top {
            iced::widget::operation::scroll_to(
                self.scrollable_id.clone(),
                AbsoluteOffset {
                    x: 0.0,
                    y: item_top,
                },
            )
        } else {
            iced::Task::none()
        }
    }

    /// Call when the scrollable viewport changes.
    pub fn on_scrolled(&mut self, viewport: iced::widget::scrollable::Viewport) {
        self.scroll_offset_y = viewport.absolute_offset().y;
        self.viewport_height = viewport.bounds().height;
        self.content_height = viewport.content_bounds().height;
    }

    /// Handle a key press. Returns a SidebarAction if the key triggered one.
    pub fn handle_key(&mut self, key: &str) -> Option<SidebarAction> {
        match key {
            "j" | "ArrowDown" => {
                if !self.flat_nav.is_empty() {
                    self.selected_index = (self.selected_index + 1).min(self.flat_nav.len() - 1);
                }
                None
            }
            "k" | "ArrowUp" => {
                self.selected_index = self.selected_index.saturating_sub(1);
                None
            }
            "h" | "ArrowLeft" => {
                self.collapse_current();
                None
            }
            "l" | "ArrowRight" => {
                self.expand_current();
                None
            }
            "Enter" => self.select_current(),
            "Escape" => Some(SidebarAction::FocusTerminal),
            _ => None,
        }
    }

    fn select_current(&mut self) -> Option<SidebarAction> {
        match self.flat_nav.get(self.selected_index).cloned() {
            Some(NavItem::ProjectHeader { project_id, .. }) => {
                self.toggle_section(&format!("project:{}", project_id));
                self.rebuild_flat_nav();
                None
            }
            Some(NavItem::StatusHeader {
                project_path,
                status,
                ..
            }) => {
                self.toggle_section(&format!("{}:{}", project_path, status));
                self.rebuild_flat_nav();
                None
            }
            Some(NavItem::OrphanSession { session_id, .. }) => {
                Some(SidebarAction::SwitchSession(session_id))
            }
            Some(NavItem::Task {
                linked_session_id: Some(sid),
                ..
            }) => Some(SidebarAction::SwitchSession(sid)),
            _ => None,
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
            Some(NavItem::ProjectHeader { project_id, .. }) => {
                Some(format!("project:{}", project_id))
            }
            Some(NavItem::StatusHeader {
                project_path,
                status,
                ..
            }) => Some(format!("{}:{}", project_path, status)),
            _ => None,
        }
    }

    fn section_key_for(item: &NavItem) -> Option<String> {
        match item {
            NavItem::ProjectHeader { project_id, .. } => Some(format!("project:{}", project_id)),
            NavItem::StatusHeader {
                project_path,
                status,
                ..
            } => Some(format!("{}:{}", project_path, status)),
            _ => None,
        }
    }

    fn is_collapsed(&self, item: &NavItem) -> bool {
        Self::section_key_for(item)
            .map(|k| self.collapsed.contains(&k))
            .unwrap_or(false)
    }

    /// Render the sidebar as an iced Element.
    /// `on_click` is called with the flat_nav index when an item is clicked.
    pub fn view<'a, M: Clone + 'a>(
        &self,
        focused: bool,
        theme: &PlaneAiTheme,
        on_click: impl Fn(usize) -> M + 'a,
        on_scroll: impl Fn(iced::widget::scrollable::Viewport) -> M + 'a,
    ) -> Element<'a, M> {
        let mut items = column![].spacing(4);

        for (i, item) in self.flat_nav.iter().enumerate() {
            let is_selected = focused && i == self.selected_index;
            let is_active = match item {
                NavItem::OrphanSession { session_id, .. } => {
                    self.active_session_id.as_deref() == Some(session_id)
                }
                NavItem::Task {
                    linked_session_id: Some(sid),
                    ..
                } => self.active_session_id.as_deref() == Some(sid),
                _ => false,
            };
            let color = match item {
                NavItem::ProjectHeader { .. } => theme.text_muted(),
                _ if is_active => theme.accent(),
                NavItem::OrphanSession { status, .. } if status == "active" => theme.text_primary(),
                NavItem::OrphanSession { .. } => theme.text_dimmed(),
                NavItem::StatusHeader { status, .. } => status_color(status),
                NavItem::Task {
                    linked_session_id: Some(_),
                    ..
                } => theme.accent(),
                NavItem::Task { .. } => theme.text_primary(),
            };

            let bg = if is_active {
                Some(Color {
                    a: 0.1,
                    ..theme.accent()
                })
            } else if is_selected {
                Some(Color {
                    a: 0.15,
                    ..theme.accent()
                })
            } else {
                None
            };

            let bold_font = Font {
                weight: iced::font::Weight::Bold,
                ..Font::default()
            };

            let chevron: Element<'_, M> = if self.is_collapsed(item) {
                icon_chevron_right(color)
            } else {
                icon_chevron_down(color)
            };

            let item_content: Element<'_, M> = match item {
                NavItem::ProjectHeader { name, .. } => {
                    let name_txt = text(name.to_uppercase())
                        .size(11)
                        .color(color)
                        .wrapping(Wrapping::None);
                    row![chevron, name_txt]
                        .spacing(4)
                        .align_y(iced::Alignment::Center)
                        .into()
                }
                NavItem::OrphanSession {
                    name, has_worktree, ..
                } => {
                    let mut r = row![].spacing(4).align_y(iced::Alignment::Center);
                    if *has_worktree {
                        r = r.push(icon_git_fork(theme.text_dimmed()));
                    }
                    let name_txt = text(name.clone())
                        .size(13)
                        .color(color)
                        .wrapping(Wrapping::None);
                    let name_txt = if is_active {
                        name_txt.font(bold_font)
                    } else {
                        name_txt
                    };
                    r = r.push(name_txt);
                    container(r)
                        .padding(Padding {
                            top: 0.0,
                            right: 0.0,
                            bottom: 0.0,
                            left: 8.0,
                        })
                        .into()
                }
                NavItem::StatusHeader { status, count, .. } => {
                    let label = format!("{} ({})", status_label(status), count);
                    let txt = text(label).size(12).color(color).wrapping(Wrapping::None);
                    container(
                        row![chevron, txt]
                            .spacing(4)
                            .align_y(iced::Alignment::Center),
                    )
                    .padding(Padding {
                        top: 0.0,
                        right: 0.0,
                        bottom: 0.0,
                        left: 8.0,
                    })
                    .into()
                }
                NavItem::Task {
                    key,
                    title,
                    parent_key,
                    is_parent,
                    ..
                } => {
                    let key_color = if is_active {
                        color
                    } else if *is_parent {
                        theme.text_dimmed()
                    } else {
                        theme.accent()
                    };
                    let mut r = row![].spacing(4).align_y(iced::Alignment::Center);
                    if let Some(pk) = parent_key {
                        r = r.push(
                            text(format!("{} ›", pk))
                                .size(10)
                                .color(theme.text_dimmed())
                                .wrapping(Wrapping::None),
                        );
                    }
                    let key_txt = text(key.clone())
                        .size(10)
                        .color(key_color)
                        .wrapping(Wrapping::None);
                    let key_txt = if is_active {
                        key_txt.font(bold_font)
                    } else {
                        key_txt
                    };
                    r = r.push(key_txt);
                    let title_txt = text(title.clone())
                        .size(13)
                        .color(color)
                        .wrapping(Wrapping::None);
                    let title_txt = if is_active {
                        title_txt.font(bold_font)
                    } else {
                        title_txt
                    };
                    r = r.push(title_txt);
                    container(r)
                        .padding(Padding {
                            top: 0.0,
                            right: 0.0,
                            bottom: 0.0,
                            left: 16.0,
                        })
                        .into()
                }
            };

            let item_container = container(item_content)
                .width(Length::Fill)
                .padding([4, 8])
                .max_width(self.width - 16.0)
                .clip(true)
                .style(move |_: &Theme| container::Style {
                    background: bg.map(|c| c.into()),
                    ..Default::default()
                });

            let clickable = mouse_area(item_container).on_press(on_click(i));
            items = items.push(clickable);
        }

        let sidebar_content = scrollable(items)
            .id(self.scrollable_id.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .on_scroll(on_scroll)
            .direction(Direction::Vertical(
                Scrollbar::new().width(0).scroller_width(0),
            ));

        let border_color = if focused {
            Color {
                a: 0.3,
                ..theme.accent()
            }
        } else {
            theme.panel_bg()
        };

        let panel_bg = theme.panel_bg();
        let handle_bg = panel_bg;

        let sidebar_panel = container(sidebar_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(4)
            .style(move |_: &Theme| container::Style {
                background: Some(panel_bg.into()),
                border: iced::Border {
                    color: border_color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            });

        // Resize handle on right edge with col-resize cursor
        let resize_handle = mouse_area(
            container(text(""))
                .width(Length::Fixed(4.0))
                .height(Length::Fill)
                .style(move |_: &Theme| container::Style {
                    background: Some(handle_bg.into()),
                    ..Default::default()
                }),
        )
        .interaction(iced::mouse::Interaction::ResizingHorizontally);

        row![sidebar_panel, resize_handle]
            .width(Length::Fixed(self.width))
            .height(Length::Fill)
            .into()
    }

    fn rebuild_flat_nav(&mut self) {
        self.flat_nav.clear();

        let all_task_keys: HashSet<&str> = self
            .cached_tasks_by_project
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
            let orphans: Vec<_> = self
                .cached_sessions
                .iter()
                .filter(|s| s.project_id == project.id)
                .filter(|s| {
                    s.task_key.is_none()
                        || !all_task_keys.contains(s.task_key.as_deref().unwrap_or(""))
                })
                .collect();
            for s in &orphans {
                self.flat_nav.push(NavItem::OrphanSession {
                    session_id: s.id.clone(),
                    name: if s.name.is_empty() {
                        s.branch.clone()
                    } else {
                        s.name.clone()
                    },
                    status: s.status.clone(),
                    has_worktree: s.worktree_path.is_some(),
                });
            }

            // Tasks grouped by status
            let project_tasks = self
                .cached_tasks_by_project
                .get(&project.path)
                .cloned()
                .unwrap_or_default();
            let parent_keys: HashSet<&str> = project_tasks
                .iter()
                .filter_map(|t| t.parent_key.as_deref())
                .collect();
            let status_order = ["in_progress", "in_review", "todo", "done"];

            for status in status_order {
                let mut group: Vec<_> = project_tasks
                    .iter()
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
                    let linked_session_id = self
                        .cached_sessions
                        .iter()
                        .find(|s| s.task_key.as_deref() == Some(&task.key))
                        .map(|s| s.id.clone());

                    self.flat_nav.push(NavItem::Task {
                        key: task.key.clone(),
                        title: task.title.clone(),
                        status: status.to_string(),
                        parent_key: task.parent_key.clone(),
                        is_parent: parent_keys.contains(task.key.as_str()),
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
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .unwrap();
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

        let headers: Vec<_> = nav
            .iter()
            .filter(|item| matches!(item, NavItem::ProjectHeader { .. }))
            .collect();
        assert_eq!(headers.len(), 2);
        assert!(matches!(&headers[0], NavItem::ProjectHeader { name, .. } if name == "myapp"));
        assert!(matches!(&headers[1], NavItem::ProjectHeader { name, .. } if name == "other"));
    }

    #[test]
    fn flat_nav_shows_orphan_sessions_and_tasks_grouped_by_status() {
        let (conn, tmp) = setup_db();
        let project = ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();

        // Create an orphan session (no task_key) — same as Tauri db::create_session_with_id
        SessionService::create(
            &conn,
            &services::CreateSessionParams {
                id: "sess-1".to_string(),
                project_id: project.id.clone(),
                name: "my session".to_string(),
                backend: "daemon".to_string(),
                branch: "main".to_string(),
                ..Default::default()
            },
        )
        .unwrap();

        // Create tasks via SqliteRepository — same codepath as Tauri commands/tasks.rs
        let prefix = derive_prefix(&project.name);
        let repo = SqliteRepository::open(tmp.path().to_str().unwrap(), &prefix).unwrap();
        repo.create(CreateParams {
            title: "Fix bug".to_string(),
            priority: 0,
            ..Default::default()
        })
        .unwrap();
        repo.create(CreateParams {
            title: "Add feature".to_string(),
            priority: 1,
            ..Default::default()
        })
        .unwrap();
        drop(repo);

        // Move second task to in_progress (same as Tauri move_task_item)
        let repo = SqliteRepository::open(tmp.path().to_str().unwrap(), &prefix).unwrap();
        let tasks = repo.list(ListFilter::default()).unwrap();
        let feat_task = tasks.iter().find(|t| t.title == "Add feature").unwrap();
        repo.update(
            &feat_task.key,
            planeai_tasks::model::UpdateParams {
                status: Some(planeai_tasks::model::Status::InProgress),
                ..Default::default()
            },
        )
        .unwrap();
        drop(repo);

        let sidebar = SidebarState::new(&conn, tmp.path());
        let nav = sidebar.flat_nav();

        // Expected: ProjectHeader, OrphanSession, StatusHeader(in_progress), Task, StatusHeader(todo), Task
        assert!(matches!(&nav[0], NavItem::ProjectHeader { name, .. } if name == "myapp"));
        assert!(matches!(&nav[1], NavItem::OrphanSession { name, .. } if name == "my session"));
        assert!(
            matches!(&nav[2], NavItem::StatusHeader { status, count, .. } if status == "in_progress" && *count == 1)
        );
        assert!(matches!(&nav[3], NavItem::Task { title, .. } if title == "Add feature"));
        assert!(
            matches!(&nav[4], NavItem::StatusHeader { status, count, .. } if status == "todo" && *count == 1)
        );
        assert!(matches!(&nav[5], NavItem::Task { title, .. } if title == "Fix bug"));
    }

    #[test]
    fn collapsing_project_removes_children_from_flat_nav() {
        let (conn, tmp) = setup_db();
        let project = ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();

        SessionService::create(
            &conn,
            &services::CreateSessionParams {
                id: "sess-1".to_string(),
                project_id: project.id.clone(),
                name: "orphan".to_string(),
                backend: "daemon".to_string(),
                branch: "main".to_string(),
                ..Default::default()
            },
        )
        .unwrap();

        let prefix = derive_prefix(&project.name);
        let repo = SqliteRepository::open(tmp.path().to_str().unwrap(), &prefix).unwrap();
        repo.create(CreateParams {
            title: "Task A".to_string(),
            ..Default::default()
        })
        .unwrap();
        drop(repo);

        let mut sidebar = SidebarState::new(&conn, tmp.path());
        assert!(sidebar.flat_nav().len() > 1); // header + children

        // Collapse the project
        sidebar.toggle_section(&format!("project:{}", project.id));
        sidebar.refresh(&conn);

        // Only the project header should remain
        assert_eq!(sidebar.flat_nav().len(), 1);
        assert!(
            matches!(&sidebar.flat_nav()[0], NavItem::ProjectHeader { name, .. } if name == "myapp")
        );
    }

    #[test]
    fn j_k_moves_selected_index_within_bounds() {
        let (conn, tmp) = setup_db();
        let project = ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();
        SessionService::create(
            &conn,
            &services::CreateSessionParams {
                id: "s1".to_string(),
                project_id: project.id.clone(),
                name: "sess1".to_string(),
                backend: "daemon".to_string(),
                branch: "main".to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        SessionService::create(
            &conn,
            &services::CreateSessionParams {
                id: "s2".to_string(),
                project_id: project.id.clone(),
                name: "sess2".to_string(),
                backend: "daemon".to_string(),
                branch: "feat".to_string(),
                ..Default::default()
            },
        )
        .unwrap();

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
        SessionService::create(
            &conn,
            &services::CreateSessionParams {
                id: "s1".to_string(),
                project_id: project.id.clone(),
                name: "orphan".to_string(),
                backend: "daemon".to_string(),
                branch: "main".to_string(),
                ..Default::default()
            },
        )
        .unwrap();

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
        SessionService::create(
            &conn,
            &services::CreateSessionParams {
                id: "sess-abc".to_string(),
                project_id: project.id.clone(),
                name: "my orphan".to_string(),
                backend: "daemon".to_string(),
                branch: "main".to_string(),
                ..Default::default()
            },
        )
        .unwrap();

        let mut sidebar = SidebarState::new(&conn, tmp.path());
        // nav[0] = ProjectHeader, nav[1] = OrphanSession
        sidebar.handle_key("j"); // move to orphan
        let action = sidebar.handle_key("Enter");
        assert_eq!(
            action,
            Some(SidebarAction::SwitchSession("sess-abc".to_string()))
        );
    }

    #[test]
    fn enter_on_task_with_linked_session_returns_switch_session() {
        let (conn, tmp) = setup_db();
        let project = ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();

        // Create a task
        let prefix = derive_prefix(&project.name);
        let repo = SqliteRepository::open(tmp.path().to_str().unwrap(), &prefix).unwrap();
        let task = repo
            .create(CreateParams {
                title: "Fix bug".to_string(),
                ..Default::default()
            })
            .unwrap();
        drop(repo);

        // Create a session linked to that task
        SessionService::create(
            &conn,
            &services::CreateSessionParams {
                id: "sess-linked".to_string(),
                project_id: project.id.clone(),
                name: "task session".to_string(),
                backend: "daemon".to_string(),
                branch: "feat".to_string(),
                task_key: Some(task.key.clone()),
                ..Default::default()
            },
        )
        .unwrap();

        let mut sidebar = SidebarState::new(&conn, tmp.path());
        // nav: [ProjectHeader, StatusHeader(todo), Task(Fix bug)]
        // The session is linked so it won't appear as orphan
        sidebar.handle_key("j"); // StatusHeader
        sidebar.handle_key("j"); // Task
        let action = sidebar.handle_key("Enter");
        assert_eq!(
            action,
            Some(SidebarAction::SwitchSession("sess-linked".to_string()))
        );
    }

    #[test]
    fn escape_returns_focus_terminal() {
        let (conn, tmp) = setup_db();
        ProjectService::create(&conn, "myapp", "/tmp/myapp").unwrap();

        let mut sidebar = SidebarState::new(&conn, tmp.path());
        let action = sidebar.handle_key("Escape");
        assert_eq!(action, Some(SidebarAction::FocusTerminal));
    }
}
