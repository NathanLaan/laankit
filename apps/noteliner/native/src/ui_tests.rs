//! Drives the app headlessly (egui_kittest) through the main flow, against a
//! temp config folder and a temp project: open a plain folder → new file →
//! type → autosave hits disk → rename → new folder → move via drag-and-drop →
//! delete, checking the files and `noteliner.json` on disk at each step. No
//! `git` is run, and the test passes with none on the `PATH`.

use std::path::Path;
use std::time::Duration;

use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};
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

/// The names of the tree's rows, in display order.
pub(crate) fn tree_rows(h: &Harness<'static, NoteLinerApp>) -> Vec<String> {
    h.query_all_by_role(Role::TreeItem)
        .map(|n| n.accesskit_node().label().unwrap_or_default())
        .collect()
}

/// Drags the tree row `from` onto the tree row `to`.
pub(crate) fn drag_row(h: &mut Harness<'static, NoteLinerApp>, from: &str, to: &str) {
    h.run_steps(2);
    let from = h
        .get_by_role_and_label(Role::TreeItem, from)
        .rect()
        .center();
    let to = h.get_by_role_and_label(Role::TreeItem, to).rect().center();
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
}

/// Everything the test trash holds, by file name.
pub(crate) fn trashed(paths: &Paths) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(paths.user_data.join("trash"))
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn open_folder_file_type_rename_move_delete() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths {
        user_data: tmp.path().join("NoteLiner"),
    };
    // A plain, empty folder: no repository, no index.
    let project = tmp.path().join("notes");
    std::fs::create_dir_all(&project).unwrap();
    let mut h = harness(paths.clone());
    h.run_steps(3);
    // One Open Folder action; New Project is gone.
    assert!(h.query_by_label("New Project").is_none());
    h.get_by_label("Open Folder");

    h.state_mut().open(project.clone());
    wait_for(&mut h, "the project", |a| a.project_index().is_some());
    // It opened with no setup dialog, and wrote only the index.
    assert!(!h.state().has_overlay());
    assert!(project.join("noteliner.json").exists());
    assert!(!project.join(".git").exists());
    assert_eq!(index(&project)["version"], 4);
    // It's now a recent project.
    let recent: Vec<Value> = laankit_core::json::read(&paths.recent_path()).unwrap();
    assert_eq!(recent[0]["path"], project.display().to_string());
    // The status bar has no branch chip, and the toolbar no Remote Sync.
    assert!(h.query_by_label("Remote Sync").is_none());
    assert!(h.query_by_label_contains("Remote Sync").is_none());
    assert!(h.query_by_label_contains("Project Settings").is_none());

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
    // The frontmatter holds the note's tags and created date, and no id,
    // name or updated time.
    let raw = std::fs::read_to_string(&first).unwrap();
    assert!(raw.starts_with("---\ntags: []\ncreated: '"), "{raw}");
    for gone in ["id:", "name:", "updated:"] {
        assert!(!raw.contains(gone), "{gone} in {raw}");
    }
    // The editor shows the whole file; what was typed went into the text.
    assert_eq!(h.state().editor_file_text(), raw);
    assert!(h
        .state()
        .editor_text()
        .starts_with("hello from the test # First Note"));

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
    // A rename changes nothing in the file.
    assert_eq!(std::fs::read_to_string(&renamed).unwrap(), raw);

    // New Folder, from the FILES header. Renamed is selected, so the folder
    // is made beside it, at the top.
    h.get_by_label("New Folder").click();
    h.run_steps(2);
    replace_text(&mut h, 0, "Work");
    h.get_by_label("OK").click();
    wait_for(&mut h, "the folder", |a| a.folders() == ["Work"]);
    assert!(project.join("Work").is_dir());
    assert_eq!(h.state().selected_folder().as_deref(), Some("Work"));
    // Folders come first.
    assert_eq!(tree_rows(&h), ["Folder Work", "Renamed", "Second Note"]);

    // Drag Second Note onto the folder: its file moves there.
    drag_row(&mut h, "Second Note", "Folder Work");
    let project2 = project.clone();
    wait_for(&mut h, "the move on disk", move |a| {
        project2.join("Work/second-note.md").exists()
            && a.entries()
                .iter()
                .any(|e| e.filename == "Work/second-note.md")
    });
    assert!(!project.join("second-note.md").exists());
    let i = index(&project);
    assert_eq!(
        file_by_name(&i, "Second Note")["filename"],
        "Work/second-note.md"
    );
    // Index entries have no parent and no order.
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
            "tags",
            "attachments",
            "createdAt",
            "modifiedAt"
        ]
    );
    // The folder it was dropped on opened or not, the note is in it.
    if !h.state().open_folders().contains(&"Work".to_string()) {
        h.get_by_role_and_label(Role::TreeItem, "Folder Work")
            .click();
        h.run_steps(2);
    }
    assert_eq!(tree_rows(&h), ["Folder Work", "Second Note", "Renamed"]);

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
    let second = project.join("Work/second-note.md");
    let second2 = second.clone();
    wait_for(&mut h, "the delete", move |a| {
        !second2.exists() && a.entries().len() == 1
    });
    assert_eq!(index(&project)["files"].as_array().unwrap().len(), 1);
    // It went to the trash, not away.
    assert_eq!(trashed(&paths), ["second-note.md"]);
    // Its tab closed: the other note's tab is shown.
    let renamed_id = h.state().entries()[0].id.clone();
    assert_eq!(h.state().selected(), Some(renamed_id));

    // Nothing here made a repository.
    assert!(!project.join(".git").exists());
    h.state_mut().backend().close_project(Vec::new());
    wait_for(&mut h, "the close", |a| a.project_index().is_none());
}
