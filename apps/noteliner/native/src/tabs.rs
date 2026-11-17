//! The open tabs: which files are open above the document, in what order,
//! and the state of the ones in the background.
//!
//! The active tab is the project's selected file; its state lives in the
//! app's editor. Leaving a tab stores that state here ([`Stashed`]) and
//! entering one takes it back, so each tab keeps its own undo history, caret
//! and scroll while it's open. Closing a tab keeps them for Reopen Closed
//! Tab, for the last [`CLOSED_KEPT`] tabs closed.
//!
//! The histories of all of these share one memory limit
//! ([`HISTORY_BUDGET`]): past it the oldest steps go, from the closed tabs
//! first, then from the background tabs, the one shown longest ago first.

use std::collections::HashMap;

use crate::diagram::DiagramStash;
use crate::editor::NoteEditor;

/// How many closed tabs Reopen Closed Tab can bring back.
pub const CLOSED_KEPT: usize = 10;
/// Bytes of undo history kept at most, over the active tab, the background
/// tabs and the closed tabs.
pub const HISTORY_BUDGET: usize = 64 * 1024 * 1024;

/// A background tab's state.
pub enum Stashed {
    Note(Box<NoteStash>),
    /// A diagram: its undo history and its pan and zoom.
    Diagram(Box<DiagramStash>),
}

impl Stashed {
    /// Bytes held by its undo history.
    pub fn history_bytes(&self) -> usize {
        match self {
            Stashed::Note(n) => n.editor.history_bytes(),
            Stashed::Diagram(d) => d.history_bytes(),
        }
    }

    /// Drops its oldest undo steps until at most `max` bytes are held.
    pub fn trim_history(&mut self, max: usize) {
        match self {
            Stashed::Note(n) => n.editor.trim_history(max),
            Stashed::Diagram(d) => d.trim_history(max),
        }
    }
}

/// A note in a background tab: its editor (text, undo history, save status),
/// its frontmatter, and where the caret and scroll were.
pub struct NoteStash {
    pub editor: NoteEditor,
    pub anchor: usize,
    pub head: usize,
    pub scroll: egui::Vec2,
}

