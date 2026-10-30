//! Copy, cut and paste, headlessly, through egui's clipboard events (what
//! the platform's Ctrl+C, Ctrl+X and Ctrl+V become): one undo step each,
//! pastes that step, a paste into a second diagram, plain text as a text
//! box, the right-click menu, and nothing while a label is being edited.
//!
//! `two_shapes` puts `a` at (100, 100) and `b` at (400, 300), 160 × 60.

use blockliner_core::{Connector, ConnectorKind, End, Rect, ShapeKind};
use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;

use crate::ui_tests::{at, click, harness, two_shapes, H};

fn ids(h: &H) -> Vec<String> {
    let d = h.state().diagram();
    d.shapes.iter().map(|s| s.id.clone()).collect()
}

fn rect(h: &H, id: &str) -> Rect {
    h.state().diagram().shape(id).unwrap().rect
}

fn send(h: &mut H, e: Event) {
    h.event(e);
    h.run_steps(2);
}

/// `two_shapes`, joined by an arrow, all selected and copied.
fn copied(h: &mut H) -> String {
    two_shapes(h);
    h.state_mut().diagram_mut().connectors.push(Connector::new(
        ConnectorKind::Arrow,
        End::glued("a", None),
        End::glued("b", None),
    ));
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
    send(h, Event::Copy);
    h.state().clipboard_text().expect("copied").to_owned()
}

#[test]
fn copy_and_paste_step_and_undo_in_one_go() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    let text = copied(&mut h);
    assert!(text.starts_with("flowchart LR\n"), "{text}");
    assert!(text.contains("  a --> b\n"), "{text}");
    assert!(
        text.contains("%% @shape a x=100 y=100 w=160 h=60\n"),
        "{text}"
    );

    // Two cells down and right, with new ids, selected.
    send(&mut h, Event::Paste(text.clone()));
    assert_eq!(ids(&h), ["a", "b", "a_2", "b_2"]);
    assert_eq!(rect(&h, "a_2"), Rect::new(140.0, 140.0, 160.0, 60.0));
    assert_eq!(h.state().selection(), ["a_2", "b_2"]);
    assert_eq!(h.state().selected_lines(), [1]);
    let arrow = &h.state().diagram().connectors[1];
    assert_eq!(
        (arrow.from.clone(), arrow.to.clone()),
        (End::glued("a_2", None), End::glued("b_2", None))
    );
    // Again: two cells further.
    send(&mut h, Event::Paste(text));
    assert_eq!(rect(&h, "a_3"), Rect::new(180.0, 180.0, 160.0, 60.0));

    // Each paste is one undo step.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(ids(&h), ["a", "b", "a_2", "b_2"]);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(ids(&h), ["a", "b"]);
    assert_eq!(h.state().diagram().connectors.len(), 1);
}

#[test]
fn cut_is_one_undo_step() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    click(&mut h, at(150.0, 120.0));
    h.run_steps(2);
    send(&mut h, Event::Cut);
    assert_eq!(ids(&h), ["b"]);
    assert!(h.state().clipboard_text().unwrap().contains("  a[\"A\"]\n"));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(ids(&h), ["a", "b"]);

    // Nothing selected: nothing copied or cut.
    let before = h.state().clipboard_text().map(str::to_owned);
    h.key_press(Key::Escape);
    h.run_steps(2);
    send(&mut h, Event::Cut);
    assert_eq!(ids(&h), ["a", "b"]);
    assert_eq!(h.state().clipboard_text().map(str::to_owned), before);
}

