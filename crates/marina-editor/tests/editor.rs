//! Drives the editor headlessly (egui_kittest): list keys, shortcuts, undo,
//! IME, `[[` completion, paste and drop hooks, find and replace, and that the
//! text paints.

use std::sync::Arc;

use egui::{vec2, ColorImage, Event, ImeEvent, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use marina_editor::{Command, Editor, EditorEvent, EditorState};

struct State {
    editor: EditorState,
    events: Vec<EditorEvent>,
}

fn harness(text: &str) -> Harness<'static, State> {
    let mut h = Harness::builder()
        .with_size(vec2(900.0, 600.0))
        // Frames 1/60 s apart, as on screen (typing groups by time).
        .with_step_dt(1.0 / 60.0)
        // No CJK face is bundled; the system fallback may be missing on CI.
        .allow_missing_glyphs()
        .with_context_setup(marina_egui::fonts::install)
        .build_ui_state(
            |ui, s: &mut State| {
                let out = Editor::new(&mut s.editor).show(ui);
                s.events.extend(out.events);
            },
            State {
                editor: EditorState::new("test"),
                events: Vec::new(),
            },
        );
    let ctx = h.ctx.clone();
    h.state_mut().editor.set_text(&ctx, text);
    h.state_mut().editor.focus();
    h.run();
    h
}

fn cursor_to(h: &mut Harness<'static, State>, c: usize) {
    let ctx = h.ctx.clone();
    h.state_mut().editor.set_cursor(&ctx, c, c);
    h.run();
}

fn select(h: &mut Harness<'static, State>, a: usize, b: usize) {
    let ctx = h.ctx.clone();
    h.state_mut().editor.set_cursor(&ctx, a, b);
    h.run();
}

fn cursor(h: &Harness<'static, State>) -> (usize, usize) {
    h.state().editor.cursor(&h.ctx)
}

fn text(h: &Harness<'static, State>) -> String {
    h.state().editor.text().to_owned()
}

fn type_text(h: &mut Harness<'static, State>, s: &str) {
    h.event(Event::Text(s.into()));
    h.run();
}

fn key(h: &mut Harness<'static, State>, key: Key, modifiers: Modifiers) {
    h.key_press_modifiers(modifiers, key);
    h.run();
}

