//! Phase 5 of the editor/slides/rich-text plan in the app: presenting with the speaker view
//! (headless: the speaker view is embedded, no real window opens or goes full screen), and math
//! in the preview typeset by Typst through the app's exporter.

use std::path::Path;
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui::Key;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use laankit_richtext::math::{self, MathImage, MathMode};
use laankit_richtext::Block;
use noteliner_core::project::Project;
use serde_json::json;

use crate::app::NoteLinerApp;
use crate::commands::Act;
use crate::config::Paths;
use crate::ui_tests::{harness, wait_for};

const TALK: &str = "## One\n\nHello $x^2$\n\n<!-- notes: Speak slowly. -->\n\n## Two\n\nWorld\n";

fn open_deck(tmp: &Path) -> Harness<'static, NoteLinerApp> {
    let project = tmp.join("notes");
    std::fs::create_dir_all(&project).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut p = Project::new();
        p.open(&project).await.unwrap();
        let talk = p
            .create_file("Talk", &json!([]), Some(TALK.into()), None)
            .await
            .unwrap();
        p.set_presentation(
            talk["filename"].as_str().unwrap(),
            &crate::backend::default_presentation(),
        )
        .await
        .unwrap();
        p.create_file("Plain", &json!([]), Some("# Plain\n\ntext\n".into()), None)
            .await
            .unwrap();
        p.create_file("Sketch", &json!([]), None, None)
            .await
            .unwrap();
    });
    let mut h = harness(Paths {
        user_data: tmp.join("NoteLiner"),
    });
    h.run_steps(2);
    h.state_mut().open(project);
    wait_for(&mut h, "the project", |a| a.entries().len() == 3);
    h.get_by_role_and_label(Role::TreeItem, "Talk").click();
    wait_for(&mut h, "the deck", |a| a.deck_slides() == Some(2));
    h
}

#[test]
fn present_with_speaker_view_and_end() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = open_deck(tmp.path());
    // The toolbar offers both while a deck is open.
    h.get_by_label("Present (F5)");
    h.get_by_label("Present with Speaker View (Alt+F5)");
    let ctx = h.ctx.clone();
    h.state_mut()
        .apply_for_test(&ctx, Act::PresentWithSpeakerView);
    h.run_steps(2);
    assert!(h.state().presenting());
    assert!(h.state().speaker_view_open());
    h.get_by_label("Current slide 1");
    h.get_by_label("Next slide 2");
    assert!(
        h.query_by_label_contains("Speak slowly").is_some(),
        "the notes"
    );
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    h.get_by_label("Current slide 2");
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!h.state().presenting());

    // Plain F5 has no speaker view.
    h.state_mut().apply_for_test(&ctx, Act::Present);
    h.run_steps(2);
    assert!(h.state().presenting() && !h.state().speaker_view_open());
}

#[test]
fn preview_documents_keep_math() {
    let doc = crate::views::preview::document("Area $\\pi r^2$\n\n$$\\int_0^1 x\\,dx$$\n");
    let Block::Text { spans, .. } = &doc.blocks[0] else {
        panic!("{:?}", doc.blocks);
    };
    assert!(spans
        .iter()
        .any(|s| s.style.math == Some(MathMode::Inline) && s.text == "\\pi r^2"));
    assert!(matches!(&doc.blocks[1], Block::Image { src, .. } if src.starts_with("math:")));
}

/// The app installs Typst as the math renderer when it starts; a formula renders through it.
#[test]
fn math_renders_through_typst() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(Paths {
        user_data: tmp.path().join("NoteLiner"),
    });
    h.run_steps(2);
    let ctx = h.ctx.clone();
    assert!(math::is_installed(&ctx));
    let start = Instant::now();
    let image = loop {
        match math::image(&ctx, "\\frac{a}{b}", MathMode::Display, 16.0) {
            MathImage::Pending if start.elapsed() < Duration::from_secs(60) => {
                std::thread::sleep(Duration::from_millis(20));
                h.run_steps(1);
            }
            other => break other,
        }
    };
    let MathImage::Ready(texture) = image else {
        panic!("not rendered: {image:?}");
    };
    // A fraction is taller than a line of 16 pt text and narrower than a line.
    assert!(
        texture.size.y > 16.0 && texture.size.x < 200.0,
        "{:?}",
        texture.size
    );
    // Unsupported LaTeX falls back.
    let start = Instant::now();
    let failed = loop {
        match math::image(&ctx, "\\color{red}{x}", MathMode::Inline, 16.0) {
            MathImage::Pending if start.elapsed() < Duration::from_secs(30) => {
                std::thread::sleep(Duration::from_millis(20));
                h.run_steps(1);
            }
            other => break other,
        }
    };
    assert!(matches!(failed, MathImage::Unavailable), "{failed:?}");
}

