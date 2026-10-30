//! Connector geometry: where an end meets its shape, the path a connector
//! takes, hit-testing lines and anchors, and removing connectors.
//!
//! A glued end with a fixed anchor sits on that side's midpoint; a dynamic
//! one (`anchor: None`) uses whichever side midpoint is nearest the other end
//! (straight), or whichever facing side gives the best route (right-angle),
//! so it turns to face it as shapes move.
//!
//! Everything that needs a connector's shape (drawing, hit-testing, the
//! marquee, export) reads [`Diagram::connector_path`], so they can't disagree.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::model::{Anchor, Connector, Diagram, End, Mid, Point, Rect, Route};
use crate::route::{self, Port};

thread_local! {
    /// Right-angle routes already worked out, by everything they depend on
    /// ([`Diagram::route_key`]). Drawing, hit-testing and the marquee all
    /// ask for every path every frame; with this, only frames where
    /// something moved do the routing.
    static ROUTES: RefCell<HashMap<u64, Vec<Point>>> = RefCell::new(HashMap::new());
}

/// Routes kept before the cache starts over.
const ROUTE_CACHE: usize = 4096;

/// A side's midpoint.
pub fn anchor_point(r: &Rect, a: Anchor) -> Point {
    match a {
        Anchor::N => Point::new(r.x + r.w / 2.0, r.y),
        Anchor::E => Point::new(r.right(), r.y + r.h / 2.0),
        Anchor::S => Point::new(r.x + r.w / 2.0, r.bottom()),
        Anchor::W => Point::new(r.x, r.y + r.h / 2.0),
    }
}

/// The side whose midpoint is nearest `p`.
pub fn nearest_anchor(r: &Rect, p: Point) -> Anchor {
    let d = |a: Anchor| distance(anchor_point(r, a), p);
    Anchor::ALL
        .into_iter()
        .min_by(|a, b| d(*a).total_cmp(&d(*b)))
        .unwrap_or(Anchor::E)
}

pub fn distance(a: Point, b: Point) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// The point half-way along `path`, by length (an edge label's place).
pub fn path_midpoint(path: &[Point]) -> Option<Point> {
    let total: f32 = path.windows(2).map(|w| distance(w[0], w[1])).sum();
    let mut left = total / 2.0;
    for w in path.windows(2) {
        let len = distance(w[0], w[1]);
        if len > 0.0 && left <= len {
            let t = left / len;
            return Some(Point::new(
                w[0].x + (w[1].x - w[0].x) * t,
                w[0].y + (w[1].y - w[0].y) * t,
            ));
        }
        left -= len;
    }
    path.first().copied()
}

/// From `p` to the nearest point of the segment `a`–`b`.
pub fn distance_to_segment(p: Point, a: Point, b: Point) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return distance(p, a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    distance(p, Point::new(a.x + t * dx, a.y + t * dy))
}

impl Diagram {
    /// What the other end of a connector aims at: a free point, a fixed
    /// anchor, or a dynamic end's shape centre.
    fn aim(&self, end: &End) -> Option<Point> {
        match end {
            End::Free(p) => Some(*p),
            End::Glued { shape, anchor } => {
                let r = self.shape(shape)?.rect;
                Some(anchor.map_or(r.center(), |a| anchor_point(&r, a)))
            }
        }
    }

    /// Where `end` sits when its other end aims at `toward`.
    pub fn end_point(&self, end: &End, toward: Point) -> Option<Point> {
        match end {
            End::Free(p) => Some(*p),
            End::Glued { shape, anchor } => {
                let r = self.shape(shape)?.rect;
                Some(anchor_point(
                    &r,
                    anchor.unwrap_or_else(|| nearest_anchor(&r, toward)),
                ))
            }
        }
    }

    /// The connector's two end points; `None` if a glued shape is missing.
    pub fn connector_points(&self, c: &Connector) -> Option<(Point, Point)> {
        let path = self.connector_path(c)?;
        Some((*path.first()?, *path.last()?))
    }

