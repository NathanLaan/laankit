//! Drives the editor headlessly (egui_kittest): undo/redo, list keys as
//! single undo steps, IME composition, and the cursor across relayouts.

use egui::text::{CCursor, CCursorRange};
use egui::{vec2, Event, ImeEvent, Key, Modifiers, TextEdit};
use egui_kittest::Harness;

use crate::editor::MarkdownEditor;
use crate::editor_area;
use crate::highlight::Strategy;

struct State {
    editor: MarkdownEditor,
    text: String,
}

fn harness(text: &str) -> Harness<'static, State> {
    let editor = MarkdownEditor::new(egui::Id::unique("test-editor"));
    let id = editor.id;
    // The default fonts have no CJK glyphs (see RESULTS.md); the IME tests only check the text.
    let mut h = Harness::builder()
        .with_size(vec2(800.0, 600.0))
        .allow_missing_glyphs()
        .build_ui_state(
            |ui, s: &mut State| {
                editor_area(ui, &mut s.editor, &mut s.text);
            },
            State {
                editor,
                text: text.to_string(),
            },
        );
    h.ctx.memory_mut(|m| m.request_focus(id));
    h.run();
    h
}

fn set_cursor(h: &mut Harness<'static, State>, c: usize) {
    let id = h.state().editor.id;
    let mut state = TextEdit::load_state(&h.ctx, id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(CCursorRange::one(CCursor::new(c))));
    state.store(&h.ctx, id);
    h.run();
}

fn cursor(h: &Harness<'static, State>) -> (usize, usize) {
    let r = TextEdit::load_state(&h.ctx, h.state().editor.id)
        .and_then(|s| s.cursor.char_range())
        .expect("cursor");
    (r.secondary.index.0, r.primary.index.0)
}

fn type_text(h: &mut Harness<'static, State>, s: &str) {
    h.event(Event::Text(s.into()));
    h.run();
}

/// Lets the undoer's `stable_time` (1 s) pass so it records an undo point.
fn settle(h: &mut Harness<'static, State>) {
    h.run_steps(8);
}

fn undo(h: &mut Harness<'static, State>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
}

fn redo(h: &mut Harness<'static, State>) {
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.run();
}

#[test]
fn typing_undo_and_redo() {
    let mut h = harness("hello");
    set_cursor(&mut h, 5);
    settle(&mut h);
    type_text(&mut h, " world");
    settle(&mut h);
    assert_eq!(h.state().text, "hello world");
    undo(&mut h);
    assert_eq!(h.state().text, "hello");
    redo(&mut h);
    assert_eq!(h.state().text, "hello world");
}

#[test]
fn list_enter_is_one_undo_step() {
    let mut h = harness("- a");
    set_cursor(&mut h, 3);
    settle(&mut h);
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().text, "- a\n- ");
    assert_eq!(cursor(&h), (6, 6));
    type_text(&mut h, "b");
    assert_eq!(h.state().text, "- a\n- b");
    // Enter twice: continue, then end the list on the empty item.
    h.key_press(Key::Enter);
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().text, "- a\n- b\n");
    settle(&mut h);
    undo(&mut h);
    assert_eq!(h.state().text, "- a\n- b\n- ");
    redo(&mut h);
    assert_eq!(h.state().text, "- a\n- b\n");
}

#[test]
fn tab_and_shift_tab_indent_the_line() {
    let mut h = harness("- a\n- b");
    set_cursor(&mut h, 7);
    h.key_press(Key::Tab);
    h.run();
    assert_eq!(h.state().text, "- a\n  - b");
    assert_eq!(cursor(&h), (9, 9));
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run();
    assert_eq!(h.state().text, "- a\n- b");
    assert!(
        h.ctx.memory(|m| m.has_focus(h.state().editor.id)),
        "Tab must not move focus"
    );
}

#[test]
fn ime_preedit_and_commit_land_at_the_cursor() {
    let mut h = harness("ab");
    set_cursor(&mut h, 1);
    h.event(Event::Ime(ImeEvent::Preedit {
        text: "に".into(),
        active_range_chars: None,
    }));
    h.run();
    assert_eq!(h.state().text, "aにb");
    h.event(Event::Ime(ImeEvent::Preedit {
        text: "にほ".into(),
        active_range_chars: None,
    }));
    h.run();
    assert_eq!(h.state().text, "aにほb");
    h.event(Event::Ime(ImeEvent::Commit("日本".into())));
    h.run();
    assert_eq!(h.state().text, "a日本b");
    assert_eq!(cursor(&h), (3, 3));
}

