use std::collections::HashSet;
use std::time::{Duration, Instant};

const SHOW_DELAY: Duration = Duration::from_millis(150);

pub struct TabSwitcher {
    cycling: bool,
    cycle_list: Vec<String>,
    index: usize,
    origin: Option<String>,
    cycle_started_at: Option<Instant>,
}

impl Default for TabSwitcher {
    fn default() -> Self {
        Self::new()
    }
}

impl TabSwitcher {
    pub fn new() -> Self {
        Self {
            cycling: false,
            cycle_list: Vec::new(),
            index: 0,
            origin: None,
            cycle_started_at: None,
        }
    }

    pub fn start_cycle(
        &mut self,
        mru: &[String],
        current: Option<&str>,
        valid_ids: Option<&HashSet<String>>,
    ) -> bool {
        let filtered: Vec<&String> = if let Some(ids) = valid_ids {
            mru.iter().filter(|id| ids.contains(id.as_str())).collect()
        } else {
            mru.iter().collect()
        };
        let others: Vec<String> = filtered
            .iter()
            .filter(|id| Some(id.as_str()) != current)
            .map(|id| id.to_string())
            .collect();
        if others.is_empty() {
            return false;
        }
        let current_in_filtered = current.filter(|c| filtered.iter().any(|id| id.as_str() == *c));
        self.cycle_list = if let Some(c) = current_in_filtered {
            let mut list = others;
            list.push(c.to_string());
            list
        } else {
            others
        };
        self.origin = current.map(|s| s.to_string());
        self.index = 0;
        self.cycling = true;
        self.cycle_started_at = Some(Instant::now());
        true
    }

    pub fn is_cycling(&self) -> bool {
        self.cycling
    }

    pub fn is_visible(&self) -> bool {
        self.cycle_started_at
            .map(|t| t.elapsed() >= SHOW_DELAY)
            .unwrap_or(false)
    }

    pub fn cycle_list(&self) -> &[String] {
        &self.cycle_list
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn advance(&mut self, direction: i32) {
        if !self.cycling || self.cycle_list.is_empty() {
            return;
        }
        let len = self.cycle_list.len() as i32;
        self.index = ((self.index as i32 + direction).rem_euclid(len)) as usize;
    }

    pub fn commit(&mut self) -> Option<String> {
        let target = self.cycle_list.get(self.index).cloned();
        self.reset();
        target
    }

    pub fn cancel(&mut self) -> Option<String> {
        let origin = self.origin.take();
        self.reset();
        origin
    }

    fn reset(&mut self) {
        self.cycling = false;
        self.cycle_list.clear();
        self.index = 0;
        self.origin = None;
        self.cycle_started_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_cycle_returns_false_when_no_other_sessions() {
        let mut switcher = TabSwitcher::new();
        let mru = vec!["a".to_string()];
        assert!(!switcher.start_cycle(&mru, Some("a"), None));
        assert!(!switcher.is_cycling());
    }

    #[test]
    fn start_cycle_sets_cycling_and_builds_cycle_list() {
        let mut switcher = TabSwitcher::new();
        let mru = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert!(switcher.start_cycle(&mru, Some("a"), None));
        assert!(switcher.is_cycling());
        assert_eq!(switcher.cycle_list(), &["b", "c", "a"]);
        assert_eq!(switcher.index(), 0);
    }

    #[test]
    fn advance_forward_and_wraps() {
        let mut switcher = TabSwitcher::new();
        let mru = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        switcher.start_cycle(&mru, Some("a"), None);
        switcher.advance(1);
        assert_eq!(switcher.index(), 1);
        switcher.advance(1);
        assert_eq!(switcher.index(), 2);
        switcher.advance(1);
        assert_eq!(switcher.index(), 0); // wraps
    }

    #[test]
    fn advance_backward_and_wraps() {
        let mut switcher = TabSwitcher::new();
        let mru = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        switcher.start_cycle(&mru, Some("a"), None);
        switcher.advance(-1);
        assert_eq!(switcher.index(), 2); // wraps to end
    }

    #[test]
    fn commit_returns_selected_and_resets() {
        let mut switcher = TabSwitcher::new();
        let mru = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        switcher.start_cycle(&mru, Some("a"), None);
        switcher.advance(1);
        let target = switcher.commit();
        assert_eq!(target.as_deref(), Some("c"));
        assert!(!switcher.is_cycling());
        assert_eq!(switcher.cycle_list().len(), 0);
    }

    #[test]
    fn cancel_returns_origin_and_resets() {
        let mut switcher = TabSwitcher::new();
        let mru = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        switcher.start_cycle(&mru, Some("a"), None);
        switcher.advance(1);
        let origin = switcher.cancel();
        assert_eq!(origin.as_deref(), Some("a"));
        assert!(!switcher.is_cycling());
    }

    #[test]
    fn is_visible_false_before_150ms_true_after() {
        let mut switcher = TabSwitcher::new();
        let mru = vec!["a".to_string(), "b".to_string()];
        switcher.start_cycle(&mru, Some("a"), None);
        assert!(!switcher.is_visible());
        std::thread::sleep(std::time::Duration::from_millis(151));
        assert!(switcher.is_visible());
    }

    #[test]
    fn start_cycle_with_valid_ids_filters_stale_entries() {
        let mut switcher = TabSwitcher::new();
        let mru = vec![
            "a".to_string(),
            "b".to_string(),
            "ghost".to_string(),
            "c".to_string(),
        ];
        let valid_ids: HashSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        switcher.start_cycle(&mru, Some("a"), Some(&valid_ids));
        assert_eq!(switcher.cycle_list(), &["b", "c", "a"]);
        assert_eq!(switcher.commit().as_deref(), Some("b"));
    }

    #[test]
    fn start_cycle_with_none_current_uses_full_mru() {
        let mut switcher = TabSwitcher::new();
        let mru = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert!(switcher.start_cycle(&mru, None, None));
        // No current to append, cycle_list = full MRU
        assert_eq!(switcher.cycle_list(), &["a", "b", "c"]);
    }
}
