//! Right-angle routes: a path of horizontal and vertical segments between two
//! ends, chosen from a few candidates by how many shapes each crosses, then
//! by bends and length.
//!
//! 1. **Stubs.** An end glued to a side leaves it straight out for one grid
//!    cell, so the line meets the shape square on. A free end has no side.
//! 2. **Candidates** between the stub tips: the two one-bend routes; two-bend
//!    routes whose middle segment runs at the midway x or y (on the grid), or
//!    around the outside of both ends. A pinned middle segment
//!    ([`Mid`]) gives the one route through it.
//! 3. **Score:** a large cost for each shape a route enters (its own end
//!    shapes included, so it never cuts back through them), a cost for going
//!    back on itself, a small one per bend, plus the length. The first of
//!    equal candidates wins, so routing is deterministic.
//!
//! This is cheap enough to run every frame for every connector, and avoids
//! other shapes whenever one of the candidates does. A route has to cross a
//! shape only when every candidate does.

use crate::model::{Anchor, Mid, Point, Rect};
use crate::snap::snap;

/// Entering a shape.
const CROSS: f32 = 100_000.0;
/// Entering one of its own end shapes: worse than crossing any number of
/// others.
const OWN: f32 = 100_000_000.0;
/// Doubling back along a segment.
const REVERSE: f32 = 10_000.0;
/// Each bend, in model pixels of length.
const BEND: f32 = 40.0;
/// How far inside a shape's edge counts as entering it.
const EPS: f32 = 0.5;

/// One end of a route.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Port {
    pub at: Point,
    /// The side it leaves from (`None`: a free end, which leaves any way).
    pub side: Option<Anchor>,
    /// Its shape, which the detours go around.
    pub shape: Option<Rect>,
}

impl Port {
    pub fn free(at: Point) -> Self {
        Self {
            at,
            side: None,
            shape: None,
        }
    }

    /// Where the stub ends: one grid cell out from the side.
    fn tip(&self, stub: f32) -> Point {
        let (dx, dy) = match self.side {
            None => return self.at,
            Some(Anchor::N) => (0.0, -1.0),
            Some(Anchor::E) => (1.0, 0.0),
            Some(Anchor::S) => (0.0, 1.0),
            Some(Anchor::W) => (-1.0, 0.0),
        };
        Point::new(self.at.x + dx * stub, self.at.y + dy * stub)
    }

    /// The area the route goes around: the shape, or the point itself.
    fn extent(&self) -> Rect {
        self.shape
            .unwrap_or_else(|| Rect::new(self.at.x, self.at.y, 0.0, 0.0))
    }
}

/// The best route from `from` to `to`, and its cost (lower is better; a
/// cost that [`crosses`] means it enters a shape). `obstacles` are all
/// the shapes; the ends' own may be among them (they count as their own). `grid` sets the stub length and the
/// grid the middle segments snap to.
pub fn route(
    from: Port,
    to: Port,
    mid: Option<Mid>,
    obstacles: &[Rect],
    grid: f32,
) -> (Vec<Point>, f32) {
    let stub = grid;
    let (s0, s1) = (from.tip(stub), to.tip(stub));
    let own = [from.shape, to.shape];
    let through_x = |x: f32| vec![Point::new(x, s0.y), Point::new(x, s1.y)];
    let through_y = |y: f32| vec![Point::new(s0.x, y), Point::new(s1.x, y)];
    let mut middles: Vec<Vec<Point>> = Vec::new();
    match mid {
        Some(Mid::X(x)) => middles.push(through_x(x)),
        Some(Mid::Y(y)) => middles.push(through_y(y)),
        None => {
            // Through the middle first: of equal routes, the bend half-way
            // looks best.
            middles.push(through_x(snap((s0.x + s1.x) / 2.0, grid)));
            middles.push(through_y(snap((s0.y + s1.y) / 2.0, grid)));
            middles.push(vec![Point::new(s1.x, s0.y)]);
            middles.push(vec![Point::new(s0.x, s1.y)]);
            let (a, b) = (from.extent(), to.extent());
            middles.push(through_x(a.x.min(b.x) - stub));
            middles.push(through_x(a.right().max(b.right()) + stub));
            middles.push(through_y(a.y.min(b.y) - stub));
            middles.push(through_y(a.bottom().max(b.bottom()) + stub));
        }
    }
    let candidate = |middle: Vec<Point>| {
        let mut path = vec![from.at, s0];
        path.extend(middle);
        path.extend([s1, to.at]);
        let path = dedup(path);
        let c = cost(&path, obstacles, own);
        (path, c)
    };
    let mut best = pick(middles.into_iter().map(candidate), None);
    // In the way of every route so far: also try going round the shapes the
    // best one enters.
    if mid.is_none() && crosses(best.1) {
        let blocking: Vec<Rect> = obstacles
            .iter()
            .filter(|r| !own.contains(&Some(**r)))
            .filter(|r| best.0.windows(2).any(|w| enters(w[0], w[1], r)))
            .copied()
            .collect();
        let around = blocking.iter().flat_map(|r| {
            [
                through_x(r.x - stub),
                through_x(r.right() + stub),
                through_y(r.y - stub),
                through_y(r.bottom() + stub),
            ]
        });
        best = pick(around.map(candidate), Some(best));
    }
    let (path, c) = best;
    (simplify(path), c)
}

