//! Stroke styles, headlessly: Solid / Dashed / Dotted from the Properties
//! pane's Stroke Style list, the right-click menu's Stroke Style ▸ and the
//! palette, for shapes, Mermaid edges (dotted ones written `-.->`) and free
//! lines; Mixed selections, a hand-written Custom dash, Reset Style, kept-
//! statement shapes left alone, and one undo step per change.
//!
//! `two_shapes` puts `a` at (100, 100) and `b` at (400, 300), 160 × 60.

use blockliner_core::style::{Dash, DashCommon};
use blockliner_core::{read, write, Anchor, Connector, ConnectorKind, End, Point};
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;

use crate::ui_tests::{at, click, harness, two_shapes, H};

fn dash_of(h: &H, id: &str) -> Option<Dash> {
    h.state().diagram().shape(id).unwrap().style.dash()
}

fn line_dash(h: &H, i: usize) -> Option<Dash> {
    h.state().diagram().connectors[i].style.dash()
}

fn selected_dash(h: &H) -> DashCommon {
    h.state().canvas().selected_style(h.state().diagram()).dash
}

fn show_pane(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.run_steps(3);
}

fn select_all(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
}

/// Opens the pane's Stroke Style list and picks `name` in it.
fn pane_pick(h: &mut H, name: &str) {
    h.get_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some("Stroke Style"))
        .click();
    h.run_steps(3);
    pick(h, name);
}

/// Clicks the row called `name` in an open list or menu.
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

fn undo(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
}

#[test]
fn the_pane_makes_a_shape_dashed() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    click(&mut h, at(180.0, 130.0));
    h.run_steps(2);
    show_pane(&mut h);
    assert_eq!(selected_dash(&h), DashCommon::Same(None));

    pane_pick(&mut h, "Dashed");
    assert_eq!(dash_of(&h, "a"), Some(Dash::Dashed));
    assert_eq!(dash_of(&h, "b"), None);
    let text = write(h.state().diagram());
    assert!(text.contains("  style a stroke-dasharray:6 4\n"), "{text}");

    pane_pick(&mut h, "Dotted");
    assert!(write(h.state().diagram()).contains("  style a stroke-dasharray:2 3\n"));

    // One undo step each.
    undo(&mut h);
    assert_eq!(dash_of(&h, "a"), Some(Dash::Dashed));
    undo(&mut h);
    assert_eq!(dash_of(&h, "a"), None);
    assert!(!write(h.state().diagram()).contains("style a"));
}

#[test]
fn a_dotted_edge_is_written_with_mermaids_dotted_link() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.state_mut().diagram_mut().connectors.push(Connector::new(
        ConnectorKind::Arrow,
        End::glued("a", Some(Anchor::E)),
        End::glued("b", Some(Anchor::W)),
    ));
    h.run_steps(2);

    // A right-click on the edge selects it, then Stroke Style ▸ Dotted.
    right_click(&mut h, at(330.0, 230.0));
    assert_eq!(h.state().selected_lines(), [0]);
    h.get_by_label("Stroke Style").click();
    h.run_steps(3);
    pick(&mut h, "Dotted");
    assert_eq!(line_dash(&h, 0), Some(Dash::Dotted));
    let text = write(h.state().diagram());
    assert!(text.contains("  a -.-> b\n"), "{text}");
    assert!(!text.contains("linkStyle"), "{text}");

    // Dashed has no link of its own: a `linkStyle`.
    right_click(&mut h, at(330.0, 230.0));
    h.get_by_label("Stroke Style").click();
    h.run_steps(3);
    pick(&mut h, "Dashed");
    let text = write(h.state().diagram());
    assert!(text.contains("  a --> b\n"), "{text}");
    assert!(
        text.contains("  linkStyle 0 stroke-dasharray:6 4\n"),
        "{text}"
    );
}

