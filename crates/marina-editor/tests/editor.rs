//! Drives the editor headlessly (egui_kittest): list keys, shortcuts, undo,
//! IME, `[[` completion, paste and drop hooks, find and replace, and that the
//! text paints. Every test runs on both engines (the core engine and the
//! `TextEdit` fallback): see `both_engines!` at the end.

use std::sync::Arc;

use egui::{vec2, ColorImage, Event, ImeEvent, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use std::cell::Cell;

use marina_editor::{Command, Editor, EditorEvent, EditorState, Engine};

thread_local! {
    /// The engine the current test runs on.
    static ENGINE: Cell<Engine> = const { Cell::new(Engine::Core) };
}

/// An editor on the current test's engine.
fn new_state(name: &str) -> EditorState {
    let mut s = EditorState::new(name);
    let mut prefs = s.prefs();
    prefs.engine = ENGINE.with(Cell::get);
    s.set_prefs(prefs);
    s
}

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
                editor: new_state("test"),
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
    let mut state = new_state("paint");
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

fn set_text_is_not_a_user_edit() {
    let mut h = harness("x");
    assert_eq!(changed(&mut h), 0);
    let ctx = h.ctx.clone();
    h.state_mut().editor.set_text(&ctx, "y");
    h.run();
    assert_eq!(changed(&mut h), 0);
    assert!(!h.state().editor.can_undo());
}

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
    // Enter twice ends the list: a blank line, and the caret on a new line.
    assert_eq!(text(&h), "- a\n- b\n\n");
    assert_eq!(cursor(&h), (9, 9));
    key(&mut h, Key::Z, CMD);
    assert_eq!(text(&h), "- a\n- b\n- ");
    key(&mut h, Key::Z, CMD | Modifiers::SHIFT);
    assert_eq!(text(&h), "- a\n- b\n\n");
}

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

fn tab_indents_and_shift_tab_outdents() {
    let mut h = harness("- a\n- b");
    cursor_to(&mut h, 7);
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(text(&h), "- a\n  - b");
    key(&mut h, Key::Tab, Modifiers::SHIFT);
    assert_eq!(text(&h), "- a\n- b");
}

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

fn brackets_auto_close() {
    let mut h = harness("");
    type_text(&mut h, "(");
    assert_eq!(text(&h), "()");
    type_text(&mut h, "x");
    type_text(&mut h, ")");
    assert_eq!(text(&h), "(x)");
    assert_eq!(cursor(&h), (3, 3));
}

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

fn go_to_line_bar() {
    let doc: String = (1..=300).map(|i| format!("line {i}\n")).collect();
    let mut h = harness(&doc);
    physical(&mut h, Key::G, Key::G, CMD | Modifiers::ALT);
    assert!(h.state().editor.goto_open());
    h.run();
    type_text(&mut h, "120:3");
    key(&mut h, Key::Enter, Modifiers::NONE);
    h.run();
    let ctx = h.ctx.clone();
    let s = h.state_mut().editor.status(&ctx);
    assert_eq!((s.line, s.col), (120, 3));
    assert!(!h.state().editor.goto_open(), "closes after jumping");
    assert!(
        h.state().editor.scroll_offset().y > 1000.0,
        "scrolled to it"
    );

    // Relative, and a bad input keeps the bar open.
    h.state_mut().editor.run(Command::GoToLine);
    h.run();
    h.run();
    type_text(&mut h, "+10");
    key(&mut h, Key::Enter, Modifiers::NONE);
    h.run();
    assert_eq!(h.state_mut().editor.status(&ctx).line, 130);
    h.state_mut().editor.open_goto();
    h.run();
    type_text(&mut h, "nope");
    key(&mut h, Key::Enter, Modifiers::NONE);
    h.run();
    assert!(h.state().editor.goto_open());
    key(&mut h, Key::Escape, Modifiers::NONE);
    h.run();
    assert!(!h.state().editor.goto_open());
}

fn ordered_lists_renumber_after_enter_delete_and_paste() {
    // Enter in a list (our list edit): one undo step with the renumbering.
    let mut h = harness("1. a\n2. b\n3. c");
    cursor_to(&mut h, 4);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(text(&h), "1. a\n2. \n3. b\n4. c");
    key(&mut h, Key::Z, CMD);
    assert_eq!(text(&h), "1. a\n2. b\n3. c");

    // Deleting a whole item (TextEdit's own Backspace on a selection).
    let mut h = harness("1. a\n2. b\n3. c\n4. d");
    select(&mut h, 5, 10);
    key(&mut h, Key::Backspace, Modifiers::NONE);
    assert_eq!(text(&h), "1. a\n2. c\n3. d");
    key(&mut h, Key::Z, CMD);
    assert_eq!(text(&h), "1. a\n2. b\n3. c\n4. d");

    // Pasting two items.
    let mut h = harness("1. a\n2. b\n3. c");
    cursor_to(&mut h, 5);
    h.event(Event::Paste("2. p\n3. q\n".into()));
    h.run();
    assert_eq!(text(&h), "1. a\n2. p\n3. q\n4. b\n5. c");

    // Tab nests an item; the outer list closes the gap.
    let mut h = harness("1. a\n2. b\n3. c");
    cursor_to(&mut h, 7);
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(text(&h), "1. a\n  2. b\n2. c");

    // Plain typing never renumbers.
    let mut h = harness("1. a\n1. b\n1. c");
    cursor_to(&mut h, 4);
    type_text(&mut h, "z");
    assert_eq!(text(&h), "1. az\n1. b\n1. c");
}

