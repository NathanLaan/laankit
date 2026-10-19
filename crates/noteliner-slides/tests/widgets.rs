//! The widgets, driven headlessly (egui_kittest): the SLIDES pane (jump,
//! context-menu edit, drag to reorder), the deck preview, the settings
//! dialog and the presenter's keys.

use std::collections::HashMap;

use egui::Key;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use noteliner_slides::{
    parse_deck, Deck, DeckPreview, PaneAction, PassThrough, Presentation, PresentationSettings,
    Presenter, PresenterAction, SettingsOutcome, SlidesPane,
};

const MD: &str = "## Alpha\n\nFirst body.\n\n## Beta\n\nSecond body.\n\n## Gamma\n\nThird.\n";

fn titles(md: &str) -> Vec<String> {
    parse_deck(md, None)
        .slides
        .into_iter()
        .map(|s| s.title.unwrap_or_default())
        .collect()
}

struct PaneState {
    deck: Deck,
    pane: SlidesPane,
    actions: Vec<PaneAction>,
}

fn pane_harness() -> Harness<'static, PaneState> {
    let state = PaneState {
        deck: parse_deck(MD, None),
        pane: SlidesPane::default(),
        actions: Vec::new(),
    };
    Harness::builder()
        .with_size(egui::vec2(360.0, 400.0))
        .with_context_setup(marina_egui::fonts::install)
        .build_ui_state(
            |ui, s: &mut PaneState| {
                let actions = s.pane.show(ui, MD, Some(&s.deck), Some(1), &PassThrough);
                s.actions.extend(actions);
            },
            state,
        )
}

fn edited(actions: &[PaneAction]) -> Option<String> {
    actions.iter().find_map(|a| match a {
        PaneAction::Edit(r) => Some(r.markdown.clone()),
        _ => None,
    })
}

#[test]
fn pane_click_jumps_to_the_slide() {
    let mut h = pane_harness();
    h.run();
    h.get_by_label("Slide 2 — Beta").click();
    h.run();
    assert!(h.state().actions.contains(&PaneAction::Jump { line: 5 }));
}

#[test]
fn pane_context_menu_moves_a_slide_up() {
    let mut h = pane_harness();
    h.run();
    h.get_by_label("Slide 3 — Gamma").click_secondary();
    h.run();
    h.get_by_label_contains("Move Up").click();
    h.run();
    let md = edited(&h.state().actions).expect("an edit");
    assert_eq!(titles(&md), ["Alpha", "Gamma", "Beta"]);
}

#[test]
fn pane_drag_reorders() {
    let mut h = pane_harness();
    h.run();
    let from = h.get_by_label("Slide 3 — Gamma").rect().center();
    let to = h.get_by_label("Slide 1 — Alpha").rect().center();
    h.drag_at(from);
    h.step();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(from + (to - from) * t);
        h.step();
    }
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    let md = edited(&h.state().actions).expect("a reorder");
    assert_eq!(titles(&md), ["Gamma", "Alpha", "Beta"]);
}

#[test]
fn deck_preview_click_jumps() {
    let deck = parse_deck(MD, None);
    let mut preview = DeckPreview::default();
    let vars = HashMap::new();
    let mut jump = None;
    let mut h = Harness::builder()
        .with_size(egui::vec2(600.0, 900.0))
        .with_context_setup(marina_egui::fonts::install)
        .build_ui(|ui| {
            if let Some(line) = preview.show(ui, Some(&deck), Some(1), &vars, &PassThrough) {
                jump = Some(line);
            }
        });
    h.run();
    h.get_by_label("Slide 3").click();
    h.run();
    drop(h);
    assert_eq!(jump, Some(9));
}

struct SettingsState {
    dialog: PresentationSettings,
    outcome: Option<SettingsOutcome>,
}

fn settings_harness() -> Harness<'static, SettingsState> {
    let state = SettingsState {
        dialog: PresentationSettings::new(&Presentation::default()),
        outcome: None,
    };
    Harness::builder()
        .with_size(egui::vec2(800.0, 800.0))
        .with_context_setup(marina_egui::fonts::install)
        .build_ui_state(
            |ui, s: &mut SettingsState| {
                let outcome = s.dialog.show(ui.ctx(), 3);
                if outcome != SettingsOutcome::Open {
                    s.outcome = Some(outcome);
                }
            },
            state,
        )
}

#[test]
fn settings_save_returns_the_draft() {
    let mut h = settings_harness();
    h.run();
    h.get_by_label("Show slide numbers").click();
    h.run();
    h.get_by_label("Save").click();
    h.run();
    let Some(SettingsOutcome::Save(p)) = &h.state().outcome else {
        panic!("expected Save, got {:?}", h.state().outcome);
    };
    assert!(!p.slide_numbers);
    assert_eq!(p.theme, "dark");
}

#[test]
fn settings_cancel_discards() {
    let mut h = settings_harness();
    h.run();
    h.get_by_label("Show slide numbers").click();
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    assert_eq!(h.state().outcome, Some(SettingsOutcome::Cancel));
}