#[test]
fn a_free_line_keeps_its_dash_in_its_layout_comment() {
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
    assert_eq!(h.state().selected_lines(), [0]);

    palette(&mut h, "Stroke Style: Dashed");
    assert_eq!(line_dash(&h, 0), Some(Dash::Dashed));
    let text = write(h.state().diagram());
    assert!(
        text.contains("%% @line line from=600,100 to=700,100 dash=dashed\n"),
        "{text}"
    );
    palette(&mut h, "Stroke Style: Solid");
    assert_eq!(line_dash(&h, 0), None);
    assert!(write(h.state().diagram()).contains("%% @line line from=600,100 to=700,100\n"));
}

#[test]
fn a_mixed_selection_takes_one_style() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    let d = h.state_mut().diagram_mut();
    d.shape_mut("a").unwrap().style.set_dash(Some(Dash::Dashed));
    d.connectors.push(Connector::new(
        ConnectorKind::Line,
        End::glued("a", Some(Anchor::E)),
        End::glued("b", Some(Anchor::W)),
    ));
    h.run_steps(2);
    select_all(&mut h);
    show_pane(&mut h);
    assert_eq!(selected_dash(&h), DashCommon::Mixed);

    pane_pick(&mut h, "Dotted");
    assert_eq!(selected_dash(&h), DashCommon::Same(Some(Dash::Dotted)));
    assert_eq!(dash_of(&h, "b"), Some(Dash::Dotted));
    assert_eq!(line_dash(&h, 0), Some(Dash::Dotted));
    assert!(write(h.state().diagram()).contains("  a -.- b\n"));
    // All three in one step.
    undo(&mut h);
    assert_eq!(dash_of(&h, "a"), Some(Dash::Dashed));
    assert_eq!(dash_of(&h, "b"), None);
    assert_eq!(line_dash(&h, 0), None);

    // Every row is showing (Change Shape, Change Line, Fill, Stroke, Stroke
    // Style, Stroke Weight, Text Weight and Style), and Reset Style is still
    // reachable.
    select_all(&mut h);
    crate::ui_tests::pane_reset_style(&mut h);
    assert_eq!(dash_of(&h, "a"), None);
}

#[test]
fn a_custom_dash_stays_until_replaced_and_reset_style_clears_it() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    *h.state_mut().diagram_mut() = read(
        "flowchart LR\n  a[\"A\"]\n  style a stroke:#333,stroke-dasharray:5 5,stroke-width:2px\n\
         %% blockliner 1 grid=20\n%% @shape a x=100 y=100 w=160 h=60\n",
    )
    .unwrap();
    h.run_steps(2);
    click(&mut h, at(180.0, 130.0));
    h.run_steps(2);
    let custom = Some(Dash::Custom("5 5".into()));
    assert_eq!(selected_dash(&h), DashCommon::Same(custom.clone()));

    // Another change leaves it as written.
    palette(&mut h, "Fill: Blue");
    assert_eq!(dash_of(&h, "a"), custom);
    assert!(write(h.state().diagram()).contains("stroke-dasharray:5 5,stroke-width:2px"));

    palette(&mut h, "Stroke Style: Dashed");
    assert_eq!(dash_of(&h, "a"), Some(Dash::Dashed));
    // Reset Style: solid again, the colours and the weight gone.
    palette(&mut h, "Reset Style");
    assert_eq!(dash_of(&h, "a"), None);
    assert!(h.state().diagram().shape("a").unwrap().style.is_empty());
}

#[test]
fn kept_statement_shapes_keep_their_stroke() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.state_mut().diagram_mut().shape_mut("b").unwrap().external = true;
    h.run_steps(2);
    select_all(&mut h);
    show_pane(&mut h);
    h.get_by_label("1 kept-statement shape not affected.");

    palette(&mut h, "Stroke Style: Dotted");
    assert_eq!(dash_of(&h, "a"), Some(Dash::Dotted));
    assert_eq!(dash_of(&h, "b"), None);
}
