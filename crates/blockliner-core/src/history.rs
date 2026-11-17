//! Undo and redo, as whole-diagram snapshots. Diagrams are small, so a
//! snapshot per step is simpler than an operation log and can't drift out of
//! step with the model.
//!
//! The app calls [`History::observe`] whenever the canvas is idle (no drag in
//! progress, no label being edited). Anything that changed since the last
//! observation becomes one undo step, so a drag, or creating a shape and
//! typing its label, undoes in one go without each edit having to say so.
//!
//! The history is bounded in steps ([`LIMIT`]) and in memory
//! ([`MAX_BYTES`]). A snapshot's size is taken as the length of the diagram
//! written as text, worked out once when the step is recorded.

use std::collections::VecDeque;

use crate::model::Diagram;

/// How many steps are kept.
pub const LIMIT: usize = 200;
/// Bytes of snapshots kept at most, as the undo steps' written size.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;

/// A snapshot and its size.
type Step = (Diagram, usize);

pub struct History {
    undo: VecDeque<Step>,
    redo: Vec<Step>,
    /// The diagram as last observed, and its size.
    stable: Diagram,
    stable_size: usize,
    /// The undo steps' sizes, added up.
    bytes: usize,
}

fn size(d: &Diagram) -> usize {
    crate::write(d).len()
}

impl History {
    pub fn new(d: &Diagram) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            stable: d.clone(),
            stable_size: size(d),
            bytes: 0,
        }
    }

    /// Starts over from `d` (a diagram was opened).
    pub fn reset(&mut self, d: &Diagram) {
        *self = Self::new(d);
    }

    /// Records a step if `d` changed since the last observation. Returns
    /// whether it did.
    pub fn observe(&mut self, d: &Diagram) -> bool {
        if *d == self.stable {
            return false;
        }
        let before = std::mem::replace(&mut self.stable, d.clone());
        let before_size = std::mem::replace(&mut self.stable_size, size(d));
        self.push(before, before_size);
        self.redo.clear();
        true
    }

    fn push(&mut self, d: Diagram, size: usize) {
        self.bytes += size;
        self.undo.push_back((d, size));
        while self.undo.len() > LIMIT || (self.bytes > MAX_BYTES && self.undo.len() > 1) {
            self.drop_oldest();
        }
    }

    fn drop_oldest(&mut self) -> bool {
        match self.undo.pop_front() {
            Some((_, size)) => {
                self.bytes -= size;
                true
            }
            None => false,
        }
    }

    /// Puts `d` back one step. `false` if there was nothing to undo.
    pub fn undo(&mut self, d: &mut Diagram) -> bool {
        self.observe(d);
        let Some((before, before_size)) = self.undo.pop_back() else {
            return false;
        };
        self.bytes -= before_size;
        let after = std::mem::replace(&mut self.stable, before.clone());
        let after_size = std::mem::replace(&mut self.stable_size, before_size);
        self.redo.push((after, after_size));
        *d = before;
        true
    }

    /// Re-applies the last undone step. `false` if there was none.
    pub fn redo(&mut self, d: &mut Diagram) -> bool {
        if self.observe(d) {
            return false; // A new change since the undo ends the redo trail.
        }
        let Some((after, after_size)) = self.redo.pop() else {
            return false;
        };
        let before = std::mem::replace(&mut self.stable, after.clone());
        let before_size = std::mem::replace(&mut self.stable_size, after_size);
        self.push(before, before_size);
        *d = after;
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Bytes held by the undo steps.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Drops the oldest undo steps until at most `max` bytes are held (the
    /// app's limit across every open diagram and note).
    pub fn trim_to(&mut self, max: usize) {
        while self.bytes > max && self.drop_oldest() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Rect, ShapeKind};

    fn with(n: usize) -> Diagram {
        let mut d = Diagram::new();
        for i in 0..n {
            d.add_shape(ShapeKind::Rect, Rect::new(i as f32, 0.0, 10.0, 10.0), "s");
        }
        d
    }

    #[test]
    fn undo_and_redo() {
        let mut d = with(0);
        let mut h = History::new(&d);
        assert!(!h.observe(&d), "no change, no step");
        d = with(1);
        assert!(h.observe(&d));
        d = with(2);
        // Not observed yet: undo records it first, then steps back over it.
        assert!(h.undo(&mut d));
        assert_eq!(d, with(1));
        assert!(h.undo(&mut d));
        assert_eq!(d, with(0));
        assert!(!h.undo(&mut d));
        assert!(h.redo(&mut d));
        assert!(h.redo(&mut d));
        assert_eq!(d, with(2));
        assert!(!h.redo(&mut d));
        assert!(h.can_undo() && !h.can_redo());
    }

    #[test]
    fn a_new_change_drops_the_redo_trail() {
        let mut d = with(1);
        let mut h = History::new(&d);
        d = with(2);
        h.observe(&d);
        h.undo(&mut d);
        assert!(h.can_redo());
        d = with(3);
        assert!(!h.redo(&mut d));
        assert_eq!(d, with(3));
        assert!(!h.can_redo());
        assert!(h.undo(&mut d));
        assert_eq!(d, with(1));
    }

    #[test]
    fn keeps_at_most_the_limit() {
        let mut d = with(0);
        let mut h = History::new(&d);
        for _ in 0..LIMIT + 10 {
            d.add_shape(ShapeKind::Rect, Rect::default(), "x");
            h.observe(&d);
        }
        let mut n = 0;
        while h.undo(&mut d) {
            n += 1;
        }
        assert_eq!(n, LIMIT);
        assert_eq!(d.shapes.len(), 10);
    }

    #[test]
    fn bytes_follow_the_steps_and_trim_drops_the_oldest() {
        let mut d = with(0);
        let mut h = History::new(&d);
        assert_eq!(h.bytes(), 0);
        for n in 1..=4 {
            d = with(n);
            h.observe(&d);
        }
        let all = h.bytes();
        assert!(all > 0);
        // Undo moves a step to the redo trail, and redo brings it back.
        h.undo(&mut d);
        assert!(h.bytes() < all);
        h.redo(&mut d);
        assert_eq!(h.bytes(), all);
        // Trimming drops the oldest: the newest steps still undo.
        h.trim_to(all / 2);
        assert!(h.bytes() <= all / 2);
        assert!(h.undo(&mut d));
        assert_eq!(d, with(3));
        h.trim_to(0);
        assert_eq!(h.bytes(), 0);
        assert!(!h.can_undo());
    }
}