    /// The points the connector runs through, from its `from` end to its
    /// `to` end: two for a straight one, more for a right-angle one. `None`
    /// if a glued shape is missing.
    pub fn connector_path(&self, c: &Connector) -> Option<Vec<Point>> {
        let (aim_from, aim_to) = (self.aim(&c.from)?, self.aim(&c.to)?);
        let Route::Elbow { mid } = c.route else {
            return Some(vec![
                self.end_point(&c.from, aim_to)?,
                self.end_point(&c.to, aim_from)?,
            ]);
        };
        let key = self.route_key(c);
        if let Some(path) = ROUTES.with(|r| r.borrow().get(&key).cloned()) {
            return Some(path);
        }
        let obstacles: Vec<Rect> = self.shapes.iter().map(|s| s.rect).collect();
        let (froms, tos) = (self.ports(&c.from, aim_to)?, self.ports(&c.to, aim_from)?);
        let mut best: Option<(Vec<Point>, f32)> = None;
        for &from in &froms {
            for &to in &tos {
                let (path, cost) = route::route(from, to, mid, &obstacles, self.grid);
                if best.as_ref().is_none_or(|(_, b)| cost < *b) {
                    best = Some((path, cost));
                }
            }
        }
        let (path, _) = best?;
        ROUTES.with(|r| {
            let mut r = r.borrow_mut();
            if r.len() >= ROUTE_CACHE {
                r.clear();
            }
            r.insert(key, path.clone());
        });
        Some(path)
    }

    /// Everything a right-angle route depends on: its ends, its pinned
    /// middle, every shape's rectangle (they're the obstacles) and the grid.
    fn route_key(&self, c: &Connector) -> u64 {
        let mut h = DefaultHasher::new();
        let f = |h: &mut DefaultHasher, v: f32| v.to_bits().hash(h);
        for end in [&c.from, &c.to] {
            match end {
                End::Free(p) => {
                    0u8.hash(&mut h);
                    f(&mut h, p.x);
                    f(&mut h, p.y);
                }
                End::Glued { shape, anchor } => {
                    1u8.hash(&mut h);
                    shape.hash(&mut h);
                    anchor.map(Anchor::as_str).hash(&mut h);
                }
            }
        }
        match c.route {
            Route::Elbow {
                mid: Some(Mid::X(x)),
            } => (1u8, x.to_bits()).hash(&mut h),
            Route::Elbow {
                mid: Some(Mid::Y(y)),
            } => (2u8, y.to_bits()).hash(&mut h),
            _ => 0u8.hash(&mut h),
        }
        f(&mut h, self.grid);
        for s in &self.shapes {
            s.id.hash(&mut h);
            for v in [s.rect.x, s.rect.y, s.rect.w, s.rect.h] {
                f(&mut h, v);
            }
        }
        h.finish()
    }

    /// The ways a right-angle route can leave `end`: its fixed side, or for
    /// dynamic glue each side facing `toward` (all four if none does).
    fn ports(&self, end: &End, toward: Point) -> Option<Vec<Port>> {
        Some(match end {
            End::Free(p) => vec![Port::free(*p)],
            End::Glued { shape, anchor } => {
                let r = self.shape(shape)?.rect;
                let port = |a: Anchor| Port {
                    at: anchor_point(&r, a),
                    side: Some(a),
                    shape: Some(r),
                };
                match anchor {
                    Some(a) => vec![port(*a)],
                    None => {
                        let facing = |a: &Anchor| match a {
                            Anchor::N => toward.y < r.y,
                            Anchor::E => toward.x > r.right(),
                            Anchor::S => toward.y > r.bottom(),
                            Anchor::W => toward.x < r.x,
                        };
                        let sides: Vec<Anchor> = Anchor::ALL.into_iter().filter(facing).collect();
                        let sides = if sides.is_empty() {
                            Anchor::ALL.to_vec()
                        } else {
                            sides
                        };
                        sides.into_iter().map(port).collect()
                    }
                }
            }
        })
    }

    /// The topmost connector within `tolerance` of `p` (connectors later in
    /// the list are drawn on top).
    pub fn connector_at(&self, p: Point, tolerance: f32) -> Option<usize> {
        self.connectors.iter().enumerate().rev().find_map(|(i, c)| {
            let path = self.connector_path(c)?;
            path.windows(2)
                .any(|w| distance_to_segment(p, w[0], w[1]) <= tolerance)
                .then_some(i)
        })
    }

    /// Connectors entirely inside `area`.
    pub fn connectors_in(&self, area: Rect) -> Vec<usize> {
        (0..self.connectors.len())
            .filter(|&i| {
                self.connector_path(&self.connectors[i])
                    .is_some_and(|path| path.iter().all(|&p| area.contains(p)))
            })
            .collect()
    }

