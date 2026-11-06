//! Notes, headlessly: the Note tool from N, 0, and the toolbar; a clicked
//! note's size and label; Change Shape to and from a note; Reset Style back
//! to yellow; Select All Shapes; one undo step; and the SVG export's folded
//! corner and dark label.

use blockliner_core::export::{scope, Objects};
use blockliner_core::style::{Colour, NOTE_FILL};
use blockliner_core::{write, Diagram, Rect, Shape, ShapeKind};
use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;

use crate::canvas::Tool;
use crate::export;
use crate::ui_tests::{at, click, harness, two_shapes, H};
use crate::ui_tests_export::context;

fn type_text(h: &mut H, text: &str) {
    h.event(Event::Text(text.into()));
    h.step();
}

fn palette(h: &mut H, command: &str) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(3);
    h.event(Event::Text(command.into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
}

fn kind(h: &H, id: &str) -> ShapeKind {
    h.state().diagram().shape(id).unwrap().kind.clone()
}

#[test]
fn n_zero_and_the_toolbar_pick_the_note_tool() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    h.key_press(Key::N);
    h.run_steps(2);
    assert_eq!(h.state().tool(), Tool::Note);
    h.key_press(Key::V);
    h.run_steps(2);
    h.key_press(Key::Num0);
    h.run_steps(2);
    assert_eq!(h.state().tool(), Tool::Note);
    h.key_press(Key::V);
    h.run_steps(2);
    h.get_by_label("Note").click();
    h.run_steps(2);
    assert_eq!(h.state().tool(), Tool::Note);
}

#[test]
fn click_for_a_note_and_name_it() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    h.key_press(Key::N);
    h.run_steps(2);
    click(&mut h, at(300.0, 300.0));
    h.run_steps(3);

    // 8 × 4 cells, centred on the click and snapped, its label being edited.
    let d = h.state().diagram();
    assert_eq!(d.shapes.len(), 1);
    assert_eq!(d.shapes[0].kind, ShapeKind::Note);
    assert_eq!(d.shapes[0].rect, Rect::new(220.0, 260.0, 160.0, 80.0));
    assert!(h.state().editing_label());
    type_text(&mut h, "Retries");
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(
        h.state().diagram().shape("retries").unwrap().label,
        "Retries"
    );
    // The Properties pane names it.
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.run_steps(3);
    h.get_by_label("Note “Retries”");
    let text = write(h.state().diagram());
    assert!(
        text.contains("  retries[\"Retries\"]:::bl_note\n"),
        "{text}"
    );
    assert!(text.contains("  classDef bl_note "), "{text}");

    // One undo step takes it away.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert!(h.state().diagram().shapes.is_empty());
}

#[test]
fn change_shape_to_a_note_and_back_keeps_the_rest() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    click(&mut h, at(180.0, 130.0));
    h.run_steps(2);
    palette(&mut h, "Change Shape: Note");
    let a = h.state().diagram().shape("a").unwrap().clone();
    assert_eq!(a.kind, ShapeKind::Note);
    assert_eq!(a.rect, Rect::new(100.0, 100.0, 160.0, 60.0));
    assert_eq!(a.label, "A");
    palette(&mut h, "Change Shape: Rectangle");
    assert_eq!(kind(&h, "a"), ShapeKind::Rect);
    assert!(!write(h.state().diagram()).contains("bl_note"));
}

#[test]
fn reset_style_makes_a_note_yellow_again() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    let d = h.state_mut().diagram_mut();
    let a = d.shape_mut("a").unwrap();
    a.kind = ShapeKind::Note;
    a.style.set_fill(Some(Colour::parse("#dbeafe")));
    h.run_steps(2);
    click(&mut h, at(180.0, 130.0));
    h.run_steps(2);
    palette(&mut h, "Reset Style");
    let a = h.state().diagram().shape("a").unwrap();
    assert!(a.style.is_empty());
    assert_eq!(a.kind, ShapeKind::Note);
    assert_eq!(a.drawn_style().fill, Some(Colour::parse(NOTE_FILL)));
}

#[test]
fn select_all_shapes_includes_notes() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    h.state_mut().diagram_mut().shape_mut("b").unwrap().kind = ShapeKind::Note;
    h.run_steps(2);
    palette(&mut h, "Select All Shapes");
    assert_eq!(h.state().selection(), ["a", "b"]);
}

#[test]
fn a_note_exports_with_its_fold_and_a_dark_label() {
    let ctx = context();
    let mut d = Diagram::new();
    d.shapes.push(Shape::new(
        "n",
        ShapeKind::Note,
        "Hi",
        Rect::new(40.0, 40.0, 160.0, 80.0),
    ));
    let s = scope(&d, Objects::All).unwrap();
    let svg = export::svg(&ctx, &d, &s, false);
    // The body with its corner cut, the stroke inside; then the fold, in
    // the fill 10 % darker.
    assert!(
        svg.contains(
            r##"<polygon points="40.5,40.5 183.5,40.5 199.5,56.5 199.5,119.5 40.5,119.5" fill="#fef9c3" stroke="#eab308""##
        ),
        "{svg}"
    );
    assert!(
        svg.contains(r##"<polygon points="183.5,40.5 183.5,56.5 199.5,56.5" fill="#e5e0b0""##),
        "{svg}"
    );
    assert!(svg.contains("#111827"), "{svg}");
    export::png(&svg, 1.0).expect("resvg reads it");
}
