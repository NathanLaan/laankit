//! Drives the app headlessly (egui_kittest) through the main flow, against a
//! temp config folder and a temp project: new project → new file → type →
//! autosave hits disk → rename → move via drag-and-drop → delete, checking
//! the files and `noteliner.json` on disk at each step. Needs `git` on the
//! PATH, like the app.

use std::path::Path;
use std::time::Duration;

use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::Value;

use crate::app::NoteLinerApp;
use crate::config::Paths;

pub(crate) fn harness(paths: Paths) -> Harness<'static, NoteLinerApp> {
    Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_context_setup(laankit_egui::fonts::install)
        .build_ui_state(
            |ui, app: &mut NoteLinerApp| app.show(ui),
            NoteLinerApp::new(None, paths),
        )
}

/// Steps the UI until `cond` holds (the backend works on other threads).
pub(crate) fn wait_for(
    h: &mut Harness<'static, NoteLinerApp>,
    what: &str,
    cond: impl Fn(&mut NoteLinerApp) -> bool,
) {
    for _ in 0..750 {
        h.step();
        if cond(h.state_mut()) {
            h.step();
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}");
}

/// Replaces the text of the `n`th single-line field.
fn replace_text(h: &mut Harness<'static, NoteLinerApp>, n: usize, text: &str) {
    // A dialog opened last frame is laid out invisibly first.
    h.run_steps(3);
    h.get_all_by_role(Role::TextInput)
        .nth(n)
        .expect("text field")
        .focus();
    h.step();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.step();
    h.get_all_by_role(Role::TextInput)
        .nth(n)
        .expect("text field")
        .type_text(text);
    h.step();
}

pub(crate) fn index(project: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(project.join("noteliner.json")).unwrap()).unwrap()
}

pub(crate) fn file_by_name<'a>(index: &'a Value, name: &str) -> &'a Value {
    index["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == name)
        .unwrap_or_else(|| panic!("no {name} in the index"))
}

/// Creates a note through the New File dialog and waits until it's open.
fn new_file(h: &mut Harness<'static, NoteLinerApp>, name: &str) {
    let before = h.state().entries().len();
    h.get_by_label("New File (Ctrl+N)").click();
    h.run_steps(2);
    replace_text(h, 0, name);
    h.get_by_label("OK").click();
    let name = name.to_string();
    wait_for(h, "the new note", move |a| {
        a.entries().len() == before + 1
            && a.selected().is_some()
            && a.editor_file_id() == a.selected()
            && a.entries().iter().any(|e| e.name == name)
    });
}

