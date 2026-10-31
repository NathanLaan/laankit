//! The presenter's speaker view, headless: egui_kittest embeds the speaker viewport as a window
//! in the same context, so its contents, buttons and keys can be driven without opening a real
//! window or putting a display into full screen. Viewport commands are checked through the
//! presenter's command log.

use std::collections::HashMap;

use egui::{Key, ViewportCommand, ViewportId};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use noteliner_slides::{parse_deck, Deck, PassThrough, Presenter, PresenterAction};

const MD: &str = "\
## Alpha

First body.

<!-- notes: Say **hello** first. -->

## Beta

Second body, with $x^2$.

## Gamma

Third.
";

struct State {
    deck: Deck,
    presenter: Presenter,
    actions: Vec<PresenterAction>,
    exited: bool,
}

fn harness(start: usize) -> Harness<'static, State> {
    let state = State {
        deck: parse_deck(MD, None),
        presenter: Presenter::new(start).with_speaker_view(true),
        actions: Vec::new(),
        exited: false,
    };
    let vars = HashMap::new();
    Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_context_setup(marina_egui::fonts::install)
        .build_ui_state(
            move |ui, s: &mut State| {
                if s.exited {
                    return;
                }
                let actions = s.presenter.show(ui.ctx(), &s.deck, &vars, &PassThrough);
                s.exited |= actions.contains(&PresenterAction::Exit);
                s.actions.extend(actions);
            },
            state,
        )
}

#[test]
fn speaker_view_shows_current_next_notes_and_timer() {
    let mut h = harness(1);
    h.run();
    assert!(h.state().presenter.speaker_view_open());
    h.get_by_label("Slide 1 of 3"); // the audience view
    h.get_by_label("Current slide 1");
    h.get_by_label("Next slide 2");
    h.get_by_label("Slide 1 / 3");
    h.get_by_label("Pause (T)");
    // The notes, rendered (bold is its own span, so look for a part).
    assert!(
        h.query_by_label_contains("hello").is_some(),
        "the notes are shown"
    );
    let timer = h.state().presenter.timer().expect("a timer");
    assert!(timer.running(), "the timer starts with the presentation");
}

#[test]
fn both_windows_move_together() {
    let mut h = harness(1);
    h.run();
    // A key (read by whichever window has focus; here the embedded one).
    h.key_press(Key::ArrowRight);
    h.run();
    assert_eq!(h.state().presenter.index(), 2);
    h.get_by_label("Slide 2 of 3");
    h.get_by_label("Current slide 2");
    h.get_by_label("Next slide 3");
    // The speaker view's own buttons.
    h.get_by_label("Next ▶").click();
    h.run();
    assert_eq!(h.state().presenter.index(), 3);
    h.get_by_label("Current slide 3");
    h.get_by_label("End of presentation");
    h.get_by_label("◀ Previous").click();
    h.run();
    assert_eq!(h.state().presenter.index(), 2);
    assert!(h
        .state()
        .actions
        .iter()
        .filter(|a| matches!(a, PresenterAction::Changed(_)))
        .eq([
            &PresenterAction::Changed(2),
            &PresenterAction::Changed(3),
            &PresenterAction::Changed(2)
        ]));
}

#[test]
fn timer_pauses_from_its_button_and_key() {
    let mut h = harness(1);
    h.run();
    h.get_by_label("Pause (T)").click();
    h.run();
    assert!(!h.state().presenter.timer().unwrap().running());
    h.get_by_label("Resume (T)");
    h.key_press(Key::T);
    h.run();
    assert!(h.state().presenter.timer().unwrap().running());
}

#[test]
fn ending_restores_the_window_and_closes_the_speaker_view() {
    let mut h = harness(2);
    h.run();
    h.get_by_label("End (Esc)").click();
    h.run();
    let state = h.state();
    assert_eq!(state.actions.last(), Some(&PresenterAction::Exit));
    assert!(!state.presenter.speaker_view_open());
    let sent = state.presenter.commands_sent();
    assert_eq!(
        sent.first(),
        Some(&(ViewportId::ROOT, ViewportCommand::Fullscreen(true))),
        "the audience window goes full screen"
    );
    assert_eq!(
        sent.last(),
        Some(&(ViewportId::ROOT, ViewportCommand::Fullscreen(false))),
        "and comes back"
    );
}

#[test]
fn escape_in_either_window_ends() {
    let mut h = harness(1);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().exited);
}

#[test]
fn without_a_speaker_view_nothing_extra_opens() {
    let state = State {
        deck: parse_deck(MD, None),
        presenter: Presenter::new(1),
        actions: Vec::new(),
        exited: false,
    };
    let vars = HashMap::new();
    let mut h = Harness::builder()
        .with_size(egui::vec2(800.0, 450.0))
        .with_context_setup(marina_egui::fonts::install)
        .build_ui_state(
            move |ui, s: &mut State| {
                let a = s.presenter.show(ui.ctx(), &s.deck, &vars, &PassThrough);
                s.actions.extend(a);
            },
            state,
        );
    h.run();
    assert!(!h.state().presenter.speaker_view_open());
    assert!(h.query_by_label("Current slide 1").is_none());
    assert!(h.state().presenter.timer().is_none());
}
