//! Stroke weights, headlessly: 1px … 4px (1.5px the default, never
//! written) from the Properties pane's Stroke Weight list, the right-click
//! menu's Stroke Weight ▸ and the palette, for shapes, Mermaid edges (dotted
//! ones too) and free lines; Mixed selections, hand-written values, Reset
//! Style, kept-statement shapes left alone, and one undo step per change.
//!
//! `two_shapes` puts `a` at (100, 100) and `b` at (400, 300), 160 × 60.

use blockliner_core::style::{Agreement, Dash, Weight};
use blockliner_core::{read, write, Anchor, Connector, ConnectorKind, End, Point};
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;

use crate::ui_tests::{at, click, harness, two_shapes, H};

fn weight_of(h: &H, id: &str) -> Option<Weight> {
    h.state().diagram().shape(id).unwrap().style.weight()
}

fn line_weight(h: &H, i: usize) -> Option<Weight> {
    h.state().diagram().connectors[i].style.weight()
}

fn selected_weight(h: &H) -> Agreement<Option<Weight>> {
    h.state()
        .canvas()
        .selected_style(h.state().diagram())
        .weight
}

fn show_pane(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.run_steps(3);
}

fn select_all(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
}

/// Opens the pane's Stroke Weight list and picks `name` in it.
fn pane_pick(h: &mut H, name: &str) {
    h.get_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some("Stroke Weight"))
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

/// Right-click at `p`, then `submenu` ▸ `name`.
fn menu_pick(h: &mut H, p: Pos2, submenu: &str, name: &str) {
    right_click(h, p);
    h.get_by_label(submenu).click();
    h.run_steps(3);
    pick(h, name);
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
fn the_pane_sets_a_shapes_weight_and_the_default_is_not_written() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    click(&mut h, at(180.0, 130.0));
    h.run_steps(2);
    show_pane(&mut h);
    assert_eq!(selected_weight(&h), Agreement::Same(None));
    // The closed list shows the default.
    h.get_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some("Stroke Weight"));

    pane_pick(&mut h, "3px");
    assert_eq!(weight_of(&h, "a"), Some(Weight::Preset(30)));
    assert_eq!(weight_of(&h, "b"), None);
    let text = write(h.state().diagram());
    assert!(text.contains("  style a stroke-width:3px\n"), "{text}");

    pane_pick(&mut h, "2.5px");
    assert!(write(h.state().diagram()).contains("  style a stroke-width:2.5px\n"));

    // One undo step each.
    undo(&mut h);
    assert_eq!(weight_of(&h, "a"), Some(Weight::Preset(30)));

    // Back to the default: nothing written.
    pane_pick(&mut h, "1.5px (Default)");
    assert_eq!(weight_of(&h, "a"), None);
    assert!(!write(h.state().diagram()).contains("style a"));
}

#[test]
fn an_edges_weight_is_in_its_link_style_and_survives_dotting() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.state_mut().diagram_mut().connectors.push(Connector::new(
        ConnectorKind::Arrow,
        End::glued("a", Some(Anchor::E)),
        End::glued("b", Some(Anchor::W)),
    ));
    h.run_steps(2);

    menu_pick(&mut h, at(330.0, 230.0), "Stroke Weight", "2.5px");
    assert_eq!(h.state().selected_lines(), [0]);
    assert_eq!(line_weight(&h, 0), Some(Weight::Preset(25)));
    let text = write(h.state().diagram());
    assert!(
        text.contains("  linkStyle 0 stroke-width:2.5px\n"),
        "{text}"
    );

    // Dotted: `-.->`, the weight still in its `linkStyle`.
    menu_pick(&mut h, at(330.0, 230.0), "Stroke Style", "Dotted");
    assert_eq!(line_weight(&h, 0), Some(Weight::Preset(25)));
    let text = write(h.state().diagram());
    assert!(text.contains("  a -.-> b\n"), "{text}");
    assert!(
        text.contains("  linkStyle 0 stroke-width:2.5px\n"),
        "{text}"
    );
    let back = read(&text).unwrap();
    assert_eq!(back.connectors[0].style.dash(), Some(Dash::Dotted));
    assert_eq!(back.connectors[0].style.weight(), Some(Weight::Preset(25)));
}

