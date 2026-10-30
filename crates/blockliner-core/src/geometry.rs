//! Rectangle geometry, the resize handles and hit-testing.

use crate::model::{Diagram, Point, Rect, Shape};
use crate::snap::snap;

impl Rect {
    /// The rectangle spanned by two opposite corners, in any order.
    pub fn from_corners(a: Point, b: Point) -> Self {
        Self::new(
            a.x.min(b.x),
            a.y.min(b.y),
            (a.x - b.x).abs(),
            (a.y - b.y).abs(),
        )
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// Edges included.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }

    pub fn contains_rect(&self, r: &Rect) -> bool {
        r.x >= self.x && r.right() <= self.right() && r.y >= self.y && r.bottom() <= self.bottom()
    }

    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

/// The eight resize handles: corners and side midpoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    NW,
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
}

impl Handle {
    pub const ALL: [Handle; 8] = [
        Handle::NW,
        Handle::N,
        Handle::NE,
        Handle::E,
        Handle::SE,
        Handle::S,
        Handle::SW,
        Handle::W,
    ];

    /// Which edges the handle moves: (left, top, right, bottom).
    fn edges(self) -> (bool, bool, bool, bool) {
        match self {
            Handle::NW => (true, true, false, false),
            Handle::N => (false, true, false, false),
            Handle::NE => (false, true, true, false),
            Handle::E => (false, false, true, false),
            Handle::SE => (false, false, true, true),
            Handle::S => (false, false, false, true),
            Handle::SW => (true, false, false, true),
            Handle::W => (true, false, false, false),
        }
    }

    pub fn position(self, r: &Rect) -> Point {
        let (l, t, rt, b) = self.edges();
        let x = if l {
            r.x
        } else if rt {
            r.right()
        } else {
            r.x + r.w / 2.0
        };
        let y = if t {
            r.y
        } else if b {
            r.bottom()
        } else {
            r.y + r.h / 2.0
        };
        Point::new(x, y)
    }
}

/// `start` with `handle` dragged by (`dx`, `dy`). The moved edges snap to
/// `grid` when `snapping`; the rectangle never gets smaller than one grid
/// cell and never flips.
pub fn resize(start: Rect, handle: Handle, dx: f32, dy: f32, grid: f32, snapping: bool) -> Rect {
    let fit = |v: f32| if snapping { snap(v, grid) } else { v };
    let (ml, mt, mr, mb) = handle.edges();
    let (mut l, mut t, mut r, mut b) = (start.x, start.y, start.right(), start.bottom());
    if ml {
        l = fit(l + dx).min(r - grid);
    }
    if mr {
        r = fit(r + dx).max(l + grid);
    }
    if mt {
        t = fit(t + dy).min(b - grid);
    }
    if mb {
        b = fit(b + dy).max(t + grid);
    }
    Rect::new(l, t, r - l, b - t)
}

impl Diagram {
    /// The topmost shape at `p` (shapes later in the list are drawn on top).
    pub fn shape_at(&self, p: Point) -> Option<&Shape> {
        self.shapes.iter().rev().find(|s| s.rect.contains(p))
    }

    /// The shapes entirely inside `area`, in drawing order.
    pub fn shapes_in(&self, area: Rect) -> impl Iterator<Item = &Shape> {
        self.shapes
            .iter()
            .filter(move |s| area.contains_rect(&s.rect))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ShapeKind;

    #[test]
    fn rects() {
        let r = Rect::from_corners(Point::new(50.0, 10.0), Point::new(10.0, 40.0));
        assert_eq!(r, Rect::new(10.0, 10.0, 40.0, 30.0));
        assert!(r.contains(Point::new(50.0, 40.0)));
        assert!(!r.contains(Point::new(50.1, 40.0)));
        assert!(r.contains_rect(&Rect::new(10.0, 20.0, 40.0, 5.0)));
        assert_eq!(Handle::SE.position(&r), Point::new(50.0, 40.0));
        assert_eq!(Handle::N.position(&r), Point::new(30.0, 10.0));
    }

    #[test]
    fn resizing_snaps_and_keeps_a_cell() {
        let r = Rect::new(100.0, 100.0, 160.0, 60.0);
        assert_eq!(
            resize(r, Handle::SE, 33.0, 2.0, 20.0, true),
            Rect::new(100.0, 100.0, 200.0, 60.0)
        );
        assert_eq!(
            resize(r, Handle::W, -27.0, 50.0, 20.0, true),
            Rect::new(80.0, 100.0, 180.0, 60.0)
        );
        assert_eq!(
            resize(r, Handle::NW, 7.0, -3.0, 20.0, false),
            Rect::new(107.0, 97.0, 153.0, 63.0)
        );
        // Dragged past the opposite edge: one cell, not flipped.
        assert_eq!(
            resize(r, Handle::E, -500.0, 0.0, 20.0, true),
            Rect::new(100.0, 100.0, 20.0, 60.0)
        );
        assert_eq!(
            resize(r, Handle::N, 0.0, 500.0, 20.0, true),
            Rect::new(100.0, 140.0, 160.0, 20.0)
        );
    }

    #[test]
    fn hit_testing() {
        let mut d = Diagram::new();
        d.shapes.push(Shape::new(
            "a",
            ShapeKind::Rect,
            "",
            Rect::new(0.0, 0.0, 100.0, 100.0),
        ));
        d.shapes.push(Shape::new(
            "b",
            ShapeKind::Rect,
            "",
            Rect::new(50.0, 50.0, 100.0, 100.0),
        ));
        assert_eq!(d.shape_at(Point::new(60.0, 60.0)).unwrap().id, "b");
        assert_eq!(d.shape_at(Point::new(10.0, 10.0)).unwrap().id, "a");
        assert!(d.shape_at(Point::new(200.0, 10.0)).is_none());
        let inside: Vec<_> = d
            .shapes_in(Rect::new(-10.0, -10.0, 120.0, 120.0))
            .map(|s| &s.id)
            .collect();
        assert_eq!(inside, ["a"]);
    }
}
