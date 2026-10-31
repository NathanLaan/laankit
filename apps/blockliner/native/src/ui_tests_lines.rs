//! Connectors, headlessly: drawing lines and arrows that glue to anchors or
//! (dynamically) to shapes, following shapes as they move, free lines on
//! the grid, re-gluing an end by dragging it, selecting, moving and
//! deleting lines, and what all of it writes to Mermaid.
//!
//! `two_shapes` puts `a` at (100, 100) and `b` at (400, 300), 160 × 60: `a`'s
//! anchors are N (180, 100), E (260, 130), S (180, 160), W (100, 130); `b`'s
//! are N (480, 300), E (560, 330), S (480, 360), W (400, 330).

use blockliner_core::{write, Anchor, Connector, ConnectorKind, End, Point, Rect};
use egui::{Key, PointerButton};

use crate::canvas::Tool;
use crate::ui_tests::{at, click, drag, harness, two_shapes, H};

fn points(h: &H, i: usize) -> (Point, Point) {
    let d = h.state().diagram();
    d.connector_points(&d.connectors[i])
        .expect("both ends resolve")
}

#[test]
fn an_arrow_between_anchors() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.key_press(Key::A);
    h.run_steps(2);
    // From near a's E anchor to near b's W anchor.
    drag(
        &mut h,
        PointerButton::Primary,
        at(258.0, 133.0),
        at(403.0, 327.0),
    );
    h.run_steps(2);

    let d = h.state().diagram();
    assert_eq!(
        d.connectors,
        [Connector::new(
            ConnectorKind::Arrow,
            End::glued("a", Some(Anchor::E)),
            End::glued("b", Some(Anchor::W)),
        )]
    );
    assert_eq!(
        h.state().tool(),
        Tool::Select,
        "one line, then back to Select"
    );
    assert_eq!(h.state().selected_lines(), [0]);
    let text = write(d);
    assert!(text.contains("\n  a --> b\n"), "{text}");
    assert!(text.contains("%% @edge a->b from=E to=W\n"), "{text}");
}

#[test]
fn dynamic_glue_follows_the_shapes() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.key_press(Key::L);
    h.run_steps(2);
    // Body to body, away from the anchors: dynamic glue at both ends.
    drag(
        &mut h,
        PointerButton::Primary,
        at(150.0, 120.0),
        at(450.0, 320.0),
    );
    h.run_steps(2);
    assert_eq!(
        h.state().diagram().connectors[0],
        Connector::new(
            ConnectorKind::Line,
            End::glued("a", None),
            End::glued("b", None)
        )
    );
    // b is further right than down: a's E to b's W.
    assert_eq!(
        points(&h, 0),
        (Point::new(260.0, 130.0), Point::new(400.0, 330.0))
    );

    // Move b straight under a: the ends turn to a's S and b's N.
    drag(
        &mut h,
        PointerButton::Primary,
        at(500.0, 320.0),
        at(200.0, 420.0),
    );
    h.run_steps(2);
    let b = h.state().diagram().shape("b").unwrap().rect;
    assert_eq!(b, Rect::new(100.0, 400.0, 160.0, 60.0));
    assert_eq!(
        points(&h, 0),
        (Point::new(180.0, 160.0), Point::new(180.0, 400.0))
    );
    // No fixed anchors, so no `@edge` line.
    let text = write(h.state().diagram());
    assert!(
        text.contains("\n  a --- b\n") && !text.contains("@edge"),
        "{text}"
    );
}

#[test]
fn a_free_line_then_glued_by_its_ends() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.key_press(Key::L);
    h.run_steps(2);
    // On empty canvas: both ends free, on the grid.
    drag(
        &mut h,
        PointerButton::Primary,
        at(603.0, 47.0),
        at(797.0, 47.0),
    );
    h.run_steps(2);
    let d = h.state().diagram();
    assert_eq!(d.connectors[0].from, End::Free(Point::new(600.0, 40.0)));
    assert_eq!(d.connectors[0].to, End::Free(Point::new(800.0, 40.0)));
    assert!(d.has_free_lines());
    assert!(write(d).contains("%% @line line from=600,40 to=800,40\n"));

    // The line is selected: drag its `to` end onto a's N anchor…
    drag(
        &mut h,
        PointerButton::Primary,
        at(800.0, 40.0),
        at(182.0, 102.0),
    );
    h.run_steps(2);
    assert_eq!(
        h.state().diagram().connectors[0].to,
        End::glued("a", Some(Anchor::N))
    );
    assert!(
        h.state().diagram().has_free_lines(),
        "the other end is still free"
    );

    // …and its `from` end onto b's body: now it's a Mermaid edge.
    drag(
        &mut h,
        PointerButton::Primary,
        at(600.0, 40.0),
        at(500.0, 320.0),
    );
    h.run_steps(2);
    let d = h.state().diagram();
    assert_eq!(d.connectors[0].from, End::glued("b", None));
    assert!(!d.has_free_lines());
    let text = write(d);
    assert!(text.contains("\n  b --- a\n"), "{text}");
    assert!(text.contains("%% @edge b->a to=N\n"), "{text}");
}

#[test]
fn a_shape_cannot_connect_to_itself() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.key_press(Key::A);
    h.run_steps(2);
    drag(
        &mut h,
        PointerButton::Primary,
        at(258.0, 130.0),
        at(102.0, 130.0),
    );
    h.run_steps(2);
    assert!(h.state().diagram().connectors.is_empty());
    assert_eq!(
        h.state().tool(),
        Tool::Arrow,
        "nothing made, so the tool stays"
    );
}

#[test]
fn select_move_and_delete_lines() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    {
        let d = h.state_mut().diagram_mut();
        d.connectors.push(Connector::new(
            ConnectorKind::Arrow,
            End::glued("a", None),
            End::glued("b", None),
        ));
        d.connectors.push(Connector::new(
            ConnectorKind::Line,
            End::Free(Point::new(600.0, 40.0)),
            End::Free(Point::new(800.0, 40.0)),
        ));
    }
    h.run_steps(2);

    // Click a line to select it; dragging it moves its free ends.
    click(&mut h, at(700.0, 42.0));
    h.run_steps(2);
    assert_eq!(h.state().selected_lines(), [1]);
    assert!(h.state().selection().is_empty());
    drag(
        &mut h,
        PointerButton::Primary,
        at(700.0, 40.0),
        at(738.0, 83.0),
    );
    h.run_steps(2);
    let c = &h.state().diagram().connectors[1];
    assert_eq!(c.from, End::Free(Point::new(640.0, 80.0)));
    assert_eq!(c.to, End::Free(Point::new(840.0, 80.0)));

    // A box around a, b and the arrow (not the free line) selects all three.
    drag(
        &mut h,
        PointerButton::Primary,
        at(60.0, 60.0),
        at(600.0, 400.0),
    );
    h.run_steps(2);
    assert_eq!(h.state().selection(), ["a", "b"]);
    assert_eq!(h.state().selected_lines(), [0]);

    // Delete just the free line: click it, Delete.
    click(&mut h, at(740.0, 80.0));
    h.run_steps(2);
    h.key_press(Key::Delete);
    h.run_steps(2);
    let d = h.state().diagram();
    assert_eq!(d.connectors.len(), 1);
    assert_eq!(d.shapes.len(), 2);

    // Deleting a shape takes its glued arrow with it.
    click(&mut h, at(150.0, 120.0));
    h.key_press(Key::Delete);
    h.run_steps(2);
    let d = h.state().diagram();
    assert!(d.connectors.is_empty());
    assert_eq!(d.shapes.len(), 1);
}
