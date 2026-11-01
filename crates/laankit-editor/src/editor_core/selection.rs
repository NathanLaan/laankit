//! The core engine's selection set: one or more ranges (carets when empty),
//! in character offsets, with one primary. Kept sorted and non-overlapping,
//! as CodeMirror's `EditorSelection` is.

/// One range: `anchor` stays put while `head` (the caret) moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sel {
    pub anchor: usize,
    pub head: usize,
}

impl Sel {
    pub fn caret(at: usize) -> Self {
        Self {
            anchor: at,
            head: at,
        }
    }

    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    pub fn from(&self) -> usize {
        self.anchor.min(self.head)
    }

    pub fn to(&self) -> usize {
        self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// Moves the head; collapses to it unless `extend`.
    pub fn moved(&self, head: usize, extend: bool) -> Self {
        if extend {
            Self::new(self.anchor, head)
        } else {
            Self::caret(head)
        }
    }
}

/// The ranges, sorted by start and never overlapping, and the primary one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selections {
    ranges: Vec<Sel>,
    primary: usize,
}

impl Default for Selections {
    fn default() -> Self {
        Self::single(Sel::caret(0))
    }
}

impl Selections {
    pub fn single(sel: Sel) -> Self {
        Self {
            ranges: vec![sel],
            primary: 0,
        }
    }

    /// `ranges` in any order, with `primary` an index into them; overlapping
    /// ranges are merged.
    pub fn new(ranges: Vec<Sel>, primary: usize) -> Self {
        let mut s = Self {
            primary: primary.min(ranges.len().saturating_sub(1)),
            ranges,
        };
        if s.ranges.is_empty() {
            s.ranges.push(Sel::caret(0));
        }
        s.normalize();
        s
    }

    pub fn ranges(&self) -> &[Sel] {
        &self.ranges
    }

    pub fn primary(&self) -> Sel {
        self.ranges[self.primary]
    }

    pub fn primary_index(&self) -> usize {
        self.primary
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub fn is_multi(&self) -> bool {
        self.ranges.len() > 1
    }

    /// Adds a range and makes it primary.
    pub fn push(&mut self, sel: Sel) {
        self.ranges.push(sel);
        self.primary = self.ranges.len() - 1;
        self.normalize();
    }

    /// Keeps only the primary range.
    pub fn collapse_to_primary(&mut self) {
        let p = self.primary();
        *self = Self::single(p);
    }

    /// Applies `f` to every range (the primary stays primary), then merges.
    pub fn map(&mut self, mut f: impl FnMut(Sel) -> Sel) {
        for r in &mut self.ranges {
            *r = f(*r);
        }
        self.normalize();
    }

    /// Clamps every offset to `len` characters.
    pub fn clamp(&mut self, len: usize) {
        self.map(|s| Sel::new(s.anchor.min(len), s.head.min(len)));
    }

    /// Sorts and merges overlapping (or, for carets, coinciding) ranges,
    /// keeping track of the primary.
    fn normalize(&mut self) {
        let primary = self.ranges[self.primary];
        let mut order: Vec<usize> = (0..self.ranges.len()).collect();
        order.sort_by_key(|&i| (self.ranges[i].from(), self.ranges[i].to()));
        let mut out: Vec<Sel> = Vec::with_capacity(self.ranges.len());
        let mut new_primary = 0;
        for i in order {
            let r = self.ranges[i];
            let merge = out.last().is_some_and(|last: &Sel| {
                r.from() < last.to() || (r.from() == last.to() && (r.is_empty() || last.is_empty()))
            });
            if merge {
                let last = out.last_mut().expect("checked");
                let (from, to) = (last.from(), last.to().max(r.to()));
                // Keep the direction of the range the head belongs to.
                *last = if last.head >= last.anchor {
                    Sel::new(from, to)
                } else {
                    Sel::new(to, from)
                };
                if i == self.primary || r == primary {
                    new_primary = out.len() - 1;
                }
            } else {
                if i == self.primary {
                    new_primary = out.len();
                }
                out.push(r);
            }
        }
        self.ranges = out;
        self.primary = new_primary.min(self.ranges.len() - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorted_and_merged() {
        let s = Selections::new(vec![Sel::caret(9), Sel::new(2, 5), Sel::new(4, 7)], 0);
        assert_eq!(s.ranges(), &[Sel::new(2, 7), Sel::caret(9)]);
        assert_eq!(s.primary(), Sel::caret(9));
        let s = Selections::new(vec![Sel::caret(3), Sel::caret(3)], 1);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn push_makes_primary_and_collapse_keeps_it() {
        let mut s = Selections::single(Sel::caret(1));
        s.push(Sel::new(10, 12));
        assert!(s.is_multi());
        assert_eq!(s.primary(), Sel::new(10, 12));
        s.collapse_to_primary();
        assert_eq!(s.ranges(), &[Sel::new(10, 12)]);
    }

    #[test]
    fn map_merges_carets_that_meet() {
        let mut s = Selections::new(vec![Sel::caret(1), Sel::caret(2)], 1);
        s.map(|r| Sel::caret(r.head.saturating_sub(1).max(1)));
        assert_eq!(s.ranges(), &[Sel::caret(1)]);
        assert_eq!(s.primary(), Sel::caret(1));
    }
}
