//! The Files pane and the tabs, headlessly: each tab keeping its own undo
//! history and view, closing a tab (the file stays in Files) and a file
//! (from its right-click menu), files from outside the project and Close
//! Folder, outside changes to a background tab, and the tabs coming back
//! after a restart.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use blockliner_core::{Rect, Shape, ShapeKind};
use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;

use crate::commands::Act;
use crate::config::Paths;
use crate::ui_tests::{app_harness, harness, H};

fn project(paths: &Paths) -> PathBuf {
    paths.prefs().last_project.expect("a project")
}

/// The harness with a second diagram in the folder, the sidebar and the
/// Files pane showing, and the folder listed.
fn two_diagrams() -> (tempfile::TempDir, Paths, H) {
    let (tmp, paths, mut h) = harness();
    fs::write(
        project(&paths).join("Second.mmd"),
        "flowchart TD\n  s[\"In second\"]\n",
    )
    .unwrap();
    h.run_steps(2);
    h.state_mut().run(Act::ToggleFiles);
    h.run_steps(2);
    next_scan(&mut h);
    (tmp, paths, h)
}

/// Past the once-a-second look at the folder (at 60 frames a second).
fn next_scan(h: &mut H) {
    h.run_steps(70);
}

fn add_shape(h: &mut H, id: &str) {
    h.state_mut().diagram_mut().shapes.push(Shape::new(
        id,
        ShapeKind::Rect,
        id,
        Rect::new(100.0, 100.0, 160.0, 60.0),
    ));
    h.run_steps(2);
}

/// Writes `text` as something other than BlockLiner would, with a later
/// modification time so the change shows on coarse-clock file systems.
fn write_outside(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    let f = fs::File::options().write(true).open(path).unwrap();
    f.set_modified(SystemTime::now() + Duration::from_secs(2))
        .unwrap();
}

fn click_tab(h: &mut H, name: &str) {
    h.get_by_role_and_label(Role::Tab, name).click();
    h.run_steps(3);
}

/// The Files pane's row: the last of the rows called `name` (the Project
/// pane's comes first).
fn click_file_row(h: &mut H, name: &str, secondary: bool) {
    let row = h
        .get_all_by(|n| n.role() == Role::ListItem && n.label().as_deref() == Some(name))
        .last()
        .expect("a Files row");
    if secondary {
        row.click_secondary();
    } else {
        row.click();
    }
    h.run_steps(3);
}

#[test]
fn each_tab_keeps_its_undo_history_and_view() {
    let (_tmp, _paths, mut h) = two_diagrams();
    assert_eq!(h.state().file_names(), ["Diagram"]);
    assert_eq!(h.state().tab_names(), ["Diagram"]);

    // From the Project pane: a second tab, active.
    h.get_by_label("Second").click();
    h.run_steps(3);
    assert_eq!(h.state().file_names(), ["Diagram", "Second"]);
    assert_eq!(h.state().tab_names(), ["Diagram", "Second"]);
    assert_eq!(h.state().doc_name(), Some("Second"));
    add_shape(&mut h, "added");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Equals);
    h.run_steps(2);
    let zoom = h.state().canvas().view.zoom;
    assert!(zoom > 1.0);

    // The other tab has its own diagram, view and history.
    click_tab(&mut h, "Diagram");
    assert_eq!(h.state().doc_name(), Some("Diagram"));
    assert!(h.state().diagram().shapes.is_empty());
    assert_eq!(h.state().canvas().view.zoom, 1.0);
    assert!(!h.state().can_undo());

    // Ctrl+Tab back: all as it was, and the edit undoes.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Tab);
    h.run_steps(3);
    assert_eq!(h.state().doc_name(), Some("Second"));
    assert_eq!(h.state().diagram().shapes.len(), 2);
    assert_eq!(h.state().canvas().view.zoom, zoom);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().diagram().shapes.len(), 1);
}

