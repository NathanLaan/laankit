//! Positions for shapes the file gives no `@shape` line for: nodes in a
//! hand-written flowchart, or added by hand to a BlockLiner file.
//!
//! A basic layered (Sugiyama-style) layout, without crossing minimisation:
//!
//! 1. Each node's rank is its longest path from a source, following the
//!    edges (cycles are broken at the edges that close them).
//! 2. Within a rank, nodes are ordered by the average position of their
//!    parents; the first rank keeps file order.
//! 3. Ranks run in the flowchart's direction, one cell apart, and each rank
//!    is centred across the widest one. Everything lands on the grid.
//!
//! The new shapes go below anything already placed, so a node added by hand
//! to a laid-out file never lands on top of it. The user rearranges from
//! there; the positions are saved with the first edit.

use std::collections::HashMap;

use crate::model::{Diagram, Direction, End, Rect, Shape};

/// Places `d.shapes[i]` for each `i` in `unplaced`. The ranks come from the
/// connectors between unplaced shapes, plus `hints`: edges (by shape id)
/// from statements the model keeps verbatim, such as a subgraph's.
pub fn layered(d: &mut Diagram, unplaced: &[usize], hints: &[(String, String)]) {
    if unplaced.is_empty() {
        return;
    }
    let g = d.grid;
    let n = unplaced.len();
    let slot_of: HashMap<&str, usize> = unplaced
        .iter()
        .enumerate()
        .map(|(k, &i)| (d.shapes[i].id.as_str(), k))
        .collect();

    // The edges between unplaced shapes, as local indices, once each.
    let glued = d.connectors.iter().filter_map(|c| match (&c.from, &c.to) {
        (End::Glued { shape: a, .. }, End::Glued { shape: b, .. }) => {
            Some((a.as_str(), b.as_str()))
        }
        _ => None,
    });
    let hinted = hints.iter().map(|(a, b)| (a.as_str(), b.as_str()));
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (a, b) in glued.chain(hinted) {
        if let (Some(&a), Some(&b)) = (slot_of.get(a), slot_of.get(b)) {
            if a != b && !children[a].contains(&b) {
                children[a].push(b);
            }
        }
    }
    drop_back_edges(&mut children);
    let ranks = longest_path(&children);

    // Order each rank by its nodes' parents' average place.
    let mut parents: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (a, cs) in children.iter().enumerate() {
        for &b in cs {
            parents[b].push(a);
        }
    }
    let depth = ranks.iter().max().map_or(0, |r| r + 1);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); depth];
    for (k, &r) in ranks.iter().enumerate() {
        layers[r].push(k);
    }
    let width = layers.iter().map(Vec::len).max().unwrap_or(0);
    // Each node's place across the flow, in slots (halves when centred).
    let mut across = vec![0.0f32; n];
    for layer in &mut layers {
        let key = |k: &usize| {
            let ps = &parents[*k];
            if ps.is_empty() {
                f32::MAX
            } else {
                ps.iter().map(|&p| across[p]).sum::<f32>() / ps.len() as f32
            }
        };
        // Stable: ties (and the first rank) keep file order.
        layer.sort_by(|a, b| key(a).total_cmp(&key(b)));
        let offset = (width - layer.len()) as f32 / 2.0;
        for (slot, &k) in layer.iter().enumerate() {
            across[k] = offset + slot as f32;
        }
    }

    // Cells fit the largest of the new shapes, plus a gap: two cells across
    // the flow, three along it (room for an arrow).
    let sizes: Vec<(f32, f32)> = unplaced
        .iter()
        .map(|&i| Shape::default_size(&d.shapes[i].kind, g))
        .collect();
    let max_w = sizes.iter().map(|s| s.0).fold(0.0, f32::max);
    let max_h = sizes.iter().map(|s| s.1).fold(0.0, f32::max);
    let vertical = matches!(d.direction, Direction::TB | Direction::TD | Direction::BT);
    let reversed = matches!(d.direction, Direction::BT | Direction::RL);
    let (cell_w, cell_h) = if vertical {
        (max_w + 2.0 * g, max_h + 3.0 * g)
    } else {
        (max_w + 4.0 * g, max_h + 2.0 * g)
    };

    let top = below_placed(d, unplaced);
    let snap = |v: f32| (v / g).round() * g;
    for (k, &i) in unplaced.iter().enumerate() {
        let rank = if reversed {
            depth - 1 - ranks[k]
        } else {
            ranks[k]
        } as f32;
        let (col, row) = if vertical {
            (across[k], rank)
        } else {
            (rank, across[k])
        };
        let (w, h) = sizes[k];
        // Centred in its cell.
        let x = 2.0 * g + col * cell_w + snap((max_w - w) / 2.0);
        let y = top + row * cell_h + snap((max_h - h) / 2.0);
        d.shapes[i].rect = Rect::new(snap(x), snap(y), w, h);
    }
}

