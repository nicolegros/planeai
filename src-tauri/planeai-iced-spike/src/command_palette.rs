//! Command palette: Cmd+K keyboard-driven command launcher with filtering.

/// A single item in the command palette.
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteItem {
    pub id: String,
    pub label: String,
    pub group: String,
    pub is_active: bool,
}

/// Result from handling a key event.
#[derive(Debug, Clone, PartialEq)]
pub enum PaletteEvent {
    /// An item was selected (Enter).
    Select(String),
    /// The palette should close (Escape).
    Close,
    /// No externally visible change.
    None,
}

/// Command palette state — owns items, search, cursor, viewport.
#[derive(Debug, Clone)]
pub struct CommandPaletteState {
    items: Vec<PaletteItem>,
    search: String,
    cursor: usize,
}

impl CommandPaletteState {
    pub fn new(items: Vec<PaletteItem>) -> Self {
        Self {
            items,
            search: String::new(),
            cursor: 0,
        }
    }

    pub fn filtered(&self) -> Vec<&PaletteItem> {
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

    /// Handle a key event.
    pub fn handle_key(&mut self, key: &str) -> PaletteEvent {
        let count = self.filtered().len();
        match key {
            "ArrowDown" => {
                if count > 0 {
                    self.cursor = (self.cursor + 1) % count;
                }
                PaletteEvent::None
            }
            "ArrowUp" => {
                if count > 0 {
                    self.cursor = if self.cursor == 0 {
                        count - 1
                    } else {
                        self.cursor - 1
                    };
                }
                PaletteEvent::None
            }
            "Enter" => {
                let filtered = self.filtered();
                match filtered.get(self.cursor) {
                    Some(item) => PaletteEvent::Select(item.id.clone()),
                    None => PaletteEvent::None,
                }
            }
            "Escape" => PaletteEvent::Close,
            "Backspace" => {
                self.search.pop();
                self.cursor = 0;
                PaletteEvent::None
            }
            ch => {
                if ch.len() <= 4 && !ch.is_empty() && ch.chars().all(|c| !c.is_control()) {
                    self.search.push_str(ch);
                    self.cursor = 0;
                }
                PaletteEvent::None
            }
        }
    }

    /// Returns the visible window of filtered items (max 10, follows cursor).
    pub fn visible_items(&self) -> Vec<&PaletteItem> {
        let filtered = self.filtered();
        let len = filtered.len();
        if len <= 10 {
            return filtered;
        }
        let start = if self.cursor < 10 { 0 } else { self.cursor - 9 };
        filtered[start..start + 10].to_vec()
    }

    /// Returns the unique group names in display order (insertion order, deduped).
    pub fn groups(&self) -> Vec<&str> {
        let mut seen = Vec::new();
        for item in &self.items {
            if !seen.contains(&item.group.as_str()) {
                seen.push(item.group.as_str());
            }
        }
        seen
    }

    /// Current search string.
    pub fn search(&self) -> &str {
        &self.search
    }

    /// Current cursor position in the filtered list.
    pub fn cursor(&self) -> usize {
        self.cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_items() -> Vec<PaletteItem> {
        vec![
            PaletteItem {
                id: "s1".into(),
                label: "feat/login".into(),
                group: "Sessions".into(),
                is_active: false,
            },
            PaletteItem {
                id: "s2".into(),
                label: "fix/header-bug".into(),
                group: "Sessions".into(),
                is_active: true,
            },
            PaletteItem {
                id: "a1".into(),
                label: "Kill session".into(),
                group: "Actions".into(),
                is_active: false,
            },
            PaletteItem {
                id: "a2".into(),
                label: "New session".into(),
                group: "Actions".into(),
                is_active: false,
            },
        ]
    }

    #[test]
    fn filter_matches_label_case_insensitive() {
        let mut state = CommandPaletteState::new(make_items());
        state.search = "login".into();
        let results = state.filtered();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "s1");
    }

    #[test]
    fn filter_empty_search_returns_all() {
        let state = CommandPaletteState::new(make_items());
        assert_eq!(state.filtered().len(), 4);
    }

    #[test]
    fn filter_case_insensitive_uppercase_query() {
        let mut state = CommandPaletteState::new(make_items());
        state.search = "KILL".into();
        let results = state.filtered();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "a1");
    }

    #[test]
    fn arrow_down_advances_cursor() {
        let mut state = CommandPaletteState::new(make_items());
        state.handle_key("ArrowDown");
        assert_eq!(state.cursor, 1);
        state.handle_key("ArrowDown");
        assert_eq!(state.cursor, 2);
    }