/// The tab strip's files, by id, and the background tabs' state.
#[derive(Default)]
pub struct Tabs {
    order: Vec<String>,
    stash: HashMap<String, Stashed>,
    /// The background tabs with state, from the one left longest ago.
    age: Vec<String>,
    /// The tabs closed last, with their state, the latest at the end.
    closed: Vec<(String, Stashed)>,
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
        self.age.clear();
        self.closed.clear();
    }

    /// Gives `id` a tab if it has none: at the end, or in `replace`'s place
    /// (Previous File and Next File reuse the active tab). Returns the tab
    /// that was replaced.
    pub fn open(&mut self, id: &str, replace: Option<&str>) -> Option<String> {
        self.reveal = true;
        if self.contains(id) {
            return None;
        }
        // Read afresh: what was kept from its closed tab is out of date.
        self.closed.retain(|(c, _)| c != id);
        match replace.and_then(|r| self.order.iter().position(|t| t == r)) {
            Some(i) => {
                let old = std::mem::replace(&mut self.order[i], id.to_string());
                self.stash.remove(&old);
                self.age.retain(|a| *a != old);
                Some(old)
            }
            None => {
                self.order.push(id.to_string());
                None
            }
        }
    }

    /// Closes `id`'s tab, keeping its state for [`Tabs::reopen`]. Returns
    /// the tab to show if it was the active one: the one to its right, else
    /// the one to its left.
    pub fn close(&mut self, id: &str) -> Option<String> {
        let next = self.remove(id);
        if let Some(state) = self.stash.remove(id) {
            self.closed.push((id.to_string(), state));
            if self.closed.len() > CLOSED_KEPT {
                self.closed.remove(0);
            }
        }
        next
    }

    /// Closes the tab of a file that is gone, and drops its state. Returns
    /// the tab to show, as [`Tabs::close`].
    pub fn forget(&mut self, id: &str) -> Option<String> {
        let next = self.remove(id);
        self.stash.remove(id);
        self.closed.retain(|(c, _)| c != id);
        next
    }

    /// Takes `id` out of the strip (its state stays in `stash` for the
    /// caller), and returns its neighbour.
    fn remove(&mut self, id: &str) -> Option<String> {
        self.age.retain(|a| a != id);
        let i = self.order.iter().position(|t| t == id)?;
        self.order.remove(i);
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
            self.age.retain(|a| a != id);
            self.age.push(id.to_string());
        }
    }

    pub fn take(&mut self, id: &str) -> Option<Stashed> {
        self.age.retain(|a| a != id);
        self.stash.remove(id)
    }

    /// How many closed tabs could be reopened.
    pub fn closed_len(&self) -> usize {
        self.closed.len()
    }

    /// Gives the tab closed last its place back, at the end of the strip,
    /// with its state in the background. A file for which `exists` is false
    /// (it was deleted since) is skipped for the one closed before it.
    /// Returns the file to show.
    pub fn reopen(&mut self, exists: impl Fn(&str) -> bool) -> Option<String> {
        while let Some((id, state)) = self.closed.pop() {
            if !exists(&id) || self.contains(&id) {
                continue;
            }
            self.order.push(id.clone());
            self.put(&id, state);
            self.reveal = true;
            return Some(id);
        }
        None
    }

    /// Bytes held by the undo histories of the background and closed tabs.
    pub fn history_bytes(&self) -> usize {
        let closed = self.closed.iter().map(|(_, s)| s.history_bytes());
        self.stash
            .values()
            .map(Stashed::history_bytes)
            .chain(closed)
            .sum()
    }

    /// Keeps the histories within `budget` bytes, counting the `active`
    /// tab's: the oldest steps go, from the closed tabs first (the one
    /// closed longest ago first), then from the background tabs (the one
    /// left longest ago first). Returns the bytes the active tab still has
    /// to give up.
    pub fn trim_histories(&mut self, active: usize, budget: usize) -> usize {
        let mut total = active + self.history_bytes();
        let mut trim = |state: &mut Stashed| {
            if total > budget {
                let held = state.history_bytes();
                state.trim_history(held.saturating_sub(total - budget));
                total -= held - state.history_bytes();
            }
        };
        for (_, state) in &mut self.closed {
            trim(state);
        }
        for id in &self.age {
            if let Some(state) = self.stash.get_mut(id) {
                trim(state);
            }
        }
        total.saturating_sub(budget)
    }

    /// The notes' editors in the background and closed tabs (a save
    /// finished, a file was renamed).
    pub fn editors_mut(&mut self) -> impl Iterator<Item = &mut NoteEditor> {
        let closed = self.closed.iter_mut().map(|(_, s)| s);
        self.stash
            .values_mut()
            .chain(closed)
            .filter_map(|s| match s {
                Stashed::Note(n) => Some(&mut n.editor),
                Stashed::Diagram(_) => None,
            })
    }

    /// The diagrams in the background and closed tabs (a save finished, a
    /// file was renamed).
    pub fn diagrams_mut(&mut self) -> impl Iterator<Item = &mut DiagramStash> {
        let closed = self.closed.iter_mut().map(|(_, s)| s);
        self.stash
            .values_mut()
            .chain(closed)
            .filter_map(|s| match s {
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

    /// A note's tab state whose undo history holds `typed` bytes, in one
    /// step.
    fn note(id: &str, typed: usize) -> Stashed {
        let mut editor = NoteEditor::default();
        editor.load(id, id, &format!("{id}.md"), String::new());
        editor.type_file(&"x".repeat(typed), 0.0);
        Stashed::Note(Box::new(NoteStash {
            editor,
            anchor: 0,
            head: 0,
            scroll: egui::Vec2::ZERO,
        }))
    }

    #[test]
    fn a_closed_tab_is_reopened_with_its_state() {
        let mut t = tabs(&["a", "b", "c"]);
        t.put("b", note("b", 10));
        assert_eq!(t.close("b"), Some("c".into()));
        assert_eq!(t.closed_len(), 1);
        assert_eq!(t.history_bytes(), 10);
        // It comes back at the end of the strip, with its history.
        assert_eq!(t.reopen(|_| true), Some("b".into()));
        assert_eq!(t.order(), ["a", "c", "b"]);
        assert_eq!(t.closed_len(), 0);
        assert_eq!(t.take("b").unwrap().history_bytes(), 10);
        assert_eq!(t.reopen(|_| true), None);

        // A file deleted since is skipped for the one closed before it.
        t.put("a", note("a", 1));
        t.put("c", note("c", 1));
        t.close("a");
        t.close("c");
        assert_eq!(t.reopen(|id| id != "c"), Some("a".into()));
        assert_eq!(t.closed_len(), 0);

        // The tab of a file that is gone keeps nothing.
        t.put("a", note("a", 1));
        t.forget("a");
        assert_eq!((t.closed_len(), t.history_bytes()), (0, 0));

        // Opened again from the tree, a file is read afresh.
        t.put("b", note("b", 1));
        t.close("b");
        t.open("b", None);
        assert_eq!(t.closed_len(), 0);
    }

    #[test]
    fn only_the_last_closed_tabs_are_kept() {
        let ids: Vec<String> = (0..CLOSED_KEPT + 2).map(|i| format!("n{i}")).collect();
        let mut t = Tabs::default();
        t.restore(ids.clone());
        for id in &ids {
            t.put(id, note(id, 1));
            t.close(id);
        }
        assert_eq!(t.closed_len(), CLOSED_KEPT);
        // The latest closed comes back first.
        assert_eq!(t.reopen(|_| true), ids.last().cloned());
    }

    #[test]
    fn the_budget_takes_from_closed_tabs_then_the_oldest_background_tab() {
        let mut t = tabs(&["a", "b", "c"]);
        t.put("a", note("a", 100));
        t.put("b", note("b", 100));
        t.put("c", note("c", 100));
        t.close("c");
        // Within the budget: nothing goes.
        assert_eq!(t.trim_histories(50, 350), 0);
        assert_eq!(t.history_bytes(), 300);
        // Over it: the closed tab gives up its history first.
        assert_eq!(t.trim_histories(50, 300), 0);
        assert_eq!(t.history_bytes(), 200);
        // Then the background tab left longest ago.
        assert_eq!(t.trim_histories(50, 150), 0);
        assert_eq!(t.history_bytes(), 100);
        assert_eq!(t.take("a").unwrap().history_bytes(), 0);
        // What is still over is the active tab's to give up.
        assert_eq!(t.trim_histories(50, 20), 30);
        assert_eq!(t.history_bytes(), 0);
    }
}
