//! Combobox with command-palette styling: search input, windowed scrolling,
//! cursor highlight with background, container with border/radius.

use iced::widget::{button, column, container, mouse_area, text};
use iced::{Color, Element, Length, Theme};

use crate::theme::PlaneAiTheme;

const MAX_VISIBLE: usize = 8;

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
    pub allow_free_text: bool,
}

impl ComboBoxState {
    pub fn new(items: Vec<ComboItem>) -> Self {
        Self { items, search: String::new(), cursor: 0, selected: None, allow_free_text: false }
    }

    pub fn new_with_free_text(items: Vec<ComboItem>, allow_free_text: bool) -> Self {
        Self { items, search: String::new(), cursor: 0, selected: None, allow_free_text }
    }

    pub fn filtered(&self) -> Vec<&ComboItem> {
        if self.search.is_empty() {
            self.items.iter().collect()
        } else {
            let q = self.search.to_lowercase();
            self.items.iter().filter(|i| i.label.to_lowercase().contains(&q)).collect()
        }
    }

    /// Returns the visible window of filtered items (max MAX_VISIBLE, follows cursor).
    pub fn visible_items(&self) -> Vec<&ComboItem> {
        let filtered = self.filtered();
        let len = filtered.len();
        if len <= MAX_VISIBLE {
            return filtered;
        }
        let start = if self.cursor < MAX_VISIBLE { 0 } else { self.cursor - (MAX_VISIBLE - 1) };
        filtered[start..start + MAX_VISIBLE].to_vec()
    }

