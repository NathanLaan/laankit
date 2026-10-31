//! Copy and paste. The clipboard holds the copied objects as a small
//! BlockLiner diagram written as Mermaid (`mermaid::write`), so a copy
//! pastes into a README as a flowchart, and back into BlockLiner with its
//! layout. A flowchart copied from elsewhere reads like an imported file
//! (laid out by `layout`); other text becomes a text box.

use crate::ids;
use crate::mermaid::read;
use crate::model::{Diagram, End, Point, Rect, Route, Shape, ShapeKind};
use crate::snap::snap;

/// Grid cells each paste of the same copy steps, down and right.
pub const STEP_CELLS: f32 = 2.0;

/// The selected shapes (by id) and lines (by index) as a diagram of their
/// own, or `None` when nothing is selected.
///
/// Connectors come along when they're selected or glued at both ends to
/// copied shapes. An end glued to a shape that isn't copied becomes a free
/// end where it is now, so a selected line is never lost. Shapes declared
/// in a kept statement are copied as ordinary shapes, declared by the copy.
pub fn copy(d: &Diagram, shapes: &[String], lines: &[usize]) -> Option<Diagram> {
    let mut clip = Diagram {
        direction: d.direction,
        grid: d.grid,
        ..Diagram::new()
    };
    clip.shapes = d
        .shapes
        .iter()
        .filter(|s| shapes.contains(&s.id))
        .map(|s| Shape {
            external: false,
            ..s.clone()
        })
        .collect();
    let copied = |e: &End| e.shape().is_some_and(|id| shapes.iter().any(|s| s == id));
    for (i, c) in d.connectors.iter().enumerate() {
        if !lines.contains(&i) && !(copied(&c.from) && copied(&c.to)) {
            continue;
        }
        let Some(path) = d.connector_path(c) else {
            continue;
        };
        let (Some(&first), Some(&last)) = (path.first(), path.last()) else {
            continue;
        };
        let mut c = c.clone();
        for (end, at) in [(&mut c.from, first), (&mut c.to, last)] {
            if end.shape().is_some() && !copied(end) {
                *end = End::Free(at);
            }
        }
        clip.connectors.push(c);
    }
    (!clip.shapes.is_empty() || !clip.connectors.is_empty()).then_some(clip)
}

/// What clipboard text pastes as: a flowchart with something in it (laid
/// out where it gives no positions), else `None` (plain text).
pub fn parse(text: &str) -> Option<Diagram> {
    read(text)
        .ok()
        .filter(|d| !d.shapes.is_empty() || !d.connectors.is_empty())
}

/// The extent of everything in `d`: shapes and connector paths.
pub fn extent(d: &Diagram) -> Option<Rect> {
    let mut points: Vec<Point> = Vec::new();
    for s in &d.shapes {
        points.push(Point::new(s.rect.x, s.rect.y));
        points.push(Point::new(s.rect.right(), s.rect.bottom()));
    }
    for c in &d.connectors {
        points.extend(d.connector_path(c).unwrap_or_default());
    }
    let first = points.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
    for p in &points {
        (x0, y0) = (x0.min(p.x), y0.min(p.y));
        (x1, y1) = (x1.max(p.x), y1.max(p.y));
    }
    Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
}

/// How far the `n`th paste of `clip` (1 for the first) moves it: two grid
/// cells down and right per paste, or, if that would leave `view` (another
/// diagram, or the originals scrolled away), to the middle of `view`.
/// Whole grid cells, so snapped shapes stay snapped.
pub fn offset(clip: &Diagram, n: usize, view: Rect, grid: f32) -> (f32, f32) {
    let Some(e) = extent(clip) else {
        return (0.0, 0.0);
    };
    let step = STEP_CELLS * grid * n as f32;
    if view.contains_rect(&e.translated(step, step)) {
        return (step, step);
    }
    let (c, v) = (e.center(), view.center());
    (snap(v.x - c.x, grid), snap(v.y - c.y, grid))
}

