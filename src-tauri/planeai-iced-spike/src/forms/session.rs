//! Session creation form — extracted from workflow.rs.
//!
//! Mirrors the Tauri app's SessionForm.svelte: mode toggle, project/task/branch
//! pickers, worktree/auto-approve toggles, task templates, validation.

use std::collections::HashMap;
use std::path::PathBuf;

use iced::keyboard;
use iced::widget::{button, checkbox, column, container, mouse_area, row, text, text_input};
use iced::{Element, Length, Theme};

use crate::components::{ComboBoxState, ComboItem};
use crate::theme::PlaneAiTheme;

// ─── Public types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Manual,
    FromTask,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Mode,
    Project,
    Task,
    Name,
    Toggles,
    Branch,
    BaseBranch,
}

/// What the form produces on successful submit.
#[derive(Debug, Clone)]
pub struct SessionLaunchRequest {
    pub project_id: String,
    pub project_path: PathBuf,
    pub name: String,
    pub branch: String,
    pub is_new_branch: bool,
    pub use_worktree: bool,
    pub base_branch: Option<String>,
    pub auto_approve: bool,
    pub provider_id: String,
    pub task_key: Option<String>,
    pub task_prompt: Option<String>,
}

/// Effects returned to the caller.
pub enum FormEffect {
    None,
    Close,
    FocusNext,
    FocusPrev,
    /// Caller should load tasks for this project path.
    LoadTasks { project_path: PathBuf },
    /// Caller should load branches for this project path.
    LoadBranches { project_path: PathBuf },
    /// Form validated, ready to launch.
    Submit(SessionLaunchRequest),
    Error(String),
}

/// Widget-emitted messages.
#[derive(Debug, Clone)]
pub enum FormMessage {
    ModeManual,
    ModeFromTask,
    NameChanged(String),
    BranchChanged(String),
    ToggleWorktree,
    ToggleAutoApprove,
    CycleProvider,
    Submit,
    Cancel,
    // Combobox click events
    ProjectFocus,
    ProjectSelected(ComboItem),
    TaskFocus,
    TaskSelected(ComboItem),
    BranchFocus,
    BranchSelected(ComboItem),
    BaseBranchFocus,
    BaseBranchSelected(ComboItem),
}

/// Task template configuration (from config.task_management.templates).
#[derive(Debug, Clone, Default)]
pub struct Templates {
    pub name: Option<String>,
    pub branch: Option<String>,
    pub prompt: Option<String>,
}

/// Session form state.
pub struct SessionFormState {
    pub visible: bool,
    pub mode: Mode,
    pub focus: Field,
    pub name: String,
    pub branch: String,
    pub use_worktree: bool,
    pub auto_approve: bool,
    pub provider_idx: usize,
    pub provider_keys: Vec<String>,
    pub error: Option<String>,
    // Comboboxes
    pub project_combo: ComboBoxState,
    pub task_combo: ComboBoxState,
    pub branch_combo: ComboBoxState,
    pub base_branch_combo: ComboBoxState,
    // Data
    pub task_list: Vec<TaskInfo>,
    pub occupied_branches: Vec<String>,
    pub templates: Templates,
    // Project path lookup (id -> path)
    project_paths: HashMap<String, PathBuf>,
}

/// Minimal task info the form needs (decoupled from planeai_tasks::model::Task).
#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub key: String,
    pub title: String,
    pub description: String,
    pub base_branch: String,
}

impl Default for SessionFormState {
    fn default() -> Self {
        Self {
            visible: false,
            mode: Mode::Manual,
            focus: Field::Mode,
            name: String::new(),
            branch: String::new(),
            use_worktree: false,
            auto_approve: true,
            provider_idx: 0,
            provider_keys: Vec::new(),
            error: None,
            project_combo: ComboBoxState::new(Vec::new()),
            task_combo: ComboBoxState::new(Vec::new()),
            branch_combo: ComboBoxState::new_with_free_text(Vec::new(), true),
            base_branch_combo: ComboBoxState::new(Vec::new()),
            task_list: Vec::new(),
            occupied_branches: Vec::new(),
            templates: Templates::default(),
            project_paths: HashMap::new(),
        }
    }
}

impl SessionFormState {
    /// Open the form with initial data.
    pub fn open(
        &mut self,
        projects: Vec<(String, String, PathBuf)>, // (id, name, path)
        providers: Vec<String>,
        default_provider: &str,
        current_project_id: Option<&str>,
        templates: Templates,
    ) {
        self.visible = true;
        self.mode = Mode::Manual;
        self.focus = Field::Mode;
        self.name.clear();
        self.branch.clear();
        self.use_worktree = false;
        self.auto_approve = true;
        self.error = None;
        self.task_list.clear();
        self.occupied_branches.clear();
        self.templates = templates;

        // Providers
        self.provider_keys = providers;
        self.provider_idx = self
            .provider_keys
            .iter()
            .position(|k| k == default_provider)
            .unwrap_or(0);

        // Projects
        self.project_paths.clear();
        let items: Vec<ComboItem> = projects
            .iter()
            .map(|(id, name, path)| {
                self.project_paths.insert(id.clone(), path.clone());
                ComboItem { id: id.clone(), label: name.clone() }
            })
            .collect();
        self.project_combo = ComboBoxState::new(items);
        if let Some(id) = current_project_id {
            self.project_combo.select_by_id(id);
        }

        // Reset combos
        self.task_combo = ComboBoxState::new(Vec::new());
        self.branch_combo = ComboBoxState::new_with_free_text(Vec::new(), true);
        self.base_branch_combo = ComboBoxState::new(Vec::new());
    }

