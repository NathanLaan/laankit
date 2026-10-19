//! The core engine's own features, driven headlessly (egui_kittest):
//! several cursors (Ctrl/Cmd+click, Ctrl/Cmd+D, find "all", Alt+drag
//! rectangles), multi-cursor typing, paste and copy, movement by word and
//! line, vertical moves that keep their column and skip folds, folding from
//! the keyboard and the gutter, mouse selection, and that only the visible
//! lines reach screen readers.

use egui::{vec2, Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use marina_editor::{Editor, EditorEvent, EditorState, Engine};

struct State {
    editor: EditorState,
    events: Vec<EditorEvent>,
}

const CMD: Modifiers = Modifiers::COMMAND;

fn harness(text: &str) -> Harness<'static, State> {
    let mut editor = EditorState::new("core");
    let mut prefs = editor.prefs();
    prefs.engine = Engine::Core;
    editor.set_prefs(prefs);
    let mut h = Harness::builder()
        .with_size(vec2(900.0, 600.0))
        .with_step_dt(1.0 / 60.0)
        .allow_missing_glyphs()
        .with_context_setup(marina_egui::fonts::install)
        .build_ui_state(
            |ui, s: &mut State| {
                let out = Editor::new(&mut s.editor).show(ui);
                s.events.extend(out.events);
            },
            State {
                editor,
                events: Vec::new(),
            },
        );
    let ctx = h.ctx.clone();
    h.state_mut().editor.set_text(&ctx, text);
    h.state_mut().editor.focus();
    h.run();
    h
}

fn text(h: &Harness<'static, State>) -> String {
    h.state().editor.text().to_owned()
}

fn sels(h: &Harness<'static, State>) -> Vec<(usize, usize)> {
    let mut v = h.state().editor.selections(&h.ctx);
    v.sort();
    v
}

fn cursor_to(h: &mut Harness<'static, State>, a: usize, b: usize) {
    let ctx = h.ctx.clone();
    h.state_mut().editor.set_cursor(&ctx, a, b);
    h.run();
}

fn key(h: &mut Harness<'static, State>, key: Key, modifiers: Modifiers) {
    h.key_press_modifiers(modifiers, key);
    h.run();
}

fn type_text(h: &mut Harness<'static, State>, s: &str) {
    h.event(Event::Text(s.into()));
    h.run();
}

/// The middle of character `c` on screen.
fn at(h: &mut Harness<'static, State>, c: usize) -> Pos2 {
    let r = h
        .state_mut()
        .editor
        .screen_rect_of(c)
        .expect("character on screen");
    r.left_center() + vec2(1.0, 0.0)
}

fn press(h: &mut Harness<'static, State>, pos: Pos2, modifiers: Modifiers, pressed: bool) {
    h.event(Event::ModifiersChanged(modifiers));
    h.event(Event::PointerMoved(pos));
    h.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers,
    });
    h.run();
}