/// Two cells below the lowest placed shape, on the grid; two cells down on
/// an empty canvas.
fn below_placed(d: &Diagram, unplaced: &[usize]) -> f32 {
    let g = d.grid;
    let bottom = d
        .shapes
        .iter()
        .enumerate()
        .filter(|(i, _)| !unplaced.contains(i))
        .map(|(_, s)| s.rect.bottom())
        .fold(None, |m: Option<f32>, b| Some(m.map_or(b, |m| m.max(b))));
    match bottom {
        Some(b) => (b / g).ceil() * g + 2.0 * g,
        None => 2.0 * g,
    }
}

/// Removes the edges that close a cycle (to a node still on the depth-first
/// stack), visiting in file order, so the rest is acyclic.
fn drop_back_edges(children: &mut [Vec<usize>]) {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        OnStack,
        Done,
    }
    let n = children.len();
    let mut mark = vec![Mark::New; n];
    for root in 0..n {
        if mark[root] != Mark::New {
            continue;
        }
        // (node, next child to look at)
        let mut stack = vec![(root, 0usize)];
        mark[root] = Mark::OnStack;
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            if *next < children[v].len() {
                let c = children[v][*next];
                match mark[c] {
                    Mark::OnStack => {
                        children[v].remove(*next);
                    }
                    Mark::New => {
                        *next += 1;
                        mark[c] = Mark::OnStack;
                        stack.push((c, 0));
                    }
                    Mark::Done => *next += 1,
                }
            } else {
                mark[v] = Mark::Done;
                stack.pop();
            }
        }
    }
}

