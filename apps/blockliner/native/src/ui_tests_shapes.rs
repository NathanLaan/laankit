//! Drawing and editing shapes, headlessly: create by dragging and by
//! clicking, label editing (and ids from first labels), sticky tools, move
//! and resize with and without snapping, box selection, delete, and shapes
//! that the Mermaid file declares in kept statements.
//!
//! The canvas starts at 100 % with the model origin at its top-left corner,
//! just right of the 48 px toolbar, so `at(x, y)` is model (x, y). kittest
//! wraps the app in an 8 px outer margin (the real window has none).

use blockliner_core::{read, Rect, Shape, ShapeKind};
use egui::{pos2, Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;

use crate::app::BlockLinerApp;
use crate::canvas::Tool;
use crate::ui_tests::{drag, harness};

type H = Harness<'static, BlockLinerApp>;

const KITTEST_MARGIN: f32 = 8.0;

fn at(x: f32, y: f32) -> Pos2 {
    pos2(
        KITTEST_MARGIN + marina_egui::toolbar::WIDTH + x,
        KITTEST_MARGIN + y,
    )
}

fn click(h: &mut H, p: Pos2) {
    for pressed in [true, false] {
        h.event(Event::PointerMoved(p));
        h.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
}

fn type_text(h: &mut H, text: &str) {
    h.event(Event::Text(text.into()));
    h.step();
}

fn shape<'a>(h: &'a H, id: &str) -> &'a Shape {
    h.state()
        .diagram()
        .shape(id)
        .unwrap_or_else(|| panic!("no shape {id}"))
}

fn two_shapes(h: &mut H) {
    let d = h.state_mut().diagram_mut();
    d.shapes.push(Shape::new(
        "a",
        ShapeKind::Rect,
        "A",
        Rect::new(100.0, 100.0, 160.0, 60.0),
    ));
    d.shapes.push(Shape::new(
        "b",
        ShapeKind::RoundRect,
        "B",
        Rect::new(400.0, 300.0, 160.0, 60.0),
    ));
    h.run_steps(2);
}

#[test]
fn drag_out_a_shape_and_name_it() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    h.key_press(Key::R);
    h.run_steps(2);
    drag(
        &mut h,
        PointerButton::Primary,
        at(103.0, 103.0),
        at(261.0, 167.0),
    );
    h.run_steps(3);

    // Snapped to the grid, selected, and its label is being edited.
    let d = h.state().diagram();
    assert_eq!(d.shapes.len(), 1);
    assert_eq!(d.shapes[0].kind, ShapeKind::Rect);
    assert_eq!(d.shapes[0].rect, Rect::new(100.0, 100.0, 160.0, 60.0));
    assert!(h.state().editing_label());
    assert_eq!(
        h.state().tool(),
        Tool::Select,
        "one shape, then back to Select"
    );

    type_text(&mut h, "Orders DB");
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert!(!h.state().editing_label());
    // The first label names the node.
    let s = shape(&h, "orders_db");
    assert_eq!(s.label, "Orders DB");
    assert_eq!(h.state().selection(), ["orders_db"]);
}

#[test]
fn click_for_a_default_shape_and_escape() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    h.key_press(Key::O);
    h.run_steps(2);
    click(&mut h, at(300.0, 310.0));
    h.run_steps(3);
    // 8 × 3 cells, centred on the click and snapped.
    let d = h.state().diagram();
    assert_eq!(d.shapes[0].kind, ShapeKind::RoundRect);
    assert_eq!(d.shapes[0].rect, Rect::new(220.0, 280.0, 160.0, 60.0));

    // Esc cancels the label (the shape stays, unnamed), then clears the
    // selection.
    type_text(&mut h, "Never mind");
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!h.state().editing_label());
    assert_eq!(shape(&h, "node").label, "");
    assert_eq!(h.state().selection(), ["node"]);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().selection().is_empty());
}