/// Adds `clip` to `d`, moved by (`dx`, `dy`). Shapes whose ids are taken get
/// new ones (`orders` → `orders_2`) and their connectors follow. Returns the
/// new shapes' ids and connectors' indices, to select.
pub fn paste(d: &mut Diagram, clip: &Diagram, dx: f32, dy: f32) -> (Vec<String>, Vec<usize>) {
    let mut ids: Vec<(String, String)> = Vec::new();
    for s in &clip.shapes {
        let new = ids::unique(&s.id, |id| {
            d.shape(id).is_some() || ids.iter().any(|(_, n)| n == id)
        });
        ids.push((s.id.clone(), new.clone()));
        d.shapes.push(Shape {
            id: new,
            rect: s.rect.translated(dx, dy),
            external: false,
            ..s.clone()
        });
    }
    let renamed = |id: &str| {
        ids.iter()
            .find(|(old, _)| old == id)
            .map(|(_, new)| new.clone())
    };
    let first = d.connectors.len();
    for c in &clip.connectors {
        let mut c = c.clone();
        for end in [&mut c.from, &mut c.to] {
            *end = match end {
                End::Glued { shape, anchor } => match renamed(shape) {
                    Some(shape) => End::Glued {
                        shape,
                        anchor: *anchor,
                    },
                    // Glued to nothing that came along: not pasteable.
                    None => continue,
                },
                End::Free(p) => End::Free(Point::new(p.x + dx, p.y + dy)),
            };
        }
        if let Route::Elbow { mid: Some(mid) } = c.route {
            c.route = Route::Elbow {
                mid: Some(mid.translated(dx, dy)),
            };
        }
        let resolves = |e: &End| e.shape().is_none_or(|id| d.shape(id).is_some());
        if resolves(&c.from) && resolves(&c.to) {
            d.connectors.push(c);
        }
    }
    let new = ids.into_iter().map(|(_, new)| new).collect();
    (new, (first..d.connectors.len()).collect())
}

