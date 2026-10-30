//! What an image export contains: all objects, or only the selected ones,
//! and the page around them. The drawing itself is the app's (it needs the
//! canvas's text layout).

use crate::model::{Diagram, Rect};

/// Empty grid cells around the exported objects.
pub const MARGIN_CELLS: f32 = 2.0;

/// Which objects to export.
#[derive(Clone, Copy, Debug)]
pub enum Objects<'a> {
    All,
    /// The selected shapes (by id) and lines (by index).
    Selected {
        shapes: &'a [String],
        lines: &'a [usize],
    },
}

/// The exported objects and the page.
#[derive(Clone, Debug, PartialEq)]
pub struct Scope {
    /// Indices into `shapes`, in drawing order.
    pub shapes: Vec<usize>,
    /// Indices into `connectors`, in drawing order.
    pub connectors: Vec<usize>,
    /// The page: the objects' extent plus the margin, on the grid.
    pub bounds: Rect,
}

/// What to export, or `None` when that's nothing.
///
/// A selection export takes the selected shapes and lines, plus connectors
/// glued at both ends to selected shapes. A connector glued to a shape that
/// isn't exported is always left out: it would point at nothing.
pub fn scope(d: &Diagram, objects: Objects<'_>) -> Option<Scope> {
    let shapes: Vec<usize> = match objects {
        Objects::All => (0..d.shapes.len()).collect(),
        Objects::Selected { shapes, .. } => (0..d.shapes.len())
            .filter(|&i| shapes.contains(&d.shapes[i].id))
            .collect(),
    };
    let exported = |id: &str| shapes.iter().any(|&i| d.shapes[i].id == id);
    let connectors: Vec<usize> = d
        .connectors
        .iter()
        .enumerate()
        .filter(|&(i, c)| {
            let ends = [&c.from, &c.to].map(|e| e.shape());
            let picked = match objects {
                Objects::All => true,
                Objects::Selected { lines, .. } => {
                    lines.contains(&i) || ends.iter().all(Option::is_some)
                }
            };
            picked && ends.iter().flatten().all(|id| exported(id))
        })
        .map(|(i, _)| i)
        .collect();

    let mut extent: Option<(f32, f32, f32, f32)> = None;
    let mut grow = |x: f32, y: f32| {
        extent = Some(match extent {
            None => (x, y, x, y),
            Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
        });
    };
    for &i in &shapes {
        let r = d.shapes[i].rect;
        grow(r.x, r.y);
        grow(r.right(), r.bottom());
    }
    for &i in &connectors {
        if let Some((a, b)) = d.connector_points(&d.connectors[i]) {
            grow(a.x, a.y);
            grow(b.x, b.y);
        }
    }
    let (x0, y0, x1, y1) = extent?;
    let g = d.grid;
    let m = MARGIN_CELLS * g;
    let (x0, y0) = ((x0 / g).floor() * g - m, (y0 / g).floor() * g - m);
    let (x1, y1) = ((x1 / g).ceil() * g + m, (y1 / g).ceil() * g + m);
    Some(Scope {
        shapes,
        connectors,
        bounds: Rect::new(x0, y0, x1 - x0, y1 - y0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Connector, ConnectorKind, End, Point, Shape, ShapeKind};

    /// a (100, 100) → b (400, 100) → c (100, 300), and a free line.
    fn diagram() -> Diagram {
        let mut d = Diagram::new();
        for (id, x, y) in [
            ("a", 100.0, 100.0),
            ("b", 400.0, 100.0),
            ("c", 100.0, 300.0),
        ] {
            d.shapes.push(Shape::new(
                id,
                ShapeKind::Rect,
                id,
                Rect::new(x, y, 160.0, 60.0),
            ));
        }
        let arrow = |a: &str, b: &str| {
            Connector::new(
                ConnectorKind::Arrow,
                End::glued(a, None),
                End::glued(b, None),
            )
        };
        d.connectors.push(arrow("a", "b"));
        d.connectors.push(arrow("b", "c"));
        d.connectors.push(Connector::new(
            ConnectorKind::Line,
            End::Free(Point::new(700.0, 50.0)),
            End::Free(Point::new(710.0, 405.0)),
        ));
        d
    }

    #[test]
    fn everything_on_a_grid_aligned_page() {
        let d = diagram();
        let s = scope(&d, Objects::All).unwrap();
        assert_eq!(s.shapes, [0, 1, 2]);
        assert_eq!(s.connectors, [0, 1, 2]);
        // x 100..710 → 100..720, y 50..405 → 40..420; two cells around.
        assert_eq!(s.bounds, Rect::new(60.0, 0.0, 700.0, 460.0));
    }

    #[test]
    fn a_selection_keeps_the_arrows_between_its_shapes() {
        let d = diagram();
        let ids = ["a".to_string(), "b".to_string()];
        let s = scope(
            &d,
            Objects::Selected {
                shapes: &ids,
                lines: &[],
            },
        )
        .unwrap();
        assert_eq!(s.shapes, [0, 1]);
        assert_eq!(s.connectors, [0], "b → c points at a shape left out");
        assert_eq!(s.bounds, Rect::new(60.0, 60.0, 540.0, 140.0));

        // A selected line glued to a shape left out stays out; a free one is in.
        let s = scope(
            &d,
            Objects::Selected {
                shapes: &ids,
                lines: &[1, 2],
            },
        )
        .unwrap();
        assert_eq!(s.connectors, [0, 2]);
    }

    #[test]
    fn nothing_to_export() {
        assert_eq!(scope(&Diagram::new(), Objects::All), None);
        assert_eq!(
            scope(
                &diagram(),
                Objects::Selected {
                    shapes: &[],
                    lines: &[]
                }
            ),
            None
        );
    }
}