#[test]
fn a_free_line_keeps_its_weight_in_its_layout_comment() {
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

    palette(&mut h, "Stroke Weight: 4px");
    assert_eq!(line_weight(&h, 0), Some(Weight::Preset(40)));
    let text = write(h.state().diagram());
    assert!(
        text.contains("%% @line line from=600,100 to=700,100 weight=4\n"),
        "{text}"
    );
    // A 4px line is clickable 2 px further out than a default one.
    click(&mut h, at(400.0, 500.0));
    h.run_steps(2);
    click(&mut h, at(650.0, 106.5));
    h.run_steps(2);
    assert_eq!(h.state().selected_lines(), [0]);

    palette(&mut h, "Stroke Weight: 1.5px (Default)");
    assert_eq!(line_weight(&h, 0), None);
    assert!(write(h.state().diagram()).contains("%% @line line from=600,100 to=700,100\n"));
}

#[test]
fn a_mixed_selection_takes_one_weight() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    let d = h.state_mut().diagram_mut();
    d.shape_mut("a")
        .unwrap()
        .style
        .set_weight(Some(Weight::Preset(10)));
    d.connectors.push(Connector::new(
        ConnectorKind::Line,
        End::glued("a", Some(Anchor::E)),
        End::glued("b", Some(Anchor::W)),
    ));
    h.run_steps(2);
    select_all(&mut h);
    show_pane(&mut h);
    assert_eq!(selected_weight(&h), Agreement::Mixed);

    pane_pick(&mut h, "2px");
    assert_eq!(
        selected_weight(&h),
        Agreement::Same(Some(Weight::Preset(20)))
    );
    assert_eq!(weight_of(&h, "b"), Some(Weight::Preset(20)));
    assert_eq!(line_weight(&h, 0), Some(Weight::Preset(20)));
    // All three in one step.
    undo(&mut h);
    assert_eq!(weight_of(&h, "a"), Some(Weight::Preset(10)));
    assert_eq!(weight_of(&h, "b"), None);
    assert_eq!(line_weight(&h, 0), None);

    // Every row is showing, and Reset Style clears the weights.
    select_all(&mut h);
    crate::ui_tests::pane_reset_style(&mut h);
    assert_eq!(weight_of(&h, "a"), None);
    assert!(write(h.state().diagram()).find("stroke-width").is_none());
}

#[test]
fn hand_written_weights_stay_as_written() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    *h.state_mut().diagram_mut() = read(
        "flowchart LR\n  a[\"A\"]\n  b[\"B\"]\n  style a stroke:#333,stroke-width:5px\n  \
         style b stroke-width:1.5px\n\
         %% blockliner 1 grid=20\n%% @shape a x=100 y=100 w=160 h=60\n\
         %% @shape b x=400 y=300 w=160 h=60\n",
    )
    .unwrap();
    h.run_steps(2);
    click(&mut h, at(180.0, 130.0));
    h.run_steps(2);
    let custom = Some(Weight::Custom("5px".into()));
    assert_eq!(selected_weight(&h), Agreement::Same(custom.clone()));

    // Another change leaves it as written.
    palette(&mut h, "Fill: Blue");
    assert_eq!(weight_of(&h, "a"), custom);
    assert!(write(h.state().diagram()).contains("stroke-width:5px"));

    // A hand-written 1.5px is the default, and kept.
    click(&mut h, at(480.0, 330.0));
    h.run_steps(2);
    assert_eq!(selected_weight(&h), Agreement::Same(None));
    palette(&mut h, "Stroke Weight: 1.5px (Default)");
    assert!(write(h.state().diagram()).contains("  style b stroke-width:1.5px\n"));
}

#[test]
fn kept_statement_shapes_keep_their_weight() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.state_mut().diagram_mut().shape_mut("b").unwrap().external = true;
    h.run_steps(2);
    select_all(&mut h);
    palette(&mut h, "Stroke Weight: 3px");
    assert_eq!(weight_of(&h, "a"), Some(Weight::Preset(30)));
    assert_eq!(weight_of(&h, "b"), None);
}