struct PresenterState {
    deck: Deck,
    presenter: Presenter,
    actions: Vec<PresenterAction>,
}

#[test]
fn presenter_keys_navigate_and_escape_exits() {
    let state = PresenterState {
        deck: parse_deck(MD, None),
        presenter: Presenter::new(2),
        actions: Vec::new(),
    };
    let vars = HashMap::new();
    let mut h = Harness::builder()
        .with_size(egui::vec2(800.0, 450.0))
        .with_context_setup(marina_egui::fonts::install)
        .build_ui_state(
            move |ui, s: &mut PresenterState| {
                let actions = s.presenter.show(ui.ctx(), &s.deck, &vars, &PassThrough);
                s.actions.extend(actions);
            },
            state,
        );
    h.run();
    h.get_by_label("Slide 2 of 3");
    let mut press = |key: Key| {
        h.key_press(key);
        h.run();
        h.state().presenter.index()
    };
    assert_eq!(press(Key::ArrowRight), 3);
    assert_eq!(press(Key::Space), 3, "stops at the last slide");
    assert_eq!(press(Key::Home), 1);
    assert_eq!(press(Key::ArrowLeft), 1, "stops at the first slide");
    assert_eq!(press(Key::End), 3);
    assert_eq!(press(Key::PageUp), 2);
    press(Key::S);
    assert!(h.state().presenter.notes_visible());
    h.key_press(Key::Escape);
    h.run();
    let actions = &h.state().actions;
    assert_eq!(actions.last(), Some(&PresenterAction::Exit));
    assert!(actions.contains(&PresenterAction::Changed(3)));
}

#[test]
fn presenter_click_advances() {
    let state = PresenterState {
        deck: parse_deck(MD, None),
        presenter: Presenter::new(1),
        actions: Vec::new(),
    };
    let vars = HashMap::new();
    let mut h = Harness::builder()
        .with_size(egui::vec2(800.0, 450.0))
        .with_context_setup(marina_egui::fonts::install)
        .build_ui_state(
            move |ui, s: &mut PresenterState| {
                let actions = s.presenter.show(ui.ctx(), &s.deck, &vars, &PassThrough);
                s.actions.extend(actions);
            },
            state,
        );
    h.run();
    h.get_by_label("Slide 1 of 3").click();
    h.run();
    assert_eq!(h.state().presenter.index(), 2);
}

#[test]
fn pane_double_click_starts_renaming() {
    let mut h = pane_harness();
    h.run();
    let row = h.get_by_label("Slide 2 — Beta").rect().center();
    h.hover_at(row);
    h.run();
    for (i, pressed) in [true, false, true, false].into_iter().enumerate() {
        h.input_mut().time = Some(1.0 + i as f64 * 0.05);
        h.event(egui::Event::PointerButton {
            pos: row,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.run();
    h.get_by_label("Slide title");
}

#[test]
fn pane_menu_rename() {
    let mut h = pane_harness();
    h.run();
    h.get_by_label("Slide 2 — Beta").click_secondary();
    h.run();
    h.get_by_label_contains("Rename").click();
    h.run();
    h.get_by_role(egui::accesskit::Role::TextInput)
        .type_text("X");
    h.run();
    h.key_press(Key::Enter);
    h.run();
    let md = edited(&h.state().actions).expect("a retitle");
    assert_eq!(titles(&md), ["Alpha", "BetaX", "Gamma"]);
}

#[test]
fn every_sample_slide_renders_at_every_size() {
    let md = include_str!("../examples/sample-deck.md");
    let deck = parse_deck(md, None);
    assert!(deck.slides.len() >= 10);
    for width in [40.0, 104.0, 640.0, 1920.0] {
        let mut h = Harness::builder()
            .with_size(egui::vec2(width + 20.0, width))
            .with_context_setup(marina_egui::fonts::install)
            .build_ui(|ui| {
                for slide in &deck.slides {
                    noteliner_slides::SlideView::new(slide, &deck.config)
                        .total(deck.slides.len())
                        .show(ui, width);
                }
            });
        h.run();
    }
}

#[test]
fn tasks_tables_and_marks_paint_in_both_slide_themes() {
    let md = "# Tasks\n\n- [ ] todo\n- [x] done\n\n---\n\n# Table\n\n\
              | L | C | R |\n|:--|:-:|--:|\n| a | b | c |\n\n---\n\n\
              # Marks\n\nsome <mark>hi</mark> and <kbd>Ctrl</kbd>\n";
    for theme in ["dark", "light"] {
        let config = serde_json::json!({ "theme": theme });
        let deck = parse_deck(md, Some(&config));
        assert_eq!(deck.slides.len(), 3);
        let mut h = Harness::builder()
            .with_size(egui::vec2(980.0, 1700.0))
            .with_context_setup(marina_egui::fonts::install)
            .build_ui(|ui| {
                for slide in &deck.slides {
                    noteliner_slides::SlideView::new(slide, &deck.config)
                        .total(deck.slides.len())
                        .show(ui, 960.0);
                }
            });
        h.run();
    }
}