#[test]
fn a_copy_pastes_into_another_diagram() {
    let (_tmp, paths, mut h) = harness();
    h.run_steps(2);
    let text = copied(&mut h);
    let folder = paths.prefs().last_project.unwrap();
    std::fs::write(folder.join("Other.mmd"), "flowchart LR\n").unwrap();
    h.state_mut().open_project(folder, Some("Other".into()));
    h.run_steps(2);
    assert_eq!(h.state().doc_name(), Some("Other"));
    assert!(ids(&h).is_empty());

    send(&mut h, Event::Paste(text));
    // Its ids are free here; it lands two cells from where it was.
    assert_eq!(ids(&h), ["a", "b"]);
    assert_eq!(rect(&h, "b"), Rect::new(440.0, 340.0, 160.0, 60.0));
    assert_eq!(h.state().diagram().connectors.len(), 1);
}

#[test]
fn plain_text_and_other_flowcharts_paste_too() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    send(&mut h, Event::Paste("Retries are async".into()));
    let d = h.state().diagram();
    assert_eq!(d.shapes.len(), 1);
    let s = &d.shapes[0];
    assert_eq!(
        (&s.kind, s.label.as_str()),
        (&ShapeKind::Text, "Retries are async")
    );
    assert_eq!(h.state().selection(), std::slice::from_ref(&s.id));
    // In the middle of the view.
    let view = h.state().canvas().visible();
    assert!((s.rect.center().x - view.center().x).abs() <= 20.0);
    assert!((s.rect.center().y - view.center().y).abs() <= 20.0);

    // A flowchart from a README: laid out, its edge kept.
    send(
        &mut h,
        Event::Paste("graph TD\n  web[Web] --> api(API)\n".into()),
    );
    let d = h.state().diagram();
    assert_eq!(d.shapes.len(), 3);
    assert_eq!(d.shape("api").unwrap().kind, ShapeKind::RoundRect);
    assert!(rect(&h, "web").bottom() <= rect(&h, "api").y);
    assert_eq!(h.state().diagram().connectors.len(), 1);
    assert_eq!(h.state().selection(), ["web", "api"]);
}

#[test]
fn the_menu_copies_and_a_label_edit_keeps_the_keys() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    let p = at(150.0, 120.0);
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
    // Its label carries the shortcut too.
    h.get_by_label("Copy Ctrl+C").click();
    h.run_steps(3);
    assert!(h.state().clipboard_text().unwrap().contains("  a[\"A\"]\n"));

    // While a label is being edited, the events are the text field's.
    h.run_steps(30);
    click(&mut h, p);
    click(&mut h, p);
    h.run_steps(3);
    assert!(h.state().editing_label());
    send(&mut h, Event::Paste("pasted".into()));
    send(&mut h, Event::Cut);
    assert_eq!(ids(&h), ["a", "b"]);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert!(h
        .state()
        .diagram()
        .shape("a")
        .unwrap()
        .label
        .contains("pasted"));
}

/// Steps until a frame's output passes `seen`, for up to 5 frames.
fn output_shows(h: &mut H, seen: impl Fn(&egui::FullOutput) -> bool) -> bool {
    (0..5).any(|_| {
        h.step();
        seen(h.output())
    })
}

#[test]
fn copy_reaches_the_system_clipboard_and_paste_asks_for_it() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    click(&mut h, at(150.0, 120.0));
    h.run_steps(2);
    h.event(Event::Copy);
    let text = |h: &H| h.state().clipboard_text().unwrap_or_default().to_owned();
    assert!(output_shows(&mut h, |o| o
        .platform_output
        .commands
        .iter()
        .any(
            |c| matches!(c, egui::OutputCommand::CopyText(t) if t.contains("a[\"A\"]"))
        )));
    assert!(text(&h).contains("a[\"A\"]"));

    // The menu's Paste asks for the clipboard (it comes back as a paste
    // event, which the tests above send themselves).
    let p = at(700.0, 500.0);
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
    assert!(
        h.query_by_label("Copy Ctrl+C").is_none(),
        "nothing selected"
    );
    h.get_by_label("Paste Ctrl+V").click();
    assert!(output_shows(&mut h, |o| o.viewport_output.values().any(
        |v| v.commands.contains(&egui::ViewportCommand::RequestPaste)
    )));
}
