//! The stacking order, headlessly: the toolbar's Order button and its list,
//! the right-click menu's four moves, their keys and palette commands, what
//! a click picks afterwards, undo, and the saved order.

use blockliner_core::{write, Rect, Shape, ShapeKind};
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};

use crate::ui_tests::{at, click, harness, H};

/// `a` and `b` overlap around (230, 140), with `b` in front; `c` is off on
/// its own, in front of both.
fn stack(h: &mut H) {
    let d = h.state_mut().diagram_mut();
    for (id, x, y) in [
        ("a", 100.0, 100.0),
        ("b", 200.0, 120.0),
        ("c", 500.0, 400.0),
    ] {
        d.shapes.push(Shape::new(
            id,
            ShapeKind::Rect,
            id.to_uppercase(),
            Rect::new(x, y, 160.0, 60.0),
        ));
    }
    h.run_steps(2);
}

fn order(h: &H) -> String {
    h.state()
        .diagram()
        .shapes
        .iter()
        .map(|s| s.id.as_str())
        .collect()
}

/// A point where only `a` is, and one where `a` and `b` overlap.
const ONLY_A: (f32, f32) = (110.0, 110.0);
const OVERLAP: (f32, f32) = (230.0, 140.0);

fn click_at(h: &mut H, (x, y): (f32, f32)) {
    click(h, at(x, y));
    h.run_steps(2);
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

fn open_order(h: &mut H) {
    h.get_by_role_and_label(Role::Button, "Order").click();
    h.run_steps(3);
}

/// Whether the Order list's row `name` is greyed.
fn row_off(h: &H, name: &str) -> bool {
    h.get_by(|n| n.role() == Role::MenuItemRadio && n.label().as_deref() == Some(name))
        .accesskit_node()
        .is_disabled()
}

fn pick(h: &mut H, name: &str) {
    h.get_by(|n| n.role() == Role::MenuItemRadio && n.label().as_deref() == Some(name))
        .click();
    h.run_steps(3);
}

fn keys(h: &mut H, modifiers: Modifiers, key: Key) {
    h.key_press_modifiers(modifiers, key);
    h.run_steps(2);
}

const SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);

#[test]
fn the_order_button_moves_the_shape_a_click_picks() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    stack(&mut h);

    // With nothing selected, every move is off.
    open_order(&mut h);
    for name in ["Move to Front", "Move Up", "Move Down", "Move to Back"] {
        assert!(row_off(&h, name), "{name}");
    }
    h.key_press(Key::Escape);
    h.run_steps(2);

    // The click takes the shape in front.
    click_at(&mut h, OVERLAP);
    assert_eq!(h.state().selection(), ["b"]);
    open_order(&mut h);
    // b is over a, with nothing it overlaps in front: it can go down, and
    // to the front past c, but not up.
    assert!(row_off(&h, "Move Up"));
    assert!(!row_off(&h, "Move to Front"));
    assert!(!row_off(&h, "Move Down"));
    pick(&mut h, "Move to Back");
    assert_eq!(order(&h), "bac");
    // Still selected, and now behind a.
    assert_eq!(h.state().selection(), ["b"]);
    click_at(&mut h, (700.0, 100.0));
    click_at(&mut h, OVERLAP);
    assert_eq!(h.state().selection(), ["a"]);

    // The file has the nodes, and their places, in the new order.
    let text = write(h.state().diagram());
    let line = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("{needle}"));
    assert!(line("b[\"B\"]") < line("a[\"A\"]"));
    assert!(line("@shape b ") < line("@shape a "));

    // One undo step.
    keys(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(order(&h), "abc");
}

#[test]
fn the_right_click_menu_and_the_keys_move_shapes() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    stack(&mut h);

    // Move Up steps over b, the shape covering a, and no further.
    right_click(&mut h, at(ONLY_A.0, ONLY_A.1));
    assert_eq!(h.state().selection(), ["a"]);
    h.get_by_role_and_label(Role::Button, "Move Up Ctrl+]")
        .click();
    h.run_steps(3);
    assert_eq!(order(&h), "bac");

    // Nothing covers it now: Move Up is off, Move to Front is not.
    right_click(&mut h, at(ONLY_A.0, ONLY_A.1));
    assert!(h
        .get_by_role_and_label(Role::Button, "Move Up Ctrl+]")
        .accesskit_node()
        .is_disabled());
    h.get_by_role_and_label(Role::Button, "Move to Front Ctrl+Shift+]")
        .click();
    h.run_steps(3);
    assert_eq!(order(&h), "bca");

    // The keys: Ctrl+[ and Ctrl+], with Shift for the back and the front.
    keys(&mut h, Modifiers::COMMAND, Key::OpenBracket);
    assert_eq!(order(&h), "abc", "down, behind b");
    keys(&mut h, Modifiers::COMMAND, Key::CloseBracket);
    assert_eq!(order(&h), "bac");
    keys(&mut h, SHIFT, Key::CloseBracket);
    assert_eq!(order(&h), "bca");
    keys(&mut h, SHIFT, Key::OpenBracket);
    assert_eq!(order(&h), "abc");
    // A key whose move is off does nothing.
    keys(&mut h, SHIFT, Key::OpenBracket);
    assert_eq!(order(&h), "abc");

    // Each was its own undo step.
    keys(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(order(&h), "bca");
}

#[test]
fn the_palette_lists_the_moves_that_apply() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    stack(&mut h);
    click_at(&mut h, ONLY_A);
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(3);
    h.event(Event::Text("Move to Front".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(order(&h), "bca");

    // A line selected with the shapes stays where it is, and lines alone
    // have nothing to move.
    let d = h.state_mut().diagram_mut();
    d.connectors.push(blockliner_core::Connector::new(
        blockliner_core::ConnectorKind::Line,
        blockliner_core::End::Free(blockliner_core::Point::new(40.0, 300.0)),
        blockliner_core::End::Free(blockliner_core::Point::new(200.0, 300.0)),
    ));
    h.run_steps(2);
    click_at(&mut h, (120.0, 300.0));
    assert_eq!(h.state().selected_lines(), [0]);
    assert!(h.state().selection().is_empty());
    keys(&mut h, SHIFT, Key::CloseBracket);
    assert_eq!(order(&h), "bca");
    open_order(&mut h);
    assert!(row_off(&h, "Move to Back"));
}
