//! Edge labels, headlessly: a double-click on a connector or its label
//! edits it in place, Enter and the right-click menu's Edit Label do too, an
//! empty label removes it, Esc keeps the old one, a click on a label selects
//! its connector, and a free line can't be labelled.
//!
//! `arrow` joins `two_shapes`' `a` (east side at (260, 130)) and `b` (west
//! side at (400, 330)), so the arrow's midpoint is (330, 230).

use blockliner_core::{write, Connector, ConnectorKind, End, Point};
use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::{NodeT, Queryable};

use crate::ui_tests::{at, click, harness, two_shapes, H};

fn arrow(h: &mut H, label: Option<&str>) {
    two_shapes(h);
    let mut c = Connector::new(
        ConnectorKind::Arrow,
        End::glued("a", None),
        End::glued("b", None),
    );
    c.label = label.map(Into::into);
    h.state_mut().diagram_mut().connectors.push(c);
    h.run_steps(2);
}

fn label(h: &H) -> Option<String> {
    h.state().diagram().connectors[0].label.clone()
}

fn double_click(h: &mut H, p: egui::Pos2) {
    // Well apart from any earlier click.
    h.run_steps(30);
    click(h, p);
    click(h, p);
    h.run_steps(3);
}

fn type_text(h: &mut H, text: &str) {
    h.event(Event::Text(text.into()));
    h.step();
}

fn right_click(h: &mut H, p: egui::Pos2) {
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

#[test]
fn double_click_a_connector_to_label_it() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    arrow(&mut h, None);

    double_click(&mut h, at(330.0, 230.0));
    assert!(h.state().editing_label());
    assert_eq!(h.state().selected_lines(), [0]);
    assert!(h.state().selection().is_empty());
    type_text(&mut h, "HTTPS");
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert!(!h.state().editing_label());
    assert_eq!(label(&h).as_deref(), Some("HTTPS"));
    let text = write(h.state().diagram());
    assert!(text.contains("\n  a -->|\"HTTPS\"| b\n"), "{text}");

    // One undo step.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(label(&h), None);
}

#[test]
fn a_label_is_clicked_edited_and_cleared() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    arrow(&mut h, Some("reads from a long label"));

    // 40 to the right of the midpoint: on the label, well off the line.
    let on_label = at(370.0, 232.0);
    click(&mut h, on_label);
    h.run_steps(2);
    assert_eq!(h.state().selected_lines(), [0]);

    // Esc keeps the old label.
    double_click(&mut h, on_label);
    assert!(h.state().editing_label());
    type_text(&mut h, "!");
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!h.state().editing_label());
    assert_eq!(label(&h).as_deref(), Some("reads from a long label"));

    // Enter edits the selected connector's label; emptied, it's gone.
    assert_eq!(h.state().selected_lines(), [0]);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(h.state().editing_label(), "Enter opens the editor");
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.key_press(Key::Backspace);
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert!(!h.state().editing_label());
    assert_eq!(label(&h), None);
    let text = write(h.state().diagram());
    assert!(text.contains("\n  a --> b\n"), "{text}");
}

#[test]
fn the_menu_edits_labels_but_not_a_free_lines() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    arrow(&mut h, None);

    right_click(&mut h, at(330.0, 230.0));
    h.get_by_label("Edit Label Enter").click();
    h.run_steps(3);
    assert!(h.state().editing_label());
    type_text(&mut h, "events");
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(label(&h).as_deref(), Some("events"));

    // A shape's label from Enter too.
    h.key_press(Key::Escape);
    click(&mut h, at(150.0, 120.0));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(h.state().editing_label());
    h.key_press(Key::Escape);
    h.run_steps(2);

    // A free line has nowhere to keep one.
    h.state_mut().diagram_mut().connectors.push(Connector::new(
        ConnectorKind::Line,
        End::Free(Point::new(600.0, 100.0)),
        End::Free(Point::new(800.0, 100.0)),
    ));
    h.run_steps(2);
    double_click(&mut h, at(700.0, 100.0));
    assert!(!h.state().editing_label());
    assert_eq!(h.state().selected_lines(), [1]);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(!h.state().editing_label());
    right_click(&mut h, at(700.0, 100.0));
    assert!(h
        .get_by_label("Edit Label Enter")
        .accesskit_node()
        .is_disabled());
}