/// A key press whose logical key differs from the physical one (another keyboard layout).
fn physical(h: &mut Harness<'static, State>, logical: Key, physical: Key, modifiers: Modifiers) {
    h.event(Event::Key {
        key: logical,
        physical_key: Some(physical),
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.run();
}

fn changed(h: &mut Harness<'static, State>) -> usize {
    let n = h
        .state()
        .events
        .iter()
        .filter(|e| matches!(e, EditorEvent::Changed))
        .count();
    h.state_mut().events.clear();
    n
}

const CMD: Modifiers = Modifiers::COMMAND;

#[test]
fn text_paints() {
    let h = harness("# Title\n\nSome **bold** text.");
    // The field is named for screen readers and holds the note.
    let node = h.get_by_label("Note text");
    assert!(node.value().is_some_and(|v| v.contains("bold")), "{node:?}");
    // A frame's output has a text shape with the note (a concatenated galley
    // with no sections would lay out but paint nothing: see highlight.rs).
    let ctx = egui::Context::default();
    marina_egui::fonts::install(&ctx);
    ctx.run_ui(Default::default(), |_| {})
        .textures_delta
        .clear();
    let mut state = EditorState::new("paint");
    state.set_text(&ctx, "Some **bold** text.");
    let mut out = ctx.run_ui(Default::default(), |ui| {
        Editor::new(&mut state).show(ui);
    });
    out.textures_delta.clear();
    let found = out.shapes.iter().any(|c| {
        matches!(&c.shape, egui::Shape::Text(t)
            if t.galley.text().contains("**bold**") && !t.galley.is_empty())
    });
    assert!(found, "no text shape painted the note");
}

#[test]
fn set_text_is_not_a_user_edit() {
    let mut h = harness("x");
    assert_eq!(changed(&mut h), 0);
    let ctx = h.ctx.clone();
    h.state_mut().editor.set_text(&ctx, "y");
    h.run();
    assert_eq!(changed(&mut h), 0);
    assert!(!h.state().editor.can_undo());
}

#[test]
fn enter_continues_lists_as_one_undo_step() {
    let mut h = harness("- a");
    cursor_to(&mut h, 3);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(text(&h), "- a\n- ");
    assert_eq!(cursor(&h), (6, 6));
    assert_eq!(changed(&mut h), 1);
    type_text(&mut h, "b");
    key(&mut h, Key::Enter, Modifiers::NONE);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(text(&h), "- a\n- b\n");
    key(&mut h, Key::Z, CMD);
    assert_eq!(text(&h), "- a\n- b\n- ");
    key(&mut h, Key::Z, CMD | Modifiers::SHIFT);
    assert_eq!(text(&h), "- a\n- b\n");
}

#[test]
fn plain_enter_and_backspace_type_normally() {
    let mut h = harness("ab");
    cursor_to(&mut h, 1);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(text(&h), "a\nb");
    key(&mut h, Key::Backspace, Modifiers::NONE);
    assert_eq!(text(&h), "ab");
    // Backspace right after a list marker removes the marker.
    let ctx = h.ctx.clone();
    h.state_mut().editor.set_text(&ctx, "- ");
    cursor_to(&mut h, 2);
    key(&mut h, Key::Backspace, Modifiers::NONE);
    assert_eq!(text(&h), "");
}

#[test]
fn tab_indents_and_shift_tab_outdents() {
    let mut h = harness("- a\n- b");
    cursor_to(&mut h, 7);
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(text(&h), "- a\n  - b");
    key(&mut h, Key::Tab, Modifiers::SHIFT);
    assert_eq!(text(&h), "- a\n- b");
}

#[test]
fn typing_groups_into_one_undo_step() {
    let mut h = harness("hello");
    cursor_to(&mut h, 5);
    for c in [" ", "w", "o", "r", "l", "d"] {
        type_text(&mut h, c);
    }
    assert_eq!(text(&h), "hello world");
    key(&mut h, Key::Z, CMD);
    assert_eq!(text(&h), "hello");
    key(&mut h, Key::Y, CMD);
    assert_eq!(text(&h), "hello world");
}

#[test]
fn emphasis_shortcuts_toggle() {
    let mut h = harness("make bold");
    select(&mut h, 5, 9);
    key(&mut h, Key::B, CMD);
    assert_eq!(text(&h), "make **bold**");
    assert_eq!(cursor(&h), (7, 11));
    key(&mut h, Key::B, CMD);
    assert_eq!(text(&h), "make bold");
    select(&mut h, 0, 4);
    key(&mut h, Key::I, CMD);
    assert_eq!(text(&h), "*make* bold");
}

#[test]
fn list_and_arrow_shortcuts_use_physical_keys() {
    let mut h = harness("alpha\nbeta");
    select(&mut h, 0, 10);
    // On a layout where Shift+8 types something else, the logical key isn't Num8.
    physical(&mut h, Key::OpenBracket, Key::Num8, CMD | Modifiers::SHIFT);
    assert_eq!(text(&h), "- alpha\n- beta");
    physical(&mut h, Key::Questionmark, Key::Num7, CMD | Modifiers::SHIFT);
    assert_eq!(text(&h), "1. alpha\n2. beta");
    let end = text(&h).chars().count();
    cursor_to(&mut h, end);
    // macOS Alt+. types "≥"; the physical key is still Period.
    physical(&mut h, Key::Comma, Key::Period, Modifiers::ALT);
    assert!(text(&h).ends_with('→'), "{}", text(&h));
    physical(&mut h, Key::Period, Key::Comma, Modifiers::ALT);
    assert!(text(&h).ends_with("→←"));
}

#[test]
fn toolbar_commands_run_on_the_next_frame() {
    let mut h = harness("one\ntwo");
    select(&mut h, 0, 7);
    h.state_mut().editor.run(Command::OrderedList);
    h.run();
    assert_eq!(text(&h), "1. one\n2. two");
    h.state_mut().editor.run(Command::Undo);
    h.run();
    assert_eq!(text(&h), "one\ntwo");
}

#[test]
fn brackets_auto_close() {
    let mut h = harness("");
    type_text(&mut h, "(");
    assert_eq!(text(&h), "()");
    type_text(&mut h, "x");
    type_text(&mut h, ")");
    assert_eq!(text(&h), "(x)");
    assert_eq!(cursor(&h), (3, 3));
}

#[test]
fn ime_commit_is_one_step_and_composition_owns_enter() {
    let mut h = harness("- a");
    cursor_to(&mut h, 3);
    h.event(Event::Ime(ImeEvent::Preedit {
        text: "ni".into(),
        active_range_chars: None,
    }));
    h.run();
    // Enter while composing belongs to the IME: no list continuation.
    h.event(Event::Ime(ImeEvent::Preedit {
        text: "nihao".into(),
        active_range_chars: None,
    }));
    h.key_press(Key::Enter);
    h.run();
    assert!(!text(&h).contains("\n- "), "{:?}", text(&h));
    assert_eq!(changed(&mut h), 0, "preedit text isn't saved");
    h.event(Event::Ime(ImeEvent::Commit("你好".into())));
    h.run();
    h.run();
    assert!(text(&h).ends_with("你好"), "{:?}", text(&h));
    assert_eq!(changed(&mut h), 1);
    // After the commit, Enter continues the list again.
    let ctx = h.ctx.clone();
    h.state_mut().editor.set_text(&ctx, "- a");
    cursor_to(&mut h, 3);
    h.event(Event::Ime(ImeEvent::Preedit {
        text: "x".into(),
        active_range_chars: None,
    }));
    h.run();
    h.event(Event::Ime(ImeEvent::Commit("ü".into())));
    h.run();
    h.run();
    assert_eq!(text(&h), "- aü");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(text(&h), "- aü\n- ");
    // The commit is one undo step.
    key(&mut h, Key::Z, CMD);
    key(&mut h, Key::Z, CMD);
    assert_eq!(text(&h), "- a");
}

#[test]
fn wikilink_completion() {
    let mut h = harness("");
    h.state_mut().editor.set_link_targets(vec![
        "Meeting notes".into(),
        "Reading list".into(),
        "Rust release".into(),
    ]);
    type_text(&mut h, "see ");
    type_text(&mut h, "[");
    type_text(&mut h, "[");
    // closeBrackets made it `[[]]`.
    assert_eq!(text(&h), "see [[]]");
    type_text(&mut h, "re");
    h.run();
    // "re" matches "Reading list" and "Rust release" (contains, any case).
    assert!(h.query_by_label("Reading list").is_some());
    assert!(h.query_by_label("Rust release").is_some());
    assert!(h.query_by_label("Meeting notes").is_none());
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(text(&h), "see [[Rust release]]");
    assert_eq!(cursor(&h).1, text(&h).chars().count());
    // Escape closes the list without inserting.
    type_text(&mut h, " [[me");
    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(text(&h).ends_with(" [[me\n"), "{:?}", text(&h));
}

#[test]
fn image_and_file_pastes_go_to_the_host() {
    let mut h = harness("x");
    h.event(Event::PasteImage(Arc::new(ColorImage::filled(
        [2, 2],
        egui::Color32::RED,
    ))));
    h.run();
    assert!(h
        .state()
        .events
        .iter()
        .any(|e| matches!(e, EditorEvent::PasteImage(i) if i.size == [2, 2])));
    h.event(Event::Paste("file:///tmp/a%20b.png\n".into()));
    h.run();
    assert!(h
        .state()
        .events
        .iter()
        .any(|e| matches!(e, EditorEvent::PasteFiles(p) if p.len() == 1)));
    assert_eq!(text(&h), "x");
    // The host saves the attachment and inserts its Markdown.
    h.state_mut()
        .editor
        .run(Command::Insert(marina_editor::attachment_markdown(
            "a b.png",
            "att-1.png",
        )));
    h.run();
    assert_eq!(text(&h), "![a b.png](./_attachments/att-1.png)\nx");
    // Plain text still pastes.
    h.event(Event::Paste("plain".into()));
    h.run();
    assert!(text(&h).contains("plain"));
}

#[test]
fn find_and_replace() {
    let mut h = harness("cat dog cat bird cat");
    key(&mut h, Key::F, CMD);
    assert!(h.state().editor.find_open());
    h.state_mut().editor.set_find_query("cat");
    h.run();
    h.state_mut().editor.focus();
    h.run();
    cursor_to(&mut h, 0);
    key(&mut h, Key::F3, Modifiers::NONE);
    assert_eq!(cursor(&h), (0, 3));
    key(&mut h, Key::F3, Modifiers::NONE);
    assert_eq!(cursor(&h), (8, 11));
    key(&mut h, Key::F3, Modifiers::SHIFT);
    assert_eq!(cursor(&h), (0, 3));
    h.get_by_label("Replace").click();
    h.run();
    h.get_by_label("Replace").type_text("cow");
    h.run();
    h.get_by_label("replace all").click();
    h.run();
    assert_eq!(text(&h), "cow dog cow bird cow");
    // One step.
    h.state_mut().editor.run(Command::Undo);
    h.run();
    assert_eq!(text(&h), "cat dog cat bird cat");
    h.state_mut().editor.focus();
    h.run();
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!h.state().editor.find_open());
}

#[test]
fn status_and_list_kind() {
    let mut h = harness("# T\n- a\n- b");
    select(&mut h, 4, 11);
    let ctx = h.ctx.clone();
    let s = h.state_mut().editor.status(&ctx);
    assert_eq!((s.line, s.col, s.selection_len), (3, 4, 7));
    assert_eq!(s.selection_lines, Some((2, 3)));
    assert_eq!(
        h.state_mut().editor.list_kind(&ctx),
        marina_editor::ListKind::Unordered
    );
}

#[test]
fn scroll_to_line_moves_the_caret() {
    let doc: String = (1..=300).map(|i| format!("line {i}\n")).collect();
    let mut h = harness(&doc);
    let ctx = h.ctx.clone();
    h.state_mut().editor.scroll_to_line(&ctx, 200);
    h.run();
    h.run();
    let s = h.state_mut().editor.status(&ctx);
    assert_eq!((s.line, s.col), (200, 1));
    assert!(h.state().editor.scroll_offset().y > 1000.0);
}
