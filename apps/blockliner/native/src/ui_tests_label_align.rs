//! Label alignment, headlessly: Left / Center / Right and Top / Middle /
//! Bottom from the Properties pane and the palette, Mixed selections, undo,
//! the `@shape` line it is saved on, and where the SVG export puts the text.
//!
//! `two_shapes` puts `a` at (100, 100) and `b` at (400, 300), 160 × 60.

use blockliner_core::export::{scope, Objects};
use blockliner_core::style::Agreement;
use blockliner_core::{read, write, HAlign, LabelAlign, VAlign};
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;

use crate::export;
use crate::ui_tests::{at, click, harness, two_shapes, H};
use crate::ui_tests_export::context;

fn align(h: &H, id: &str) -> LabelAlign {
    h.state().diagram().shape(id).unwrap().align
}

/// Shows the pane, scrolled down to its alignment lists: in the 800-high
/// test window the pane is shorter than its rows.
fn show_pane(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.run_steps(3);
    crate::ui_tests::pane_scroll_down(h);
}

/// Opens the pane's `list` and picks `name`.
fn pane_pick(h: &mut H, list: &str, name: &str) {
    h.get_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some(list))
        .click();
    h.run_steps(3);
    h.get_by(|n| n.role() == Role::MenuItemRadio && n.label().as_deref() == Some(name))
        .click();
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

fn undo(h: &mut H) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
}

#[test]
fn the_pane_aligns_a_label_and_the_file_keeps_it() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    click(&mut h, at(180.0, 130.0));
    h.run_steps(2);
    show_pane(&mut h);
    assert_eq!(align(&h, "a"), LabelAlign::default());
    // Centred is not written.
    let text = write(h.state().diagram());
    assert!(
        text.contains("%% @shape a x=100 y=100 w=160 h=60\n"),
        "{text}"
    );

    pane_pick(&mut h, "Label Alignment", "Left");
    assert_eq!(align(&h, "a").h, HAlign::Left);
    assert_eq!(align(&h, "b"), LabelAlign::default());
    pane_pick(&mut h, "Vertical Alignment", "Bottom");
    assert_eq!(align(&h, "a").v, VAlign::Bottom);

    let text = write(h.state().diagram());
    assert!(
        text.contains("%% @shape a x=100 y=100 w=160 h=60 align=left valign=bottom\n"),
        "{text}"
    );
    assert!(
        text.contains("%% @shape b x=400 y=300 w=160 h=60\n"),
        "{text}"
    );
    let back = read(&text).unwrap();
    assert_eq!(back.shape("a").unwrap().align, align(&h, "a"));
    assert_eq!(back.shape("b").unwrap().align, LabelAlign::default());

    // One undo step each.
    undo(&mut h);
    assert_eq!(
        (align(&h, "a").h, align(&h, "a").v),
        (HAlign::Left, VAlign::Middle)
    );
    undo(&mut h);
    assert_eq!(align(&h, "a"), LabelAlign::default());
}

#[test]
fn the_palette_aligns_every_selected_shape_and_mixed_shows() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    click(&mut h, at(180.0, 130.0));
    h.run_steps(2);
    palette(&mut h, "Label Alignment: Right");
    assert_eq!(align(&h, "a").h, HAlign::Right);

    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
    let style = h.state().canvas().selected_style(h.state().diagram());
    assert_eq!(style.halign, Agreement::Mixed);
    assert_eq!(style.valign, Agreement::Same(VAlign::Middle));

    palette(&mut h, "Label Vertical Alignment: Top");
    assert_eq!(align(&h, "a").v, VAlign::Top);
    assert_eq!(align(&h, "b").v, VAlign::Top);
    // The other way is left as it was.
    assert_eq!(align(&h, "a").h, HAlign::Right);
    assert_eq!(align(&h, "b").h, HAlign::Center);
}

/// The first `<tspan>`'s `y` in the label of the shape whose text is `label`.
fn tspan_y(svg: &str, label: &str) -> f32 {
    let end = svg.find(&format!(">{label}</tspan>")).unwrap();
    let start = svg[..end].rfind("y=\"").unwrap() + 3;
    svg[start..end].trim_end_matches('"').parse().unwrap()
}

#[test]
fn the_export_puts_the_text_where_the_canvas_does() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    let ctx = context();
    let svg_of = |h: &H| {
        let d = h.state().diagram();
        export::svg(&ctx, d, &scope(d, Objects::All).unwrap(), false)
    };
    let label = h.state().diagram().shape("a").unwrap().label.clone();
    let centred = svg_of(&h);
    assert!(!centred.contains("text-anchor=\"start\""), "{centred}");
    let middle = tspan_y(&centred, &label);

    {
        let a = h.state_mut().diagram_mut().shape_mut("a").unwrap();
        a.align = LabelAlign {
            h: HAlign::Left,
            v: VAlign::Top,
        };
    }
    let svg = svg_of(&h);
    // 6 px of padding inside the shape's left side.
    assert!(
        svg.contains(r#"text-anchor="start" dominant-baseline="central"><tspan x="106""#),
        "{svg}"
    );
    assert!(tspan_y(&svg, &label) < middle - 5.0);

    {
        let a = h.state_mut().diagram_mut().shape_mut("a").unwrap();
        a.align = LabelAlign {
            h: HAlign::Right,
            v: VAlign::Bottom,
        };
    }
    let svg = svg_of(&h);
    assert!(
        svg.contains(r#"text-anchor="end" dominant-baseline="central"><tspan x="254""#),
        "{svg}"
    );
    assert!(tspan_y(&svg, &label) > middle + 5.0);
    // Shape `b` stays centred throughout.
    assert!(svg.contains("text-anchor=\"middle\""), "{svg}");
}
