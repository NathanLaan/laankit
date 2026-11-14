//! egui_kittest flows for the tabs above the document, against a project
//! built with `noteliner-core` in a temp folder:
//!
//! - choosing a file opens a tab; each tab keeps its text and undo history;
//! - Close Tab (the button and Ctrl+W) shows the neighbour, and the last one
//!   leaves nothing open;
//! - Next File reuses the active tab;
//! - the tabs and the active tab come back when the project reopens.
//!
//! Needs `git` on the PATH, like the app.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use noteliner_core::git::NoteGit;
use noteliner_core::project::Project;
use serde_json::json;

use crate::app::NoteLinerApp;
use crate::commands::Act;
use crate::config::Paths;
use crate::ui_tests::{harness, wait_for};

/// Alpha, Beta and Gamma, each with a line of text.
fn fixture(dir: &Path) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut p = Project::new(NoteGit::new(Arc::new(|_| {})));
        p.init(dir, None).await.unwrap();
        p.set_git_config("Test User", "test@localhost")
            .await
            .unwrap();
        for name in ["Alpha", "Beta", "Gamma"] {
            p.create_file(name, &json!([]), Some(format!("{name} text\n")), None)
                .await
                .unwrap();
        }
    });
}

fn open(user_data: &Path, project: &Path) -> Harness<'static, NoteLinerApp> {
    let mut h = harness(Paths {
        user_data: user_data.to_path_buf(),
    });
    h.run_steps(2);
    h.state_mut().open(project.to_path_buf());
    wait_for(&mut h, "the project", |a| a.entries().len() == 3);
    h
}

fn open_project(tmp: &Path) -> (Harness<'static, NoteLinerApp>, PathBuf) {
    let project = tmp.join("notes");
    std::fs::create_dir_all(&project).unwrap();
    fixture(&project);
    (open(&tmp.join("NoteLiner"), &project), project)
}

fn id_of(h: &Harness<'static, NoteLinerApp>, name: &str) -> String {
    h.state()
        .entries()
        .iter()
        .find(|e| e.name == name)
        .map(|e| e.id.clone())
        .unwrap()
}

fn wait_open(h: &mut Harness<'static, NoteLinerApp>, name: &str) {
    let id = id_of(h, name);
    wait_for(h, name, move |a| {
        a.editor_file_id().as_deref() == Some(id.as_str())
    });
}

/// Chooses a file in the FILES tree and waits until it's open.
fn choose(h: &mut Harness<'static, NoteLinerApp>, name: &str) {
    h.get_by_role_and_label(Role::TreeItem, name).click();
    wait_open(h, name);
}

fn act(h: &mut Harness<'static, NoteLinerApp>, act: Act) {
    let ctx = h.ctx.clone();
    h.state_mut().apply_for_test(&ctx, act);
    h.run_steps(2);
}

/// The open tabs, by note name.
fn tabs(h: &Harness<'static, NoteLinerApp>) -> Vec<String> {
    let entries = h.state().entries();
    h.state()
        .tab_ids()
        .iter()
        .map(|id| {
            entries
                .iter()
                .find(|e| &e.id == id)
                .map(|e| e.name.clone())
                .unwrap()
        })
        .collect()
}

#[test]
fn each_tab_keeps_its_text_and_undo_history() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    choose(&mut h, "Alpha");
    h.get_by_role(Role::MultilineTextInput).focus();
    h.run_steps(2);
    h.event(Event::Text("typed ".into()));
    h.run_steps(2);
    assert!(h.state().editor_text().contains("typed "));

    // A second tab; the edit lands in Alpha's file, not Beta's.
    choose(&mut h, "Beta");
    assert_eq!(tabs(&h), ["Alpha", "Beta"]);
    assert_eq!(h.state().editor_text(), "Beta text\n");
    let alpha = project.join("alpha.md");
    wait_for(&mut h, "Alpha's edit on disk", move |_| {
        std::fs::read_to_string(&alpha).is_ok_and(|s| s.contains("typed "))
    });

    // Back on Alpha's tab, the edit is there and can still be undone.
    h.get_by_role_and_label(Role::Tab, "Alpha").click();
    wait_open(&mut h, "Alpha");
    assert!(h.state().editor_text().contains("typed "));
    h.get_by_role(Role::MultilineTextInput).focus();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(h.state().editor_text(), "Alpha text\n");
    assert_eq!(tabs(&h), ["Alpha", "Beta"]);
}

#[test]
fn closing_tabs_shows_the_neighbour() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, _project) = open_project(tmp.path());
    for name in ["Alpha", "Beta", "Gamma"] {
        choose(&mut h, name);
    }
    assert_eq!(tabs(&h), ["Alpha", "Beta", "Gamma"]);

    // Next Tab and Previous Tab wrap around.
    act(&mut h, Act::NextTab);
    wait_open(&mut h, "Alpha");
    act(&mut h, Act::PreviousTab);
    wait_open(&mut h, "Gamma");

    // Closing a background tab leaves the active one alone.
    h.get_by_role_and_label(Role::Tab, "Beta").hover();
    h.run_steps(2);
    h.get_by_label("Close Beta").click();
    h.run_steps(2);
    assert_eq!(tabs(&h), ["Alpha", "Gamma"]);
    assert_eq!(h.state().selected(), Some(id_of(&h, "Gamma")));

    // Ctrl+W closes the active tab: the one to its left is shown.
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(2);
    wait_open(&mut h, "Alpha");
    assert_eq!(tabs(&h), ["Alpha"]);

    // The last tab: nothing is open.
    act(&mut h, Act::CloseTab);
    assert!(tabs(&h).is_empty());
    assert_eq!(h.state().selected(), None);
    assert_eq!(h.state().editor_file_id(), None);
    assert!(h.query_by_role(Role::Tab).is_none());
}

#[test]
fn next_file_reuses_the_active_tab() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, _project) = open_project(tmp.path());
    choose(&mut h, "Alpha");
    act(&mut h, Act::NextFile);
    wait_open(&mut h, "Beta");
    assert_eq!(tabs(&h), ["Beta"]);

    // Choosing a file opens a tab of its own.
    choose(&mut h, "Gamma");
    assert_eq!(tabs(&h), ["Beta", "Gamma"]);

    // Stepping onto a file that has a tab shows that tab.
    act(&mut h, Act::PrevFile);
    wait_open(&mut h, "Beta");
    assert_eq!(tabs(&h), ["Beta", "Gamma"]);
}

#[test]
fn tabs_come_back_when_the_project_reopens() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    for name in ["Gamma", "Alpha", "Beta"] {
        choose(&mut h, name);
    }
    h.get_by_role_and_label(Role::Tab, "Alpha").click();
    wait_open(&mut h, "Alpha");
    h.run_steps(3);
    h.state_mut().shutdown();
    drop(h);

    let mut h = open(&tmp.path().join("NoteLiner"), &project);
    wait_open(&mut h, "Alpha");
    assert_eq!(tabs(&h), ["Gamma", "Alpha", "Beta"]);
}