#[test]
fn enter_during_composition_belongs_to_the_ime() {
    let enter = Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    // Some platforms deliver the confirming Enter in the same frame as the commit.
    let mut h = harness("- a");
    set_cursor(&mut h, 3);
    h.input_mut()
        .events
        .extend([Event::Ime(ImeEvent::Commit("字".into())), enter.clone()]);
    h.step();
    h.run();
    assert!(h.state().text.starts_with("- a字"), "{:?}", h.state().text);
    assert!(
        !h.state().text.contains("\n- "),
        "list continuation fired during IME: {:?}",
        h.state().text
    );
    // Others deliver an Enter while the preedit is still open.
    let mut h = harness("- a");
    set_cursor(&mut h, 3);
    h.event(Event::Ime(ImeEvent::Preedit {
        text: "じ".into(),
        active_range_chars: None,
    }));
    h.event(enter.clone());
    h.run();
    assert!(
        !h.state().text.contains("\n- "),
        "list continuation fired during IME: {:?}",
        h.state().text
    );
    // After the composition ends, Enter continues the list again.
    let mut h = harness("- a");
    set_cursor(&mut h, 3);
    h.event(Event::Ime(ImeEvent::Preedit {
        text: "じ".into(),
        active_range_chars: None,
    }));
    h.event(Event::Ime(ImeEvent::Commit("字".into())));
    h.event(enter);
    h.run();
    assert_eq!(h.state().text, "- a字\n- ");
}

#[test]
fn cursor_survives_relayout() {
    let doc = crate::gen::document(2000);
    let mut h = harness(&doc);
    let c = doc.chars().count() / 2;
    set_cursor(&mut h, c);
    // A narrower window rewraps every line.
    h.set_size(vec2(420.0, 600.0));
    h.run();
    assert_eq!(cursor(&h), (c, c));
    // Switching strategy and highlighting rebuilds the galley from scratch.
    h.state_mut().editor.highlighter.strategy = Strategy::WholeJob;
    h.state_mut().editor.plain = true;
    h.run();
    assert_eq!(cursor(&h), (c, c));
    // An edit far above the cursor shifts it by the inserted length only.
    let id = h.state().editor.id;
    h.state_mut().text.insert_str(0, "xyz");
    let mut state = TextEdit::load_state(&h.ctx, id).unwrap();
    state
        .cursor
        .set_char_range(Some(CCursorRange::one(CCursor::new(c + 3))));
    state.store(&h.ctx, id);
    h.run();
    type_text(&mut h, "!");
    assert_eq!(cursor(&h), (c + 4, c + 4));
    assert_eq!(h.state().text.chars().nth(c + 3), Some('!'));
}

#[test]
fn line_galleys_match_whole_job_layout() {
    let doc = crate::gen::document(300);
    let mut h = harness(&doc);
    h.run();
    let rows = |h: &Harness<'static, State>| h.state().editor.highlighter.stats;
    let before = rows(&h);
    assert!(before.lines_laid_out >= 300);
    set_cursor(&mut h, 40);
    type_text(&mut h, "x");
    // One line changed, so one line laid out again (twice at most: TextEdit lays out per event and per frame).
    let after = rows(&h);
    assert!(
        after.lines_laid_out - before.lines_laid_out <= 2,
        "{before:?} → {after:?}"
    );
}

#[test]
fn every_strategy_paints_text() {
    for strategy in [
        Strategy::Incremental,
        Strategy::LineGalleys,
        Strategy::WholeJob,
    ] {
        let mut h = harness("# Title\n\n- item\n");
        h.state_mut().editor.highlighter.strategy = strategy;
        h.run();
        let glyphs: usize = h
            .output()
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(t) => {
                    Some(t.galley.rows.iter().map(|r| r.glyphs.len()).sum::<usize>())
                }
                _ => None,
            })
            .sum();
        assert!(
            glyphs >= "#Titleitem".len(),
            "{strategy:?} painted {glyphs} glyphs"
        );
    }
}