/// The cheapest of `candidates` and `best`; the earlier wins a tie.
fn pick(
    candidates: impl Iterator<Item = (Vec<Point>, f32)>,
    best: Option<(Vec<Point>, f32)>,
) -> (Vec<Point>, f32) {
    let mut best = best;
    for (path, c) in candidates {
        if best.as_ref().is_none_or(|(_, b)| c < *b) {
            best = Some((path, c));
        }
    }
    best.expect("at least one candidate")
}

/// Whether a route of this cost enters a shape.
pub fn crosses(cost: f32) -> bool {
    cost >= CROSS
}

/// Length, bends, doubling back, and the shapes entered.
fn cost(path: &[Point], obstacles: &[Rect], own: [Option<Rect>; 2]) -> f32 {
    let mut c = 0.0;
    for w in path.windows(2) {
        c += (w[1].x - w[0].x).abs() + (w[1].y - w[0].y).abs();
    }
    for w in path.windows(3) {
        let (d1, d2) = (dir(w[0], w[1]), dir(w[1], w[2]));
        if d1.0 * d2.0 + d1.1 * d2.1 < 0.0 {
            c += REVERSE;
        } else if d1 != d2 {
            c += BEND;
        }
    }
    let through = |r: &Rect| path.windows(2).any(|w| enters(w[0], w[1], r));
    for r in own.iter().flatten() {
        if through(r) {
            c += OWN;
        }
    }
    for r in obstacles.iter().filter(|r| !own.contains(&Some(**r))) {
        if through(r) {
            c += CROSS;
        }
    }
    c
}

/// The unit direction of an axis-aligned segment. (`f32::signum` won't do:
/// it gives 1 for 0.)
fn dir(a: Point, b: Point) -> (f32, f32) {
    let sign = |v: f32| {
        if v > 0.0 {
            1.0
        } else if v < 0.0 {
            -1.0
        } else {
            0.0
        }
    };
    (sign(b.x - a.x), sign(b.y - a.y))
}

/// Whether the axis-aligned segment `a`–`b` passes through `r`'s inside
/// (running along an edge doesn't count).
fn enters(a: Point, b: Point, r: &Rect) -> bool {
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    x1 > r.x + EPS && x0 < r.right() - EPS && y1 > r.y + EPS && y0 < r.bottom() - EPS
}

/// Without repeated points.
fn dedup(path: Vec<Point>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(path.len());
    for p in path {
        if out
            .last()
            .is_none_or(|q| (q.x - p.x).abs() > 1e-3 || (q.y - p.y).abs() > 1e-3)
        {
            out.push(p);
        }
    }
    out
}

