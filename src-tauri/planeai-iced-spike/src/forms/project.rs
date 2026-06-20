//! Project creation form component (Cmd+Shift+N).
//!
//! Mirrors the tauri app's ProjectForm: validates git repo, checks name uniqueness,
//! calls ProjectService::create.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use iced::keyboard;
use iced::widget::{button, column, row, text, text_input};
use iced::Length;
use planeai_core::services::{Project, ProjectService};
use rusqlite::Connection;

use crate::theme::PlaneAiTheme;

const PATH_INPUT_ID: &str = "project_form_path";

/// Messages emitted by the project form (widget callbacks).
#[derive(Debug, Clone)]
pub enum FormMessage {
    PathChanged(String),
    NameChanged(String),
    Submit,
    Cancel,
}

/// Effects returned by update/handle_key for the caller to act on.
pub enum FormEffect {
    None,
    Close,
    FocusNext,
    FocusPrev,
    Created(Project, PathBuf),
    Error(String),
}

/// State for the project creation form.
#[derive(Default)]
pub struct ProjectFormState {
    pub visible: bool,
    pub path: String,
    pub name: String,
    pub name_edited: bool,
    pub error: Option<String>,
}

impl ProjectFormState {
    /// Open the form, pre-filling path from config's projects_base_path.
    pub fn open(&mut self) -> iced::Task<FormMessage> {
        let base_path = load_projects_base_path().unwrap_or_default();
        self.visible = true;
        self.path = if base_path.is_empty() {
            String::new()
        } else if base_path.ends_with('/') {
            base_path
        } else {
            format!("{}/", base_path)
        };
        self.name.clear();
        self.name_edited = false;
        self.error = None;
        iced::widget::operation::focus(iced::widget::Id::new(PATH_INPUT_ID))
    }

    /// Close the form.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Handle keyboard accelerators (Escape, Tab, Cmd+Enter).
    pub fn handle_key(
        &mut self,
        key: &keyboard::Key,
        modifiers: &keyboard::Modifiers,
        db: &Option<Arc<Mutex<Connection>>>,
    ) -> FormEffect {
        match key {
            keyboard::Key::Named(keyboard::key::Named::Escape) => {
                self.close();
                FormEffect::Close
            }
            keyboard::Key::Named(keyboard::key::Named::Tab) => {
                if modifiers.shift() {
                    FormEffect::FocusPrev
                } else {
                    FormEffect::FocusNext
                }
            }
            keyboard::Key::Named(keyboard::key::Named::Enter) => {
                let cmd = if cfg!(target_os = "macos") {
                    modifiers.command()
                } else {
                    modifiers.control()
                };
                if cmd {
                    self.submit(db)
                } else {
                    FormEffect::None
                }
            }
            _ => FormEffect::None,
        }
    }

    /// Handle a widget-emitted form message.
    pub fn update(
        &mut self,
        message: FormMessage,
        db: &Option<Arc<Mutex<Connection>>>,
    ) -> FormEffect {
        match message {
            FormMessage::PathChanged(val) => {
                if !self.name_edited {
                    self.name = PathBuf::from(&val)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                }
                self.path = val;
                self.error = None;
                FormEffect::None
            }
            FormMessage::NameChanged(val) => {
                self.name = val;
                self.name_edited = true;
                FormEffect::None
            }
            FormMessage::Submit => self.submit(db),
            FormMessage::Cancel => {
                self.close();
                FormEffect::Close
            }
        }
    }

