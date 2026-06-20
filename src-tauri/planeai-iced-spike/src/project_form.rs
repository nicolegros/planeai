//! Project creation form component (Cmd+Shift+N).
//!
//! Mirrors the tauri app's ProjectForm: validates git repo, checks name uniqueness,
//! calls ProjectService::create.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use iced::widget::{button, column, row, text, text_input};
use iced::{Font, Length};
use planeai_core::services::{Project, ProjectService};
use rusqlite::Connection;

use crate::theme::PlaneAiTheme;

const PATH_INPUT_ID: &str = "project_form_path";

/// Messages emitted by the project form.
#[derive(Debug, Clone)]
pub enum FormMessage {
    PathChanged(String),
    NameChanged(String),
    Submit,
    Cancel,
}

/// Result of submitting the form.
pub enum SubmitResult {
    /// Project created successfully.
    Created(Project, PathBuf),
    /// Validation or DB error.
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

    /// Handle a form message. Returns Some(SubmitResult) on submit.
    pub fn update(
        &mut self,
        message: FormMessage,
        db: &Option<Arc<Mutex<Connection>>>,
    ) -> Option<SubmitResult> {
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
                None
            }
            FormMessage::NameChanged(val) => {
                self.name = val;
                self.name_edited = true;
                None
            }
            FormMessage::Submit => Some(self.submit(db)),
            FormMessage::Cancel => {
                self.close();
                None
            }
        }
    }

    fn submit(&mut self, db: &Option<Arc<Mutex<Connection>>>) -> SubmitResult {
        let path_str = self.path.trim().to_string();
        if path_str.is_empty() {
            self.error = Some("Path is required.".into());
            return SubmitResult::Error(self.error.clone().unwrap());
        }
        let expanded = planeai_core::session_launch::expand_tilde(&path_str);
        let path = PathBuf::from(&expanded);
        if !path.is_dir() {
            let msg = format!("Not a directory: {}", expanded);
            self.error = Some(msg.clone());
            return SubmitResult::Error(msg);
        }
        // Same validation as tauri's validate_git_repo
        if !path.join(".git").exists() {
            let msg = "Not a valid git repository (no .git found).".to_string();
            self.error = Some(msg.clone());
            return SubmitResult::Error(msg);
        }
        let name = if self.name.trim().is_empty() {
            path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "project".into())
        } else {
            self.name.trim().to_string()
        };
        // Same codepath as tauri create_project command
        let Some(db) = db else {
            let msg = "Database unavailable.".to_string();
            self.error = Some(msg.clone());
            return SubmitResult::Error(msg);
        };
        match db.lock() {
            Ok(conn) => {
                match ProjectService::name_exists(&conn, &name) {
                    Ok(true) => {
                        let msg = format!("A project named '{}' already exists.", name);
                        self.error = Some(msg.clone());
                        return SubmitResult::Error(msg);
                    }
                    Err(e) => {
                        let msg = format!("DB error: {e}");
                        self.error = Some(msg.clone());
                        return SubmitResult::Error(msg);
                    }
                    _ => {}
                }
                match ProjectService::create(&conn, &name, &expanded) {
                    Ok(proj) => {
                        self.visible = false;
                        SubmitResult::Created(proj, path)
                    }
                    Err(e) => {
                        let msg = format!("Failed to create project: {e}");
                        self.error = Some(msg.clone());
                        SubmitResult::Error(msg)
                    }
                }
            }
            Err(e) => {
                let msg = format!("DB lock failed: {e}");
                self.error = Some(msg.clone());
                SubmitResult::Error(msg)
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
        col = col.push(
            text("Add Project")
                .size(13)
                .color(theme.text_primary())
                .font(Font::MONOSPACE),
        );
        col = col.push(
            text("  Repository path:")
                .size(11)
                .color(theme.text_muted())
                .font(Font::MONOSPACE),
        );
        col = col.push(
            text_input("/path/to/repo", &self.path)
                .id(iced::widget::Id::new(PATH_INPUT_ID))
                .on_input(on_path)
                .on_submit(on_submit.clone())
                .size(14)
                .width(Length::Fill),
        );
        col = col.push(
            text("  Name:")
                .size(11)
                .color(theme.text_muted())
                .font(Font::MONOSPACE),
        );
        col = col.push(
            text_input("my-project", &self.name)
                .on_input(on_name)
                .on_submit(on_submit.clone())
                .size(14)
                .width(Length::Fill),
        );
        if let Some(ref err) = self.error {
            col = col.push(
                text(format!("  ⚠ {}", err))
                    .size(11)
                    .color(theme.error())
                    .font(Font::MONOSPACE),
            );
        }
        let button_row = row![
            button(text("Cancel").size(12).font(Font::MONOSPACE))
                .on_press(on_cancel)
                .padding([4, 12]),
            iced::widget::Space::new().width(Length::Fill),
            button(text("Add Project ⌘↵").size(12).font(Font::MONOSPACE))
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