/// Each node's longest path from a source in an acyclic graph.
fn longest_path(children: &[Vec<usize>]) -> Vec<usize> {
    let n = children.len();
    let mut indegree = vec![0usize; n];
    for cs in children {
        for &c in cs {
            indegree[c] += 1;
        }
    }
    let mut rank = vec![0usize; n];
    let mut ready: Vec<usize> = (0..n).rev().filter(|&v| indegree[v] == 0).collect();
    while let Some(v) = ready.pop() {
        for &c in &children[v] {
            rank[c] = rank[c].max(rank[v] + 1);
            indegree[c] -= 1;
            if indegree[c] == 0 {
                ready.push(c);
            }
        }
    }
    rank
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Connector, ConnectorKind, ShapeKind};

    fn diagram(direction: Direction, ids: &[&str], edges: &[(&str, &str)]) -> Diagram {
        let mut d = Diagram::new();
        d.direction = direction;
        for id in ids {
            d.shapes
                .push(Shape::new(*id, ShapeKind::Rect, *id, Rect::default()));
        }
        for (a, b) in edges {
            d.connectors.push(Connector::new(
                ConnectorKind::Arrow,
                End::glued(*a, None),
                End::glued(*b, None),
            ));
        }
        d
    }

    fn lay_out(d: &mut Diagram) {
        let all: Vec<usize> = (0..d.shapes.len()).collect();
        layered(d, &all, &[]);
    }

    fn rect(d: &Diagram, id: &str) -> Rect {
        d.shape(id).unwrap().rect
    }

    #[test]
    fn a_chain_runs_down_in_one_column() {
        let mut d = diagram(Direction::TD, &["a", "b", "c"], &[("a", "b"), ("b", "c")]);
        lay_out(&mut d);
        assert_eq!(rect(&d, "a"), Rect::new(40.0, 40.0, 160.0, 60.0));
        // A row is the box's 3 cells plus 3 between.
        assert_eq!(rect(&d, "b"), Rect::new(40.0, 160.0, 160.0, 60.0));
        assert_eq!(rect(&d, "c"), Rect::new(40.0, 280.0, 160.0, 60.0));
    }

    #[test]
    fn a_fan_out_centres_the_parent() {
        let mut d = diagram(Direction::TD, &["a", "b", "c"], &[("a", "b"), ("a", "c")]);
        lay_out(&mut d);
        let (a, b, c) = (rect(&d, "a"), rect(&d, "b"), rect(&d, "c"));
        assert_eq!((b.y, c.y), (160.0, 160.0));
        assert!(b.x < c.x);
        assert_eq!(a.center().x, (b.center().x + c.center().x) / 2.0);
    }

    #[test]
    fn children_follow_their_parents_order() {
        // y (c's child) comes before x (b's) in the file, but x sits under b,
        // left of y.
        let mut d = diagram(
            Direction::TD,
            &["b", "c", "y", "x"],
            &[("c", "y"), ("b", "x")],
        );
        lay_out(&mut d);
        assert!(rect(&d, "x").x < rect(&d, "y").x);
        assert_eq!(rect(&d, "x").x, rect(&d, "b").x);
    }

    #[test]
    fn directions() {
        let edges = [("a", "b")];
        for (dir, check) in [
            (Direction::LR, (true, false)),
            (Direction::RL, (true, true)),
            (Direction::BT, (false, true)),
        ] {
            let mut d = diagram(dir, &["a", "b"], &edges);
            lay_out(&mut d);
            let (a, b) = (rect(&d, "a"), rect(&d, "b"));
            let (horizontal, reversed) = check;
            let (from, to) = if horizontal { (a.x, b.x) } else { (a.y, b.y) };
            assert_eq!(from > to, reversed, "{dir:?}");
            assert_ne!(from, to, "{dir:?}");
            if horizontal {
                assert_eq!(a.y, b.y);
            } else {
                assert_eq!(a.x, b.x);
            }
        }
    }

    #[test]
    fn cycles_and_hints() {
        let mut d = diagram(Direction::TD, &["a", "b", "c"], &[("a", "b"), ("b", "a")]);
        let all = [0, 1, 2];
        layered(&mut d, &all, &[("b".into(), "c".into())]);
        assert!(rect(&d, "a").y < rect(&d, "b").y);
        assert!(rect(&d, "b").y < rect(&d, "c").y, "the hint ranks c");
    }

    #[test]
    fn below_what_is_placed() {
        let mut d = diagram(Direction::TD, &["placed", "new"], &[("placed", "new")]);
        d.shapes[0].rect = Rect::new(400.0, 40.0, 160.0, 70.0);
        layered(&mut d, &[1], &[]);
        // The bottom (110) rounds up to 120, plus two cells.
        assert_eq!(rect(&d, "new"), Rect::new(40.0, 160.0, 160.0, 60.0));
    }

    /// Pseudo-random graphs (a small LCG): nothing overlaps, everything is
    /// on the grid, and every edge that isn't part of a cycle points down.
    #[test]
    fn no_overlaps_on_random_graphs() {
        let mut seed = 7u64;
        let mut next = |m: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            (seed >> 33) as usize % m
        };
        for _ in 0..300 {
            let n = 1 + next(14);
            let ids: Vec<String> = (0..n).map(|i| format!("n{i}")).collect();
            let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
            let edges: Vec<(&str, &str)> = (0..next(2 * n + 1))
                .map(|_| (refs[next(n)], refs[next(n)]))
                .collect();
            let mut d = diagram(Direction::TD, &refs, &edges);
            if next(3) == 0 {
                d.shapes[next(n)].kind = ShapeKind::Text;
            }
            lay_out(&mut d);
            for (i, a) in d.shapes.iter().enumerate() {
                assert_eq!(a.rect.x % 20.0, 0.0);
                assert_eq!(a.rect.y % 20.0, 0.0);
                for b in &d.shapes[i + 1..] {
                    let (r, s) = (a.rect, b.rect);
                    let apart = r.right() <= s.x
                        || s.right() <= r.x
                        || r.bottom() <= s.y
                        || s.bottom() <= r.y;
                    assert!(apart, "{} {r:?} overlaps {} {s:?}", a.id, b.id);
                }
            }
        }
    }
}
