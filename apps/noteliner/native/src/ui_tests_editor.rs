//! egui_kittest flows for the marina-editor and noteliner-slides
//! integration, against a project built with `noteliner-core` in a temp
//! folder:
//!
//! - typing autosaves; Enter continues a list; `[[` completes a note name;
//!   a pasted image becomes an attachment and its Markdown; Ctrl+click on a
//!   wikilink opens that note.
//! - a presentation: the SLIDES pane inserts a slide (written to disk at
//!   once), the settings dialog saves the `presentation:` block, and the
//!   presenter opens and closes (headless: no real window goes full screen).
//!
//! Needs `git` on the PATH, like the app.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::accesskit::Role;
use egui::{ColorImage, Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use noteliner_core::git::NoteGit;
use noteliner_core::project::Project;
use serde_json::json;

use crate::app::NoteLinerApp;
use crate::config::Paths;
use crate::ui_tests::{harness, wait_for};

const TALK: &str = "## One\n\nHello\n\n## Two\n\nWorld\n";

/// Alpha (empty), Beta linking to Alpha, and a two-slide presentation.
fn fixture(dir: &Path) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut p = Project::new(NoteGit::new(Arc::new(|_| {})));
        p.init(dir, None).await.unwrap();
        p.set_git_config("Test User", "test@localhost")
            .await
            .unwrap();
        p.create_file("Alpha", &json!([]), Some(String::new()), None)
            .await
            .unwrap();
        p.create_file(
            "Beta",
            &json!([]),
            Some("[[Alpha]] is linked.\n".into()),
            None,
        )
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
}

fn open_project(tmp: &Path) -> (Harness<'static, NoteLinerApp>, PathBuf) {
    let project = tmp.join("notes");
    std::fs::create_dir_all(&project).unwrap();
    fixture(&project);
    let mut h = harness(Paths {
        user_data: tmp.join("NoteLiner"),
    });
    h.run_steps(2);
    h.state_mut().open(project.clone());
    wait_for(&mut h, "the project", |a| a.entries().len() == 3);
    (h, project)
}

fn id_of(h: &Harness<'static, NoteLinerApp>, name: &str) -> String {
    h.state()
        .entries()
        .iter()
        .find(|e| e.name == name)
        .map(|e| e.id.clone())
        .unwrap()
}

fn select(h: &mut Harness<'static, NoteLinerApp>, name: &str) {
    h.get_by_role_and_label(Role::TreeItem, name).click();
    let id = id_of(h, name);
    wait_for(h, name, move |a| {
        a.editor_file_id().as_deref() == Some(id.as_str())
    });
}

fn type_text(h: &mut Harness<'static, NoteLinerApp>, text: &str) {
    h.event(Event::Text(text.into()));
    h.run_steps(2);
}

fn key(h: &mut Harness<'static, NoteLinerApp>, key: Key) {
    h.key_press(key);
    h.run_steps(2);
}

/// Waits until the note's file on disk contains `needle`.
fn wait_on_disk(h: &mut Harness<'static, NoteLinerApp>, file: PathBuf, needle: &'static str) {
    wait_for(h, needle, move |_| {
        std::fs::read_to_string(&file).is_ok_and(|s| s.contains(needle))
    });
}

#[test]
fn editor_lists_completion_paste_and_links() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    let alpha = project.join("alpha.md");
    select(&mut h, "Alpha");

    // Typing autosaves; Enter continues the list.
    h.get_by_role(Role::MultilineTextInput).focus();
    h.run_steps(2);
    type_text(&mut h, "- one");
    key(&mut h, Key::Enter);
    type_text(&mut h, "two");
    assert!(
        h.state().editor_text().contains("- one\n- two"),
        "{:?}",
        h.state().editor_text()
    );
    wait_on_disk(&mut h, alpha.clone(), "- one\n- two");

    // Enter on the empty item ends the list; `[[` completes a note name.
    key(&mut h, Key::Enter);
    key(&mut h, Key::Enter);
    type_text(&mut h, "[[");
    type_text(&mut h, "Be");
    // The FILES tree has a "Beta" row too; the completion list adds another.
    assert_eq!(
        h.query_all_by_label("Beta").count(),
        2,
        "the completion list"
    );
    key(&mut h, Key::Enter);
    assert!(
        h.state().editor_text().contains("[[Beta]]"),
        "{:?}",
        h.state().editor_text()
    );
    wait_on_disk(&mut h, alpha.clone(), "[[Beta]]");

    // A pasted image is saved under _attachments and referenced in the note.
    h.event(Event::PasteImage(Arc::new(ColorImage::filled(
        [2, 2],
        egui::Color32::RED,
    ))));
    wait_for(&mut h, "the pasted image", |a| {
        a.editor_text().contains("![image.png](./_attachments/")
    });
    wait_on_disk(&mut h, alpha.clone(), "![image.png](./_attachments/");
    let stored: Vec<_> = std::fs::read_dir(project.join("_attachments"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
        .collect();
    assert_eq!(stored.len(), 1, "one PNG attachment");

    // Ctrl+click on `[[Alpha]]` in Beta opens Alpha.
    select(&mut h, "Beta");
    let rect = h.get_by_role(Role::MultilineTextInput).rect();
    let at = rect.min + egui::vec2(14.0, 8.0);
    h.event(Event::PointerMoved(at));
    h.step();
    // egui reads Ctrl from the input state, not from the click event.
    h.event(Event::ModifiersChanged(Modifiers::COMMAND));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::COMMAND,
        });
        h.step();
    }
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    let alpha_id = id_of(&h, "Alpha");
    wait_for(&mut h, "Alpha from the wikilink", move |a| {
        a.editor_file_id().as_deref() == Some(alpha_id.as_str())
    });
}

#[test]
fn slides_pane_settings_and_presenter() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    if let Some(l) = h.state_mut().layout_mut() {
        l.show_slides = true;
    }
    select(&mut h, "Talk");
    wait_for(&mut h, "the deck", |a| a.deck_slides() == Some(2));

    // SLIDES pane: "New Slide Below" on slide 2 writes the note at once.
    h.get_by_label("Slide 2 — Two").click_secondary();
    h.run_steps(2);
    h.get_by_label_contains("New Slide Below").click();
    h.run_steps(2);
    wait_for(&mut h, "three slides", |a| a.deck_slides() == Some(3));
    let talk = project.join("talk.md");
    wait_for(&mut h, "the slide on disk", move |_| {
        std::fs::read_to_string(&talk).is_ok_and(|s| s.matches("## ").count() == 3)
    });

    // Presentation settings: turning slide numbers off is saved in the frontmatter.
    let ctx = h.ctx.clone();
    h.state_mut()
        .apply_for_test(&ctx, crate::commands::Act::PresentationSettings);
    wait_for(&mut h, "the settings dialog", |a| {
        a.presentation_settings_open()
    });
    h.get_by_label("Show slide numbers").click();
    h.run_steps(2);
    h.get_by_label("Save").click();
    wait_for(&mut h, "the settings saved", |a| {
        !a.presentation_settings_open()
    });
    let raw = std::fs::read_to_string(project.join("talk.md")).unwrap();
    assert!(raw.contains("slideNumbers: false"), "{raw}");

    // Present (F5), then Escape closes the presenter.
    h.state_mut()
        .apply_for_test(&ctx, crate::commands::Act::Present);
    h.run_steps(2);
    assert!(h.state().presenting());
    key(&mut h, Key::Escape);
    h.run_steps(2);
    assert!(!h.state().presenting());
}