#[test]
fn a_tool_picked_with_shift_stays() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    h.key_press_modifiers(Modifiers::SHIFT, Key::T);
    h.run_steps(2);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    click(&mut h, at(200.0, 200.0));
    h.run_steps(3);
    type_text(&mut h, "First");
    // Clicking elsewhere commits the label and places the next text box.
    click(&mut h, at(600.0, 400.0));
    h.run_steps(3);
    assert_eq!(h.state().tool(), Tool::Text);
    let d = h.state().diagram();
    assert_eq!(d.shapes.len(), 2);
    assert_eq!(d.shapes[0].id, "first");
    assert!(d.shapes.iter().all(|s| s.kind == ShapeKind::Text));
    // 10 × 2 cells for text.
    assert_eq!(d.shapes[1].rect, Rect::new(500.0, 380.0, 200.0, 40.0));
}

#[test]
fn move_resize_box_select_and_delete() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    assert_eq!(h.state().canvas().origin(), at(0.0, 0.0));
    two_shapes(&mut h);

    // Drag `a` by (47, 18): the move snaps to whole cells.
    drag(
        &mut h,
        PointerButton::Primary,
        at(180.0, 130.0),
        at(227.0, 148.0),
    );
    h.run_steps(2);
    assert_eq!(h.state().selection(), ["a"]);
    assert_eq!(shape(&h, "a").rect, Rect::new(140.0, 120.0, 160.0, 60.0));

    // With Alt, exactly where it's put.
    h.event(Event::ModifiersChanged(Modifiers::ALT));
    drag(
        &mut h,
        PointerButton::Primary,
        at(200.0, 150.0),
        at(225.0, 163.0),
    );
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    assert_eq!(shape(&h, "a").rect, Rect::new(165.0, 133.0, 160.0, 60.0));

    // The bottom-right handle: the moved edges snap to the grid.
    drag(
        &mut h,
        PointerButton::Primary,
        at(325.0, 193.0),
        at(358.0, 195.0),
    );
    h.run_steps(2);
    assert_eq!(shape(&h, "a").rect, Rect::new(165.0, 133.0, 195.0, 67.0));

    // Box-select both, then delete them.
    drag(
        &mut h,
        PointerButton::Primary,
        at(20.0, 20.0),
        at(700.0, 500.0),
    );
    h.run_steps(2);
    assert_eq!(h.state().selection(), ["a", "b"]);
    h.key_press(Key::Delete);
    h.run_steps(2);
    assert!(h.state().diagram().shapes.is_empty());
    assert!(h.state().selection().is_empty());
}

#[test]
fn shift_click_and_double_click_to_edit() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    click(&mut h, at(150.0, 120.0));
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    click(&mut h, at(450.0, 320.0));
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    assert_eq!(h.state().selection(), ["a", "b"]);

    // Double-click `b` (after a pause, so it isn't a triple click):
    // Shift+Enter is a new line, Enter commits.
    h.run_steps(30);
    click(&mut h, at(450.0, 320.0));
    click(&mut h, at(450.0, 320.0));
    h.run_steps(3);
    assert!(h.state().editing_label());
    h.key_press(Key::End);
    type_text(&mut h, " one");
    h.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
    type_text(&mut h, "two");
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(shape(&h, "b").label, "B one\ntwo");
    // An existing shape keeps its id when relabelled.
    assert_eq!(h.state().diagram().shapes[1].id, "b");
}

#[test]
fn shapes_from_kept_statements_are_not_editable() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    *h.state_mut().diagram_mut() =
        read("flowchart LR\n  a --> b\n  b -.-> c\n").expect("a flowchart");
    h.run_steps(2);
    let c = shape(&h, "c").rect;
    assert!(shape(&h, "c").external);

    // Double-clicking `c` doesn't edit it.
    let mid = at(c.x + c.w / 2.0, c.y + c.h / 2.0);
    click(&mut h, mid);
    click(&mut h, mid);
    h.run_steps(3);
    assert!(!h.state().editing_label());

    // Select everything and delete: `c` stays (its declaration is in the
    // kept `-.->` statement), and the a → b edge went with its shapes.
    drag(
        &mut h,
        PointerButton::Primary,
        at(5.0, 5.0),
        at(1100.0, 700.0),
    );
    h.run_steps(2);
    assert_eq!(h.state().selection().len(), 3);
    h.key_press(Key::Delete);
    h.run_steps(2);
    let d = h.state().diagram();
    let ids: Vec<_> = d.shapes.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["c"]);
    assert!(d.connectors.is_empty());
}
