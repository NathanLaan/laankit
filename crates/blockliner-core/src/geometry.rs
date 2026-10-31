//! Rectangle geometry, the resize handles, the outlines of the shapes that
//! aren't boxes, their label areas and hit-testing.

use crate::connect::anchor_point;
use crate::model::{Anchor, Diagram, Point, Rect, Shape, ShapeKind};
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

/// How far a subroutine's inner lines sit in from its sides.
pub const SUBROUTINE_INSET: f32 = 8.0;

/// The height of each of a database's elliptical ends.
pub fn cap_height(r: &Rect) -> f32 {
    (r.h / 5.0).min(12.0)
}

/// How far a hexagon's left and right points stick out past its flat top
/// and bottom.
pub fn hexagon_inset(r: &Rect) -> f32 {
    (r.h / 2.0).min(r.w / 4.0)
}

/// How far a parallelogram's or trapezoid's slanted sides lean across.
pub fn slant(r: &Rect) -> f32 {
    (r.h / 3.0).min(r.w / 4.0)
}

/// A stadium's corner radius: half its height (or width, if narrower).
pub fn stadium_radius(r: &Rect) -> f32 {
    r.h.min(r.w) / 2.0
}

/// The outline of a shape drawn as a polygon (diamond, hexagon,
/// parallelogram, trapezoid), clockwise from the top left; `None` for the
/// others.
pub fn polygon(kind: &ShapeKind, r: &Rect) -> Option<Vec<Point>> {
    let c = r.center();
    match kind {
        ShapeKind::Parallelogram => {
            let k = slant(r);
            Some(vec![
                Point::new(r.x + k, r.y),
                Point::new(r.right(), r.y),
                Point::new(r.right() - k, r.bottom()),
                Point::new(r.x, r.bottom()),
            ])
        }
        ShapeKind::Trapezoid => {
            let k = slant(r);
            Some(vec![
                Point::new(r.x + k, r.y),
                Point::new(r.right() - k, r.y),
                Point::new(r.right(), r.bottom()),
                Point::new(r.x, r.bottom()),
            ])
        }
        ShapeKind::Diamond => Some(vec![
            Point::new(c.x, r.y),
            Point::new(r.right(), c.y),
            Point::new(c.x, r.bottom()),
            Point::new(r.x, c.y),
        ]),
        ShapeKind::Hexagon => {
            let k = hexagon_inset(r);
            Some(vec![
                Point::new(r.x + k, r.y),
                Point::new(r.right() - k, r.y),
                Point::new(r.right(), c.y),
                Point::new(r.right() - k, r.bottom()),
                Point::new(r.x + k, r.bottom()),
                Point::new(r.x, c.y),
            ])
        }
        _ => None,
    }
}

/// A database's outline with caps `cap` high (in the units of `r`, so a
/// caller drawing at a zoom scales [`cap_height`]), `steps` points per half
/// ellipse: the top cap's upper half, the right side, the bottom cap's
/// lower half and the left side (convex, clockwise). Then the top cap's
/// lower half, the lid's edge, drawn over the body.
pub fn cylinder(r: &Rect, cap: f32, steps: usize) -> (Vec<Point>, Vec<Point>) {
    let (rx, ry) = (r.w / 2.0, cap / 2.0);
    let cx = r.x + rx;
    let (top, bottom) = (r.y + ry, r.bottom() - ry);
    // Half an ellipse around (cx, cy), left to right, above (`-1`) or
    // below (`1`) the centre.
    let half = |cy: f32, side: f32| -> Vec<Point> {
        (0..=steps)
            .map(|i| {
                let a = std::f32::consts::PI * i as f32 / steps as f32;
                Point::new(cx - rx * a.cos(), cy + side * ry * a.sin())
            })
            .collect()
    };
    let mut outline = half(top, -1.0);
    let mut lower = half(bottom, 1.0);
    lower.reverse();
    outline.extend(lower);
    (outline, half(top, 1.0))
}