    fn submit(&mut self, db: &Option<Arc<Mutex<Connection>>>) -> FormEffect {
        let path_str = self.path.trim().to_string();
        if path_str.is_empty() {
            self.error = Some("Path is required.".into());
            return FormEffect::Error(self.error.clone().unwrap());
        }
        let expanded = planeai_core::session_launch::expand_tilde(&path_str);
        let path = PathBuf::from(&expanded);
        if !path.is_dir() {
            let msg = format!("Not a directory: {}", expanded);
            self.error = Some(msg.clone());
            return FormEffect::Error(msg);
        }
        if !path.join(".git").exists() {
            let msg = "Not a valid git repository (no .git found).".to_string();
            self.error = Some(msg.clone());
            return FormEffect::Error(msg);
        }
        let name = if self.name.trim().is_empty() {
            path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "project".into())
        } else {
            self.name.trim().to_string()
        };
        let Some(db) = db else {
            let msg = "Database unavailable.".to_string();
            self.error = Some(msg.clone());
            return FormEffect::Error(msg);
        };
        match db.lock() {
            Ok(conn) => {
                match ProjectService::name_exists(&conn, &name) {
                    Ok(true) => {
                        let msg = format!("A project named '{}' already exists.", name);
                        self.error = Some(msg.clone());
                        return FormEffect::Error(msg);
                    }
                    Err(e) => {
                        let msg = format!("DB error: {e}");
                        self.error = Some(msg.clone());
                        return FormEffect::Error(msg);
                    }
                    _ => {}
                }
                match ProjectService::create(&conn, &name, &expanded) {
                    Ok(proj) => {
                        self.visible = false;
                        FormEffect::Created(proj, path)
                    }
                    Err(e) => {
                        let msg = format!("Failed to create project: {e}");
                        self.error = Some(msg.clone());
                        FormEffect::Error(msg)
                    }
                }
            }
            Err(e) => {
                let msg = format!("DB lock failed: {e}");
                self.error = Some(msg.clone());
                FormEffect::Error(msg)
            }
        }
    }

    /// Render the form content (to be wrapped in modal_overlay by the caller).
    pub fn view<'a, M>(
        &'a self,
        theme: &PlaneAiTheme,
        on_path: impl Fn(String) -> M + 'a,
        on_name: impl Fn(String) -> M + 'a,
        on_submit: M,
        on_cancel: M,
    ) -> iced::widget::Column<'a, M>
    where
        M: Clone + 'a,
    {
        let mut col = column![].spacing(3).width(Length::Fill).padding(8);
        col = col.push(text("Add Project").color(theme.text_primary()));
        col = col.push(text("  Repository path:").color(theme.text_muted()));
        col = col.push(
            text_input("/path/to/repo", &self.path)
                .id(iced::widget::Id::new(PATH_INPUT_ID))
                .on_input(on_path)
                .on_submit(on_submit.clone())
                .width(Length::Fill),
        );
        col = col.push(text("  Name:").color(theme.text_muted()));
        col = col.push(
            text_input("my-project", &self.name)
                .on_input(on_name)
                .on_submit(on_submit.clone())
                .width(Length::Fill),
        );
        if let Some(ref err) = self.error {
            col = col.push(text(format!("  ⚠ {}", err)).color(theme.error()));
        }
        let button_row = row![
            button(text("Cancel")).on_press(on_cancel).padding([4, 12]),
            iced::widget::Space::new().width(Length::Fill),
            button(text("Add Project ⌘↵"))
                .on_press(on_submit)
                .padding([4, 12]),
        ]
        .width(Length::Fill);
        col = col.push(iced::widget::Space::new().height(16.0));
        col = col.push(button_row);
        col
    }
}

/// Load projects_base_path from the shared config.json.
fn load_projects_base_path() -> Option<String> {
    let config_path = planeai_core::session_launch::config_dir().join("config.json");
    let content = std::fs::read_to_string(config_path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&content).ok()?;
    val.get("projects_base_path")?
        .as_str()
        .map(planeai_core::session_launch::expand_tilde)
}

#[cfg(test)]
mod tests {
    use super::*;
    use planeai_core::services;

