//! The Properties pane, headlessly: showing and hiding it (button, `Ctrl+I`,
//! the pref, a hidden sidebar coming back), a Mixed selection made one fill
//! by a swatch (one undo step), hex values applied and refused, and the
//! reasons it gives when nothing can be styled.
//!
//! The harness starts with the sidebar hidden, so shapes are selected (in
//! canvas coordinates, `at`) before the pane is shown.

use blockliner_core::style::{Colour, PRESETS};
use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;

use crate::overlays::colour::INVALID;
use crate::ui_tests::{at, click, harness, two_shapes, H};

fn fill(h: &H, id: &str) -> Option<String> {
    let s = h.state().diagram().shape(id).unwrap();
    s.style.fill.as_ref().map(|c| c.text.clone())
}

fn show_pane(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.run_steps(3);
}

#[test]
fn show_and_hide_with_the_button_and_ctrl_i() {
    let (_tmp, paths, mut h) = harness();
    h.run_steps(2);
    assert!(
        h.query_by_label("Nothing selected").is_none(),
        "sidebar hidden"
    );

    // Ctrl+I shows the pane, and the sidebar it lives in.
    show_pane(&mut h);
    h.get_by_label("Nothing selected");
    let prefs = paths.prefs();
    assert!(prefs.properties_visible && prefs.sidebar_visible);

    // The toolbar button hides it again (the diagram list stays).
    h.get_by_label("Properties").click();
    h.run_steps(3);
    assert!(h.query_by_label("Nothing selected").is_none());
    assert!(!paths.prefs().properties_visible);
    assert!(paths.prefs().sidebar_visible);

    show_pane(&mut h);
    h.get_by_label("Nothing selected");
    // Nothing selected: the rows are off and say why.
    h.get_by_label("Select shapes or lines to change their colours.");
    h.get_by_label("Fill: Blue").click();
    h.run_steps(2);
    assert!(h.state().diagram().shapes.is_empty());
}

#[test]
fn a_mixed_fill_becomes_one_with_a_swatch() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    let blue = Colour::parse(PRESETS[1].fill);
    let d = h.state_mut().diagram_mut();
    d.shape_mut("a").unwrap().style.set_fill(Some(blue));
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
    show_pane(&mut h);

    h.get_by_label("2 shapes");
    // a is blue, b the default: both rows are Mixed.
    h.get_by_label("Current fill: Mixed");
    h.get_by_label("Current stroke: Mixed");

    // A swatch sets every selected shape, as one undo step.
    h.get_by_label("Fill: Red").click();
    h.run_steps(3);
    assert_eq!(fill(&h, "a").as_deref(), Some("#fee2e2"));
    assert_eq!(fill(&h, "b").as_deref(), Some("#fee2e2"));
    h.get_by_label("Current fill: #fee2e2");
    // Both had their stroke follow (a's came with blue, b had none).
    h.get_by_label("Current stroke: #ef4444");

    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(fill(&h, "a").as_deref(), Some("#dbeafe"));
    assert_eq!(fill(&h, "b"), None);
    h.get_by_label("Current fill: Mixed");
}

#[test]
fn hex_values_are_applied_or_refused() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    click(&mut h, at(150.0, 120.0));
    h.run_steps(2);
    show_pane(&mut h);
    h.get_by_label("Rectangle “A”");
    h.get_by_label("Current fill: Default");

    // The first text field is Fill's.
    let focus = |h: &H| {
        h.get_all_by_role(egui::accesskit::Role::TextInput)
            .next()
            .unwrap()
            .focus();
    };
    focus(&h);
    h.run_steps(2);
    h.event(egui::Event::Text("1E3A8A".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(fill(&h, "a").as_deref(), Some("#1e3a8a"));
    h.get_by_label("Current fill: #1e3a8a");

    // Something that isn't a colour: refused, with the reason.
    focus(&h);
    h.run_steps(2);
    h.event(egui::Event::Text("zz".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    h.get_by_label(INVALID);
    assert_eq!(fill(&h, "a").as_deref(), Some("#1e3a8a"));
}

#[test]
fn lines_alone_take_a_stroke() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    // A line from a to b, selected after drawing.
    h.key_press(Key::L);
    h.run_steps(2);
    crate::ui_tests::drag(
        &mut h,
        egui::PointerButton::Primary,
        at(258.0, 133.0),
        at(403.0, 327.0),
    );
    h.run_steps(2);
    assert_eq!(h.state().selected_lines(), [0]);
    show_pane(&mut h);
    h.get_by_label("1 line");
    h.get_by_label("Lines take a stroke colour only.");
    // Fill is off; Stroke colours the line.
    h.get_by_label("Fill: Blue").click();
    h.run_steps(2);
    assert!(h.state().diagram().shapes[0].style.is_empty());
    h.get_by_label("Stroke: Red").click();
    h.run_steps(2);
    let line = &h.state().diagram().connectors[0];
    assert_eq!(line.style.props(), "stroke:#ef4444");
    assert!(
        blockliner_core::write(h.state().diagram()).contains("\n  linkStyle 0 stroke:#ef4444\n")
    );
    h.get_by_label("Reset Style").click();
    h.run_steps(2);
    assert!(h.state().diagram().connectors[0].style.is_empty());
}