fn click(h: &mut Harness<'static, State>, pos: Pos2, modifiers: Modifiers) {
    press(h, pos, modifiers, true);
    press(h, pos, modifiers, false);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

#[test]
fn ctrl_d_selects_occurrences_and_typing_edits_them_all() {
    let mut h = harness("foo x foo y foo");
    cursor_to(&mut h, 1, 1);
    key(&mut h, Key::D, CMD);
    assert_eq!(sels(&h), vec![(0, 3)], "the word at the caret first");
    key(&mut h, Key::D, CMD);
    key(&mut h, Key::D, CMD);
    assert_eq!(sels(&h), vec![(0, 3), (6, 9), (12, 15)]);
    type_text(&mut h, "bar");
    assert_eq!(text(&h), "bar x bar y bar");
    assert_eq!(sels(&h), vec![(3, 3), (9, 9), (15, 15)]);
    // One undo step for every cursor.
    key(&mut h, Key::Z, CMD);
    assert_eq!(text(&h), "foo x foo y foo");
    key(&mut h, Key::Y, CMD);
    // Escape collapses to the primary (the last added).
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(sels(&h), vec![(15, 15)]);
}

#[test]
fn ctrl_click_adds_a_caret_but_opens_links() {
    let mut h = harness("one\ntwo\nsee [[Page]]");
    cursor_to(&mut h, 0, 0);
    let p = at(&mut h, 4);
    click(&mut h, p, CMD);
    assert_eq!(sels(&h), vec![(0, 0), (4, 4)]);
    type_text(&mut h, "X");
    assert_eq!(text(&h), "Xone\nXtwo\nsee [[Page]]");
    // On a wikilink, Ctrl/Cmd+click opens it and adds nothing.
    key(&mut h, Key::Escape, Modifiers::NONE);
    let n = sels(&h).len();
    let p = at(&mut h, 18);
    click(&mut h, p, CMD);
    assert_eq!(sels(&h).len(), n);
    assert!(h
        .state()
        .events
        .iter()
        .any(|e| matches!(e, EditorEvent::OpenWikilink(p) if p == "Page")));
}

#[test]
fn find_all_selects_every_match() {
    let mut h = harness("cat dog cat bird cat");
    key(&mut h, Key::F, CMD);
    h.state_mut().editor.set_find_query("cat");
    h.run();
    h.get_by_label("all").click();
    h.run();
    assert_eq!(sels(&h), vec![(0, 3), (8, 11), (17, 20)]);
    h.state_mut().editor.focus();
    h.run();
    type_text(&mut h, "cow");
    assert_eq!(text(&h), "cow dog cow bird cow");
}

#[test]
fn alt_drag_makes_a_rectangle() {
    let mut h = harness("abcd\nefgh\nijkl");
    let from = at(&mut h, 1);
    let to = at(&mut h, 13);
    press(&mut h, from, Modifiers::ALT, true);
    for k in 1..=4 {
        let p = from + (to - from) * (k as f32 / 4.0);
        h.event(Event::ModifiersChanged(Modifiers::ALT));
        h.event(Event::PointerMoved(p));
        h.run();
    }
    press(&mut h, to, Modifiers::ALT, false);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    assert_eq!(sels(&h), vec![(1, 3), (6, 8), (11, 13)]);
    type_text(&mut h, "-");
    assert_eq!(text(&h), "a-d\ne-h\ni-l");
}

#[test]
fn paste_spreads_lines_over_cursors_and_copy_joins_them() {
    let mut h = harness("a\nb\nc");
    let (p1, p2) = (at(&mut h, 2), at(&mut h, 4));
    cursor_to(&mut h, 0, 0);
    click(&mut h, p1, CMD);
    click(&mut h, p2, CMD);
    assert_eq!(sels(&h).len(), 3);
    h.event(Event::Paste("1\n2\n3".into()));
    h.run();
    assert_eq!(text(&h), "1a\n2b\n3c");
    // Copy: one line per non-empty selection.
    let ctx = h.ctx.clone();
    h.state_mut().editor.set_cursor(&ctx, 0, 2);
    h.run();
    h.event(Event::Copy);
    h.step();
    let copied = h
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "1a"));
    assert!(copied, "{:?}", h.output().platform_output.commands);
}

#[test]
fn words_lines_and_smart_home() {
    let mut h = harness("foo.bar baz\n    - item");
    cursor_to(&mut h, 0, 0);
    let word = if cfg!(target_os = "macos") {
        Modifiers::ALT
    } else {
        Modifiers::CTRL
    };
    key(&mut h, Key::ArrowRight, word);
    assert_eq!(sels(&h), vec![(3, 3)]);
    key(&mut h, Key::ArrowRight, word);
    assert_eq!(sels(&h), vec![(4, 4)]);
    key(&mut h, Key::ArrowRight, word | Modifiers::SHIFT);
    assert_eq!(sels(&h), vec![(4, 7)]);
    key(&mut h, Key::End, Modifiers::NONE);
    assert_eq!(sels(&h), vec![(11, 11)]);
    // Smart Home: the indentation, then the line start.
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::End, Modifiers::NONE);
    key(&mut h, Key::Home, Modifiers::NONE);
    assert_eq!(sels(&h), vec![(16, 16)]);
    key(&mut h, Key::Home, Modifiers::NONE);
    assert_eq!(sels(&h), vec![(12, 12)]);
    // Word deletion.
    cursor_to(&mut h, 11, 11);
    key(&mut h, Key::Backspace, word);
    assert_eq!(text(&h), "foo.bar \n    - item");
}