#[test]
fn closing_a_tab_keeps_the_file_in_files() {
    let (_tmp, _paths, mut h) = two_diagrams();
    h.get_by_label("Second").click();
    h.run_steps(3);

    // Its close button: the left neighbour is active.
    h.get_by_label("Close Second").click();
    h.run_steps(3);
    assert_eq!(h.state().tab_names(), ["Diagram"]);
    assert_eq!(h.state().file_names(), ["Diagram", "Second"]);
    assert_eq!(h.state().doc_name(), Some("Diagram"));

    // Ctrl+W: no tab left, both files still listed.
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(3);
    assert!(h.state().tab_names().is_empty());
    assert_eq!(h.state().doc_name(), None);
    assert_eq!(h.state().file_names(), ["Diagram", "Second"]);
    h.get_by_label("No diagram open");

    // Its Files row opens the tab again.
    click_file_row(&mut h, "Second", false);
    assert_eq!(h.state().tab_names(), ["Second"]);
    assert_eq!(h.state().doc_name(), Some("Second"));
    assert_eq!(h.state().diagram().shapes[0].label, "In second");
}

#[test]
fn close_from_the_files_menu_closes_the_tab_too() {
    let (_tmp, _paths, mut h) = two_diagrams();
    h.get_by_label("Second").click();
    h.run_steps(3);
    click_file_row(&mut h, "Second", true);
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert_eq!(h.state().file_names(), ["Diagram"]);
    assert_eq!(h.state().tab_names(), ["Diagram"]);
    assert_eq!(h.state().doc_name(), Some("Diagram"));
    // The diagram is still in the folder, and in the Project pane.
    assert_eq!(h.state().diagram_names(), ["Diagram", "Second"]);
}

#[test]
fn files_from_elsewhere_stay_open_when_the_folder_closes() {
    let (_tmp, _paths, mut h) = two_diagrams();
    let other = tempfile::tempdir().unwrap();
    let outside = other.path().join("Notes.mmd");
    fs::write(&outside, "flowchart LR\n  n[\"Note\"]\n").unwrap();
    h.state_mut().open_path(&outside);
    h.run_steps(3);
    assert_eq!(h.state().file_names(), ["Diagram", "Notes"]);
    assert_eq!(h.state().doc_name(), Some("Notes"));
    // Not one of the folder's: Rename and Delete stay off.
    h.key_press(Key::F2);
    h.run_steps(2);
    assert!(!h.state().has_overlay());

    click_tab(&mut h, "Diagram");
    h.get_by_label("Close Folder").click();
    h.run_steps(3);
    assert_eq!(h.state().file_names(), ["Notes"]);
    assert_eq!(h.state().tab_names(), ["Notes"]);
    assert_eq!(h.state().doc_name(), Some("Notes"));
}

#[test]
fn outside_changes_reach_a_background_tab() {
    let (_tmp, paths, mut h) = two_diagrams();
    let second = project(&paths).join("Second.mmd");
    h.get_by_label("Second").click();
    h.run_steps(3);
    click_tab(&mut h, "Diagram");

    write_outside(&second, "flowchart TD\n  t[\"Changed\"]\n");
    next_scan(&mut h);
    click_tab(&mut h, "Second");
    assert_eq!(h.state().diagram().shapes[0].label, "Changed");
    // The reload is an undoable step in its own history.
    assert!(h.state().can_undo());

    // Deleted while in the background: its tab closes, its row stays,
    // missing.
    click_tab(&mut h, "Diagram");
    fs::remove_file(&second).unwrap();
    next_scan(&mut h);
    assert_eq!(h.state().tab_names(), ["Diagram"]);
    assert_eq!(h.state().file_names(), ["Diagram", "Second"]);
    assert!(h.state().is_missing("Second"));
    assert_eq!(h.state().doc_name(), Some("Diagram"));
}

#[test]
fn the_files_and_tabs_come_back_after_a_restart() {
    let (tmp, paths, mut h) = two_diagrams();
    let other = tempfile::tempdir().unwrap();
    let outside = other.path().join("Notes.mmd");
    fs::write(&outside, "flowchart LR\n").unwrap();
    h.get_by_label("Second").click();
    h.run_steps(3);
    h.state_mut().open_path(&outside);
    h.run_steps(3);
    click_tab(&mut h, "Second");
    // Notes keeps its row but not its tab.
    h.state_mut().open_path(&outside);
    h.run_steps(2);
    h.state_mut().run(Act::CloseTab);
    h.run_steps(2);
    click_tab(&mut h, "Second");
    drop(h);

    let (_tmp, _paths, mut h) = app_harness(paths, tmp);
    h.run_steps(2);
    assert_eq!(h.state().file_names(), ["Diagram", "Second", "Notes"]);
    assert_eq!(h.state().tab_names(), ["Diagram", "Second"]);
    assert_eq!(h.state().doc_name(), Some("Second"));
}
