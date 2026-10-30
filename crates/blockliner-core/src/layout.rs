//! Positions for shapes the file gives no `@shape` line for: nodes in a
//! hand-written flowchart, or added by hand to a BlockLiner file. A plain
//! row-by-row grid below whatever is already placed; the plan's layered
//! layout (phase 6) replaces it.

use crate::model::{Diagram, Rect, Shape};

const COLUMNS: usize = 4;

/// Places `d.shapes[i]` for each `i` in `unplaced`, snapped to the grid.
pub fn place(d: &mut Diagram, unplaced: &[usize]) {
    if unplaced.is_empty() {
        return;
    }
    let g = d.grid;
    let bottom = d
        .shapes
        .iter()
        .enumerate()
        .filter(|(i, _)| !unplaced.contains(i))
        .map(|(_, s)| s.rect.bottom())
        .fold(None, |m: Option<f32>, b| Some(m.map_or(b, |m| m.max(b))));
    let top = match bottom {
        Some(b) => (b / g).ceil() * g + 2.0 * g,
        None => 2.0 * g,
    };
    // Cells fit the largest default size (8 × 3 boxes, 10 × 2 text) plus a
    // two-cell gap.
    let (cell_w, cell_h) = (12.0 * g, 5.0 * g);
    for (n, &i) in unplaced.iter().enumerate() {
        let (w, h) = Shape::default_size(&d.shapes[i].kind, g);
        let (col, row) = ((n % COLUMNS) as f32, (n / COLUMNS) as f32);
        d.shapes[i].rect = Rect::new(2.0 * g + col * cell_w, top + row * cell_h, w, h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ShapeKind;

    #[test]
    fn places_below_existing_shapes_on_the_grid() {
        let mut d = Diagram::new();
        d.shapes.push(Shape::new(
            "a",
            ShapeKind::Rect,
            "A",
            Rect::new(40.0, 40.0, 160.0, 70.0),
        ));
        for id in ["b", "c", "d", "e", "f"] {
            d.shapes
                .push(Shape::new(id, ShapeKind::Rect, id, Rect::default()));
        }
        place(&mut d, &[1, 2, 3, 4, 5]);
        // The bottom (110) rounds up to 120, plus two cells.
        assert_eq!(d.shapes[1].rect, Rect::new(40.0, 160.0, 160.0, 60.0));
        assert_eq!(d.shapes[2].rect.x, 40.0 + 240.0);
        // The fifth wraps to the next row.
        assert_eq!(d.shapes[5].rect, Rect::new(40.0, 260.0, 160.0, 60.0));
        for s in &d.shapes[1..] {
            assert_eq!(s.rect.x % 20.0, 0.0);
            assert_eq!(s.rect.y % 20.0, 0.0);
        }
    }
}
