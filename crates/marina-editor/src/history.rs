//! Undo and redo as a list of diffs, with a memory bound.
//!
//! egui's `TextEdit` undoer keeps up to 100 whole-text snapshots, which the
//! Phase 0 spike measured at about 250 MB for a 2.5 MB note. Here each step
//! stores only what changed (the replaced bytes and their replacement, plus
//! the cursor before and after), and the oldest steps are dropped once the
//! history passes [`MAX_BYTES`] or [`MAX_STEPS`].
//!
//! Typing is grouped as CodeMirror groups it (`newGroupDelay`, 500 ms):
//! consecutive insertions, or consecutive Backspace/Delete presses, that
//! follow on from each other within the delay become one step. Commands
//! (list edits, toggles, paste, replace, completions) are always a step of
//! their own.

use std::collections::VecDeque;

/// Steps kept at most.
pub const MAX_STEPS: usize = 1000;
/// Bytes of changed text kept at most (both directions).
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
/// Typing within this many seconds of the previous keystroke joins its step.
pub const GROUP_DELAY: f64 = 0.5;

/// A cursor: (anchor, head), in characters.
pub type Cursor = (usize, usize);

/// One undoable change: bytes `at..at + removed.len()` of the old text were
/// replaced by `inserted`.
#[derive(Clone, Debug, PartialEq)]
struct Step {
    at: usize,
    removed: String,
    inserted: String,
    before: Cursor,
    after: Cursor,
    time: f64,
    /// Joinable with the next keystroke.
    typing: bool,
}

impl Step {
    fn bytes(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }
}

#[derive(Default)]
pub struct History {
    undos: VecDeque<Step>,
    redos: Vec<Step>,
    bytes: usize,
}

impl History {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn can_undo(&self) -> bool {
        !self.undos.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redos.is_empty()
    }

    /// Bytes held (for tests and the benchmark).
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Records the change from `old` to `new`. `typing` steps may join the
    /// previous step; commands never do.
    pub fn record(
        &mut self,
        old: &str,
        new: &str,
        before: Cursor,
        after: Cursor,
        time: f64,
        typing: bool,
    ) {
        let Some((at, removed, inserted)) = diff(old, new) else {
            return;
        };
        self.redos.clear();
        let step = Step {
            at,
            removed: removed.to_owned(),
            inserted: inserted.to_owned(),
            before,
            after,
            time,
            typing,
        };
        if typing && self.join(&step) {
            return;
        }
        self.bytes += step.bytes();
        self.undos.push_back(step);
        while self.undos.len() > MAX_STEPS || (self.bytes > MAX_BYTES && self.undos.len() > 1) {
            if let Some(old) = self.undos.pop_front() {
                self.bytes -= old.bytes();
            }
        }
    }

    /// Joins `step` into the last one when it continues the same typing run.
    fn join(&mut self, step: &Step) -> bool {
        let Some(last) = self.undos.back_mut() else {
            return false;
        };
        if !last.typing || step.time - last.time > GROUP_DELAY || step.inserted.contains('\n') {
            return false;
        }
        let insertion = step.removed.is_empty() && last.removed.is_empty();
        let continues_insert = insertion && step.at == last.at + last.inserted.len();
        let deletion = step.inserted.is_empty() && last.inserted.is_empty();
        // Backspace: the new deletion ends where the last one started.
        let backspace = deletion && step.at + step.removed.len() == last.at;
        // Delete: same start, the text after moves in.
        let delete = deletion && step.at == last.at;
        if !(continues_insert || backspace || delete) {
            return false;
        }
        self.bytes += step.bytes();
        if continues_insert {
            last.inserted.push_str(&step.inserted);
        } else if backspace {
            last.removed.insert_str(0, &step.removed);
            last.at = step.at;
        } else {
            last.removed.push_str(&step.removed);
        }
        last.after = step.after;
        last.time = step.time;
        true
    }

    /// Undoes the last step on `text`; returns the cursor to restore.
    pub fn undo(&mut self, text: &mut String) -> Option<Cursor> {
        let step = self.undos.pop_back()?;
        self.bytes -= step.bytes();
        text.replace_range(step.at..step.at + step.inserted.len(), &step.removed);
        let cursor = step.before;
        self.redos.push(step);
        Some(cursor)
    }

