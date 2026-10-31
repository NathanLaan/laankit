//! Undo, redo and the keyboard, headlessly: one undo step per finished
//! change (creating and naming a shape, a whole drag, each nudge), redo and
//! its trail, select all, and nudging with the arrow keys (Shift: 5 steps,
//! Alt: 1 px), including free line ends.

use blockliner_core::{Connector, ConnectorKind, End, Point, Rect};
use egui::{Event, Key, Modifiers, PointerButton};

use crate::ui_tests::{at, click, drag, harness, two_shapes, H};

fn rect(h: &H, id: &str) -> Rect {
    h.state().diagram().shape(id).expect("the shape").rect
}

fn undo(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
}

fn redo(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.run_steps(2);
}

#[test]
fn creating_and_naming_a_shape_is_one_step() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    h.key_press(Key::R);
    h.run_steps(2);
    drag(
        &mut h,
        PointerButton::Primary,
        at(100.0, 100.0),
        at(260.0, 160.0),
    );
    h.run_steps(3);
    h.event(Event::Text("API".into()));
    h.step();
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().diagram().shapes[0].id, "api");

    undo(&mut h);
    assert!(
        h.state().diagram().shapes.is_empty(),
        "shape and label together"
    );
    redo(&mut h);
    let d = h.state().diagram();
    assert_eq!(
        (d.shapes[0].id.as_str(), d.shapes[0].label.as_str()),
        ("api", "API")
    );

    // Ctrl+Y redoes too, and there's nothing left to redo.
    undo(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run_steps(2);
    assert_eq!(h.state().diagram().shapes.len(), 1);
    redo(&mut h);
    assert_eq!(h.state().diagram().shapes.len(), 1);
}

#[test]
fn a_drag_is_one_step_and_a_new_change_ends_redo() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    let start = rect(&h, "a");
    // The drag passes through several positions, but undoes in one step.
    drag(
        &mut h,
        PointerButton::Primary,
        at(150.0, 120.0),
        at(250.0, 180.0),
    );
    h.run_steps(2);
    assert_eq!(rect(&h, "a"), start.translated(100.0, 60.0));
    undo(&mut h);
    assert_eq!(rect(&h, "a"), start);

    // A new change after an undo drops what could have been redone.
    click(&mut h, at(450.0, 320.0));
    h.key_press(Key::Delete);
    h.run_steps(2);
    redo(&mut h);
    assert_eq!(rect(&h, "a"), start);
    assert!(h.state().diagram().shape("b").is_none());
    undo(&mut h);
    assert!(h.state().diagram().shape("b").is_some());
}

#[test]
fn select_all_and_nudge() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.state_mut().diagram_mut().connectors.push(Connector::new(
        ConnectorKind::Arrow,
        End::glued("a", None),
        End::Free(Point::new(700.0, 100.0)),
    ));
    h.run_steps(2);

    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
    assert_eq!(h.state().selection(), ["a", "b"]);
    assert_eq!(h.state().selected_lines(), [0]);

    let (a, b) = (rect(&h, "a"), rect(&h, "b"));
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowDown);
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::ALT, Key::ArrowLeft);
    h.run_steps(2);
    // One grid step right, five down, one pixel left; the free end too.
    assert_eq!(rect(&h, "a"), a.translated(19.0, 100.0));
    assert_eq!(rect(&h, "b"), b.translated(19.0, 100.0));
    assert_eq!(
        h.state().diagram().connectors[0].to,
        End::Free(Point::new(719.0, 200.0))
    );

    // Each nudge is its own step.
    undo(&mut h);
    assert_eq!(rect(&h, "a"), a.translated(20.0, 100.0));
    undo(&mut h);
    undo(&mut h);
    assert_eq!(rect(&h, "a"), a);
    assert_eq!(
        h.state().diagram().connectors[0].to,
        End::Free(Point::new(700.0, 100.0))
    );
}

#[test]
fn arrows_do_nothing_without_a_selection() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    let a = rect(&h, "a");
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(rect(&h, "a"), a);
}
