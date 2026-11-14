//! The open tabs: which files are open above the document, in what order,
//! and the state of the ones in the background.
//!
//! The active tab is the project's selected file; its state lives in the
//! app's editor. Leaving a tab stores that state here ([`Stashed`]) and
//! entering one takes it back, so each tab keeps its own undo history, caret
//! and scroll while it's open. Closing a tab drops them.

use std::collections::HashMap;

use serde_json::Value;

use crate::diagram::DiagramStash;
use crate::editor::NoteEditor;

/// A background tab's state.
pub enum Stashed {
    Note(Box<NoteStash>),
    /// A diagram: its undo history and its pan and zoom.
    Diagram(Box<DiagramStash>),
}

/// A note in a background tab: its editor (text, undo history, save status),
/// its frontmatter, and where the caret and scroll were.
pub struct NoteStash {
    pub editor: NoteEditor,
    pub frontmatter: Value,
    pub anchor: usize,
    pub head: usize,
    pub scroll: egui::Vec2,
}

/// The tab strip's files, by id, and the background tabs' state.
#[derive(Default)]
pub struct Tabs {
    order: Vec<String>,
    stash: HashMap<String, Stashed>,
    /// Scroll the active tab into view on the next frame.
    pub reveal: bool,
}

impl Tabs {
    pub fn order(&self) -> &[String] {
        &self.order
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.order.iter().any(|t| t == id)
    }

    /// The saved tabs, as a project reopens.
    pub fn restore(&mut self, order: Vec<String>) {
        self.clear();
        for id in order {
            if !self.contains(&id) {
                self.order.push(id);
            }
        }
    }

    pub fn clear(&mut self) {
        self.order.clear();
        self.stash.clear();
    }

    /// Gives `id` a tab if it has none: at the end, or in `replace`'s place
    /// (Previous File and Next File reuse the active tab). Returns the tab
    /// that was replaced.
    pub fn open(&mut self, id: &str, replace: Option<&str>) -> Option<String> {
        self.reveal = true;
        if self.contains(id) {
            return None;
        }
        match replace.and_then(|r| self.order.iter().position(|t| t == r)) {
            Some(i) => {
                let old = std::mem::replace(&mut self.order[i], id.to_string());
                self.stash.remove(&old);
                Some(old)
            }
            None => {
                self.order.push(id.to_string());
                None
            }
        }
    }

    /// Closes `id`'s tab. Returns the tab to show if it was the active one:
    /// the one to its right, else the one to its left.
    pub fn close(&mut self, id: &str) -> Option<String> {
        let i = self.order.iter().position(|t| t == id)?;
        self.order.remove(i);
        self.stash.remove(id);
        self.order
            .get(i)
            .or_else(|| i.checked_sub(1).and_then(|l| self.order.get(l)))
            .cloned()
    }

    /// The tab `delta` places from `active`, wrapping around.
    pub fn step(&self, active: Option<&str>, delta: isize) -> Option<String> {
        let n = self.order.len() as isize;
        if n < 2 {
            return None;
        }
        let i = active.and_then(|a| self.order.iter().position(|t| t == a))? as isize;
        self.order.get((i + delta).rem_euclid(n) as usize).cloned()
    }

    pub fn put(&mut self, id: &str, state: Stashed) {
        if self.contains(id) {
            self.stash.insert(id.to_string(), state);
        }
    }

    pub fn take(&mut self, id: &str) -> Option<Stashed> {
        self.stash.remove(id)
    }

    /// The background notes' editors (a save finished, a file was renamed).
    pub fn editors_mut(&mut self) -> impl Iterator<Item = &mut NoteEditor> {
        self.stash.values_mut().filter_map(|s| match s {
            Stashed::Note(n) => Some(&mut n.editor),
            Stashed::Diagram(_) => None,
        })
    }

    /// The background diagrams (a save finished, a file was renamed).
    pub fn diagrams_mut(&mut self) -> impl Iterator<Item = &mut DiagramStash> {
        self.stash.values_mut().filter_map(|s| match s {
            Stashed::Diagram(d) => Some(&mut **d),
            Stashed::Note(_) => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(ids: &[&str]) -> Tabs {
        let mut t = Tabs::default();
        t.restore(ids.iter().map(|s| s.to_string()).collect());
        t
    }

    #[test]
    fn opening_adds_at_the_end_or_in_place() {
        let mut t = tabs(&["a", "b"]);
        assert_eq!(t.open("c", None), None);
        assert_eq!(t.order(), ["a", "b", "c"]);
        // Already open: nothing moves, with or without a tab to replace.
        assert_eq!(t.open("a", Some("b")), None);
        assert_eq!(t.order(), ["a", "b", "c"]);
        // Previous File and Next File reuse the active tab.
        assert_eq!(t.open("d", Some("b")), Some("b".into()));
        assert_eq!(t.order(), ["a", "d", "c"]);
        // Nothing to replace: a new tab.
        assert_eq!(t.open("e", Some("zzz")), None);
        assert_eq!(t.order(), ["a", "d", "c", "e"]);
    }

    #[test]
    fn closing_picks_the_right_then_the_left_neighbour() {
        let mut t = tabs(&["a", "b", "c"]);
        assert_eq!(t.close("b"), Some("c".into()));
        assert_eq!(t.close("c"), Some("a".into()));
        assert_eq!(t.close("a"), None);
        assert_eq!(t.close("a"), None);
        assert_eq!(t.len(), 0);
    }

    #[test]
    fn stepping_wraps() {
        let t = tabs(&["a", "b", "c"]);
        assert_eq!(t.step(Some("c"), 1), Some("a".into()));
        assert_eq!(t.step(Some("a"), -1), Some("c".into()));
        assert_eq!(t.step(Some("a"), 1), Some("b".into()));
        assert_eq!(t.step(None, 1), None);
        assert_eq!(tabs(&["a"]).step(Some("a"), 1), None);
    }

    #[test]
    fn restore_drops_duplicates() {
        assert_eq!(tabs(&["a", "b", "a"]).order(), ["a", "b"]);
    }
}