impl Shape {
    /// `p` is inside the shape's outline (edges included). A diamond, a
    /// hexagon, a parallelogram, a trapezoid, a circle or a stadium misses
    /// in its empty corners; the others are their rectangle.
    pub fn contains(&self, p: Point) -> bool {
        let r = &self.rect;
        if !r.contains(p) {
            return false;
        }
        let c = r.center();
        // 0 across the middle, 1 at the top and bottom.
        let off_middle = |half: f32| {
            if half > 0.0 {
                (p.y - c.y).abs() / half
            } else {
                0.0
            }
        };
        match self.kind {
            ShapeKind::Diamond => {
                let across = if r.w > 0.0 {
                    (p.x - c.x).abs() / (r.w / 2.0)
                } else {
                    0.0
                };
                across + off_middle(r.h / 2.0) <= 1.0 + 1e-4
            }
            ShapeKind::Hexagon => {
                let k = hexagon_inset(r) * off_middle(r.h / 2.0);
                p.x >= r.x + k - 1e-4 && p.x <= r.right() - k + 1e-4
            }
            ShapeKind::Parallelogram | ShapeKind::Trapezoid => {
                // 0 at the top, 1 at the bottom.
                let down = if r.h > 0.0 { (p.y - r.y) / r.h } else { 0.0 };
                let k = slant(r);
                let (left, right) = if self.kind == ShapeKind::Parallelogram {
                    (r.x + k * (1.0 - down), r.right() - k * down)
                } else {
                    (r.x + k * (1.0 - down), r.right() - k * (1.0 - down))
                };
                p.x >= left - 1e-4 && p.x <= right + 1e-4
            }
            ShapeKind::Circle => {
                let (rx, ry) = (r.w / 2.0, r.h / 2.0);
                if rx <= 0.0 || ry <= 0.0 {
                    return true;
                }
                let (dx, dy) = ((p.x - c.x) / rx, (p.y - c.y) / ry);
                dx * dx + dy * dy <= 1.0 + 1e-4
            }
            ShapeKind::Stadium => {
                // Outside the straight middle, within the radius of an end's
                // centre.
                let k = stadium_radius(r);
                let ends = [
                    Point::new(r.x + k, r.y + k),
                    Point::new(r.right() - k, r.bottom() - k),
                ];
                let nearest = Point::new(
                    p.x.clamp(ends[0].x, ends[1].x),
                    p.y.clamp(ends[0].y, ends[1].y),
                );
                (p.x - nearest.x).hypot(p.y - nearest.y) <= k + 1e-4
            }
            _ => true,
        }
    }

    /// Where a connector glued to `a` meets the shape: the side's midpoint,
    /// except on a parallelogram's or trapezoid's slanted sides, whose
    /// midpoints are half the slant in.
    pub fn anchor(&self, a: Anchor) -> Point {
        let p = anchor_point(&self.rect, a);
        match (&self.kind, a) {
            (ShapeKind::Parallelogram | ShapeKind::Trapezoid, Anchor::E) => {
                Point::new(p.x - slant(&self.rect) / 2.0, p.y)
            }
            (ShapeKind::Parallelogram | ShapeKind::Trapezoid, Anchor::W) => {
                Point::new(p.x + slant(&self.rect) / 2.0, p.y)
            }
            _ => p,
        }
    }

    /// Where the label goes, inside the outline: a diamond's middle half,
    /// between a hexagon's points, below a database's lid, between a
    /// subroutine's inner lines, else the whole rectangle.
    pub fn label_rect(&self) -> Rect {
        let r = self.rect;
        match self.kind {
            ShapeKind::Diamond => Rect::new(r.x + r.w / 4.0, r.y + r.h / 4.0, r.w / 2.0, r.h / 2.0),
            ShapeKind::Hexagon => {
                let k = hexagon_inset(&r);
                Rect::new(r.x + k, r.y, r.w - 2.0 * k, r.h)
            }
            ShapeKind::Database => {
                let cap = cap_height(&r);
                Rect::new(r.x, r.y + cap, r.w, (r.h - 1.5 * cap).max(0.0))
            }
            ShapeKind::Subroutine => {
                let k = SUBROUTINE_INSET.min(r.w / 4.0);
                Rect::new(r.x + k, r.y, r.w - 2.0 * k, r.h)
            }
            ShapeKind::Parallelogram | ShapeKind::Trapezoid => {
                let k = slant(&r);
                Rect::new(r.x + k, r.y, r.w - 2.0 * k, r.h)
            }
            ShapeKind::Stadium => {
                let k = stadium_radius(&r) / 2.0;
                Rect::new(r.x + k, r.y, r.w - 2.0 * k, r.h)
            }
            // The rectangle inside the ellipse.
            ShapeKind::Circle => {
                let (w, h) = (
                    r.w / std::f32::consts::SQRT_2,
                    r.h / std::f32::consts::SQRT_2,
                );
                Rect::new(r.x + (r.w - w) / 2.0, r.y + (r.h - h) / 2.0, w, h)
            }
            _ => r,
        }
    }
}