/// Every filled rect in the frame's shapes.
fn rect_fills(shapes: &[egui::epaint::ClippedShape]) -> Vec<egui::Color32> {
    fn walk(shape: &egui::Shape, out: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            egui::Shape::Rect(r) => out.push(r.fill),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for c in shapes {
        walk(&c.shape, &mut out);
    }
    out
}

/// The index of the first clipped shape holding a filled rect of `fill`,
/// and of the first holding the note's text.
fn paint_order(shapes: &[egui::epaint::ClippedShape], fill: egui::Color32) -> (usize, usize) {
    fn has_fill(shape: &egui::Shape, fill: egui::Color32) -> bool {
        match shape {
            egui::Shape::Vec(v) => v.iter().any(|s| has_fill(s, fill)),
            egui::Shape::Rect(r) => r.fill == fill,
            _ => false,
        }
    }
    let rect = shapes
        .iter()
        .position(|c| has_fill(&c.shape, fill))
        .unwrap();
    let text = shapes
        .iter()
        .position(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text().contains("call(")))
        .unwrap();
    (rect, text)
}

fn bracket_and_selection_matches_paint() {
    let ctx = egui::Context::default();
    marina_egui::fonts::install(&ctx);
    ctx.run_ui(Default::default(), |_| {})
        .textures_delta
        .clear();
    let colors = marina_editor::EditorColors::from_palette(&marina_egui::theme::palette(&ctx));
    let mut state = new_state("decor");
    state.set_text(&ctx, "call(foo) then foo and foo; foo_bar\n(unclosed");
    // A selection needs the focus (TextEdit collapses it otherwise).
    state.focus();
    let frame = |state: &mut EditorState| {
        let mut out = ctx.run_ui(Default::default(), |ui| {
            Editor::new(state).show(ui);
        });
        out.textures_delta.clear();
        rect_fills(&out.shapes)
    };
    frame(&mut state);

    // Caret before `(`: the pair is marked, over the text (code spans paint
    // their own background, which would hide a mark behind them).
    state.set_cursor(&ctx, 4, 4);
    let fills = frame(&mut state);
    let mut out = ctx.run_ui(Default::default(), |ui| {
        Editor::new(&mut state).show(ui);
    });
    out.textures_delta.clear();
    let (rect, text) = paint_order(&out.shapes, colors.bracket_match);
    assert!(rect > text, "the bracket mark is painted before the text");
    let count = |fills: &[egui::Color32], c| fills.iter().filter(|f| **f == c).count();
    assert_eq!(count(&fills, colors.bracket_match), 2, "{fills:?}");

    // An unmatched bracket is marked alone, in red.
    state.set_cursor(&ctx, 36, 36);
    let fills = frame(&mut state);
    assert_eq!(count(&fills, colors.bracket_unmatched), 1, "{fills:?}");

    // Selecting `foo` (a whole word) marks the other whole-word `foo` only:
    // not the selection itself, and not `foo_bar`.
    state.set_cursor(&ctx, 5, 8);
    frame(&mut state);
    let fills = frame(&mut state);
    assert_eq!(count(&fills, colors.selection_match), 2, "{fills:?}");
    assert_eq!(count(&fills, colors.bracket_match), 0);
}

/// Runs each listed test once per engine.
macro_rules! both_engines {
    ($($name:ident),* $(,)?) => {
        mod core_engine {
            $(
                #[test]
                fn $name() {
                    super::ENGINE.with(|e| e.set(marina_editor::Engine::Core));
                    super::$name();
                }
            )*
        }
        mod textedit_engine {
            $(
                #[test]
                fn $name() {
                    super::ENGINE.with(|e| e.set(marina_editor::Engine::TextEdit));
                    super::$name();
                }
            )*
        }
    };
}

both_engines!(
    text_paints,
    set_text_is_not_a_user_edit,
    enter_continues_lists_as_one_undo_step,
    plain_enter_and_backspace_type_normally,
    tab_indents_and_shift_tab_outdents,
    typing_groups_into_one_undo_step,
    emphasis_shortcuts_toggle,
    list_and_arrow_shortcuts_use_physical_keys,
    toolbar_commands_run_on_the_next_frame,
    brackets_auto_close,
    ime_commit_is_one_step_and_composition_owns_enter,
    wikilink_completion,
    image_and_file_pastes_go_to_the_host,
    find_and_replace,
    status_and_list_kind,
    scroll_to_line_moves_the_caret,
    go_to_line_bar,
    ordered_lists_renumber_after_enter_delete_and_paste,
    bracket_and_selection_matches_paint,
);