fn stop(h: &mut Harness<'static, NoteLinerApp>) {
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!h.state().presenting());
}

fn open_note(h: &mut Harness<'static, NoteLinerApp>, name: &str) {
    h.get_by_role_and_label(Role::TreeItem, name).click();
    let name = name.to_string();
    wait_for(h, "the note", move |a| {
        let id = a
            .entries()
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.id.clone());
        id.is_some() && a.editor_file_id() == id
    });
    h.run_steps(2);
}

/// The standard buttons keep their place whatever is open; the presentation
/// buttons come after them, for a presentation only.
#[test]
fn the_toolbar_keeps_presentation_buttons_below_the_standard_ones() {
    const STANDARD: [&str; 6] = [
        "Files (Ctrl+E)",
        "Outline (Ctrl+Shift+O)",
        "Tags (Ctrl+Shift+T)",
        "Attachments (Ctrl+Shift+A)",
        "Search (Ctrl+F)",
        "Backlinks (Ctrl+Shift+B)",
    ];
    const CONTEXT: [&str; 3] = [
        "Slides (Ctrl+Shift+G)",
        "Present (F5)",
        "Present with Speaker View (Alt+F5)",
    ];
    let tmp = tempfile::tempdir().unwrap();
    let mut h = open_deck(tmp.path());
    let tops = |h: &Harness<'static, NoteLinerApp>, labels: &[&str]| -> Vec<f32> {
        labels
            .iter()
            .map(|l| h.get_by_label(l).rect().top())
            .collect()
    };
    let deck = tops(&h, &STANDARD);
    assert!(deck.windows(2).all(|w| w[0] < w[1]), "{deck:?}");
    let context = tops(&h, &CONTEXT);
    assert!(context.windows(2).all(|w| w[0] < w[1]), "{context:?}");
    assert!(
        context[0] > deck[5],
        "below Backlinks: {context:?} {deck:?}"
    );

    open_note(&mut h, "Plain");
    assert_eq!(tops(&h, &STANDARD), deck, "a note");
    for label in CONTEXT {
        assert!(h.query_by_label(label).is_none(), "{label} on a note");
    }
}

#[test]
fn present_from_the_first_slide_the_menus_and_the_slides_pane() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = open_deck(tmp.path());
    let ctx = h.ctx.clone();
    if let Some(l) = h.state_mut().layout_mut() {
        l.show_slides = true;
    }
    h.run_steps(2);

    // The caret on slide 2: Present starts there, Shift+F5 at slide 1.
    h.get_by_label("Slide 2 — Two").click();
    h.run_steps(3);
    h.state_mut().apply_for_test(&ctx, Act::Present);
    h.run_steps(2);
    assert_eq!(h.state().presenting_slide(), Some(2));
    stop(&mut h);
    h.key_press_modifiers(egui::Modifiers::SHIFT, Key::F5);
    h.run_steps(2);
    assert_eq!(h.state().presenting_slide(), Some(1));
    stop(&mut h);

    // The SLIDES pane: its header's button, and a slide's menu.
    h.get_by_label("Present").click();
    h.run_steps(2);
    assert_eq!(h.state().presenting_slide(), Some(2), "the caret's slide");
    stop(&mut h);
    h.get_by_label("Slide 1 — One").click_secondary();
    h.run_steps(3);
    h.get_by_label_contains("Present from This Slide").click();
    h.run_steps(3);
    assert_eq!(h.state().presenting_slide(), Some(1));
    stop(&mut h);

    // The FILES pane: Present on a presentation that isn't open opens it first.
    open_note(&mut h, "Plain");
    assert!(!h.state().presenting());
    h.get_by_role_and_label(Role::TreeItem, "Talk")
        .click_secondary();
    h.run_steps(3);
    // The menu's item, not the toolbar's button (which names its key in brackets).
    h.get_all_by_label_contains("Present with Speaker View")
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| !l.contains("(Alt+F5)"))
        })
        .expect("the menu item")
        .click();
    wait_for(&mut h, "presenting Talk", |a| a.presenting());
    assert!(h.state().speaker_view_open());
    stop(&mut h);
    // A note's menu has no Present.
    h.get_by_role_and_label(Role::TreeItem, "Plain")
        .click_secondary();
    h.run_steps(3);
    assert!(h
        .query_by_label_contains("Convert to Presentation")
        .is_some());
    assert!(h
        .query_all_by_label_contains("Present with Speaker View")
        .all(|n| n
            .accesskit_node()
            .label()
            .is_some_and(|l| l.contains("(Alt+F5)"))));
}
