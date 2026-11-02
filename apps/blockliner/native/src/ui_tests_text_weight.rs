//! Text weight and style, headlessly: Normal / Bold and Normal / Italic
//! from the Properties pane, the right-click menu and the palette, Ctrl+B,
//! for shape labels, Mermaid edge labels and free lines; Mixed selections,
//! hand-written values, Reset Style, and clicking a bold edge label.
//!
//! `two_shapes` puts `a` at (100, 100) and `b` at (400, 300), 160 × 60.

use blockliner_core::style::Agreement;
use blockliner_core::{read, write, Anchor, Connector, ConnectorKind, End, Point};
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;

use crate::ui_tests::{at, click, harness, two_shapes, H};

fn bold(h: &H, id: &str) -> bool {
    h.state().diagram().shape(id).unwrap().style.bold()
}

fn italic(h: &H, id: &str) -> bool {
    h.state().diagram().shape(id).unwrap().style.italic()
}

fn selected_bold(h: &H) -> Agreement<bool> {
    h.state().canvas().selected_style(h.state().diagram()).bold
}

fn show_pane(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.run_steps(3);
}

fn select_all(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
}

/// Opens the pane's `list` (Text Weight or Text Style) and picks `name`.
fn pane_pick(h: &mut H, list: &str, name: &str) {
    h.get_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some(list))
        .click();
    h.run_steps(3);
    pick(h, name);
}

fn pick(h: &mut H, name: &str) {
    h.get_by(|n| n.role() == Role::MenuItemRadio && n.label().as_deref() == Some(name))
        .click();
    h.run_steps(3);
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

fn palette(h: &mut H, command: &str) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(3);
    h.event(Event::Text(command.into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
}

fn ctrl_b(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::B);
    h.run_steps(2);
}

fn undo(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
}

#[test]
fn the_pane_makes_a_label_bold_and_italic() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    click(&mut h, at(180.0, 130.0));
    h.run_steps(2);
    show_pane(&mut h);
    assert_eq!(selected_bold(&h), Agreement::Same(false));

    pane_pick(&mut h, "Text Weight", "Bold");
    assert!(bold(&h, "a"));
    assert!(!bold(&h, "b"));
    let text = write(h.state().diagram());
    assert!(text.contains("  style a font-weight:bold\n"), "{text}");

    pane_pick(&mut h, "Text Style", "Italic");
    assert!(italic(&h, "a"));
    let text = write(h.state().diagram());
    assert!(
        text.contains("  style a font-weight:bold,font-style:italic\n"),
        "{text}"
    );

    // One undo step each.
    undo(&mut h);
    assert!(bold(&h, "a") && !italic(&h, "a"));
    undo(&mut h);
    assert!(!bold(&h, "a"));
    assert!(!write(h.state().diagram()).contains("style a"));
}

#[test]
fn an_edge_label_goes_bold_from_the_menu_and_stays_clickable() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    let mut edge = Connector::new(
        ConnectorKind::Arrow,
        End::glued("a", Some(Anchor::E)),
        End::glued("b", Some(Anchor::W)),
    );
    edge.label = Some("a fairly long label".into());
    h.state_mut().diagram_mut().connectors.push(edge);
    h.run_steps(2);

    right_click(&mut h, at(330.0, 230.0));
    assert_eq!(h.state().selected_lines(), [0]);
    h.get_by_label("Text Weight").click();
    h.run_steps(3);
    pick(&mut h, "Bold");
    let text = write(h.state().diagram());
    assert!(text.contains("  linkStyle 0 font-weight:bold\n"), "{text}");
    assert!(read(&text).unwrap().connectors[0].style.bold());

    // The bold label's patch still picks its line: 50 px right of the
    // midpoint is on the label, well off the line itself.
    click(&mut h, at(700.0, 600.0));
    h.run_steps(2);
    assert!(h.state().selected_lines().is_empty());
    click(&mut h, at(380.0, 230.0));
    h.run_steps(2);
    assert_eq!(h.state().selected_lines(), [0]);
}

#[test]
fn a_free_line_keeps_its_text_style_in_its_layout_comment() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    h.state_mut().diagram_mut().connectors.push(Connector::new(
        ConnectorKind::Line,
        End::Free(Point::new(600.0, 100.0)),
        End::Free(Point::new(700.0, 100.0)),
    ));
    h.run_steps(2);
    click(&mut h, at(650.0, 100.0));
    h.run_steps(2);

    palette(&mut h, "Text Weight: Bold");
    let text = write(h.state().diagram());
    assert!(
        text.contains("%% @line line from=600,100 to=700,100 text=bold\n"),
        "{text}"
    );
    palette(&mut h, "Text Style: Italic");
    let text = write(h.state().diagram());
    assert!(
        text.contains("%% @line line from=600,100 to=700,100 text=bold-italic\n"),
        "{text}"
    );
}

#[test]
fn ctrl_b_toggles_bold() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.state_mut()
        .diagram_mut()
        .shape_mut("a")
        .unwrap()
        .style
        .set_bold(true);
    h.run_steps(2);
    select_all(&mut h);
    assert_eq!(selected_bold(&h), Agreement::Mixed);

    // Mixed: all Bold, in one step.
    ctrl_b(&mut h);
    assert!(bold(&h, "a") && bold(&h, "b"));
    // All Bold: all Normal.
    ctrl_b(&mut h);
    assert!(!bold(&h, "a") && !bold(&h, "b"));
    undo(&mut h);
    assert!(bold(&h, "a") && bold(&h, "b"));
    undo(&mut h);
    assert!(bold(&h, "a") && !bold(&h, "b"));
}

#[test]
fn ctrl_b_while_editing_a_label_leaves_the_style_alone() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    click(&mut h, at(180.0, 130.0));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    ctrl_b(&mut h);
    assert!(!bold(&h, "a"));
}

#[test]
fn hand_written_values_and_reset_style() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    *h.state_mut().diagram_mut() = read(
        "flowchart LR\n  a[\"A\"]\n  style a font-weight:600,font-style:oblique\n\
         %% blockliner 1 grid=20\n%% @shape a x=100 y=100 w=160 h=60\n",
    )
    .unwrap();
    h.run_steps(2);
    click(&mut h, at(180.0, 130.0));
    h.run_steps(2);
    show_pane(&mut h);
    assert!(bold(&h, "a") && italic(&h, "a"));

    // Picking what it already is changes nothing.
    pane_pick(&mut h, "Text Weight", "Bold");
    assert!(write(h.state().diagram()).contains("  style a font-weight:600,font-style:oblique\n"));

    crate::ui_tests::pane_reset_style(&mut h);
    assert!(!bold(&h, "a") && !italic(&h, "a"));
    assert!(!write(h.state().diagram()).contains("style a"));
}