    /// Redoes the last undone step on `text`; returns the cursor to restore.
    pub fn redo(&mut self, text: &mut String) -> Option<Cursor> {
        let mut step = self.redos.pop()?;
        text.replace_range(step.at..step.at + step.removed.len(), &step.inserted);
        let cursor = step.after;
        // A redone step never joins later typing.
        step.typing = false;
        self.bytes += step.bytes();
        self.undos.push_back(step);
        Some(cursor)
    }
}

/// The change from `old` to `new` as (byte offset, removed, inserted), at
/// character boundaries; `None` when they're equal.
fn diff<'a>(old: &'a str, new: &'a str) -> Option<(usize, &'a str, &'a str)> {
    if old == new {
        return None;
    }
    let (o, n) = (old.as_bytes(), new.as_bytes());
    let mut p = o.iter().zip(n).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(p) || !new.is_char_boundary(p) {
        p -= 1;
    }
    let max_s = o.len().min(n.len()) - p;
    let mut s = o
        .iter()
        .rev()
        .zip(n.iter().rev())
        .take(max_s)
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(o.len() - s) || !new.is_char_boundary(n.len() - s) {
        s -= 1;
    }
    Some((p, &old[p..o.len() - s], &new[p..n.len() - s]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(h: &mut History, text: &mut String, at: usize, s: &str, time: f64) {
        let old = text.clone();
        text.insert_str(at, s);
        h.record(
            &old,
            text,
            (at, at),
            (at + s.len(), at + s.len()),
            time,
            true,
        );
    }

    #[test]
    fn typing_groups_and_undoes_in_one_step() {
        let mut h = History::default();
        let mut text = String::from("ab");
        typed(&mut h, &mut text, 2, "c", 0.0);
        typed(&mut h, &mut text, 3, "d", 0.2);
        typed(&mut h, &mut text, 4, "e", 0.4);
        // A pause starts a new step.
        typed(&mut h, &mut text, 5, "f", 2.0);
        assert_eq!(text, "abcdef");
        assert_eq!(h.undo(&mut text), Some((5, 5)));
        assert_eq!(text, "abcde");
        assert_eq!(h.undo(&mut text), Some((2, 2)));
        assert_eq!(text, "ab");
        assert_eq!(h.redo(&mut text), Some((5, 5)));
        assert_eq!(text, "abcde");
        assert_eq!(h.redo(&mut text), Some((6, 6)));
        assert_eq!(text, "abcdef");
        assert!(!h.can_redo());
    }

    #[test]
    fn backspaces_group() {
        let mut h = History::default();
        let mut text = String::from("hello");
        for (i, t) in [(4, 0.0), (3, 0.1), (2, 0.2)] {
            let old = text.clone();
            text.remove(i);
            h.record(&old, &text, (i + 1, i + 1), (i, i), t, true);
        }
        assert_eq!(text, "he");
        h.undo(&mut text);
        assert_eq!(text, "hello");
    }

    #[test]
    fn commands_are_their_own_step() {
        let mut h = History::default();
        let mut text = String::from("a");
        typed(&mut h, &mut text, 1, "b", 0.0);
        let old = text.clone();
        text = "- ab".into();
        h.record(&old, &text, (2, 2), (4, 4), 0.1, false);
        typed(&mut h, &mut text, 4, "c", 0.2);
        h.undo(&mut text);
        assert_eq!(text, "- ab");
        h.undo(&mut text);
        assert_eq!(text, "ab");
    }

    #[test]
    fn memory_is_bounded() {
        let mut h = History::default();
        let big = "x".repeat(MAX_BYTES / 4);
        let mut text = String::new();
        for i in 0..10 {
            let old = text.clone();
            text.push_str(&big);
            h.record(&old, &text, (0, 0), (0, 0), f64::from(i) * 10.0, false);
        }
        assert!(h.bytes() <= MAX_BYTES, "{}", h.bytes());
        assert!(h.can_undo());
    }

    #[test]
    fn diff_respects_char_boundaries() {
        assert_eq!(diff("aéb", "aèb"), Some((1, "é", "è")));
        assert_eq!(diff("same", "same"), None);
        assert_eq!(diff("ab", "abab"), Some((2, "", "ab")));
    }
}