    /// The anchor within `tolerance` of `p`, topmost shape first.
    pub fn anchor_at(&self, p: Point, tolerance: f32) -> Option<(String, Anchor)> {
        self.shapes.iter().rev().find_map(|s| {
            Anchor::ALL
                .into_iter()
                .find(|&a| distance(anchor_point(&s.rect, a), p) <= tolerance)
                .map(|a| (s.id.clone(), a))
        })
    }

    /// The end a connector gets when it's dropped at `p`: an anchor near
    /// `p` (fixed glue), else the shape under `p` (dynamic glue), else
    /// nothing (the caller makes it a free point).
    pub fn glue_at(&self, p: Point, tolerance: f32) -> Option<End> {
        if let Some((shape, anchor)) = self.anchor_at(p, tolerance) {
            return Some(End::Glued {
                shape,
                anchor: Some(anchor),
            });
        }
        self.shape_at(p).map(|s| End::glued(s.id.clone(), None))
    }

    /// Removes the connectors at these indices. Returns how many went.
    pub fn remove_connectors(&mut self, indices: &[usize]) -> usize {
        let before = self.connectors.len();
        let mut i = 0;
        self.connectors.retain(|_| {
            let keep = !indices.contains(&i);
            i += 1;
            keep
        });
        before - self.connectors.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConnectorKind, Shape, ShapeKind};

    fn diagram() -> Diagram {
        let mut d = Diagram::new();
        d.shapes.push(Shape::new(
            "a",
            ShapeKind::Rect,
            "",
            Rect::new(0.0, 0.0, 100.0, 60.0),
        ));
        d.shapes.push(Shape::new(
            "b",
            ShapeKind::Rect,
            "",
            Rect::new(300.0, 200.0, 100.0, 60.0),
        ));
        d
    }

    #[test]
    fn anchors() {
        let r = Rect::new(0.0, 0.0, 100.0, 60.0);
        assert_eq!(anchor_point(&r, Anchor::E), Point::new(100.0, 30.0));
        assert_eq!(anchor_point(&r, Anchor::S), Point::new(50.0, 60.0));
        assert_eq!(nearest_anchor(&r, Point::new(500.0, 40.0)), Anchor::E);
        assert_eq!(nearest_anchor(&r, Point::new(60.0, -300.0)), Anchor::N);
    }

    #[test]
    fn midpoints_by_length() {
        let p = Point::new;
        assert_eq!(
            path_midpoint(&[p(0.0, 0.0), p(100.0, 0.0)]),
            Some(p(50.0, 0.0))
        );
        // 100 across then 300 down: half of 400 is 100 into the second leg.
        assert_eq!(
            path_midpoint(&[p(0.0, 0.0), p(100.0, 0.0), p(100.0, 300.0)]),
            Some(p(100.0, 100.0))
        );
        assert_eq!(path_midpoint(&[p(5.0, 5.0)]), Some(p(5.0, 5.0)));
        assert_eq!(path_midpoint(&[]), None);
    }

    #[test]
    fn segments() {
        let (a, b) = (Point::new(0.0, 0.0), Point::new(100.0, 0.0));
        assert_eq!(distance_to_segment(Point::new(50.0, 7.0), a, b), 7.0);
        assert_eq!(distance_to_segment(Point::new(-3.0, 4.0), a, b), 5.0);
        assert_eq!(distance_to_segment(Point::new(3.0, 4.0), a, a), 5.0);
    }

    #[test]
    fn dynamic_ends_face_each_other() {
        let mut d = diagram();
        let c = Connector::new(
            ConnectorKind::Arrow,
            End::glued("a", None),
            End::glued("b", None),
        );
        // b is down and to the right, further right than down: E → W.
        assert_eq!(
            d.connector_points(&c),
            Some((Point::new(100.0, 30.0), Point::new(300.0, 230.0)))
        );
        // Move b straight below a: S → N.
        d.shape_mut("b").unwrap().rect = Rect::new(0.0, 300.0, 100.0, 60.0);
        assert_eq!(
            d.connector_points(&c),
            Some((Point::new(50.0, 60.0), Point::new(50.0, 300.0)))
        );
        // A fixed anchor stays put; a free end is where it is.
        let c = Connector::new(
            ConnectorKind::Line,
            End::glued("a", Some(Anchor::W)),
            End::Free(Point::new(-50.0, 500.0)),
        );
        assert_eq!(
            d.connector_points(&c),
            Some((Point::new(0.0, 30.0), Point::new(-50.0, 500.0)))
        );
        let missing = Connector::new(
            ConnectorKind::Line,
            End::glued("x", None),
            End::glued("a", None),
        );
        assert_eq!(d.connector_points(&missing), None);
    }