    fn test_db() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().unwrap();
        services::migrate(&conn).unwrap();
        Arc::new(Mutex::new(conn))
    }

    #[test]
    fn update_path_changed_auto_fills_name() {
        let mut form = ProjectFormState::default();
        form.update(FormMessage::PathChanged("/foo/my-repo".into()), &None);
        assert_eq!(form.path, "/foo/my-repo");
        assert_eq!(form.name, "my-repo");
    }

    #[test]
    fn update_path_changed_does_not_override_edited_name() {
        let mut form = ProjectFormState::default();
        form.update(FormMessage::NameChanged("custom".into()), &None);
        form.update(FormMessage::PathChanged("/foo/bar".into()), &None);
        assert_eq!(form.name, "custom");
    }

    #[test]
    fn update_name_changed_sets_name_edited() {
        let mut form = ProjectFormState::default();
        assert!(!form.name_edited);
        form.update(FormMessage::NameChanged("x".into()), &None);
        assert!(form.name_edited);
        assert_eq!(form.name, "x");
    }

    #[test]
    fn update_cancel_closes_form() {
        let mut form = ProjectFormState { visible: true, ..Default::default() };
        let effect = form.update(FormMessage::Cancel, &None);
        assert!(!form.visible);
        assert!(matches!(effect, FormEffect::Close));
    }

    #[test]
    fn submit_empty_path_returns_error() {
        let mut form = ProjectFormState::default();
        form.path = "  ".into();
        let effect = form.update(FormMessage::Submit, &None);
        assert!(matches!(effect, FormEffect::Error(_)));
        assert!(form.error.is_some());
    }

    #[test]
    fn submit_nonexistent_path_returns_error() {
        let mut form = ProjectFormState::default();
        form.path = "/nonexistent_path_xyz_123".into();
        let effect = form.update(FormMessage::Submit, &None);
        assert!(matches!(effect, FormEffect::Error(_)));
    }

    #[test]
    fn submit_no_db_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        // Create a .git dir so git validation passes
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        let mut form = ProjectFormState::default();
        form.path = tmp.path().to_string_lossy().to_string();
        let effect = form.update(FormMessage::Submit, &None);
        assert!(matches!(effect, FormEffect::Error(_)));
        assert_eq!(form.error.as_deref(), Some("Database unavailable."));
    }

    #[test]
    fn submit_valid_creates_project() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        let db = test_db();
        let mut form = ProjectFormState::default();
        form.path = tmp.path().to_string_lossy().to_string();
        form.name = "test-proj".into();
        let effect = form.update(FormMessage::Submit, &Some(db));
        assert!(matches!(effect, FormEffect::Created(_, _)));
        assert!(!form.visible);
    }

    #[test]
    fn submit_duplicate_name_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        let db = test_db();
        {
            let conn = db.lock().unwrap();
            ProjectService::create(&conn, "dupe", &tmp.path().to_string_lossy()).unwrap();
        }
        let mut form = ProjectFormState::default();
        form.path = tmp.path().to_string_lossy().to_string();
        form.name = "dupe".into();
        let effect = form.update(FormMessage::Submit, &Some(db));
        assert!(matches!(effect, FormEffect::Error(_)));
        assert!(form.error.as_deref().unwrap().contains("already exists"));
    }

    #[test]
    fn handle_key_escape_closes() {
        let mut form = ProjectFormState { visible: true, ..Default::default() };
        let effect = form.handle_key(
            &keyboard::Key::Named(keyboard::key::Named::Escape),
            &keyboard::Modifiers::default(),
            &None,
        );
        assert!(!form.visible);
        assert!(matches!(effect, FormEffect::Close));
    }

    #[test]
    fn handle_key_tab_returns_focus_next() {
        let mut form = ProjectFormState::default();
        let effect = form.handle_key(
            &keyboard::Key::Named(keyboard::key::Named::Tab),
            &keyboard::Modifiers::default(),
            &None,
        );
        assert!(matches!(effect, FormEffect::FocusNext));
    }

    #[test]
    fn handle_key_shift_tab_returns_focus_prev() {
        let mut form = ProjectFormState::default();
        let effect = form.handle_key(
            &keyboard::Key::Named(keyboard::key::Named::Tab),
            &keyboard::Modifiers::SHIFT,
            &None,
        );
        assert!(matches!(effect, FormEffect::FocusPrev));
    }

    #[test]
    fn handle_key_cmd_enter_submits() {
        let mut form = ProjectFormState::default();
        form.path = "  ".into(); // Will fail validation but still triggers submit
        let modifiers = keyboard::Modifiers::COMMAND;
        let effect = form.handle_key(
            &keyboard::Key::Named(keyboard::key::Named::Enter),
            &modifiers,
            &None,
        );
        assert!(matches!(effect, FormEffect::Error(_)));
    }

    #[test]
    fn handle_key_enter_without_mod_does_nothing() {
        let mut form = ProjectFormState::default();
        let effect = form.handle_key(
            &keyboard::Key::Named(keyboard::key::Named::Enter),
            &keyboard::Modifiers::default(),
            &None,
        );
        assert!(matches!(effect, FormEffect::None));
    }

    #[test]
    fn handle_key_other_keys_return_none() {
        let mut form = ProjectFormState::default();
        let effect = form.handle_key(
            &keyboard::Key::Character("a".into()),
            &keyboard::Modifiers::default(),
            &None,
        );
        assert!(matches!(effect, FormEffect::None));
    }
}