#[test]
fn new_project_file_type_rename_move_delete() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths {
        user_data: tmp.path().join("NoteLiner"),
    };
    let project = tmp.path().join("notes");
    let mut h = harness(paths.clone());
    h.run_steps(3);

    // New Project: location, git name and email.
    h.get_by_label("New Project").click();
    h.run_steps(2);
    replace_text(&mut h, 0, &project.display().to_string());
    replace_text(&mut h, 1, "Test User");
    replace_text(&mut h, 2, "test@localhost");
    h.get_by_label("OK").click();
    wait_for(&mut h, "the project", |a| a.project_index().is_some());
    assert!(project.join(".git").exists());
    assert!(project.join("noteliner.json").exists());
    // It's now a recent project, in the Tauri host's shape.
    let recent: Vec<Value> = laankit_core::json::read(&paths.recent_path()).unwrap();
    assert_eq!(recent[0]["path"], project.display().to_string());

    // New File, then type into it: the autosave writes after 500 ms.
    new_file(&mut h, "First Note");
    let first = project.join("first-note.md");
    assert!(first.exists());
    let editor = h.get_by_role(Role::MultilineTextInput);
    editor.focus();
    h.step();
    h.get_by_role(Role::MultilineTextInput)
        .type_text("hello from the test ");
    h.step();
    let first2 = first.clone();
    wait_for(&mut h, "the autosave", move |_| {
        std::fs::read_to_string(&first2).is_ok_and(|s| s.contains("hello from the test"))
    });
    // The frontmatter is still mirrored from the index.
    let raw = std::fs::read_to_string(&first).unwrap();
    assert!(raw.starts_with("---\n"), "{raw}");
    assert!(raw.contains("name: First Note"), "{raw}");

    new_file(&mut h, "Second Note");

    // Rename the first note: select it, F2, type, Enter.
    h.get_by_role_and_label(Role::TreeItem, "First Note")
        .click();
    wait_for(&mut h, "the first note", |a| {
        a.entries()
            .iter()
            .find(|e| e.name == "First Note")
            .is_some_and(|e| a.selected().as_deref() == Some(e.id.as_str()))
    });
    h.key_press(Key::F2);
    h.run_steps(3);
    let rename = h.get_by_label("Rename");
    rename.focus();
    h.step();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.step();
    h.get_by_label("Rename").type_text("Renamed");
    h.step();
    h.key_press(Key::Enter);
    let renamed = project.join("renamed.md");
    let renamed2 = renamed.clone();
    wait_for(&mut h, "the rename", move |a| {
        renamed2.exists() && a.entries().iter().any(|e| e.name == "Renamed")
    });
    assert!(!first.exists());

    // Drag Second Note onto the middle of Renamed: it becomes its child.
    h.run_steps(2);
    let from = h
        .get_by_role_and_label(Role::TreeItem, "Second Note")
        .rect()
        .center();
    let to = h
        .get_by_role_and_label(Role::TreeItem, "Renamed")
        .rect()
        .center();
    h.hover_at(from);
    h.step();
    h.drag_at(from);
    h.step();
    for i in 1..=10 {
        let t = i as f32 / 10.0;
        h.hover_at(from + (to - from) * t);
        h.step();
    }
    h.drop_at(to);
    h.step();
    let project2 = project.clone();
    wait_for(&mut h, "the move on disk", move |_| {
        let i = index(&project2);
        file_by_name(&i, "Second Note")["parentId"] == file_by_name(&i, "Renamed")["id"]
    });
    let i = index(&project);
    assert_eq!(file_by_name(&i, "Second Note")["order"], 0);
    // Index entries keep the shape `file:create` gave them.
    let keys: Vec<&String> = file_by_name(&i, "Second Note")
        .as_object()
        .unwrap()
        .keys()
        .collect();
    assert_eq!(
        keys,
        [
            "id",
            "name",
            "filename",
            "parentId",
            "order",
            "tags",
            "attachments",
            "createdAt",
            "modifiedAt"
        ]
    );

    // Delete Second Note: select it, Ctrl+D, confirm.
    h.get_by_role_and_label(Role::TreeItem, "Second Note")
        .click();
    wait_for(&mut h, "the second note", |a| {
        a.entries()
            .iter()
            .find(|e| e.name == "Second Note")
            .is_some_and(|e| a.selected().as_deref() == Some(e.id.as_str()))
    });
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run_steps(3);
    assert!(h.state().has_overlay());
    h.get_by_label("Delete").click();
    let second = project.join("second-note.md");
    let second2 = second.clone();
    wait_for(&mut h, "the delete", move |a| {
        !second2.exists() && a.entries().len() == 1
    });
    assert_eq!(index(&project)["files"].as_array().unwrap().len(), 1);
    assert!(h.state().selected().is_none());

    // Every change was committed.
    let log = std::process::Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(&project)
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&log.stdout);
    for msg in [
        "Add First Note",
        "Update first-note.md",
        "Rename first-note.md to renamed.md",
        "Update index",
        "Delete Second Note",
    ] {
        assert!(log.contains(msg), "missing commit {msg:?} in:\n{log}");
    }
    // Close the project so the push flush and MCP stop run.
    h.state_mut().backend().close_project(None);
    wait_for(&mut h, "the close", |a| a.project_index().is_none());
}
