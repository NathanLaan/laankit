//! Connector geometry: where an end meets its shape, hit-testing lines and
//! anchors, and removing connectors.
//!
//! Connectors are straight segments in the MVP (orthogonal routing comes
//! later). A glued end with a fixed anchor sits on that side's midpoint; a
//! dynamic one (`anchor: None`) uses whichever side midpoint is nearest the
//! other end, so it turns to face it as shapes move.

use crate::model::{Anchor, Connector, Diagram, End, Point, Rect};

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
        let (aim_from, aim_to) = (self.aim(&c.from)?, self.aim(&c.to)?);
        Some((
            self.end_point(&c.from, aim_to)?,
            self.end_point(&c.to, aim_from)?,
        ))
    }

    /// The topmost connector within `tolerance` of `p` (connectors later in
    /// the list are drawn on top).
    pub fn connector_at(&self, p: Point, tolerance: f32) -> Option<usize> {
        self.connectors.iter().enumerate().rev().find_map(|(i, c)| {
            let (a, b) = self.connector_points(c)?;
            (distance_to_segment(p, a, b) <= tolerance).then_some(i)
        })
    }

    /// Connectors with both ends inside `area`.
    pub fn connectors_in(&self, area: Rect) -> Vec<usize> {
        (0..self.connectors.len())
            .filter(|&i| {
                self.connector_points(&self.connectors[i])
                    .is_some_and(|(a, b)| area.contains(a) && area.contains(b))
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
}