/// Pastes plain text as a new text box centred on `at`, sized to the text
/// (whole grid cells, at least the default 10 × 2). Returns its id.
pub fn paste_text(d: &mut Diagram, text: &str, at: Point) -> Option<String> {
    let label = text.replace("\r\n", "\n");
    let label = label.trim_end();
    if label.trim().is_empty() {
        return None;
    }
    let g = d.grid;
    let (min_w, min_h) = Shape::default_size(&ShapeKind::Text, g);
    // About 8 px a character and a cell a line, at the default grid.
    let longest = label.lines().map(|l| l.chars().count()).max().unwrap_or(0);
    let w = ((longest as f32 * 8.0 / g).ceil() * g).clamp(min_w, 30.0 * g);
    let h = ((label.lines().count() as f32 + 1.0) * g).max(min_h);
    let rect = Rect::new(snap(at.x - w / 2.0, g), snap(at.y - h / 2.0, g), w, h);
    Some(d.add_shape(ShapeKind::Text, rect, label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Anchor, Connector, ConnectorKind, Mid};
    use crate::style::Style;
    use crate::write;

    /// `a` (100, 100) and `b` (400, 100), 160 × 60, joined by an elbow
    /// arrow with a label and a pinned middle; `c` (100, 300), joined to `a`
    /// by a line; and a free line.
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
                id.to_uppercase(),
                Rect::new(x, y, 160.0, 60.0),
            ));
        }
        d.shapes[1].style = Style::parse("fill:#dbeafe,stroke:#3b82f6").unwrap();
        let mut arrow = Connector::new(
            ConnectorKind::Arrow,
            End::glued("a", Some(Anchor::E)),
            End::glued("b", Some(Anchor::W)),
        )
        .with_route(Route::Elbow {
            mid: Some(Mid::X(320.0)),
        });
        arrow.label = Some("HTTPS".into());
        d.connectors.push(arrow);
        d.connectors.push(Connector::new(
            ConnectorKind::Line,
            End::glued("a", Some(Anchor::S)),
            End::glued("c", Some(Anchor::N)),
        ));
        d.connectors.push(Connector::new(
            ConnectorKind::Line,
            End::Free(Point::new(600.0, 400.0)),
            End::Free(Point::new(700.0, 400.0)),
        ));
        d
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn copying_takes_connectors_between_copied_shapes() {
        let d = diagram();
        assert!(copy(&d, &[], &[]).is_none());
        let clip = copy(&d, &ids(&["a", "b"]), &[]).unwrap();
        assert_eq!(clip.shapes.len(), 2);
        // The arrow between them, with its route, label and style; not
        // the line to c, nor the free line.
        assert_eq!(clip.connectors, d.connectors[..1]);
        assert_eq!(clip.shapes[1].style, d.shapes[1].style);
    }

    #[test]
    fn an_end_on_a_shape_left_behind_becomes_free() {
        let d = diagram();
        // The a → c line alone, and a: its c end is freed where it is.
        let clip = copy(&d, &ids(&["a"]), &[1]).unwrap();
        assert_eq!(clip.connectors.len(), 1);
        assert_eq!(clip.connectors[0].from, End::glued("a", Some(Anchor::S)));
        assert_eq!(clip.connectors[0].to, End::Free(Point::new(180.0, 300.0)));
        // A line without its shapes: both ends free.
        let clip = copy(&d, &[], &[0]).unwrap();
        assert!(clip.shapes.is_empty());
        assert!(matches!(clip.connectors[0].from, End::Free(_)));
        assert!(matches!(clip.connectors[0].to, End::Free(_)));
    }

    #[test]
    fn external_shapes_are_copied_as_ordinary_ones() {
        let mut d = diagram();
        d.shapes[0].external = true;
        let clip = copy(&d, &ids(&["a"]), &[]).unwrap();
        assert!(!clip.shapes[0].external);
        assert!(write(&clip).contains("  a[\"A\"]\n"));
    }

    #[test]
    fn the_clipboard_text_reads_back_as_the_copy() {
        let d = diagram();
        let clip = copy(&d, &ids(&["a", "b", "c"]), &[2]).unwrap();
        let text = write(&clip);
        assert!(text.starts_with("flowchart LR\n"), "{text}");
        assert!(text.contains("a -->|\"HTTPS\"| b"), "{text}");
        assert_eq!(parse(&text).unwrap(), clip);
    }

    #[test]
    fn pasting_renames_taken_ids_and_moves_everything() {
        let mut d = diagram();
        let clip = copy(&d, &ids(&["a", "b"]), &[2]).unwrap();
        let (shapes, lines) = paste(&mut d, &clip, 40.0, 40.0);
        assert_eq!(shapes, ["a_2", "b_2"]);
        assert_eq!(lines, [3, 4]);
        assert_eq!(
            d.shape("a_2").unwrap().rect,
            Rect::new(140.0, 140.0, 160.0, 60.0)
        );
        assert_eq!(d.shape("b_2").unwrap().style, d.shape("b").unwrap().style);
        let arrow = &d.connectors[3];
        assert_eq!(arrow.from, End::glued("a_2", Some(Anchor::E)));
        assert_eq!(arrow.to, End::glued("b_2", Some(Anchor::W)));
        assert_eq!(arrow.label.as_deref(), Some("HTTPS"));
        assert_eq!(
            arrow.route,
            Route::Elbow {
                mid: Some(Mid::X(360.0))
            }
        );
        assert_eq!(d.connectors[4].from, End::Free(Point::new(640.0, 440.0)));

        // Again: the next free ids.
        let (shapes, _) = paste(&mut d, &clip, 80.0, 80.0);
        assert_eq!(shapes, ["a_3", "b_3"]);
        // Into an empty diagram the ids stay.
        let mut empty = Diagram::new();
        assert_eq!(paste(&mut empty, &clip, 0.0, 0.0).0, ["a", "b"]);
    }

    #[test]
    fn pastes_step_until_they_leave_the_view() {
        let d = diagram();
        let clip = copy(&d, &ids(&["a"]), &[]).unwrap();
        let view = Rect::new(0.0, 0.0, 600.0, 400.0);
        assert_eq!(offset(&clip, 1, view, 20.0), (40.0, 40.0));
        assert_eq!(offset(&clip, 2, view, 20.0), (80.0, 80.0));
        // a is at y 100..160: six pastes (+240) still fit, eight don't;
        // then it's centred: its centre (180, 130) → the view's (300, 200),
        // to the nearest cell.
        assert_eq!(offset(&clip, 6, view, 20.0), (240.0, 240.0));
        assert_eq!(offset(&clip, 8, view, 20.0), (120.0, 80.0));
        // Scrolled away: centred.
        let far = Rect::new(2000.0, 1000.0, 600.0, 400.0);
        assert_eq!(offset(&clip, 1, far, 20.0), (2120.0, 1080.0));
    }

    #[test]
    fn a_plain_flowchart_pastes_laid_out() {
        let clip = parse("graph TD\n  web[Web] --> api(API)\n").unwrap();
        assert_eq!(clip.shapes.len(), 2);
        assert!(clip.shapes[0].rect.bottom() <= clip.shapes[1].rect.y);
        let mut d = diagram();
        let (shapes, lines) = paste(&mut d, &clip, 0.0, 0.0);
        assert_eq!(shapes, ["web", "api"]);
        assert_eq!(d.connectors[lines[0]].to, End::glued("api", None));
        // Not flowcharts: plain text.
        for text in [
            "",
            "flowchart LR\n",
            "hello world",
            "sequenceDiagram\n  a->>b: hi",
        ] {
            assert!(parse(text).is_none(), "{text:?}");
        }
    }

    #[test]
    fn plain_text_becomes_a_text_box() {
        let mut d = Diagram::new();
        assert_eq!(paste_text(&mut d, "  \n", Point::new(0.0, 0.0)), None);
        let id = paste_text(
            &mut d,
            "Retries are async\r\nmaybe\n",
            Point::new(300.0, 200.0),
        )
        .unwrap();
        let s = d.shape(&id).unwrap();
        assert_eq!(
            (s.kind.clone(), s.label.as_str()),
            (ShapeKind::Text, "Retries are async\nmaybe")
        );
        // 17 characters → 7 cells, so the 10-cell minimum; 2 lines → 3 cells.
        assert_eq!(s.rect, Rect::new(200.0, 180.0, 200.0, 60.0));
        let long = "x".repeat(40);
        let id = paste_text(&mut d, &long, Point::new(300.0, 200.0)).unwrap();
        assert_eq!(d.shape(&id).unwrap().rect.w, 320.0);
    }
}
