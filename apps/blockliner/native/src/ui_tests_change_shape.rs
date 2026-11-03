//! Change Shape and Change Line, headlessly: the Properties pane's Change
//! Shape and Change Line lists (Line, Arrow, Right-Angle Line, Right-Angle
//! Arrow), the right-click menu's Change Shape ▸ and Add / Remove Arrowhead,
//! the palette's commands, Mixed selections, kept-statement shapes left
//! alone, and one undo step per change.

use blockliner_core::{
    write, Anchor, Connector, ConnectorKind, End, Point, Rect, Route, ShapeKind,
};
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};

use crate::canvas::{Agreed, SelectionKinds, Tool};
use crate::ui_tests::{at, click, harness, two_shapes, H};

fn kind(h: &H, id: &str) -> ShapeKind {
    h.state().diagram().shape(id).unwrap().kind.clone()
}

fn line_kinds(h: &H) -> Vec<ConnectorKind> {
    h.state()
        .diagram()
        .connectors
        .iter()
        .map(|c| c.kind)
        .collect()
}

fn kinds(h: &H) -> SelectionKinds {
    h.state().canvas().selected_kinds(h.state().diagram())
}

/// Model (x, y) on screen, wherever the canvas is (the pane's sidebar
/// moves it right).
fn on_canvas(h: &H, x: f32, y: f32) -> Pos2 {
    h.state().canvas().origin() + egui::vec2(x, y)
}

fn show_pane(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.run_steps(3);
}

fn select_all(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
}

/// Opens one of the pane's lists by its name ("Change Shape").
fn open_list(h: &mut H, name: &str) {
    h.get_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some(name))
        .click();
    h.run_steps(3);
}

/// Clicks the row called `name` in an open list or menu: a row, not the
/// toolbar button of the same name.
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
fn the_pane_changes_a_shape_keeping_everything_else() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    let blue = blockliner_core::style::Colour::parse("#dbeafe");
    let d = h.state_mut().diagram_mut();
    d.shape_mut("a").unwrap().style.set_fill(Some(blue));
    d.connectors.push(Connector::new(
        ConnectorKind::Arrow,
        End::glued("a", Some(Anchor::E)),
        End::glued("b", None),
    ));
    h.run_steps(2);
    click(&mut h, at(180.0, 130.0));
    h.run_steps(2);
    show_pane(&mut h);
    assert_eq!(kinds(&h).shape, Agreed::Same(ShapeKind::Rect));

    open_list(&mut h, "Change Shape");
    pick(&mut h, "Diamond");
    let a = h.state().diagram().shape("a").unwrap().clone();
    assert_eq!(a.kind, ShapeKind::Diamond);
    assert_eq!(a.rect, Rect::new(100.0, 100.0, 160.0, 60.0));
    assert_eq!(a.label, "A");
    assert_eq!(
        a.style.fill.as_ref().map(|c| c.text.as_str()),
        Some("#dbeafe")
    );
    // Still glued to its east side, and saved with a diamond's brackets.
    assert_eq!(
        h.state().diagram().connectors[0].from,
        End::glued("a", Some(Anchor::E))
    );
    let text = write(h.state().diagram());
    assert!(text.contains("\n  a{\"A\"}\n"), "{text}");
    h.get_by_label("Diamond “A”");

    // One undo step brings the rectangle back.
    undo(&mut h);
    assert_eq!(kind(&h, "a"), ShapeKind::Rect);
}

#[test]
fn a_mixed_selection_becomes_one_kind() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    select_all(&mut h);
    show_pane(&mut h);
    assert_eq!(kinds(&h).shape, Agreed::Mixed);

    open_list(&mut h, "Change Shape");
    pick(&mut h, "Hexagon");
    assert_eq!(kind(&h, "a"), ShapeKind::Hexagon);
    assert_eq!(kind(&h, "b"), ShapeKind::Hexagon);
    assert_eq!(kinds(&h).shape, Agreed::Same(ShapeKind::Hexagon));
    // Both in one step.
    undo(&mut h);
    assert_eq!(kind(&h, "a"), ShapeKind::Rect);
    assert_eq!(kind(&h, "b"), ShapeKind::RoundRect);
}

