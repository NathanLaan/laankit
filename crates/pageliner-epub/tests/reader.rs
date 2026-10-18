//! The reading screen through egui_kittest: select → highlight → reopen, bookmarks, contents.

mod common;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use marina_egui::theme::ThemeState;
use pageliner_epub::{Book, EpubEvent, EpubView, NativeState, ReaderPrefs};

struct State {
    view: EpubView,
    events: Vec<EpubEvent>,
}

fn harness(state: NativeState) -> Harness<'static, State> {
    harness_with(state, 700.0, ReaderPrefs::default())
}

/// A window `height` tall; short windows and big text make the little book scroll.
fn harness_with(state: NativeState, height: f32, prefs: ReaderPrefs) -> Harness<'static, State> {
    let book = Book::from_bytes(common::synthetic_epub(true)).unwrap();
    let view = EpubView::new(book, state, prefs);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1100.0, height))
        // Scrolling to a restored position settles over several frames.
        .with_max_steps(40)
        .with_context_setup(|ctx| {
            marina_egui::fonts::install(ctx);
            ThemeState::default().apply(ctx);
        })
        .build_ui_state(
            |ui, s: &mut State| {
                let events = s.view.show(ui);
                s.events.extend(events);
            },
            State {
                view,
                events: Vec::new(),
            },
        );
    h.run();
    h
}

fn saved_states(h: &Harness<'_, State>) -> Vec<NativeState> {
    h.state()
        .events
        .iter()
        .filter_map(|e| match e {
            EpubEvent::SaveState(v) => Some(NativeState::from_book_state(v.as_object().unwrap())),
            _ => None,
        })
        .collect()
}

fn drag(h: &mut Harness<'_, State>, from: egui::Pos2, to: egui::Pos2) {
    h.drag_at(from);
    h.run();
    h.hover_at(to);
    h.run();
    h.drop_at(to);
    h.run();
}

#[test]
fn select_highlight_and_reopen() {
    let mut h = harness(NativeState::default());
    let from = h.get_by_label_contains("Plain italic").rect().left_center() + egui::vec2(2.0, 0.0);
    let to = h.get_by_label_contains("See the end").rect().left_center() + egui::vec2(20.0, 0.0);
    drag(&mut h, from, to);
    assert!(h
        .state()
        .view
        .rich_view()
        .selection(&h.state().view.book().document)
        .is_some());

    h.get_by_label("Highlight selection").click();
    h.run();
    let state = h.state().view.state().clone();
    assert_eq!(state.highlights.len(), 1);
    let hl = &state.highlights[0];
    assert!(hl.text.starts_with("Plain italic text"), "{:?}", hl.text);
    assert_eq!((hl.start.spine, hl.start.block), (0, 1));
    assert_eq!((hl.end.spine, hl.end.block), (0, 2));
    assert!(saved_states(&h)
        .iter()
        .any(|s| s.highlights == state.highlights));
    // Listed in the Annotations pane, and the selection is gone.
    assert!(h.query_all_by_label_contains("Plain italic text").count() >= 2);
    assert!(h
        .state()
        .view
        .rich_view()
        .selection(&h.state().view.book().document)
        .is_none());

    // Reopened from the saved state: the mark is painted again.
    let h2 = harness(state.clone());
    assert_eq!(h2.state().view.rich_view().marks().len(), 1);
    assert_eq!(h2.state().view.state().highlights, state.highlights);

    // Deleting it from the pane.
    h.get_by_label_contains("Remove highlight").click();
    h.run();
    assert!(h.state().view.state().highlights.is_empty());
    assert!(h.state().view.rich_view().marks().is_empty());
}

#[test]
fn bookmark_toggles_at_the_current_location() {
    let mut h = harness(NativeState::default());
    h.get_by_label("Bookmark this location").click();
    h.run();
    let marks = &h.state().view.state().bookmarks;
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].label, "One");
    assert_eq!(marks[0].location.spine, 0);
    h.get_by_label("Remove bookmark").click();
    h.run();
    assert!(h.state().view.state().bookmarks.is_empty());
    assert_eq!(saved_states(&h).last().unwrap().bookmarks, []);
}

#[test]
fn contents_jump_and_position_is_saved() {
    // Big text, so "One A" (block 2) can scroll to the top.
    let prefs = ReaderPrefs {
        font_size: 220,
        ..Default::default()
    };
    let mut h = harness_with(NativeState::default(), 500.0, prefs);
    h.get_by_label("One A").click();
    h.run();
    let loc = h.state().view.location();
    assert_eq!((loc.spine, loc.block), (0, 2), "{loc:?}");
    let status: Vec<_> = h
        .state()
        .events
        .iter()
        .filter_map(|e| match e {
            EpubEvent::Status(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(status.last(), Some(&"One A"));
    // The position is debounced; closing flushes it.
    h.get_by_label("Library").click();
    h.run();
    assert!(h.state().events.contains(&EpubEvent::Close));
    let saved = saved_states(&h);
    assert_eq!(saved.last().unwrap().position, Some(loc));

    // Reopening there.
    let h2 = harness_with(saved.last().unwrap().clone(), 500.0, prefs);
    assert_eq!(h2.state().view.location(), loc);
}
