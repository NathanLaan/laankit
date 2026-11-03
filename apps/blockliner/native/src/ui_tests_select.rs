//! Selecting by kind, headlessly: Ctrl+Shift+A (every shape but the text
//! boxes), Ctrl+Alt+A (every line) and Ctrl+Shift+T (every text box), from
//! the keyboard, the right-click menu and the palette; and that A, Shift+A,
//! T and Ctrl+A still do what they did.
//!
//! `two_shapes` puts `a` at (100, 100) and `b` at (400, 300), 160 × 60.

use blockliner_core::{Anchor, Connector, ConnectorKind, End, Point, Rect, ShapeKind};
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;

use crate::canvas::Tool;
use crate::ui_tests::{at, harness, two_shapes, H};

/// `a`, `b`, a text box `note` at (100, 400), an edge a→b and a free line.
fn mixed(h: &mut H) -> String {
    two_shapes(h);
    let d = h.state_mut().diagram_mut();
    let note = d.add_shape(
        ShapeKind::Text,
        Rect::new(100.0, 400.0, 160.0, 40.0),
        "Note",
    );
    d.connectors.push(Connector::new(
        ConnectorKind::Arrow,
        End::glued("a", Some(Anchor::E)),
        End::glued("b", Some(Anchor::W)),
    ));
    d.connectors.push(Connector::new(
        ConnectorKind::Line,
        End::Free(Point::new(600.0, 100.0)),
        End::Free(Point::new(700.0, 100.0)),
    ));
    h.run_steps(2);
    note
}

fn keys(h: &mut H, modifiers: Modifiers, key: Key) {
    h.key_press_modifiers(modifiers, key);
    h.run_steps(2);
}

fn selected(h: &H) -> (Vec<String>, Vec<usize>) {
    let mut shapes = h.state().selection().to_vec();
    shapes.sort();
    (shapes, h.state().selected_lines().to_vec())
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

const SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);
const ALT: Modifiers = Modifiers::COMMAND.plus(Modifiers::ALT);

#[test]
fn the_keys_select_shapes_lines_or_text() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    let note = mixed(&mut h);

    keys(&mut h, SHIFT, Key::A);
    assert_eq!(selected(&h), (vec!["a".into(), "b".into()], vec![]));

    keys(&mut h, ALT, Key::A);
    assert_eq!(selected(&h), (vec![], vec![0, 1]));

    keys(&mut h, SHIFT, Key::T);
    assert_eq!(selected(&h), (vec![note.clone()], vec![]));

    // Ctrl+A still takes everything.
    keys(&mut h, Modifiers::COMMAND, Key::A);
    assert_eq!(
        selected(&h),
        (vec!["a".into(), "b".into(), note], vec![0, 1])
    );
}

#[test]
fn the_tool_letters_are_unchanged() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    mixed(&mut h);
    keys(&mut h, Modifiers::NONE, Key::A);
    assert_eq!(h.state().tool(), Tool::Arrow);
    keys(&mut h, Modifiers::SHIFT, Key::T);
    assert_eq!(h.state().tool(), Tool::Text);
    assert_eq!(selected(&h), (vec![], vec![]));
}

#[test]
fn the_menu_and_palette_select_by_kind() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    let note = mixed(&mut h);

    right_click(&mut h, at(700.0, 600.0));
    h.get_by_label("Select All Text Ctrl+Shift+T").click();
    h.run_steps(3);
    assert_eq!(selected(&h), (vec![note], vec![]));

    right_click(&mut h, at(700.0, 600.0));
    h.get_by_label("Select All Lines Ctrl+Alt+A").click();
    h.run_steps(3);
    assert_eq!(selected(&h), (vec![], vec![0, 1]));

    keys(&mut h, Modifiers::COMMAND, Key::K);
    h.run_steps(1);
    h.event(Event::Text("Select All Shapes".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(selected(&h), (vec!["a".into(), "b".into()], vec![]));
}

#[test]
fn an_empty_kind_selects_nothing() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    keys(&mut h, Modifiers::COMMAND, Key::A);
    keys(&mut h, SHIFT, Key::T);
    assert_eq!(selected(&h), (vec![], vec![]));
    keys(&mut h, SHIFT, Key::A);
    keys(&mut h, ALT, Key::A);
    assert_eq!(selected(&h), (vec![], vec![]));
}