#[test]
fn vertical_moves_keep_their_column_and_skip_folds() {
    let mut h = harness("abcdef\nab\nabcdef\n# A\nhidden\nalso\n# B\nend");
    cursor_to(&mut h, 5, 5);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(sels(&h), vec![(9, 9)], "end of the short line");
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(sels(&h), vec![(15, 15)], "back to column 5");
    // Fold `# A` (lines 4–5 hidden) and step over it.
    cursor_to(&mut h, 17, 17);
    h.key_press_modifiers(CMD | Modifiers::SHIFT, Key::OpenBracket);
    h.run();
    assert_eq!(h.state().editor.folds(), &[(3, 5)]);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    let ctx = h.ctx.clone();
    assert_eq!(h.state_mut().editor.status(&ctx).line, 7, "onto `# B`");
    // Moving the caret into the fold (find, go to line) opens it.
    h.state_mut().editor.scroll_to_line(&ctx, 5);
    h.run();
    assert!(h.state().editor.folds().is_empty());
}

#[test]
fn fold_gutter_and_edits_around_folds() {
    let mut h = harness("# One\ntext\nmore\n# Two\nlast");
    // Chevrons show while the pointer is over the gutter.
    h.event(Event::PointerMoved(Pos2::new(8.0, 30.0)));
    h.run();
    h.get_by_label("Fold line 1").click();
    h.run();
    assert_eq!(h.state().editor.folds(), &[(0, 2)]);
    // Typing above the fold moves it down (the chevron kept the focus).
    cursor_to(&mut h, 0, 0);
    h.event(Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert_eq!(h.state().editor.folds(), &[(1, 3)]);
    h.get_by_label("Unfold line 2").click();
    h.run();
    assert!(h.state().editor.folds().is_empty());
}

#[test]
fn double_and_triple_click_select_word_and_line() {
    let mut h = harness("hello wide world\nnext");
    let p = at(&mut h, 8);
    click(&mut h, p, Modifiers::NONE);
    click(&mut h, p, Modifiers::NONE);
    assert_eq!(sels(&h), vec![(6, 10)]);
    click(&mut h, p, Modifiers::NONE);
    assert_eq!(sels(&h), vec![(0, 17)], "the line and its newline");
    // A drag selects from the press to the pointer.
    let (a, b) = (at(&mut h, 1), at(&mut h, 18));
    h.run_steps(30);
    press(&mut h, a, Modifiers::NONE, true);
    for k in 1..=4 {
        h.event(Event::PointerMoved(a + (b - a) * (k as f32 / 4.0)));
        h.run();
    }
    press(&mut h, b, Modifiers::NONE, false);
    assert_eq!(sels(&h), vec![(1, 18)]);
}

#[test]
fn screen_readers_get_the_visible_lines_only() {
    let doc: String = (1..=3000).map(|i| format!("line {i}\n")).collect();
    let mut h = harness(&doc);
    h.ctx.enable_accesskit();
    h.run();
    let node = h.get_by_label("Note text");
    let value = node.value().unwrap_or_default();
    assert!(value.starts_with("line 1\n"), "{value:.40}");
    assert!(!value.contains("line 2999"), "only what's on screen");
    assert!(value.len() < 2_000, "{} bytes", value.len());
    // Scrolled down, the window follows.
    let ctx = h.ctx.clone();
    h.state_mut().editor.scroll_to_line(&ctx, 2500);
    h.run();
    h.run();
    let value = h.get_by_label("Note text").value().unwrap_or_default();
    assert!(value.contains("line 2500"), "{value:.60}");
}

#[test]
fn large_notes_keep_galleys_bounded() {
    let doc: String = (1..=20_000)
        .map(|i| format!("line {i} with some words\n"))
        .collect();
    let mut h = harness(&doc);
    let ctx = h.ctx.clone();
    for line in [5_000, 10_000, 15_000, 19_990] {
        h.state_mut().editor.scroll_to_line(&ctx, line);
        h.run();
        h.run();
        assert_eq!(h.state_mut().editor.status(&ctx).line, line);
    }
    let held = h.state().editor.held_galleys();
    assert!(held < 4_000, "{held} line galleys held");
    type_text(&mut h, "x");
    assert!(text(&h).contains("xline 19990 with"));
}
