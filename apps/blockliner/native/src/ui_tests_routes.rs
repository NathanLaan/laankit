//! Right-angle connectors, headlessly: drawing them with their tools,
//! dragging the middle segment (and a double-click back to automatic),
//! switching selected connectors between straight and right-angle from the
//! right-click menu and the palette, undo, and what the file says.
//!
//! `two_shapes` puts `a` at (100, 100) and `b` at (400, 300), 160 × 60: `a`'s
//! E anchor is (260, 130), `b`'s W anchor (400, 330).

use blockliner_core::{write, Mid, Point, Route};
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;

use crate::canvas::Tool;
use crate::ui_tests::{at, click, drag, harness, two_shapes, H};

fn path(h: &H, i: usize) -> Vec<Point> {
    let d = h.state().diagram();
    d.connector_path(&d.connectors[i])
        .expect("both ends resolve")
}

fn pts(v: &[(f32, f32)]) -> Vec<Point> {
    v.iter().map(|&(x, y)| Point::new(x, y)).collect()
}

fn right_click(h: &mut H, p: Pos2) {
    for pressed in [true, false] {
        h.event(Event::PointerMoved(p));
        h.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(2);
}

/// A right-angle arrow from a's E anchor to b's W anchor.
fn elbow_arrow(h: &mut H) {
    h.key_press(Key::E);
    h.run_steps(2);
    assert_eq!(h.state().tool(), Tool::ElbowArrow);
    drag(
        h,
        PointerButton::Primary,
        at(258.0, 133.0),
        at(403.0, 327.0),
    );
    h.run_steps(2);
}

#[test]
fn draw_drag_the_middle_and_undo() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    elbow_arrow(&mut h);

    assert_eq!(h.state().tool(), Tool::Select, "one line, then Select");
    assert_eq!(h.state().diagram().connectors[0].route, Route::ELBOW);
    // Out east one cell, down at the midway x (on the grid), in from the west.
    assert_eq!(
        path(&h, 0),
        pts(&[
            (260.0, 130.0),
            (340.0, 130.0),
            (340.0, 330.0),
            (400.0, 330.0)
        ])
    );
    let text = write(h.state().diagram());
    assert!(text.contains("\n  a --> b\n"), "Mermaid unchanged: {text}");
    assert!(
        text.contains("%% @edge a->b from=E to=W route=elbow\n"),
        "{text}"
    );

    // The selected line's middle handle sits half-way down the middle
    // segment; dragging it moves that segment across, on the grid.
    drag(
        &mut h,
        PointerButton::Primary,
        at(340.0, 230.0),
        at(297.0, 260.0),
    );
    h.run_steps(2);
    assert_eq!(
        h.state().diagram().connectors[0].route,
        Route::Elbow {
            mid: Some(Mid::X(300.0))
        }
    );
    assert_eq!(
        path(&h, 0),
        pts(&[
            (260.0, 130.0),
            (300.0, 130.0),
            (300.0, 330.0),
            (400.0, 330.0)
        ])
    );
    assert!(write(h.state().diagram()).contains("route=elbow mid=x:300\n"));

    // One undo step puts it back; redo again.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().diagram().connectors[0].route, Route::ELBOW);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.run_steps(2);
    assert_eq!(path(&h, 0)[1], Point::new(300.0, 130.0));

    // A double-click on the handle: automatic again.
    click(&mut h, at(300.0, 230.0));
    click(&mut h, at(300.0, 230.0));
    h.run_steps(2);
    assert_eq!(h.state().diagram().connectors[0].route, Route::ELBOW);
}

#[test]
fn a_pinned_middle_moves_with_the_selection() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    elbow_arrow(&mut h);
    drag(
        &mut h,
        PointerButton::Primary,
        at(340.0, 230.0),
        at(300.0, 230.0),
    );
    h.run_steps(2);

    // Everything one cell right: the middle segment comes along.
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(
        h.state().diagram().connectors[0].route,
        Route::Elbow {
            mid: Some(Mid::X(320.0))
        }
    );
    assert_eq!(path(&h, 0)[0], Point::new(280.0, 130.0));
}

#[test]
fn switch_between_straight_and_right_angle() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.key_press(Key::L);
    h.run_steps(2);
    drag(
        &mut h,
        PointerButton::Primary,
        at(258.0, 133.0),
        at(403.0, 327.0),
    );
    h.run_steps(2);
    assert_eq!(h.state().diagram().connectors[0].route, Route::Straight);
    assert_eq!(path(&h, 0).len(), 2);

    // Nothing selected; a right-click on the line selects it and offers
    // Right-Angle (Straight is off: it already is).
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().selected_lines().is_empty());
    right_click(&mut h, at(330.0, 230.0));
    assert_eq!(h.state().selected_lines(), [0]);
    h.get_by_label("Right-Angle").click();
    h.run_steps(3);
    assert_eq!(h.state().diagram().connectors[0].route, Route::ELBOW);
    assert_eq!(path(&h, 0).len(), 4);

    // And back from the palette; undo makes it right-angle again.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(3);
    h.event(Event::Text("Make Connectors Straight".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().diagram().connectors[0].route, Route::Straight);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().diagram().connectors[0].route, Route::ELBOW);

    // A right-click on empty canvas clears the selection; Delete is off
    // there, Select All isn't.
    right_click(&mut h, at(700.0, 600.0));
    assert!(h.state().selected_lines().is_empty());
    h.get_by_label_contains("Select All").click();
    h.run_steps(3);
    assert_eq!(h.state().selected_lines(), [0]);
}