#[test]
fn kept_statement_shapes_stay_as_they_are() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.state_mut().diagram_mut().shape_mut("b").unwrap().external = true;
    h.run_steps(2);
    select_all(&mut h);
    show_pane(&mut h);
    h.get_by_label("1 kept-statement shape not affected.");

    palette(&mut h, "Change Shape: Circle");
    assert_eq!(kind(&h, "a"), ShapeKind::Circle);
    assert_eq!(kind(&h, "b"), ShapeKind::RoundRect);

    // The kept-statement shape alone: Change Shape is off.
    h.key_press(Key::Escape);
    h.run_steps(2);
    let b = on_canvas(&h, 480.0, 330.0);
    click(&mut h, b);
    h.run_steps(2);
    assert_eq!(h.state().selection(), ["b".to_string()]);
    let list =
        h.get_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some("Change Shape"));
    assert!(list.accesskit_node().is_disabled());
}

#[test]
fn the_right_click_menu_changes_shapes() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    // A right-click selects the shape under it, then Change Shape ▸.
    right_click(&mut h, at(180.0, 130.0));
    assert_eq!(h.state().selection(), ["a".to_string()]);
    h.get_by_label("Change Shape").click();
    h.run_steps(3);
    pick(&mut h, "Database");
    assert_eq!(kind(&h, "a"), ShapeKind::Database);
    assert_eq!(kind(&h, "b"), ShapeKind::RoundRect);

    // To text and back: the label stays.
    right_click(&mut h, at(180.0, 130.0));
    h.get_by_label("Change Shape").click();
    h.run_steps(3);
    pick(&mut h, "Text");
    assert_eq!(kind(&h, "a"), ShapeKind::Text);
    assert!(write(h.state().diagram()).contains("  a[\"A\"]:::bl_text\n"));
    palette(&mut h, "Change Shape: Rounded Rectangle");
    assert_eq!(kind(&h, "a"), ShapeKind::RoundRect);
    assert_eq!(h.state().diagram().shape("a").unwrap().label, "A");
}

#[test]
fn lines_gain_and_lose_arrowheads() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    let d = h.state_mut().diagram_mut();
    let mut edge = Connector::new(
        ConnectorKind::Line,
        End::glued("a", Some(Anchor::E)),
        End::glued("b", Some(Anchor::W)),
    );
    edge.label = Some("SQL".into());
    d.connectors.push(edge);
    d.connectors.push(Connector::new(
        ConnectorKind::Arrow,
        End::Free(Point::new(600.0, 100.0)),
        End::Free(Point::new(700.0, 100.0)),
    ));
    h.run_steps(2);

    // Both lines: Mixed, and the menu offers both.
    select_all(&mut h);
    assert_eq!(kinds(&h).line, Agreed::Mixed);
    palette(&mut h, "Add Arrowheads");
    assert_eq!(line_kinds(&h), [ConnectorKind::Arrow, ConnectorKind::Arrow]);
    assert_eq!(
        h.state().diagram().connectors[0].label.as_deref(),
        Some("SQL")
    );
    assert!(write(h.state().diagram()).contains("  a -->|\"SQL\"| b\n"));
    undo(&mut h);
    assert_eq!(line_kinds(&h), [ConnectorKind::Line, ConnectorKind::Arrow]);

    // The pane's Change Line, for both.
    select_all(&mut h);
    show_pane(&mut h);
    open_list(&mut h, "Change Line");
    pick(&mut h, "Line");
    assert_eq!(line_kinds(&h), [ConnectorKind::Line, ConnectorKind::Line]);
    assert!(write(h.state().diagram()).contains("%% @line line from=600,100"));

    // A right-click on the free line selects it alone: Add Arrowhead.
    h.key_press(Key::Escape);
    h.run_steps(2);
    let free = on_canvas(&h, 650.0, 100.0);
    right_click(&mut h, free);
    assert_eq!(h.state().selected_lines(), [1]);
    h.get_by_label("Add Arrowhead").click();
    h.run_steps(3);
    assert_eq!(line_kinds(&h), [ConnectorKind::Line, ConnectorKind::Arrow]);
    // Now it has one: Remove Arrowhead.
    right_click(&mut h, free);
    h.get_by_label("Remove Arrowhead").click();
    h.run_steps(3);
    assert_eq!(line_kinds(&h), [ConnectorKind::Line, ConnectorKind::Line]);
}

