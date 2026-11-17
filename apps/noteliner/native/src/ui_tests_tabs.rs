//! egui_kittest flows for the tabs above the document, against a project
//! built with `noteliner-core` in a temp folder:
//!
//! - choosing a file opens a tab; each tab keeps its text and undo history;
//! - Close Tab (the button and Ctrl+W) shows the neighbour, and the last one
//!   leaves nothing open;
//! - Next File reuses the active tab;
//! - Undo and Redo on the toolbar, and their keys while the editor doesn't
//!   have the keyboard;
//! - Reopen Closed Tab brings a tab back with its undo history;
//! - the tabs and the active tab come back when the project reopens.
//!
//!

use std::path::{Path, PathBuf};

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
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
        std::fs::create_dir_all(dir).unwrap();
        let mut p = Project::new();
        p.open(dir).await.unwrap();
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

/// Types into the open note, with the keyboard in the editor.
fn type_text(h: &mut Harness<'static, NoteLinerApp>, text: &str) {
    h.get_by_role(Role::MultilineTextInput).focus();
    h.run_steps(2);
    h.event(Event::Text(text.into()));
    h.run_steps(2);
}

fn enabled(h: &Harness<'static, NoteLinerApp>, label: &str) -> bool {
    !h.get_by_label(label).accesskit_node().is_disabled()
}

#[test]
fn undo_and_redo_from_the_toolbar_and_the_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    choose(&mut h, "Alpha");
    h.run_steps(2);
    // Nothing to undo yet: both buttons are off.
    assert!(!enabled(&h, "Undo (Ctrl+Z)"));
    assert!(!enabled(&h, "Redo (Ctrl+Shift+Z)"));

    type_text(&mut h, "typed ");
    assert!(h.state().editor_text().contains("typed "));
    assert!(enabled(&h, "Undo (Ctrl+Z)"));
    assert!(!enabled(&h, "Redo (Ctrl+Shift+Z)"));

    h.get_by_label("Undo (Ctrl+Z)").click();
    h.run_steps(4);
    assert_eq!(h.state().editor_text(), "Alpha text\n");
    assert!(!enabled(&h, "Undo (Ctrl+Z)"));
    h.get_by_label("Redo (Ctrl+Shift+Z)").click();
    h.run_steps(4);
    assert!(h.state().editor_text().contains("typed "));

    // The keys reach the note while nothing has the keyboard.
    let ctx = h.ctx.clone();
    ctx.memory_mut(|m| {
        if let Some(id) = m.focused() {
            m.surrender_focus(id);
        }
    });
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(4);
    assert_eq!(h.state().editor_text(), "Alpha text\n");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run_steps(4);
    assert!(h.state().editor_text().contains("typed "));

    // An undo is an edit like any other: it is saved.
    act(&mut h, Act::Undo);
    h.run_steps(2);
    let alpha = project.join("alpha.md");
    wait_for(&mut h, "the undo on disk", move |_| {
        std::fs::read_to_string(&alpha).is_ok_and(|s| !s.contains("typed "))
    });
}

#[test]
fn reopen_closed_tab_brings_back_its_history() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    choose(&mut h, "Alpha");
    type_text(&mut h, "typed ");
    choose(&mut h, "Beta");
    let alpha = project.join("alpha.md");
    let on_disk = alpha.clone();
    wait_for(&mut h, "Alpha's edit on disk", move |_| {
        std::fs::read_to_string(&on_disk).is_ok_and(|s| s.contains("typed "))
    });

    // Close Alpha's tab, then bring it back with Ctrl+Shift+W.
    h.get_by_role_and_label(Role::Tab, "Alpha").click();
    wait_open(&mut h, "Alpha");
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(2);
    wait_open(&mut h, "Beta");
    assert_eq!(tabs(&h), ["Beta"]);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::W);
    h.run_steps(2);
    wait_open(&mut h, "Alpha");
    assert_eq!(tabs(&h), ["Beta", "Alpha"]);
    assert!(h.state().editor_text().contains("typed "));
    // Its history came back with it.
    act(&mut h, Act::Undo);
    h.run_steps(2);
    assert_eq!(h.state().editor_text(), "Alpha text\n");
    act(&mut h, Act::Redo);
    h.run_steps(2);
    assert!(h.state().editor_text().contains("typed "));
    let on_disk = alpha.clone();
    wait_for(&mut h, "the redo on disk", move |a| {
        a.editor_status() == crate::editor::SaveStatus::Saved
            && std::fs::read_to_string(&on_disk).is_ok_and(|s| s.contains("typed "))
    });

    // Another program changes the file while its tab is closed: reopening
    // takes the change as one step on top of the history.
    act(&mut h, Act::CloseTab);
    wait_open(&mut h, "Beta");
    std::fs::write(&alpha, "Changed elsewhere\n").unwrap();
    act(&mut h, Act::ReopenClosedTab);
    wait_open(&mut h, "Alpha");
    wait_for(&mut h, "the change from disk", |a| {
        a.editor_text() == "Changed elsewhere\n"
    });
    act(&mut h, Act::Undo);
    h.run_steps(2);
    assert!(h.state().editor_text().contains("typed "));

    // Nothing more to reopen.
    act(&mut h, Act::ReopenClosedTab);
    assert_eq!(tabs(&h), ["Beta", "Alpha"]);
}
