//! Phase 5 of the editor/slides/rich-text plan in the app: presenting with the speaker view
//! (headless: the speaker view is embedded, no real window opens or goes full screen), and math
//! in the preview typeset by Typst through the app's exporter.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui::Key;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use marina_richtext::math::{self, MathImage, MathMode};
use marina_richtext::Block;
use noteliner_core::git::NoteGit;
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
        let mut p = Project::new(NoteGit::new(Arc::new(|_| {})));
        p.init(&project, None).await.unwrap();
        p.set_git_config("Test User", "test@localhost")
            .await
            .unwrap();
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
    });
    let mut h = harness(Paths {
        user_data: tmp.join("NoteLiner"),
    });
    h.run_steps(2);
    h.state_mut().open(project);
    wait_for(&mut h, "the project", |a| a.entries().len() == 1);
    h.get_by_role_and_label(Role::TreeItem, "Talk").click();
    wait_for(&mut h, "the deck", |a| a.deck_slides() == Some(2));
    h
}

#[test]
fn present_with_speaker_view_and_end() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = open_deck(tmp.path());
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