    #[test]
    fn hit_testing_and_removal() {
        let mut d = diagram();
        d.connectors.push(Connector::new(
            ConnectorKind::Arrow,
            End::glued("a", None),
            End::glued("b", None),
        ));
        d.connectors.push(Connector::new(
            ConnectorKind::Line,
            End::Free(Point::new(0.0, 400.0)),
            End::Free(Point::new(200.0, 400.0)),
        ));
        assert_eq!(d.connector_at(Point::new(100.0, 403.0), 5.0), Some(1));
        assert_eq!(d.connector_at(Point::new(100.0, 410.0), 5.0), None);
        assert_eq!(d.connector_at(Point::new(200.0, 130.0), 5.0), Some(0));
        assert_eq!(d.connectors_in(Rect::new(-10.0, 390.0, 220.0, 20.0)), [1]);

        assert_eq!(
            d.anchor_at(Point::new(98.0, 33.0), 6.0),
            Some(("a".into(), Anchor::E))
        );
        assert_eq!(
            d.glue_at(Point::new(50.0, 20.0), 6.0),
            Some(End::glued("a", None))
        );
        assert_eq!(
            d.glue_at(Point::new(50.0, 2.0), 6.0),
            Some(End::glued("a", Some(Anchor::N)))
        );
        assert_eq!(d.glue_at(Point::new(200.0, 100.0), 6.0), None);

        assert_eq!(d.remove_connectors(&[0, 7]), 1);
        assert_eq!(d.connectors[0].kind, ConnectorKind::Line);
    }

    #[test]
    fn right_angle_paths() {
        let mut d = diagram();
        let c = Connector::new(
            ConnectorKind::Arrow,
            End::glued("a", None),
            End::glued("b", None),
        )
        .with_route(Route::ELBOW);
        // a (0,0 100×60) to b (300,200 100×60), both glued dynamically: of
        // the facing sides, a's east to b's north needs just one bend.
        assert_eq!(
            d.connector_path(&c).unwrap(),
            [
                Point::new(100.0, 30.0),
                Point::new(350.0, 30.0),
                Point::new(350.0, 200.0)
            ]
        );
        // Fixed sides: east to west, down at the midway x.
        let fixed = Connector {
            from: End::glued("a", Some(Anchor::E)),
            to: End::glued("b", Some(Anchor::W)),
            ..c.clone()
        };
        assert_eq!(
            d.connector_path(&fixed).unwrap(),
            [
                Point::new(100.0, 30.0),
                Point::new(200.0, 30.0),
                Point::new(200.0, 230.0),
                Point::new(300.0, 230.0)
            ]
        );
        // Moving a shape gives a new route (the cache never goes stale).
        let before = d.connector_path(&fixed).unwrap();
        d.shape_mut("b").unwrap().rect = Rect::new(300.0, 400.0, 100.0, 60.0);
        let after = d.connector_path(&fixed).unwrap();
        assert_ne!(before, after);
        assert_eq!(after.last(), Some(&Point::new(300.0, 430.0)));
        d.shape_mut("b").unwrap().rect = Rect::new(300.0, 200.0, 100.0, 60.0);
        assert_eq!(d.connector_path(&fixed).unwrap(), before);

        // Hit-testing follows the bends, not the straight line between the
        // ends.
        d.connectors.push(c);
        assert_eq!(d.connector_at(Point::new(353.0, 120.0), 5.0), Some(0));
        assert_eq!(d.connector_at(Point::new(200.0, 100.0), 5.0), None);
        // The marquee needs the whole path inside.
        assert_eq!(d.connectors_in(Rect::new(90.0, 20.0, 270.0, 190.0)), [0]);
        assert!(d
            .connectors_in(Rect::new(90.0, 20.0, 200.0, 190.0))
            .is_empty());
    }
}