fn line_tools(h: &H) -> Vec<Tool> {
    h.state()
        .diagram()
        .connectors
        .iter()
        .map(Tool::for_line)
        .collect()
}

#[test]
fn the_pane_changes_a_line_to_each_line_type() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    let mut edge = Connector::new(
        ConnectorKind::Line,
        End::glued("a", Some(Anchor::E)),
        End::glued("b", Some(Anchor::W)),
    );
    edge.label = Some("SQL".into());
    h.state_mut().diagram_mut().connectors.push(edge);
    h.run_steps(2);
    select_all(&mut h);
    show_pane(&mut h);
    assert_eq!(kinds(&h).line_tool, Agreed::Same(Tool::Line));

    // Arrowhead and route in one pick, and one undo step.
    open_list(&mut h, "Change Line");
    pick(&mut h, "Right-Angle Arrow");
    assert_eq!(line_tools(&h), [Tool::ElbowArrow]);
    let text = write(h.state().diagram());
    assert!(text.contains("  a -->|\"SQL\"| b\n"), "{text}");
    assert!(text.contains("route=elbow"), "{text}");
    assert_eq!(kinds(&h).line_tool, Agreed::Same(Tool::ElbowArrow));
    undo(&mut h);
    assert_eq!(line_tools(&h), [Tool::Line]);

    for (name, tool) in [
        ("Right-Angle Line", Tool::ElbowLine),
        ("Arrow", Tool::Arrow),
        ("Right-Angle Arrow", Tool::ElbowArrow),
        ("Line", Tool::Line),
    ] {
        select_all(&mut h);
        open_list(&mut h, "Change Line");
        pick(&mut h, name);
        assert_eq!(line_tools(&h), [tool], "{name}");
    }
    assert_eq!(
        h.state().diagram().connectors[0].label.as_deref(),
        Some("SQL")
    );
    assert!(!write(h.state().diagram()).contains("route=elbow"));
}

#[test]
fn a_mixed_selection_becomes_one_line_type_keeping_pinned_middles() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    let d = h.state_mut().diagram_mut();
    d.connectors.push(Connector::new(
        ConnectorKind::Line,
        End::Free(Point::new(600.0, 100.0)),
        End::Free(Point::new(700.0, 100.0)),
    ));
    let mut elbow = Connector::new(
        ConnectorKind::Line,
        End::Free(Point::new(600.0, 300.0)),
        End::Free(Point::new(700.0, 400.0)),
    );
    let pinned = Route::Elbow {
        mid: Some(blockliner_core::Mid::X(640.0)),
    };
    elbow.route = pinned;
    d.connectors.push(elbow);
    h.run_steps(2);
    select_all(&mut h);
    show_pane(&mut h);
    assert_eq!(kinds(&h).line_tool, Agreed::Mixed);

    open_list(&mut h, "Change Line");
    pick(&mut h, "Right-Angle Arrow");
    assert_eq!(line_tools(&h), [Tool::ElbowArrow, Tool::ElbowArrow]);
    assert_eq!(h.state().diagram().connectors[1].route, pinned);
    undo(&mut h);
    assert_eq!(line_tools(&h), [Tool::Line, Tool::ElbowLine]);

    // The palette, too.
    select_all(&mut h);
    palette(&mut h, "Change Line: Arrow");
    assert_eq!(line_tools(&h), [Tool::Arrow, Tool::Arrow]);
}
