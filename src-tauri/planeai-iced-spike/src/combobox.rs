//! Keyboard-driven combobox: type to filter, ↑↓ to navigate, Enter to select.

use iced::widget::{column, text};
use iced::{Color, Element, Font};

/// A single item in the combobox.
#[derive(Debug, Clone, PartialEq)]
pub struct ComboItem {
    pub id: String,
    pub label: String,
}

/// Combobox state — owns the items, search string, and selection.
#[derive(Debug, Clone)]
pub struct ComboBoxState {
    pub items: Vec<ComboItem>,
    pub search: String,
    pub cursor: usize,
    pub selected: Option<ComboItem>,
}

impl ComboBoxState {
    pub fn new(items: Vec<ComboItem>) -> Self {
        Self {
            items,
            search: String::new(),
            cursor: 0,
            selected: None,
        }
    }

    pub fn filtered(&self) -> Vec<&ComboItem> {
        if self.search.is_empty() {
            self.items.iter().collect()
        } else {
            let q = self.search.to_lowercase();
            self.items
                .iter()
                .filter(|i| i.label.to_lowercase().contains(&q))
                .collect()
        }
    }

    /// Handle a key event. Returns Some(selected item) on Enter selection.
    pub fn handle_key(&mut self, key: &str) -> Option<ComboItem> {
        match key {
            "ArrowDown" => {
                let count = self.filtered().len();
                if count > 0 {
                    self.cursor = (self.cursor + 1).min(count - 1);
                }
                None
            }
            "ArrowUp" => {
                self.cursor = self.cursor.saturating_sub(1);
                None
            }
            "Backspace" => {
                self.search.pop();
                self.cursor = 0;
                None
            }
            "Enter" => {
                let filtered = self.filtered();
                if let Some(item) = filtered.get(self.cursor) {
                    let item = (*item).clone();
                    self.selected = Some(item.clone());
                    self.search.clear();
                    self.cursor = 0;
                    Some(item)
                } else {
                    None
                }
            }
            ch => {
                // Only accept printable chars (skip control chars)
                if ch.len() <= 4 && !ch.is_empty() && ch.chars().all(|c| !c.is_control()) {
                    self.search.push_str(ch);
                    self.cursor = 0;
                }
                None
            }
        }
    }

    /// Set selection without going through search.
    pub fn select_by_id(&mut self, id: &str) {
        if let Some(item) = self.items.iter().find(|i| i.id == id) {
            self.selected = Some(item.clone());
        }
    }

    /// Render the combobox as an iced Element (text-based, no native widget focus needed).
    pub fn view<'a, M: 'a>(&self, label: &str, focused: bool) -> Element<'a, M> {
        let prefix = if focused { "▶ " } else { "  " };
        let display = if focused && !self.search.is_empty() {
            format!("{}{}▏", self.search, "")
        } else {
            self.selected
                .as_ref()
                .map(|s| s.label.clone())
                .unwrap_or_else(|| "(none)".into())
        };

        let color = if focused {
            Color::from_rgb8(100, 220, 255)
        } else {
            Color::from_rgb8(160, 160, 160)
        };

        let mut col = column![text(format!("{}{}: {}", prefix, label, display))
            .size(11)
            .color(color)
            .font(Font::MONOSPACE)]
        .spacing(1);

        // Show filtered dropdown when focused
        if focused {
            let filtered = self.filtered();
            let show = filtered.len().min(6);
            for (i, item) in filtered.iter().take(show).enumerate() {
                let marker = if i == self.cursor { "▸" } else { " " };
                let item_color = if i == self.cursor {
                    Color::from_rgb8(100, 220, 255)
                } else {
                    Color::from_rgb8(140, 140, 140)
                };
                col = col.push(
                    text(format!("    {} {}", marker, item.label))
                        .size(10)
                        .color(item_color)
                        .font(Font::MONOSPACE),
                );
            }
            if filtered.len() > show {
                col = col.push(
                    text(format!("    ... +{} more", filtered.len() - show))
                        .size(10)
                        .color(Color::from_rgb8(100, 100, 100))
                        .font(Font::MONOSPACE),
                );
            }
        }

        col.into()
    }
}