    /// Handle a key event. Returns Some(selected item) on Enter selection.
    pub fn handle_key(&mut self, key: &str) -> Option<ComboItem> {
        match key {
            "ArrowDown" => {
                let count = self.filtered().len();
                if count > 0 {
                    self.cursor = (self.cursor + 1) % count;
                }
                None
            }
            "ArrowUp" => {
                let count = self.filtered().len();
                if count > 0 {
                    self.cursor = if self.cursor == 0 { count - 1 } else { self.cursor - 1 };
                }
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
                } else if self.allow_free_text && !self.search.is_empty() {
                    let item = ComboItem { id: self.search.clone(), label: self.search.clone() };
                    self.selected = Some(item.clone());
                    self.search.clear();
                    self.cursor = 0;
                    Some(item)
                } else {
                    None
                }
            }
            ch => {
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

    /// Render the combobox with command-palette styling.
    pub fn view<'a, M: Clone + 'a>(
        &self,
        label: &str,
        focused: bool,
        theme: &PlaneAiTheme,
        on_focus: M,
        on_select: impl Fn(ComboItem) -> M + 'a,
    ) -> Element<'a, M> {
        let header_display = self
            .selected
            .as_ref()
            .map(|s| s.label.clone())
            .unwrap_or_else(|| "(none)".into());
        let header_color = if focused { theme.accent() } else { theme.text_muted() };

        let header_text = text(format!("{}: {}", label, header_display)).color(header_color);
        let accent = theme.accent();
        let dim_border = theme.border();
        let border_color = if focused { accent } else { dim_border };
        let field_bg = theme.panel_bg();

        let header = mouse_area(
            container(header_text)
                .padding([8, 12])
                .width(Length::Fill)
                .style(move |_: &Theme| container::Style {
                    background: Some(field_bg.into()),
                    border: iced::Border { color: border_color, width: 1.0, radius: 4.0.into() },
                    ..Default::default()
                }),
        )
        .on_press(on_focus);

        if !focused {
            return column![header].into();
        }

        // Dropdown panel (palette style)
        let mut items_col = column![].spacing(0);

        // Search input row
        let search_display = if self.search.is_empty() {
            "Type to search...".to_string()
        } else {
            format!("{}▏", self.search)
        };
        let search_color = if self.search.is_empty() {
            theme.text_dimmed()
        } else {
            theme.text_primary()
        };
        items_col = items_col.push(
            container(text(search_display).color(search_color))
                .width(Length::Fill)
                .padding([6, 8]),
        );

        // Visible items with cursor highlight
        let visible = self.visible_items();
        let filtered_len = self.filtered().len();
        let window_start = if filtered_len <= MAX_VISIBLE {
            0
        } else if self.cursor < MAX_VISIBLE {
            0
        } else {
            self.cursor - (MAX_VISIBLE - 1)
        };

        for (vi, item) in visible.iter().enumerate() {
            let abs_idx = window_start + vi;
            let is_cursor = abs_idx == self.cursor;
            let label_color = if is_cursor {
                theme.text_primary()
            } else {
                theme.text_secondary()
            };
            let item_bg = if is_cursor {
                Some(Color { a: 0.12, ..theme.accent() })
            } else {
                None
            };
            let item_clone = (*item).clone();
            let txt = text(item.label.clone()).color(label_color);
            items_col = items_col.push(
                button(
                    container(txt).width(Length::Fill).padding([3, 8]).style(
                        move |_: &Theme| container::Style {
                            background: item_bg.map(|c| c.into()),
                            ..Default::default()
                        },
                    ),
                )
                .on_press(on_select(item_clone))
                .style(button::text)
                .width(Length::Fill),
            );
        }

        let panel_bg = theme.panel_bg();
        let border_color = theme.border();
        let panel = container(items_col)
            .padding(4)
            .width(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(panel_bg.into()),
                border: iced::Border {
                    color: border_color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            });

        column![header, panel].spacing(2).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_items() -> Vec<ComboItem> {
        vec![
            ComboItem { id: "a".into(), label: "Apple".into() },
            ComboItem { id: "b".into(), label: "Banana".into() },
            ComboItem { id: "c".into(), label: "Cherry".into() },
        ]
    }

    #[test]
    fn test_new_defaults_allow_free_text_false() {
        let state = ComboBoxState::new(sample_items());
        assert!(!state.allow_free_text);
    }

    #[test]
    fn test_new_with_free_text_sets_flag() {
        let state = ComboBoxState::new_with_free_text(sample_items(), true);
        assert!(state.allow_free_text);
    }

    #[test]
    fn test_handle_key_enter_free_text_no_match_accepts_search() {
        let mut state = ComboBoxState::new_with_free_text(sample_items(), true);
        state.search = "custom".into();
        let result = state.handle_key("Enter");
        assert_eq!(result, Some(ComboItem { id: "custom".into(), label: "custom".into() }));
    }

    #[test]
    fn test_handle_key_enter_free_text_empty_search_returns_none() {
        let mut state = ComboBoxState::new_with_free_text(vec![], true);
        let result = state.handle_key("Enter");
        assert_eq!(result, None);
    }

    #[test]
    fn test_handle_key_enter_free_text_with_match_selects_from_list() {
        let mut state = ComboBoxState::new_with_free_text(sample_items(), true);
        state.search = "app".into();
        let result = state.handle_key("Enter");
        assert_eq!(result, Some(ComboItem { id: "a".into(), label: "Apple".into() }));
    }

    #[test]
    fn test_handle_key_enter_no_free_text_no_match_returns_none() {
        let mut state = ComboBoxState::new(sample_items());
        state.search = "xyz".into();
        let result = state.handle_key("Enter");
        assert_eq!(result, None);
    }

    #[test]
    fn test_handle_key_arrow_down_wraps() {
        let mut state = ComboBoxState::new(sample_items());
        assert_eq!(state.cursor, 0);
        state.handle_key("ArrowDown");
        assert_eq!(state.cursor, 1);
        state.handle_key("ArrowDown");
        assert_eq!(state.cursor, 2);
        state.handle_key("ArrowDown");
        assert_eq!(state.cursor, 0); // wraps
    }

    #[test]
    fn test_handle_key_arrow_up_wraps() {
        let mut state = ComboBoxState::new(sample_items());
        assert_eq!(state.cursor, 0);
        state.handle_key("ArrowUp");
        assert_eq!(state.cursor, 2); // wraps to end
        state.handle_key("ArrowUp");
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn test_handle_key_backspace_removes_char_resets_cursor() {
        let mut state = ComboBoxState::new(sample_items());
        state.search = "ab".into();
        state.cursor = 2;
        state.handle_key("Backspace");
        assert_eq!(state.search, "a");
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn test_handle_key_printable_appends_and_resets_cursor() {
        let mut state = ComboBoxState::new(sample_items());
        state.cursor = 1;
        state.handle_key("x");
        assert_eq!(state.search, "x");
        assert_eq!(state.cursor, 0);
        state.handle_key("y");
        assert_eq!(state.search, "xy");
    }

    #[test]
    fn test_filtered_empty_search_returns_all() {
        let state = ComboBoxState::new(sample_items());
        assert_eq!(state.filtered().len(), 3);
    }

    #[test]
    fn test_filtered_with_search_filters_case_insensitive() {
        let mut state = ComboBoxState::new(sample_items());
        state.search = "AN".into();
        let filtered = state.filtered();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "b");
    }

    #[test]
    fn test_select_by_id_sets_selected() {
        let mut state = ComboBoxState::new(sample_items());
        state.select_by_id("b");
        assert_eq!(state.selected, Some(ComboItem { id: "b".into(), label: "Banana".into() }));
    }

    #[test]
    fn test_visible_items_returns_all_when_under_max() {
        let state = ComboBoxState::new(sample_items());
        assert_eq!(state.visible_items().len(), 3);
    }

    #[test]
    fn test_visible_items_caps_at_max_and_follows_cursor() {
        let items: Vec<ComboItem> = (0..20)
            .map(|i| ComboItem { id: format!("{i}"), label: format!("Item {i}") })
            .collect();
        let mut state = ComboBoxState::new(items);
        // Cursor at 0: window is 0..8
        assert_eq!(state.visible_items().len(), MAX_VISIBLE);
        assert_eq!(state.visible_items()[0].id, "0");
        // Move cursor to 10
        state.cursor = 10;
        let vis = state.visible_items();
        assert_eq!(vis.len(), MAX_VISIBLE);
        // Window should include cursor at the end
        assert_eq!(vis.last().unwrap().id, "10");
    }
}
