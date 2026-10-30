//! The folder of diagrams, headlessly: opening the last folder and diagram,
//! the autosave (once a change is finished, debounced, flushed when
//! switching), new/rename/delete from the keyboard, sidebar and dialogs,
//! and outside changes (reloaded as an undoable step; a deleted file closes
//! the diagram).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use blockliner_core::{Rect, Shape, ShapeKind};
use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;

use crate::config::Paths;
use crate::ui_tests::{app_harness, at, drag, harness, H};

fn project(paths: &Paths) -> PathBuf {
    paths.prefs().last_project.expect("a project")
}

fn read(folder: &Path, name: &str) -> String {
    fs::read_to_string(folder.join(format!("{name}.mmd"))).unwrap()
}

/// Steps until the autosave has written (real time: it runs on a thread).
fn wait_saved(h: &mut H) {
    for _ in 0..400 {
        h.step();
        if !h.state().is_saving() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the autosave never wrote");
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

/// Past the once-a-second look at the folder (at 60 frames a second).
fn next_scan(h: &mut H) {
    h.run_steps(70);
}

fn type_name(h: &mut H, name: &str) {
    h.event(Event::Text(name.into()));
    h.step();
    h.key_press(Key::Enter);
    h.run_steps(3);
}

#[test]
fn opens_the_last_folder_and_autosaves_finished_changes() {
    let (_tmp, paths, mut h) = harness();
    h.state_mut().set_save_delay(Duration::from_millis(30));
    h.run_steps(2);
    let folder = project(&paths);
    assert_eq!(h.state().doc_name(), Some("Diagram"));
    assert_eq!(h.state().window_title(), "Diagram — BlockLiner");
    assert!(!h.state().is_saving(), "opening writes nothing");

    // Draw a rectangle: nothing is saved mid-drag, then the whole shape is.
    h.key_press(Key::R);
    h.run_steps(2);
    drag(
        &mut h,
        PointerButton::Primary,
        at(100.0, 100.0),
        at(260.0, 160.0),
    );
    h.run_steps(2);
    h.event(Event::Text("Orders".into()));
    h.step();
    assert!(!h.state().is_saving(), "still naming it");
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert!(h.state().is_saving());
    wait_saved(&mut h);
    let text = read(&folder, "Diagram");
    assert!(text.contains("orders[\"Orders\"]"), "{text}");
    assert!(
        text.contains("%% @shape orders x=100 y=100 w=160 h=60"),
        "{text}"
    );
}

#[test]
fn new_rename_and_delete() {
    let (_tmp, paths, mut h) = harness();
    h.run_steps(2);
    let folder = project(&paths);

    // Ctrl+N suggests "Untitled", selected: typing replaces it.
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run_steps(3);
    assert!(h.state().has_overlay());
    type_name(&mut h, "Orders");
    assert!(!h.state().has_overlay());
    assert_eq!(h.state().diagram_names(), ["Diagram", "Orders"]);
    assert_eq!(h.state().doc_name(), Some("Orders"));
    assert!(read(&folder, "Orders").contains("%% blockliner 1 grid=20"));
    assert!(h.state().diagram().shapes.is_empty());

    // F2 renames; a taken name keeps the dialog open with the reason.
    add_shape(&mut h, "x");
    h.key_press(Key::F2);
    h.run_steps(3);
    type_name(&mut h, "diagram");
    assert!(h.state().has_overlay(), "taken (case-insensitively)");
    h.get_by_label("There's already a diagram with that name.");
    type_name(&mut h, "Payments");
    assert!(!h.state().has_overlay());
    assert_eq!(h.state().diagram_names(), ["Diagram", "Payments"]);
    assert_eq!(h.state().doc_name(), Some("Payments"));
    assert!(!folder.join("Orders.mmd").exists());
    assert!(
        read(&folder, "Payments").contains("x[\"x\"]"),
        "edits waiting for the old name moved with it"
    );
    assert_eq!(paths.prefs().last_diagram.as_deref(), Some("Payments"));

    // Delete (from the palette), confirmed: its neighbour opens.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(3);
    h.event(Event::Text("Delete Diagram".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    h.get_by_label("Delete").click();
    h.run_steps(3);
    assert_eq!(h.state().diagram_names(), ["Diagram"]);
    assert!(!folder.join("Payments.mmd").exists());
    assert_eq!(h.state().doc_name(), Some("Diagram"));
}

#[test]
fn the_sidebar_switches_diagrams_and_writes_edits_first() {
    let (_tmp, paths, mut h) = harness();
    let folder = project(&paths);
    fs::write(
        folder.join("Second.mmd"),
        "flowchart TD\n  s[\"In second\"]\n",
    )
    .unwrap();
    h.run_steps(2);
    // Ctrl+E shows the sidebar (the harness hides it).
    h.key_press_modifiers(Modifiers::COMMAND, Key::E);
    h.run_steps(2);
    next_scan(&mut h);
    assert_eq!(h.state().diagram_names(), ["Diagram", "Second"]);

    add_shape(&mut h, "kept");
    assert!(h.state().is_saving());
    h.get_by_label("Second").click();
    h.run_steps(3);
    assert_eq!(h.state().doc_name(), Some("Second"));
    assert_eq!(h.state().diagram().shapes[0].label, "In second");
    assert!(read(&folder, "Diagram").contains("kept[\"kept\"]"));
    // Opening a hand-written file doesn't rewrite it.
    assert!(!h.state().is_saving());
    assert_eq!(
        read(&folder, "Second"),
        "flowchart TD\n  s[\"In second\"]\n"
    );
    // Nothing to undo in a diagram just opened.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().diagram().shapes.len(), 1);
}

#[test]
fn outside_changes_reload_and_undo() {
    let (_tmp, paths, mut h) = harness();
    h.run_steps(2);
    let path = project(&paths).join("Diagram.mmd");
    write_outside(&path, "flowchart LR\n  ext[\"From outside\"]\n");
    next_scan(&mut h);
    assert_eq!(h.state().diagram().shapes[0].id, "ext");
    assert!(!h.state().is_saving(), "a reload isn't written back");

    // The reload is one undo step; undoing it saves the old diagram.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert!(h.state().diagram().shapes.is_empty());
    h.state().flush();
    assert!(!fs::read_to_string(&path).unwrap().contains("ext"));

    // A deleted file closes the diagram.
    fs::remove_file(&path).unwrap();
    next_scan(&mut h);
    assert_eq!(h.state().doc_name(), None);
    assert!(h.state().diagram_names().is_empty());
    h.get_by_label("No diagram open");
}

#[test]
fn without_a_folder_and_reopening() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths {
        user_data: tmp.path().join("BlockLiner"),
    };
    let (tmp, paths, mut h) = app_harness(paths, tmp);
    h.run_steps(2);
    assert_eq!(h.state().doc_name(), None);
    h.get_by_label("No folder open");
    h.get_by_label("Open Folder…");
    // No diagram: Ctrl+N has nowhere to go.
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run_steps(2);
    assert!(!h.state().has_overlay());

    // Open a folder (as the folder picker would) and pick its second diagram;
    // the next start reopens both.
    let folder = tmp.path().join("diagrams");
    fs::create_dir(&folder).unwrap();
    for name in ["a", "b"] {
        fs::write(folder.join(format!("{name}.mmd")), "flowchart TD\n").unwrap();
    }
    h.state_mut().open_project(folder.clone(), Some("b".into()));
    h.run_steps(2);
    assert_eq!(h.state().doc_name(), Some("b"));
    drop(h);
    let (_tmp, _paths, mut h) = app_harness(paths, tmp);
    h.run_steps(2);
    assert_eq!(h.state().doc_name(), Some("b"));
    assert_eq!(h.state().diagram_names(), ["a", "b"]);
}
