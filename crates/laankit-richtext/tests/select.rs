//! Selecting across blocks, copying, and clicking highlights, driven through egui_kittest.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use laankit_egui::theme::ThemeState;
use laankit_richtext::{
    from_html, Document, Mark, Options, Position, RichOutput, RichStyle, RichView, WebResolver,
};

const HTML: &str = "<p>Alpha paragraph text.</p><ul><li>Beta item</li></ul>\
                    <p>Gamma paragraph text.</p><p>Delta.</p>";

struct State {
    doc: Document,
    view: RichView,
    out: RichOutput,
    clicked: Vec<String>,
    ready: bool,
}

fn harness() -> Harness<'static, State> {
    let state = State {
        doc: from_html(HTML, &Options::default()),
        view: RichView::default(),
        out: RichOutput::default(),
        clicked: Vec::new(),
        ready: false,
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(700.0, 500.0))
        .build_ui_state(
            |ui, s: &mut State| {
                if !s.ready {
                    // Fonts are installed in one frame and used from the next.
                    laankit_egui::fonts::install(ui.ctx());
                    ThemeState::default().apply(ui.ctx());
                    s.ready = true;
                    return;
                }
                let style = RichStyle::from_ctx(ui.ctx());
                s.out = s.view.show(ui, &s.doc, &style, &mut WebResolver);
                if let Some(id) = s.out.clicked_mark.clone() {
                    s.clicked.push(id);
                }
            },
            state,
        );
    h.run();
    h
}

/// A point `frac` of the way across the label containing `text`, vertically centred.
fn at(h: &Harness<'_, State>, text: &str, frac: f32) -> egui::Pos2 {
    let r = h.get_by_label_contains(text).rect();
    egui::pos2(r.left() + r.width() * frac, r.center().y)
}

#[test]
fn drag_selects_across_blocks_and_copies() {
    let mut h = harness();
    let from = at(&h, "Alpha", 0.5);
    let to = at(&h, "Gamma", 0.3);
    h.drag_at(from);
    h.run();
    h.hover_at(to);
    h.run();
    h.drop_at(to);
    h.run();

    let sel = h
        .state()
        .view
        .selection(&h.state().doc)
        .expect("a selection");
    assert_eq!(sel.start.block, 0);
    assert_eq!(sel.end.block, 2);
    assert!(sel.start.offset > 0 && sel.end.offset > 0, "{sel:?}");
    assert!(sel.text.contains("Beta item"), "{:?}", sel.text);
    // From inside block 0 (wherever the press landed) to the start of "Gamma …".
    let first = sel.text.lines().next().unwrap();
    assert!("Alpha paragraph text.".ends_with(first), "{:?}", sel.text);
    assert!(
        sel.text.lines().last().unwrap().starts_with("Gam"),
        "{:?}",
        sel.text
    );

    // Ctrl+C copies it.
    h.event(egui::Event::Copy);
    // One step: the copy command is in that frame's output.
    h.step();
    let copied = h
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if *t == sel.text));
    assert!(copied, "{:?}", h.output().platform_output.commands);

    // A click elsewhere clears it.
    let delta = at(&h, "Delta", 0.5);
    h.drag_at(delta);
    h.run();
    h.drop_at(delta);
    h.run();
    assert!(h.state().view.selection(&h.state().doc).is_none());
}

#[test]
fn clicking_a_mark_reports_it() {
    let mut h = harness();
    h.state_mut().view.set_marks(vec![Mark {
        id: "hl-1".into(),
        start: Position {
            block: 2,
            offset: 0,
        },
        end: Position {
            block: 2,
            offset: 5,
        },
        color: egui::Color32::from_rgba_unmultiplied(240, 200, 0, 80),
    }]);
    h.run();
    // Inside the mark ("Gamma"), then outside it ("text.").
    let inside = at(&h, "Gamma", 0.05);
    h.drag_at(inside);
    h.run();
    h.drop_at(inside);
    h.run();
    assert_eq!(h.state().clicked, ["hl-1"]);
    let outside = at(&h, "Gamma", 0.9);
    h.drag_at(outside);
    h.run();
    h.drop_at(outside);
    h.run();
    assert_eq!(h.state().clicked, ["hl-1"]);
    assert!(h.state().view.selection(&h.state().doc).is_none());
}

/// Presses and releases the primary button at `p` `times` times in quick succession (kittest only
/// counts a double or triple click if the input clock moves between them).
fn clicks_at(h: &mut Harness<'_, State>, p: egui::Pos2, times: usize, modifiers: egui::Modifiers) {
    h.hover_at(p);
    h.run();
    let start = h.ctx.input(|i| i.time) + 1.0;
    // Held modifiers come from the keyboard (as a real one reports them), not just the event.
    h.event(egui::Event::ModifiersChanged(modifiers));
    for i in 0..times * 2 {
        h.input_mut().time = Some(start + i as f64 * 0.05);
        h.event(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: i % 2 == 0,
            modifiers,
        });
        h.step();
    }
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run();
}

fn selected(h: &Harness<'_, State>) -> Option<String> {
    h.state().view.selection(&h.state().doc).map(|s| s.text)
}

#[test]
fn double_click_selects_a_word() {
    let mut h = harness();
    // Inside "paragraph" of "Alpha paragraph text.".
    let p = at(&h, "Alpha", 0.4);
    clicks_at(&mut h, p, 2, egui::Modifiers::NONE);
    assert_eq!(selected(&h).as_deref(), Some("paragraph"));
}

#[test]
fn triple_click_selects_the_paragraph() {
    let mut h = harness();
    let p = at(&h, "Gamma", 0.4);
    clicks_at(&mut h, p, 3, egui::Modifiers::NONE);
    assert_eq!(selected(&h).as_deref(), Some("Gamma paragraph text."));
}

#[test]
fn shift_click_extends_from_the_last_click() {
    let mut h = harness();
    let p = at(&h, "Alpha", 0.0);
    clicks_at(&mut h, p, 1, egui::Modifiers::NONE);
    assert!(selected(&h).is_none(), "a plain click selects nothing");
    let p = at(&h, "Delta", 1.0);
    clicks_at(&mut h, p, 1, egui::Modifiers::SHIFT);
    let text = selected(&h).expect("Shift+click extends");
    assert!(text.starts_with("Alpha"), "{text:?}");
    assert!(
        text.contains("Beta item") && text.contains("Delta"),
        "{text:?}"
    );
    // Shift+click again moves the end, keeping the start.
    let p = at(&h, "Beta", 0.99);
    clicks_at(&mut h, p, 1, egui::Modifiers::SHIFT);
    let text = selected(&h).expect("still selected");
    assert!(
        text.starts_with("Alpha") && !text.contains("Gamma"),
        "{text:?}"
    );
}

#[test]
fn select_all_and_copy() {
    let mut h = harness();
    // Not hovered and nothing selected: Ctrl+A isn't this view's.
    h.hover_at(egui::pos2(-10.0, -10.0));
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    assert!(selected(&h).is_none());
    // Over the view: everything.
    h.hover_at(at(&h, "Gamma", 0.5));
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    let all = selected(&h).expect("everything selected");
    assert_eq!(
        all,
        "Alpha paragraph text.\nBeta item\nGamma paragraph text.\nDelta."
    );
    h.event(egui::Event::Copy);
    h.step();
    let copied = h
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if *t == all));
    assert!(copied, "{:?}", h.output().platform_output.commands);
}
