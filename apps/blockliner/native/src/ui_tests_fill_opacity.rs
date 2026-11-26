//! Fill opacity, headlessly: 100 % … 0 % (100 % the default, never written)
//! from the Properties pane's Fill Opacity list, the right-click menu's
//! Fill Opacity ▸ and the palette, for shapes only; the label colour that
//! leaves a faint fill, Mixed selections, hand-written values, Reset Style,
//! and one undo step per change.
//!
//! `two_shapes` puts `a` at (100, 100) and `b` at (400, 300), 160 × 60.

use blockliner_core::style::{Agreement, Colour, Opacity, PRESETS};
use blockliner_core::{read, write, Anchor, Connector, ConnectorKind, End};
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};

use crate::ui_tests::{at, click, harness, two_shapes, H};

fn opacity_of(h: &H, id: &str) -> Option<Opacity> {
    h.state().diagram().shape(id).unwrap().style.opacity()
}

fn selected_opacity(h: &H) -> Agreement<Option<Opacity>> {
    h.state()
        .canvas()
        .selected_style(h.state().diagram())
        .opacity
}

fn show_pane(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.run_steps(3);
}

fn select_all(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
}

/// Opens the pane's Fill Opacity list and picks `name` in it.
fn pane_pick(h: &mut H, name: &str) {
    h.get_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some("Fill Opacity"))
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
fn the_pane_sets_a_shapes_opacity_and_the_default_is_not_written() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    click(&mut h, at(180.0, 130.0));
    h.run_steps(2);
    show_pane(&mut h);
    assert_eq!(selected_opacity(&h), Agreement::Same(None));

    // Without a fill of its own: the theme's fill, thinned.
    pane_pick(&mut h, "50%");
    assert_eq!(opacity_of(&h, "a"), Some(Opacity::Preset(50)));
    assert_eq!(opacity_of(&h, "b"), None);
    let text = write(h.state().diagram());
    assert!(text.contains("  style a fill-opacity:0.5\n"), "{text}");
    assert_eq!(
        read(&text).unwrap().shape("a").unwrap().style.opacity(),
        Some(Opacity::Preset(50))
    );

    pane_pick(&mut h, "25%");
    assert!(write(h.state().diagram()).contains("  style a fill-opacity:0.25\n"));

    // One undo step each.
    undo(&mut h);
    assert_eq!(opacity_of(&h, "a"), Some(Opacity::Preset(50)));

    // Back to the default: nothing written.
    pane_pick(&mut h, "100% (Default)");
    assert_eq!(opacity_of(&h, "a"), None);
    assert!(!write(h.state().diagram()).contains("style a"));
}

#[test]
fn the_menu_and_the_palette_set_it_and_a_faint_fill_drops_its_label_colour() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.state_mut()
        .diagram_mut()
        .shape_mut("a")
        .unwrap()
        .style
        .set_fill(Some(Colour::parse(PRESETS[1].fill)));
    h.run_steps(2);

    right_click(&mut h, at(180.0, 130.0));
    h.get_by_label("Fill Opacity").click();
    h.run_steps(3);
    pick(&mut h, "75%");
    let text = write(h.state().diagram());
    assert!(
        text.contains("  style a fill:#dbeafe,stroke:#3b82f6,color:#111827,fill-opacity:0.75\n"),
        "{text}"
    );

    // Under 50 %, the label colour that came with the fill goes…
    palette(&mut h, "Fill Opacity: 25%");
    let text = write(h.state().diagram());
    assert!(
        text.contains("  style a fill:#dbeafe,stroke:#3b82f6,fill-opacity:0.25\n"),
        "{text}"
    );
    // …and comes back with the fill.
    palette(&mut h, "Fill Opacity: 100% (Default)");
    let text = write(h.state().diagram());
    assert!(
        text.contains("  style a fill:#dbeafe,stroke:#3b82f6,color:#111827\n"),
        "{text}"
    );
}

#[test]
fn a_mixed_selection_takes_one_opacity_and_lines_take_none() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    let d = h.state_mut().diagram_mut();
    d.shape_mut("a")
        .unwrap()
        .style
        .set_opacity(Some(Opacity::Preset(75)));
    d.connectors.push(Connector::new(
        ConnectorKind::Line,
        End::glued("a", Some(Anchor::E)),
        End::glued("b", Some(Anchor::W)),
    ));
    h.run_steps(2);
    select_all(&mut h);
    show_pane(&mut h);
    assert_eq!(selected_opacity(&h), Agreement::Mixed);

    pane_pick(&mut h, "0%");
    assert_eq!(
        selected_opacity(&h),
        Agreement::Same(Some(Opacity::Preset(0)))
    );
    assert_eq!(opacity_of(&h, "b"), Some(Opacity::Preset(0)));
    // A line has no fill.
    assert!(h.state().diagram().connectors[0].style.is_empty());
    assert!(!write(h.state().diagram()).contains("linkStyle"));
    // Both shapes in one step.
    undo(&mut h);
    assert_eq!(opacity_of(&h, "a"), Some(Opacity::Preset(75)));
    assert_eq!(opacity_of(&h, "b"), None);

    // Every row is showing, and Reset Style clears the opacity.
    select_all(&mut h);
    crate::ui_tests::pane_reset_style(&mut h);
    assert_eq!(opacity_of(&h, "a"), None);
    assert!(!write(h.state().diagram()).contains("fill-opacity"));

    // With only the line selected, the list is off.
    h.key_press(Key::Escape);
    h.run_steps(2);
    palette(&mut h, "Select All Lines");
    let list =
        h.get_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some("Fill Opacity"));
    assert!(list.accesskit_node().is_disabled());
}

#[test]
fn hand_written_opacities_stay_as_written() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    let text = "flowchart LR\n  a[\"A\"]\n  style a fill:#dbeafe,fill-opacity:0.4\n\
                %% blockliner 1 grid=20\n%% @shape a x=100 y=100 w=160 h=60\n";
    *h.state_mut().diagram_mut() = read(text).unwrap();
    h.run_steps(2);
    click(&mut h, at(180.0, 130.0));
    h.run_steps(2);
    show_pane(&mut h);
    let custom = Some(Opacity::Custom("0.4".into()));
    assert_eq!(selected_opacity(&h), Agreement::Same(custom));
    assert!(write(h.state().diagram()).contains("fill:#dbeafe,fill-opacity:0.4\n"));

    pane_pick(&mut h, "50%");
    assert_eq!(opacity_of(&h, "a"), Some(Opacity::Preset(50)));
}
