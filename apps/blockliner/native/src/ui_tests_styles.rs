//! Shape colours, headlessly: a preset fill from the right-click menu on two
//! shapes (one undo step), a stroke, the Custom… dialog (a bad value
//! refused, then a dark fill with white labels), Reset Style, and what the
//! file says.
//!
//! `two_shapes` puts `a` at (100, 100) and `b` at (400, 300), 160 × 60.

use blockliner_core::style::{Colour, LIGHT_TEXT};
use blockliner_core::write;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;

use crate::overlays::colour::INVALID;
use crate::ui_tests::{at, click, harness, two_shapes, H};

fn props(h: &H, id: &str) -> String {
    h.state().diagram().shape(id).unwrap().style.props()
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

/// Opens the right-click menu on `p`, then `submenu`, then picks `item`.
fn menu(h: &mut H, p: Pos2, submenu: &str, item: &str) {
    right_click(h, p);
    h.get_by_label(submenu).click();
    h.run_steps(3);
    h.get_by_label(item).click();
    h.run_steps(3);
}

fn palette(h: &mut H, command: &str) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(3);
    h.event(Event::Text(command.into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
}

#[test]
fn a_preset_fill_on_two_shapes_is_one_undo_step() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);

    // A right-click on a selected shape keeps the selection.
    menu(&mut h, at(150.0, 120.0), "Fill", "Blue");
    for id in ["a", "b"] {
        assert_eq!(
            props(&h, id),
            "fill:#dbeafe,stroke:#3b82f6,color:#111827",
            "{id}"
        );
    }
    let text = write(h.state().diagram());
    assert!(
        text.contains("  style a fill:#dbeafe,stroke:#3b82f6,color:#111827\n"),
        "{text}"
    );

    // A stroke of its own on b alone: select it, then the menu.
    click(&mut h, at(450.0, 320.0));
    h.run_steps(2);
    assert_eq!(h.state().selection(), ["b"]);
    menu(&mut h, at(450.0, 320.0), "Stroke", "Red");
    assert_eq!(props(&h, "b"), "fill:#dbeafe,stroke:#ef4444,color:#111827");
    assert_eq!(props(&h, "a"), "fill:#dbeafe,stroke:#3b82f6,color:#111827");

    // Undo: b's stroke, then both fills at once.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(props(&h, "b"), "fill:#dbeafe,stroke:#3b82f6,color:#111827");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(
        (props(&h, "a"), props(&h, "b")),
        (String::new(), String::new())
    );
}

#[test]
fn custom_fill_and_reset_style() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    click(&mut h, at(150.0, 120.0));
    h.run_steps(2);
    assert_eq!(h.state().selection(), ["a"]);

    palette(&mut h, "Fill: Custom");
    assert!(h.state().has_overlay());
    h.event(Event::Text("navy".into()));
    h.run_steps(2);
    h.get_by_label("Apply").click();
    h.run_steps(2);
    h.get_by_label(INVALID);
    assert!(h.state().has_overlay(), "a bad value keeps the dialog open");

    // The refused value is selected again: typing replaces it. A dark blue
    // gets white labels.
    h.event(Event::Text("1E3A8A".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(!h.state().has_overlay());
    let style = &h.state().diagram().shape("a").unwrap().style;
    assert_eq!(style.fill, Colour::custom("#1e3a8a"));
    assert_eq!(style.color.as_ref().unwrap().text, LIGHT_TEXT);
    assert_eq!(
        style.stroke, None,
        "a custom fill keeps the stroke as it was"
    );
    assert_eq!(props(&h, "b"), "");

    // Reset Style, from the right-click menu.
    right_click(&mut h, at(150.0, 120.0));
    h.get_by_label("Reset Style").click();
    h.run_steps(3);
    assert_eq!(props(&h, "a"), "");
    assert!(!write(h.state().diagram()).contains("style a"));
}