impl Diagram {
    /// The topmost shape at `p` (shapes later in the list are drawn on top).
    pub fn shape_at(&self, p: Point) -> Option<&Shape> {
        self.shapes.iter().rev().find(|s| s.contains(p))
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

    fn shape(kind: ShapeKind) -> Shape {
        Shape::new("s", kind, "", Rect::new(0.0, 0.0, 160.0, 80.0))
    }

    #[test]
    fn outlines() {
        let r = Rect::new(0.0, 0.0, 160.0, 80.0);
        let p = |x, y| Point::new(x, y);
        assert_eq!(
            polygon(&ShapeKind::Diamond, &r).unwrap(),
            [p(80.0, 0.0), p(160.0, 40.0), p(80.0, 80.0), p(0.0, 40.0)]
        );
        // min(h / 2, w / 4) = 40.
        assert_eq!(
            polygon(&ShapeKind::Hexagon, &r).unwrap(),
            [
                p(40.0, 0.0),
                p(120.0, 0.0),
                p(160.0, 40.0),
                p(120.0, 80.0),
                p(40.0, 80.0),
                p(0.0, 40.0)
            ]
        );
        assert_eq!(hexagon_inset(&Rect::new(0.0, 0.0, 400.0, 60.0)), 30.0);
        assert!(polygon(&ShapeKind::Rect, &r).is_none());

        // Caps of min(h / 5, 12) = 12: from y 0 to 12, and 68 to 80.
        assert_eq!(cap_height(&r), 12.0);
        assert_eq!(cap_height(&Rect::new(0.0, 0.0, 100.0, 40.0)), 8.0);
        let (outline, lid) = cylinder(&r, cap_height(&r), 4);
        assert_eq!(outline.len(), 10);
        assert_eq!(outline[0], p(0.0, 6.0));
        assert!((outline[2].y - 0.0).abs() < 1e-4, "{:?}", outline[2]);
        assert!((outline[4].x - 160.0).abs() < 1e-4);
        assert!((outline[7].y - 80.0).abs() < 1e-4, "{:?}", outline[7]);
        assert!((lid[2].y - 12.0).abs() < 1e-4, "{:?}", lid[2]);
    }

    #[test]
    fn diamonds_and_hexagons_miss_in_their_corners() {
        let diamond = shape(ShapeKind::Diamond);
        assert!(diamond.contains(Point::new(80.0, 40.0)));
        assert!(diamond.contains(Point::new(0.0, 40.0)), "a vertex");
        assert!(diamond.contains(Point::new(40.0, 20.0)), "an edge");
        assert!(!diamond.contains(Point::new(10.0, 10.0)));
        assert!(!diamond.contains(Point::new(150.0, 75.0)));

        let hexagon = shape(ShapeKind::Hexagon);
        assert!(hexagon.contains(Point::new(5.0, 40.0)));
        assert!(hexagon.contains(Point::new(45.0, 2.0)));
        assert!(!hexagon.contains(Point::new(5.0, 5.0)));
        assert!(!hexagon.contains(Point::new(155.0, 75.0)));

        for kind in [ShapeKind::Database, ShapeKind::Subroutine] {
            assert!(shape(kind).contains(Point::new(1.0, 1.0)));
        }

        // What's behind a diamond's corner is hit instead.
        let mut d = Diagram::new();
        d.shapes.push(Shape::new(
            "under",
            ShapeKind::Rect,
            "",
            Rect::new(0.0, 0.0, 40.0, 40.0),
        ));
        d.shapes.push(diamond);
        assert_eq!(d.shape_at(Point::new(10.0, 10.0)).unwrap().id, "under");
        assert_eq!(d.shape_at(Point::new(80.0, 40.0)).unwrap().id, "s");
    }

    #[test]
    fn label_areas() {
        let area = |kind| shape(kind).label_rect();
        assert_eq!(area(ShapeKind::Rect), Rect::new(0.0, 0.0, 160.0, 80.0));
        assert_eq!(area(ShapeKind::Diamond), Rect::new(40.0, 20.0, 80.0, 40.0));
        assert_eq!(area(ShapeKind::Hexagon), Rect::new(40.0, 0.0, 80.0, 80.0));
        assert_eq!(area(ShapeKind::Database), Rect::new(0.0, 12.0, 160.0, 62.0));
        assert_eq!(
            area(ShapeKind::Subroutine),
            Rect::new(8.0, 0.0, 144.0, 80.0)
        );
    }

    #[test]
    fn stadiums_circles_parallelograms_and_trapezoids() {
        let p = Point::new;
        let r = Rect::new(0.0, 0.0, 160.0, 60.0);
        let s = |kind| Shape::new("s", kind, "", r);
        // A slant of 20 (a third of the height).
        assert_eq!(
            polygon(&ShapeKind::Parallelogram, &r).unwrap(),
            [p(20.0, 0.0), p(160.0, 0.0), p(140.0, 60.0), p(0.0, 60.0)]
        );
        assert_eq!(
            polygon(&ShapeKind::Trapezoid, &r).unwrap(),
            [p(20.0, 0.0), p(140.0, 0.0), p(160.0, 60.0), p(0.0, 60.0)]
        );

        // Their empty corners miss.
        let para = s(ShapeKind::Parallelogram);
        assert!(!para.contains(p(2.0, 2.0)));
        assert!(para.contains(p(158.0, 2.0)));
        assert!(!para.contains(p(158.0, 58.0)));
        assert!(para.contains(p(2.0, 58.0)));
        let trap = s(ShapeKind::Trapezoid);
        assert!(!trap.contains(p(2.0, 2.0)));
        assert!(!trap.contains(p(158.0, 2.0)));
        assert!(trap.contains(p(2.0, 58.0)));
        let stadium = s(ShapeKind::Stadium);
        assert!(!stadium.contains(p(3.0, 3.0)));
        assert!(stadium.contains(p(30.0, 1.0)));
        assert!(stadium.contains(p(1.0, 30.0)));
        let circle = Shape::new("c", ShapeKind::Circle, "", Rect::new(0.0, 0.0, 80.0, 80.0));
        assert!(!circle.contains(p(6.0, 6.0)));
        assert!(circle.contains(p(40.0, 1.0)));
        assert!(circle.contains(p(40.0, 40.0)));

        // Label areas inside the outlines.
        assert_eq!(para.label_rect(), Rect::new(20.0, 0.0, 120.0, 60.0));
        assert_eq!(trap.label_rect(), Rect::new(20.0, 0.0, 120.0, 60.0));
        assert_eq!(stadium.label_rect(), Rect::new(15.0, 0.0, 130.0, 60.0));
        let inner = circle.label_rect();
        assert!((inner.w - 56.57).abs() < 0.01 && (inner.x - 11.72).abs() < 0.01);

        // A slanted side's anchor is on it.
        assert_eq!(para.anchor(Anchor::E), p(150.0, 30.0));
        assert_eq!(para.anchor(Anchor::W), p(10.0, 30.0));
        assert_eq!(trap.anchor(Anchor::E), p(150.0, 30.0));
        assert_eq!(para.anchor(Anchor::N), p(80.0, 0.0));
        assert_eq!(stadium.anchor(Anchor::E), p(160.0, 30.0));
    }
}