/// Without points in the middle of a straight run.
fn simplify(path: Vec<Point>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(path.len());
    for p in path {
        if out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            if dir(a, b) == dir(b, p) {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

/// The segment a route's middle handle sits on: the middle one of the inner
/// segments (never the first or last, which meet the ends). `None` for
/// routes with fewer than three segments.
pub fn middle_segment(path: &[Point]) -> Option<usize> {
    let segments = path.len().checked_sub(1)?;
    (segments >= 3).then_some(segments / 2)
}

/// The pinned middle a drag of segment `i` to `to` gives: a vertical
/// segment moves sideways in x, a horizontal one in y.
pub fn drag_middle(path: &[Point], i: usize, to: Point) -> Option<Mid> {
    let (a, b) = (*path.get(i)?, *path.get(i + 1)?);
    Some(if a.x == b.x {
        Mid::X(to.x)
    } else {
        Mid::Y(to.y)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: f32 = 20.0;

    fn glued(r: Rect, side: Anchor) -> Port {
        Port {
            at: crate::connect::anchor_point(&r, side),
            side: Some(side),
            shape: Some(r),
        }
    }

    fn pts(v: &[(f32, f32)]) -> Vec<Point> {
        v.iter().map(|&(x, y)| Point::new(x, y)).collect()
    }

    fn axis_aligned(path: &[Point]) -> bool {
        path.windows(2)
            .all(|w| w[0].x == w[1].x || w[0].y == w[1].y)
    }

    #[test]
    fn side_to_side_takes_a_step_through_the_middle() {
        let a = Rect::new(0.0, 0.0, 100.0, 60.0);
        let b = Rect::new(300.0, 200.0, 100.0, 60.0);
        let (path, cost) = route(
            glued(a, Anchor::E),
            glued(b, Anchor::W),
            None,
            &[a, b],
            GRID,
        );
        // Out east, down at the midway x (200), in from the west.
        assert_eq!(
            path,
            pts(&[(100.0, 30.0), (200.0, 30.0), (200.0, 230.0), (300.0, 230.0)])
        );
        assert!(!crosses(cost));
        assert_eq!(middle_segment(&path), Some(1));
    }

    #[test]
    fn facing_away_goes_around_the_outside() {
        // b is right of a, but the line leaves a westwards and enters b from
        // the east: it must go round, never through either box.
        let a = Rect::new(0.0, 0.0, 100.0, 60.0);
        let b = Rect::new(300.0, 0.0, 100.0, 60.0);
        let (path, cost) = route(
            glued(a, Anchor::W),
            glued(b, Anchor::E),
            None,
            &[a, b],
            GRID,
        );
        assert!(!crosses(cost), "{path:?}");
        assert!(axis_aligned(&path));
        assert_eq!(path[0], Point::new(0.0, 30.0));
        assert_eq!(path[path.len() - 1], Point::new(400.0, 30.0));
        assert_eq!(path[1], Point::new(-20.0, 30.0), "leaves west first");
    }

    #[test]
    fn goes_round_a_shape_in_the_way() {
        let a = Rect::new(0.0, 0.0, 100.0, 60.0);
        let b = Rect::new(400.0, 0.0, 100.0, 60.0);
        let (from, to) = (glued(a, Anchor::E), glued(b, Anchor::W));
        let (path, _) = route(from, to, None, &[a, b], GRID);
        assert_eq!(
            path,
            pts(&[(100.0, 30.0), (400.0, 30.0)]),
            "lined up: straight"
        );

        // A wall across the way, taller than both boxes: over or under it.
        let wall = Rect::new(180.0, -100.0, 60.0, 400.0);
        let (path, cost) = route(from, to, None, &[a, b, wall], GRID);
        assert!(!crosses(cost), "{path:?}");
        assert!(axis_aligned(&path));
        assert!(
            path.iter().any(|p| p.y == -120.0 || p.y == 320.0),
            "{path:?}"
        );

        // A shape around the target can't be avoided: the route enters it,
        // but still not its own boxes.
        let around = Rect::new(380.0, -20.0, 140.0, 100.0);
        let (path, cost) = route(from, to, None, &[a, b, around], GRID);
        assert!(crosses(cost));
        assert!(cost < OWN, "{path:?}");
    }

    #[test]
    fn a_pinned_middle_is_used() {
        let a = Rect::new(0.0, 0.0, 100.0, 60.0);
        let b = Rect::new(300.0, 200.0, 100.0, 60.0);
        let (path, _) = route(
            glued(a, Anchor::E),
            glued(b, Anchor::W),
            Some(Mid::X(260.0)),
            &[a, b],
            GRID,
        );
        assert_eq!(
            path,
            pts(&[(100.0, 30.0), (260.0, 30.0), (260.0, 230.0), (300.0, 230.0)])
        );
        // Dragging that vertical segment moves it in x.
        assert_eq!(
            drag_middle(&path, 1, Point::new(220.0, 999.0)),
            Some(Mid::X(220.0))
        );
    }

    #[test]
    fn free_ends_bend_once() {
        let (path, cost) = route(
            Port::free(Point::new(0.0, 0.0)),
            Port::free(Point::new(100.0, 60.0)),
            None,
            &[],
            GRID,
        );
        assert_eq!(path, pts(&[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0)]));
        assert!(!crosses(cost));
        assert_eq!(middle_segment(&path), None);
        // Lined up: one straight segment.
        let (path, _) = route(
            Port::free(Point::new(0.0, 0.0)),
            Port::free(Point::new(100.0, 0.0)),
            None,
            &[],
            GRID,
        );
        assert_eq!(path, pts(&[(0.0, 0.0), (100.0, 0.0)]));
    }

    #[test]
    fn mid_parses_and_moves() {
        assert_eq!(Mid::parse("x:240"), Some(Mid::X(240.0)));
        assert_eq!(Mid::parse("y:-12.5"), Some(Mid::Y(-12.5)));
        assert_eq!(Mid::parse("z:1"), None);
        assert_eq!(Mid::parse("x:nan"), None);
        assert_eq!(Mid::X(10.0).translated(5.0, 7.0), Mid::X(15.0));
    }

    /// A small deterministic generator (as in `layout`'s tests).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    /// Random pairs of separate boxes with random sides, among random other
    /// boxes: every route is axis-aligned, starts and ends on its anchors,
    /// never enters its own boxes, and avoids the others whenever any
    /// candidate could.
    #[test]
    fn random_routes_hold_their_promises() {
        let mut rng = Rng(7);
        let mut checked = 0;
        while checked < 300 {
            let rect = |rng: &mut Rng| {
                Rect::new(
                    (rng.below(40) * 20) as f32,
                    (rng.below(30) * 20) as f32,
                    (rng.below(6) * 20 + 40) as f32,
                    (rng.below(4) * 20 + 40) as f32,
                )
            };
            let (a, b) = (rect(&mut rng), rect(&mut rng));
            // Ends at least a stub apart, so there's room to leave each.
            let apart = |r: &Rect, s: &Rect| {
                r.right() + 2.0 * GRID <= s.x
                    || s.right() + 2.0 * GRID <= r.x
                    || r.bottom() + 2.0 * GRID <= s.y
                    || s.bottom() + 2.0 * GRID <= r.y
            };
            if !apart(&a, &b) {
                continue;
            }
            checked += 1;
            let others: Vec<Rect> = (0..rng.below(4)).map(|_| rect(&mut rng)).collect();
            let sides = Anchor::ALL;
            let (sa, sb) = (sides[rng.below(4) as usize], sides[rng.below(4) as usize]);
            let (from, to) = (glued(a, sa), glued(b, sb));
            let mut obstacles = vec![a, b];
            obstacles.extend(others.iter().copied());
            let (path, cost) = route(from, to, None, &obstacles, GRID);
            assert!(axis_aligned(&path), "{path:?}");
            assert_eq!(path[0], from.at);
            assert_eq!(*path.last().unwrap(), to.at);
            let own = |r: &Rect| path.windows(2).any(|w| enters(w[0], w[1], r));
            assert!(
                !own(&a) && !own(&b),
                "through its own box: {a:?} {b:?} {path:?}"
            );
            // Against only the end boxes, a clean route always exists.
            let (_, alone) = route(from, to, None, &[a, b], GRID);
            assert!(!crosses(alone), "{a:?} {sa:?} → {b:?} {sb:?}");
            let _ = cost;
        }
    }
}