    #[test]
    fn arrow_down_wraps_at_end() {
        let mut state = CommandPaletteState::new(make_items());
        state.handle_key("ArrowDown");
        state.handle_key("ArrowDown");
        state.handle_key("ArrowDown");
        assert_eq!(state.cursor, 3);
        state.handle_key("ArrowDown");
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn arrow_up_wraps_at_beginning() {
        let mut state = CommandPaletteState::new(make_items());
        assert_eq!(state.cursor, 0);
        state.handle_key("ArrowUp");
        assert_eq!(state.cursor, 3);
    }

    #[test]
    fn arrow_up_decrements_cursor() {
        let mut state = CommandPaletteState::new(make_items());
        state.cursor = 2;
        state.handle_key("ArrowUp");
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn enter_returns_selected_item_id() {
        let mut state = CommandPaletteState::new(make_items());
        state.handle_key("ArrowDown");
        let result = state.handle_key("Enter");
        assert_eq!(result, PaletteEvent::Select("s2".to_string()));
    }

    #[test]
    fn enter_on_first_item_returns_first_id() {
        let mut state = CommandPaletteState::new(make_items());
        let result = state.handle_key("Enter");
        assert_eq!(result, PaletteEvent::Select("s1".to_string()));
    }

    #[test]
    fn escape_returns_close() {
        let mut state = CommandPaletteState::new(make_items());
        let result = state.handle_key("Escape");
        assert_eq!(result, PaletteEvent::Close);
    }

    fn make_many_items(n: usize) -> Vec<PaletteItem> {
        (0..n)
            .map(|i| PaletteItem {
                id: format!("item{i}"),
                label: format!("Item {i}"),
                group: "All".into(),
                is_active: false,
            })
            .collect()
    }

    #[test]
    fn visible_items_caps_at_10() {
        let state = CommandPaletteState::new(make_many_items(20));
        assert_eq!(state.visible_items().len(), 10);
    }

    #[test]
    fn visible_items_returns_all_when_fewer_than_10() {
        let state = CommandPaletteState::new(make_items());
        assert_eq!(state.visible_items().len(), 4);
    }

    #[test]
    fn visible_items_follows_cursor_down() {
        let mut state = CommandPaletteState::new(make_many_items(20));
        // Move cursor to item 12 (past the initial window of 0..10)
        for _ in 0..12 {
            state.handle_key("ArrowDown");
        }
        let visible = state.visible_items();
        // Cursor should be visible in the window
        assert!(visible.iter().any(|i| i.id == "item12"));
        assert_eq!(visible.len(), 10);
    }

    #[test]
    fn visible_items_follows_cursor_wrapping_to_top() {
        let mut state = CommandPaletteState::new(make_many_items(20));
        // Move cursor to last item, then wrap
        for _ in 0..20 {
            state.handle_key("ArrowDown");
        }
        // cursor is now 0 (wrapped)
        let visible = state.visible_items();
        assert_eq!(visible[0].id, "item0");
    }

    #[test]
    fn filtered_preserves_group_order() {
        let items = vec![
            PaletteItem {
                id: "s1".into(),
                label: "feat/new-login".into(),
                group: "Sessions".into(),
                is_active: false,
            },
            PaletteItem {
                id: "a1".into(),
                label: "New session".into(),
                group: "Actions".into(),
                is_active: false,
            },
            PaletteItem {
                id: "t1".into(),
                label: "New homepage".into(),
                group: "Tasks".into(),
                is_active: false,
            },
        ];
        let mut state = CommandPaletteState::new(items);
        state.search = "new".into();
        let results = state.filtered();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].group, "Sessions");
        assert_eq!(results[1].group, "Actions");
        assert_eq!(results[2].group, "Tasks");
    }

    #[test]
    fn groups_returns_ordered_unique_group_names() {
        let state = CommandPaletteState::new(make_items());
        let groups = state.groups();
        assert_eq!(groups, vec!["Sessions", "Actions"]);
    }

    #[test]
    fn typing_appends_to_search() {
        let mut state = CommandPaletteState::new(make_items());
        state.handle_key("f");
        state.handle_key("e");
        assert_eq!(state.search(), "fe");
    }

    #[test]
    fn typing_resets_cursor_to_zero() {
        let mut state = CommandPaletteState::new(make_items());
        state.handle_key("ArrowDown");
        state.handle_key("ArrowDown");
        assert_eq!(state.cursor, 2);
        state.handle_key("k");
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn typing_filters_immediately() {
        let mut state = CommandPaletteState::new(make_items());
        state.handle_key("k");
        state.handle_key("i");
        state.handle_key("l");
        state.handle_key("l");
        assert_eq!(state.filtered().len(), 1);
        assert_eq!(state.filtered()[0].id, "a1");
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut state = CommandPaletteState::new(make_items());
        state.handle_key("k");
        state.handle_key("i");
        state.handle_key("Backspace");
        assert_eq!(state.search(), "k");
    }

    #[test]
    fn backspace_on_empty_search_does_nothing() {
        let mut state = CommandPaletteState::new(make_items());
        state.handle_key("Backspace");
        assert_eq!(state.search(), "");
        assert_eq!(state.filtered().len(), 4);
    }

    #[test]
    fn backspace_resets_cursor() {
        let mut state = CommandPaletteState::new(make_items());
        state.handle_key("f");
        state.handle_key("ArrowDown");
        state.handle_key("Backspace");
        assert_eq!(state.cursor, 0);
    }
}