    /// Feed tasks after caller loads them.
    pub fn set_tasks(&mut self, tasks: Vec<TaskInfo>) {
        let items: Vec<ComboItem> = tasks
            .iter()
            .map(|t| ComboItem {
                id: t.key.clone(),
                label: format!("{}: {}", t.key, t.title),
            })
            .collect();
        self.task_combo = ComboBoxState::new(items);
        self.task_list = tasks;
    }

    /// Feed branches after caller loads them.
    pub fn set_branches(&mut self, branches: Vec<String>) {
        let items: Vec<ComboItem> = branches
            .iter()
            .map(|b| ComboItem { id: b.clone(), label: b.clone() })
            .collect();
        self.branch_combo = ComboBoxState::new_with_free_text(items, true);
        self.base_branch_combo = ComboBoxState::new(
            branches.iter().map(|b| ComboItem { id: b.clone(), label: b.clone() }).collect(),
        );
    }

    /// Set occupied branches for "already used" warning.
    pub fn set_occupied_branches(&mut self, branches: Vec<String>) {
        self.occupied_branches = branches;
    }

    /// Get the currently selected project path.
    pub fn selected_project_path(&self) -> Option<&PathBuf> {
        self.project_combo
            .selected
            .as_ref()
            .and_then(|s| self.project_paths.get(&s.id))
    }

    /// Check if the current branch is new (not in the branch list).
    pub fn is_new_branch(&self) -> bool {
        let branch = self.effective_branch();
        !branch.is_empty()
            && !self.branch_combo.items.iter().any(|i| i.id == branch)
    }

    /// Check if the branch is already used by another session.
    pub fn is_branch_occupied(&self) -> bool {
        let branch = self.effective_branch();
        !self.use_worktree && !branch.is_empty() && self.occupied_branches.contains(&branch)
    }

