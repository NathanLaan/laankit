//! Undo and redo, as whole-diagram snapshots. Diagrams are small, so a
//! snapshot per step is simpler than an operation log and can't drift out of
//! step with the model.
//!
//! The app calls [`History::observe`] whenever the canvas is idle (no drag in
//! progress, no label being edited). Anything that changed since the last
//! observation becomes one undo step, so a drag, or creating a shape and
//! typing its label, undoes in one go without each edit having to say so.

use crate::model::Diagram;

/// How many steps are kept.
pub const LIMIT: usize = 200;

pub struct History {
    undo: Vec<Diagram>,
    redo: Vec<Diagram>,
    /// The diagram as last observed.
    stable: Diagram,
}

impl History {
    pub fn new(d: &Diagram) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            stable: d.clone(),
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
        self.undo.push(before);
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        true
    }

    /// Puts `d` back one step. `false` if there was nothing to undo.
    pub fn undo(&mut self, d: &mut Diagram) -> bool {
        self.observe(d);
        let Some(before) = self.undo.pop() else {
            return false;
        };
        self.redo
            .push(std::mem::replace(&mut self.stable, before.clone()));
        *d = before;
        true
    }

    /// Re-applies the last undone step. `false` if there was none.
    pub fn redo(&mut self, d: &mut Diagram) -> bool {
        if self.observe(d) {
            return false; // A new change since the undo ends the redo trail.
        }
        let Some(after) = self.redo.pop() else {
            return false;
        };
        self.undo
            .push(std::mem::replace(&mut self.stable, after.clone()));
        *d = after;
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
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
}