    /// Effective branch value (from combo selection or text input).
    pub fn effective_branch(&self) -> String {
        if let Some(ref sel) = self.branch_combo.selected {
            sel.id.clone()
        } else if !self.branch.is_empty() {
            self.branch.clone()
        } else {
            // Auto-derive from name
            self.name
                .to_lowercase()
                .replace(' ', "-")
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '/')
                .collect()
        }
    }

    /// Current provider id.
    pub fn current_provider(&self) -> &str {
        self.provider_keys
            .get(self.provider_idx)
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// Handle keyboard events. Returns a FormEffect for the caller.
    pub fn handle_key(
        &mut self,
        key: &keyboard::Key,
        modifiers: &keyboard::Modifiers,
    ) -> FormEffect {
        // Escape always closes
        if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
            self.visible = false;
            return FormEffect::Close;
        }

        // Cmd+Enter always submits
        if matches!(key, keyboard::Key::Named(keyboard::key::Named::Enter)) && modifiers.command()
        {
            return self.validate_and_submit();
        }

        // Combobox-focused fields delegate key handling
        match self.focus {
            Field::Project => return self.handle_combo_key(key, modifiers, Field::Project),
            Field::Task => return self.handle_combo_key(key, modifiers, Field::Task),
            Field::Branch => return self.handle_combo_key(key, modifiers, Field::Branch),
            Field::BaseBranch => return self.handle_combo_key(key, modifiers, Field::BaseBranch),
            _ => {}
        }

        match key {
            keyboard::Key::Named(keyboard::key::Named::Tab) => {
                if modifiers.shift() {
                    self.focus_prev();
                } else {
                    self.focus_next();
                }
                FormEffect::None
            }
            keyboard::Key::Named(keyboard::key::Named::Enter) => {
                if self.focus == Field::Mode {
                    self.toggle_mode()
                } else {
                    FormEffect::None
                }
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                if self.focus == Field::Mode {
                    self.mode = Mode::Manual;
                }
                FormEffect::None
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                if self.focus == Field::Mode {
                    return self.set_mode_from_task();
                }
                FormEffect::None
            }
            keyboard::Key::Named(keyboard::key::Named::Backspace) => {
                match self.focus {
                    Field::Name => { self.name.pop(); }
                    _ => {}
                }
                FormEffect::None
            }
            keyboard::Key::Character(c) => self.handle_char(c.as_str()),
            _ => FormEffect::None,
        }
    }

    /// Handle widget-emitted messages.
    pub fn update(&mut self, msg: FormMessage) -> FormEffect {
        match msg {
            FormMessage::ModeManual => {
                self.mode = Mode::Manual;
                FormEffect::None
            }
            FormMessage::ModeFromTask => self.set_mode_from_task(),
            FormMessage::NameChanged(val) => {
                self.name = val;
                self.error = None;
                FormEffect::None
            }
            FormMessage::BranchChanged(val) => {
                self.branch = val;
                self.error = None;
                FormEffect::None
            }
            FormMessage::ToggleWorktree => {
                self.use_worktree = !self.use_worktree;
                FormEffect::None
            }
            FormMessage::ToggleAutoApprove => {
                self.auto_approve = !self.auto_approve;
                FormEffect::None
            }
            FormMessage::CycleProvider => {
                if !self.provider_keys.is_empty() {
                    self.provider_idx = (self.provider_idx + 1) % self.provider_keys.len();
                }
                FormEffect::None
            }
            FormMessage::Submit => self.validate_and_submit(),
            FormMessage::Cancel => {
                self.visible = false;
                FormEffect::Close
            }
            FormMessage::ProjectFocus => {
                self.focus = Field::Project;
                FormEffect::None
            }
            FormMessage::ProjectSelected(item) => self.on_project_selected(item),
            FormMessage::TaskFocus => {
                self.focus = Field::Task;
                FormEffect::None
            }
            FormMessage::TaskSelected(item) => {
                self.task_combo.selected = Some(item.clone());
                self.task_combo.search.clear();
                self.apply_task(&item.id);
                FormEffect::None
            }
            FormMessage::BranchFocus => {
                self.focus = Field::Branch;
                FormEffect::None
            }
            FormMessage::BranchSelected(item) => {
                self.branch_combo.selected = Some(item.clone());
                self.branch_combo.search.clear();
                self.branch = item.id;
                FormEffect::None
            }
            FormMessage::BaseBranchFocus => {
                self.focus = Field::BaseBranch;
                FormEffect::None
            }
            FormMessage::BaseBranchSelected(item) => {
                self.base_branch_combo.selected = Some(item.clone());
                self.base_branch_combo.search.clear();
                FormEffect::None
            }
        }
    }

    // ─── Private helpers ─────────────────────────────────────────────────────

    fn toggle_mode(&mut self) -> FormEffect {
        match self.mode {
            Mode::Manual => self.set_mode_from_task(),
            Mode::FromTask => {
                self.mode = Mode::Manual;
                FormEffect::None
            }
        }
    }

    fn set_mode_from_task(&mut self) -> FormEffect {
        self.mode = Mode::FromTask;
        if self.task_list.is_empty() {
            if let Some(path) = self.selected_project_path().cloned() {
                return FormEffect::LoadTasks { project_path: path };
            }
        }
        FormEffect::None
    }

    fn on_project_selected(&mut self, item: ComboItem) -> FormEffect {
        self.project_combo.selected = Some(item.clone());
        self.project_combo.search.clear();
        // Request branches for selected project
        if let Some(path) = self.project_paths.get(&item.id).cloned() {
            return FormEffect::LoadBranches { project_path: path };
        }
        FormEffect::None
    }

    fn handle_combo_key(&mut self, key: &keyboard::Key, modifiers: &keyboard::Modifiers, field: Field) -> FormEffect {
        // Tab/Shift+Tab navigates between fields
        if matches!(key, keyboard::Key::Named(keyboard::key::Named::Tab)) {
            if modifiers.shift() {
                self.focus_prev();
            } else {
                self.focus_next();
            }
            return FormEffect::None;
        }

        let key_str = match key {
            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => "ArrowDown",
            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => "ArrowUp",
            keyboard::Key::Named(keyboard::key::Named::Backspace) => "Backspace",
            keyboard::Key::Named(keyboard::key::Named::Enter) => "Enter",
            keyboard::Key::Character(c) => c.as_str(),
            _ => return FormEffect::None,
        };

        let selected = match field {
            Field::Project => {
                if let Some(item) = self.project_combo.handle_key(key_str) {
                    let effect = self.on_project_selected(item);
                    self.focus_next();
                    return effect;
                }
                false
            }
            Field::Task => {
                if let Some(item) = self.task_combo.handle_key(key_str) {
                    self.apply_task(&item.id);
                    true
                } else {
                    false
                }
            }
            Field::Branch => {
                if let Some(item) = self.branch_combo.handle_key(key_str) {
                    self.branch = item.id;
                    true
                } else {
                    false
                }
            }
            Field::BaseBranch => {
                self.base_branch_combo.handle_key(key_str).is_some()
            }
            _ => false,
        };
        if selected {
            self.focus_next();
        }
        FormEffect::None
    }

    fn handle_char(&mut self, ch: &str) -> FormEffect {
        match self.focus {
            Field::Mode => {
                match ch {
                    "m" => self.mode = Mode::Manual,
                    "t" => return self.set_mode_from_task(),
                    _ => {}
                }
                FormEffect::None
            }
            Field::Name => {
                self.name.push_str(ch);
                FormEffect::None
            }
            Field::Toggles => {
                match ch {
                    "w" => self.use_worktree = !self.use_worktree,
                    "a" => self.auto_approve = !self.auto_approve,
                    "p" if !self.provider_keys.is_empty() => {
                        self.provider_idx = (self.provider_idx + 1) % self.provider_keys.len();
                    }
                    _ => {}
                }
                FormEffect::None
            }
            _ => FormEffect::None,
        }
    }

    fn apply_task(&mut self, task_key: &str) {
        let task = match self.task_list.iter().find(|t| t.key == task_key) {
            Some(t) => t.clone(),
            None => return,
        };

        let mut vars: HashMap<&str, &str> = HashMap::new();
        vars.insert("key", &task.key);
        vars.insert("title", &task.title);
        vars.insert("description", &task.description);
        vars.insert("status", "todo");
        vars.insert("base_branch", &task.base_branch);

        self.name = match &self.templates.name {
            Some(tmpl) => planeai_core::template::render(tmpl, &vars),
            None => format!("{}: {}", task.key, task.title),
        };

        let branch = match &self.templates.branch {
            Some(tmpl) => planeai_core::template::render(tmpl, &vars),
            None => format!(
                "{}/{}",
                task.key.to_lowercase(),
                task.title.to_lowercase().split_whitespace().collect::<Vec<_>>().join("-")
                    .chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '/').collect::<String>()
            ),
        };
        self.branch = branch.clone();
        self.branch_combo.selected = Some(ComboItem { id: branch.clone(), label: branch });
    }

    fn focus_next(&mut self) {
        self.focus = match (&self.mode, &self.focus) {
            (_, Field::Mode) => Field::Project,
            (Mode::FromTask, Field::Project) => Field::Task,
            (Mode::Manual, Field::Project) => Field::Name,
            (_, Field::Task) => Field::Name,
            (_, Field::Name) => Field::Toggles,
            (_, Field::Toggles) => Field::Branch,
            (_, Field::Branch) if self.is_new_branch() || self.use_worktree => Field::BaseBranch,
            (_, Field::Branch) => Field::Mode,
            (_, Field::BaseBranch) => Field::Mode,
        };
    }

    fn focus_prev(&mut self) {
        self.focus = match (&self.mode, &self.focus) {
            (_, Field::Mode) => {
                if self.is_new_branch() || self.use_worktree {
                    Field::BaseBranch
                } else {
                    Field::Branch
                }
            }
            (_, Field::Project) => Field::Mode,
            (_, Field::Task) => Field::Project,
            (Mode::FromTask, Field::Name) => Field::Task,
            (Mode::Manual, Field::Name) => Field::Project,
            (_, Field::Toggles) => Field::Name,
            (_, Field::Branch) => Field::Toggles,
            (_, Field::BaseBranch) => Field::Branch,
        };
    }

    fn validate_and_submit(&mut self) -> FormEffect {
        let project = match &self.project_combo.selected {
            Some(p) => p.clone(),
            None => {
                self.error = Some("Select a project.".into());
                return FormEffect::Error(self.error.clone().unwrap());
            }
        };

        let project_path = match self.project_paths.get(&project.id) {
            Some(p) => p.clone(),
            None => {
                self.error = Some("Project path not found.".into());
                return FormEffect::Error(self.error.clone().unwrap());
            }
        };

        let branch = self.effective_branch();
        if branch.is_empty() {
            self.error = Some("Branch name is required.".into());
            return FormEffect::Error(self.error.clone().unwrap());
        }

        let is_new_branch = self.is_new_branch();

        let base_branch = if self.use_worktree || is_new_branch {
            Some(
                self.base_branch_combo
                    .selected
                    .as_ref()
                    .map(|s| s.id.clone())
                    .unwrap_or_else(|| "main".to_string()),
            )
        } else {
            None
        };

        // Build task prompt if in FromTask mode
        let (task_key, task_prompt) = if self.mode == Mode::FromTask {
            let selected_key = match &self.task_combo.selected {
                Some(item) => item.id.clone(),
                None => {
                    self.error = Some("No task selected.".into());
                    return FormEffect::Error(self.error.clone().unwrap());
                }
            };
            let task = match self.task_list.iter().find(|t| t.key == selected_key) {
                Some(t) => t.clone(),
                None => {
                    self.error = Some("Task not found.".into());
                    return FormEffect::Error(self.error.clone().unwrap());
                }
            };

            let mut vars: HashMap<&str, &str> = HashMap::new();
            vars.insert("key", &task.key);
            vars.insert("title", &task.title);
            vars.insert("description", &task.description);
            vars.insert("base_branch", &task.base_branch);

            let prompt = match &self.templates.prompt {
                Some(tmpl) => planeai_core::template::render(tmpl, &vars),
                None => format!("Implement task {}: {}\n\n{}", task.key, task.title, task.description),
            };
            (Some(task.key), Some(prompt))
        } else {
            (None, None)
        };

        let provider_id = self.current_provider().to_string();

        self.visible = false;
        FormEffect::Submit(SessionLaunchRequest {
            project_id: project.id,
            project_path,
            name: self.name.clone(),
            branch,
            is_new_branch,
            use_worktree: self.use_worktree,
            base_branch,
            auto_approve: self.auto_approve,
            provider_id,
            task_key,
            task_prompt,
        })
    }

    /// Render the form.
    pub fn view<'a, M: Clone + 'a>(
        &'a self,
        theme: &PlaneAiTheme,
        on_msg: impl Fn(FormMessage) -> M + 'a + Copy,
    ) -> Element<'a, M> {
        let mut col = column![].spacing(8).width(Length::Fill).padding(8);

        // Title
        col = col.push(text("New Session").color(theme.text_primary()));

        // Helper: wrap a section in a container with border when focused
        let accent = theme.accent();
        let dim_border = theme.border();

        // Mode toggle (segmented control like Svelte)
        let mode_focused = self.focus == Field::Mode;
        let mode_border = if mode_focused { accent } else { dim_border };
        let manual_active = self.mode == Mode::Manual;
        let task_active = self.mode == Mode::FromTask;
        let panel_bg = theme.panel_bg();
        let chrome_bg = theme.chrome_bg();
        let manual_btn = button(
            container(text("Manual").color(if manual_active { chrome_bg } else { theme.text_muted() }))
                .padding([4, 12])
                .center_x(Length::Fill)
                .style(move |_: &Theme| container::Style {
                    background: if manual_active { Some(accent.into()) } else { Some(panel_bg.into()) },
                    border: iced::Border { color: iced::Color::TRANSPARENT, width: 0.0, radius: 4.0.into() },
                    ..Default::default()
                }),
        )
        .on_press(on_msg(FormMessage::ModeManual))
        .style(button::text)
        .width(Length::FillPortion(1));
        let task_btn = button(
            container(text("From task").color(if task_active { chrome_bg } else { theme.text_muted() }))
                .padding([4, 12])
                .center_x(Length::Fill)
                .style(move |_: &Theme| container::Style {
                    background: if task_active { Some(accent.into()) } else { Some(panel_bg.into()) },
                    border: iced::Border { color: iced::Color::TRANSPARENT, width: 0.0, radius: 4.0.into() },
                    ..Default::default()
                }),
        )
        .on_press(on_msg(FormMessage::ModeFromTask))
        .style(button::text)
        .width(Length::FillPortion(1));
        col = col.push(
            container(row![manual_btn, task_btn].spacing(2))
                .padding(2)
                .width(Length::Fill)
                .style(move |_: &Theme| container::Style {
                    border: iced::Border { color: mode_border, width: 1.0, radius: 6.0.into() },
                    background: Some(panel_bg.into()),
                    ..Default::default()
                }),
        );

        // Project combo
        let proj_focused = self.focus == Field::Project;
        col = col.push(self.project_combo.view(
            "Project",
            proj_focused,
            theme,
            on_msg(FormMessage::ProjectFocus),
            {
                let on_msg = on_msg;
                move |item| on_msg(FormMessage::ProjectSelected(item))
            },
        ));

        // Task combo (FromTask mode only)
        if self.mode == Mode::FromTask {
            let task_focused = self.focus == Field::Task;
            col = col.push(self.task_combo.view(
                "Task",
                task_focused,
                theme,
                on_msg(FormMessage::TaskFocus),
                {
                    let on_msg = on_msg;
                    move |item| on_msg(FormMessage::TaskSelected(item))
                },
            ));
        }

        // Name field
        let name_focused = self.focus == Field::Name;
        let name_border = if name_focused { accent } else { dim_border };
        col = col.push(
            container(
                column![
                    text("Name:").color(theme.text_muted()),
                    text_input("session name...", &self.name)
                        .on_input(move |v| on_msg(FormMessage::NameChanged(v)))
                        .on_submit(on_msg(FormMessage::Submit))
                        .width(Length::Fill),
                ].spacing(2)
            )
            .padding([4, 8])
            .width(Length::Fill)
            .style(move |_: &Theme| container::Style {
                border: iced::Border { color: name_border, width: 1.0, radius: 4.0.into() },
                ..Default::default()
            }),
        );

        // Toggles (checkboxes)
        let toggles_focused = self.focus == Field::Toggles;
        let t_border = if toggles_focused { accent } else { dim_border };
        let provider = self.current_provider();
        col = col.push(
            container(
                column![
                    row![
                        checkbox(self.use_worktree)
                            .label("Worktree")
                            .on_toggle(move |_| on_msg(FormMessage::ToggleWorktree)),
                        checkbox(self.auto_approve)
                            .label("Auto-approve")
                            .on_toggle(move |_| on_msg(FormMessage::ToggleAutoApprove)),
                    ].spacing(16),
                    mouse_area(
                        text(format!("Provider: {}", provider)).color(theme.text_muted())
                    ).on_press(on_msg(FormMessage::CycleProvider)),
                ].spacing(4)
            )
            .padding([4, 8])
            .width(Length::Fill)
            .style(move |_: &Theme| container::Style {
                border: iced::Border { color: t_border, width: 1.0, radius: 4.0.into() },
                ..Default::default()
            }),
        );

        // Branch combo
        let branch_focused = self.focus == Field::Branch;
        col = col.push(self.branch_combo.view(
            "Branch",
            branch_focused,
            theme,
            on_msg(FormMessage::BranchFocus),
            {
                let on_msg = on_msg;
                move |item| on_msg(FormMessage::BranchSelected(item))
            },
        ));

        // Base branch (shown when new branch or worktree)
        if self.is_new_branch() || self.use_worktree {
            let base_focused = self.focus == Field::BaseBranch;
            col = col.push(self.base_branch_combo.view(
                "Base branch",
                base_focused,
                theme,
                on_msg(FormMessage::BaseBranchFocus),
                {
                    let on_msg = on_msg;
                    move |item| on_msg(FormMessage::BaseBranchSelected(item))
                },
            ));
        }

        // Branch already used warning
        if self.is_branch_occupied() {
            col = col.push(
                text("  ⚠ Another session is using this branch")
                    .color(theme.warning()),
            );
        }

        // Error
        if let Some(ref err) = self.error {
            col = col.push(text(format!("  ⚠ {}", err)).color(theme.error()));
        }

        // Footer buttons
        let button_row = row![
            button(text("Cancel")).on_press(on_msg(FormMessage::Cancel)).padding([4, 12]),
            iced::widget::Space::new().width(Length::Fill),
            button(text("Launch ⌘↵")).on_press(on_msg(FormMessage::Submit)).padding([4, 12]),
        ]
        .width(Length::Fill);
        col = col.push(iced::widget::Space::new().height(8.0));
        col = col.push(
            text("  Tab=next | Escape=cancel | ⌘↵=launch").color(theme.text_dimmed()),
        );
        col = col.push(button_row);

        col.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_projects() -> Vec<(String, String, PathBuf)> {
        vec![
            ("p1".into(), "ProjectA".into(), PathBuf::from("/tmp/proj-a")),
            ("p2".into(), "ProjectB".into(), PathBuf::from("/tmp/proj-b")),
        ]
    }

    fn sample_providers() -> Vec<String> {
        vec!["kiro".into(), "claude".into(), "copilot".into()]
    }

    fn sample_tasks() -> Vec<TaskInfo> {
        vec![
            TaskInfo {
                key: "PA-1".into(),
                title: "Add login".into(),
                description: "Implement login flow".into(),
                base_branch: "main".into(),
            },
            TaskInfo {
                key: "PA-2".into(),
                title: "Fix bug".into(),
                description: "Fix null pointer".into(),
                base_branch: "develop".into(),
            },
        ]
    }

    fn open_form() -> SessionFormState {
        let mut form = SessionFormState::default();
        form.open(
            sample_projects(),
            sample_providers(),
            "claude",
            Some("p1"),
            Templates::default(),
        );
        form
    }

    // ─── Open / initialization ───────────────────────────────────────────

    #[test]
    fn open_sets_visible_and_defaults() {
        let form = open_form();
        assert!(form.visible);
        assert_eq!(form.mode, Mode::Manual);
        assert_eq!(form.focus, Field::Mode);
        assert!(form.name.is_empty());
        assert!(form.auto_approve);
        assert!(!form.use_worktree);
    }

    #[test]
    fn open_preselects_current_project() {
        let form = open_form();
        assert_eq!(form.project_combo.selected.as_ref().unwrap().id, "p1");
    }

    #[test]
    fn open_sets_provider_index_to_default() {
        let form = open_form();
        assert_eq!(form.provider_idx, 1); // "claude" is index 1
        assert_eq!(form.current_provider(), "claude");
    }

    // ─── Focus cycling ───────────────────────────────────────────────────

    #[test]
    fn focus_next_manual_mode() {
        let mut form = open_form();
        assert_eq!(form.focus, Field::Mode);
        form.focus_next();
        assert_eq!(form.focus, Field::Project);
        form.focus_next();
        assert_eq!(form.focus, Field::Name); // skips Task in Manual
        form.focus_next();
        assert_eq!(form.focus, Field::Toggles);
        form.focus_next();
        assert_eq!(form.focus, Field::Branch);
        form.focus_next();
        assert_eq!(form.focus, Field::Mode); // wraps (no new branch)
    }

    #[test]
    fn focus_next_from_task_mode() {
        let mut form = open_form();
        form.mode = Mode::FromTask;
        form.focus = Field::Project;
        form.focus_next();
        assert_eq!(form.focus, Field::Task);
        form.focus_next();
        assert_eq!(form.focus, Field::Name);
    }

    #[test]
    fn focus_prev_wraps() {
        let mut form = open_form();
        assert_eq!(form.focus, Field::Mode);
        form.focus_prev();
        assert_eq!(form.focus, Field::Branch); // wraps back
    }

    // ─── Mode switching ──────────────────────────────────────────────────

    #[test]
    fn handle_key_m_sets_manual() {
        let mut form = open_form();
        form.mode = Mode::FromTask;
        form.handle_key(&keyboard::Key::Character("m".into()), &keyboard::Modifiers::default());
        assert_eq!(form.mode, Mode::Manual);
    }

    #[test]
    fn handle_key_t_sets_from_task_and_requests_load() {
        let mut form = open_form();
        let effect = form.handle_key(
            &keyboard::Key::Character("t".into()),
            &keyboard::Modifiers::default(),
        );
        assert_eq!(form.mode, Mode::FromTask);
        assert!(matches!(effect, FormEffect::LoadTasks { .. }));
    }

    #[test]
    fn arrow_right_on_mode_switches_to_from_task() {
        let mut form = open_form();
        let effect = form.handle_key(
            &keyboard::Key::Named(keyboard::key::Named::ArrowRight),
            &keyboard::Modifiers::default(),
        );
        assert_eq!(form.mode, Mode::FromTask);
        assert!(matches!(effect, FormEffect::LoadTasks { .. }));
    }

    // ─── Escape / close ──────────────────────────────────────────────────

    #[test]
    fn escape_closes_form() {
        let mut form = open_form();
        let effect = form.handle_key(
            &keyboard::Key::Named(keyboard::key::Named::Escape),
            &keyboard::Modifiers::default(),
        );
        assert!(!form.visible);
        assert!(matches!(effect, FormEffect::Close));
    }

    // ─── Toggles ─────────────────────────────────────────────────────────

    #[test]
    fn toggle_keys_in_toggles_field() {
        let mut form = open_form();
        form.focus = Field::Toggles;
        form.handle_key(&keyboard::Key::Character("w".into()), &keyboard::Modifiers::default());
        assert!(form.use_worktree);
        form.handle_key(&keyboard::Key::Character("a".into()), &keyboard::Modifiers::default());
        assert!(!form.auto_approve);
        form.handle_key(&keyboard::Key::Character("p".into()), &keyboard::Modifiers::default());
        assert_eq!(form.provider_idx, 2); // claude(1) -> copilot(2)
    }

    // ─── Name field ──────────────────────────────────────────────────────

    #[test]
    fn typing_in_name_field() {
        let mut form = open_form();
        form.focus = Field::Name;
        form.handle_key(&keyboard::Key::Character("h".into()), &keyboard::Modifiers::default());
        form.handle_key(&keyboard::Key::Character("i".into()), &keyboard::Modifiers::default());
        assert_eq!(form.name, "hi");
        form.handle_key(&keyboard::Key::Named(keyboard::key::Named::Backspace), &keyboard::Modifiers::default());
        assert_eq!(form.name, "h");
    }

    // ─── Task templates ──────────────────────────────────────────────────

    #[test]
    fn apply_task_uses_default_templates() {
        let mut form = open_form();
        form.set_tasks(sample_tasks());
        form.apply_task("PA-1");
        assert_eq!(form.name, "PA-1: Add login");
        assert!(form.branch.starts_with("pa-1/"));
    }

    #[test]
    fn apply_task_uses_custom_templates() {
        let mut form = SessionFormState::default();
        form.open(
            sample_projects(),
            sample_providers(),
            "claude",
            Some("p1"),
            Templates {
                name: Some("{key}: {title}".into()),
                branch: Some("{key:lower}/{title:slug}".into()),
                prompt: Some("Do {key}".into()),
            },
        );
        form.set_tasks(sample_tasks());
        form.apply_task("PA-1");
        assert_eq!(form.name, "PA-1: Add login");
        assert_eq!(form.branch, "pa-1/add-login");
    }

    // ─── Branch detection ────────────────────────────────────────────────

    #[test]
    fn is_new_branch_when_not_in_list() {
        let mut form = open_form();
        form.set_branches(vec!["main".into(), "develop".into()]);
        form.branch_combo.selected = Some(ComboItem { id: "feat/new".into(), label: "feat/new".into() });
        assert!(form.is_new_branch());
    }

    #[test]
    fn is_not_new_branch_when_in_list() {
        let mut form = open_form();
        form.set_branches(vec!["main".into(), "develop".into()]);
        form.branch_combo.selected = Some(ComboItem { id: "main".into(), label: "main".into() });
        assert!(!form.is_new_branch());
    }

    // ─── Branch occupied ─────────────────────────────────────────────────

    #[test]
    fn branch_occupied_warning() {
        let mut form = open_form();
        form.set_occupied_branches(vec!["feat/login".into()]);
        form.branch_combo.selected = Some(ComboItem { id: "feat/login".into(), label: "feat/login".into() });
        assert!(form.is_branch_occupied());
    }

    #[test]
    fn branch_not_occupied_with_worktree() {
        let mut form = open_form();
        form.use_worktree = true;
        form.set_occupied_branches(vec!["feat/login".into()]);
        form.branch_combo.selected = Some(ComboItem { id: "feat/login".into(), label: "feat/login".into() });
        assert!(!form.is_branch_occupied()); // worktree isolates
    }

    // ─── Validation / submit ─────────────────────────────────────────────

    #[test]
    fn submit_no_project_returns_error() {
        let mut form = SessionFormState::default();
        form.visible = true;
        let effect = form.validate_and_submit();
        assert!(matches!(effect, FormEffect::Error(_)));
    }

    #[test]
    fn submit_no_branch_returns_error() {
        let mut form = open_form();
        form.name.clear();
        form.branch.clear();
        form.branch_combo.selected = None;
        let effect = form.validate_and_submit();
        assert!(matches!(effect, FormEffect::Error(_)));
    }

    #[test]
    fn submit_valid_manual_mode() {
        let mut form = open_form();
        form.name = "test session".into();
        form.branch = "feat/test".into();
        let effect = form.validate_and_submit();
        match effect {
            FormEffect::Submit(req) => {
                assert_eq!(req.project_id, "p1");
                assert_eq!(req.name, "test session");
                assert_eq!(req.branch, "feat/test");
                assert_eq!(req.provider_id, "claude");
                assert!(req.auto_approve);
                assert!(!req.use_worktree);
                assert!(req.task_key.is_none());
                assert!(req.task_prompt.is_none());
            }
            _ => panic!("Expected Submit"),
        }
        assert!(!form.visible);
    }

    #[test]
    fn submit_from_task_mode_includes_task_data() {
        let mut form = open_form();
        form.mode = Mode::FromTask;
        form.set_tasks(sample_tasks());
        form.task_combo.selected = Some(ComboItem { id: "PA-1".into(), label: "PA-1: Add login".into() });
        form.name = "PA-1: Add login".into();
        form.branch = "pa-1/add-login".into();
        let effect = form.validate_and_submit();
        match effect {
            FormEffect::Submit(req) => {
                assert_eq!(req.task_key, Some("PA-1".into()));
                assert!(req.task_prompt.unwrap().contains("Add login"));
            }
            _ => panic!("Expected Submit"),
        }
    }

    #[test]
    fn submit_from_task_no_selection_returns_error() {
        let mut form = open_form();
        form.mode = Mode::FromTask;
        form.name = "something".into();
        form.branch = "feat/x".into();
        let effect = form.validate_and_submit();
        assert!(matches!(effect, FormEffect::Error(_)));
    }

    #[test]
    fn submit_with_worktree_includes_base_branch() {
        let mut form = open_form();
        form.use_worktree = true;
        form.name = "test".into();
        form.branch = "feat/test".into();
        form.base_branch_combo.selected = Some(ComboItem { id: "develop".into(), label: "develop".into() });
        let effect = form.validate_and_submit();
        match effect {
            FormEffect::Submit(req) => {
                assert!(req.use_worktree);
                assert_eq!(req.base_branch, Some("develop".into()));
            }
            _ => panic!("Expected Submit"),
        }
    }

    // ─── FormMessage (widget) handling ───────────────────────────────────

    #[test]
    fn update_cancel_closes() {
        let mut form = open_form();
        let effect = form.update(FormMessage::Cancel);
        assert!(!form.visible);
        assert!(matches!(effect, FormEffect::Close));
    }

    #[test]
    fn update_toggle_worktree() {
        let mut form = open_form();
        assert!(!form.use_worktree);
        form.update(FormMessage::ToggleWorktree);
        assert!(form.use_worktree);
    }

    #[test]
    fn update_cycle_provider() {
        let mut form = open_form();
        assert_eq!(form.current_provider(), "claude");
        form.update(FormMessage::CycleProvider);
        assert_eq!(form.current_provider(), "copilot");
        form.update(FormMessage::CycleProvider);
        assert_eq!(form.current_provider(), "kiro"); // wraps
    }

    #[test]
    fn update_project_selected_returns_load_branches() {
        let mut form = open_form();
        let effect = form.update(FormMessage::ProjectSelected(ComboItem {
            id: "p2".into(),
            label: "ProjectB".into(),
        }));
        assert!(matches!(effect, FormEffect::LoadBranches { .. }));
    }

    #[test]
    fn effective_branch_auto_derives_from_name() {
        let mut form = open_form();
        form.name = "My Feature".into();
        form.branch.clear();
        form.branch_combo.selected = None;
        assert_eq!(form.effective_branch(), "my-feature");
    }

    // ─── Cmd+Enter submit via handle_key ─────────────────────────────────

    #[test]
    fn cmd_enter_submits() {
        let mut form = open_form();
        form.name = "test".into();
        form.branch = "feat/x".into();
        let effect = form.handle_key(
            &keyboard::Key::Named(keyboard::key::Named::Enter),
            &keyboard::Modifiers::COMMAND,
        );
        assert!(matches!(effect, FormEffect::Submit(_)));
    }
}
