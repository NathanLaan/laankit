//! egui_kittest flows for the diagrams in a project (BlockLiner's editor in
//! a tab), against a project built with `noteliner-core` in a temp folder:
//!
//! - New Diagram creates a `.mmd` file and shows the canvas with BlockLiner's
//!   tools in the document toolbar;
//! - drawing with a toolbar button and with a tool key autosaves the file,
//!   and a tool key typed into a text field doesn't draw;
//! - a note tab and a diagram tab each keep their state, and the toolbar
//!   follows the active tab;
//! - Export writes an SVG.
//!
//!

use std::path::{Path, PathBuf};

use blockliner_egui::canvas::Tool;
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use noteliner_core::project::{FileKind, Project};
use serde_json::json;

use crate::app::NoteLinerApp;
use crate::commands::Act;
use crate::config::Paths;
use crate::ui_tests::{harness, wait_for};

type H = Harness<'static, NoteLinerApp>;

/// One box, as BlockLiner writes it.
const MAP: &str =
    "flowchart LR\n  api[\"API Gateway\"]\n%% blockliner 1 grid=20\n%% @shape api x=40 y=40 w=160 h=80\n";

/// A note (Alpha) and a diagram (Map).
fn fixture(dir: &Path) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        std::fs::create_dir_all(dir).unwrap();
        let mut p = Project::new();
        p.open(dir).await.unwrap();
        p.create_file("Alpha", &json!([]), Some("Alpha text\n".into()), None)
            .await
            .unwrap();
        p.create(FileKind::Diagram, "Map", &json!([]), Some(MAP.into()), None)
            .await
            .unwrap();
    });
}

fn open_project(tmp: &Path) -> (H, PathBuf) {
    let project = tmp.join("notes");
    std::fs::create_dir_all(&project).unwrap();
    fixture(&project);
    let mut h = harness(Paths {
        user_data: tmp.join("NoteLiner"),
    });
    h.run_steps(2);
    h.state_mut().open(project.clone());
    wait_for(&mut h, "the project", |a| a.entries().len() == 2);
    (h, project)
}

fn id_of(h: &H, name: &str) -> String {
    h.state()
        .entries()
        .iter()
        .find(|e| e.name == name)
        .map(|e| e.id.clone())
        .unwrap()
}

/// Chooses the diagram in the FILES tree and waits for the canvas.
fn open_diagram(h: &mut H, name: &str) {
    h.get_by_role_and_label(Role::TreeItem, name).click();
    let id = id_of(h, name);
    wait_for(h, name, move |a| {
        a.diagram_file_id().as_deref() == Some(id.as_str())
    });
    h.run_steps(2);
}

fn open_note(h: &mut H, name: &str) {
    h.get_by_role_and_label(Role::TreeItem, name).click();
    let id = id_of(h, name);
    wait_for(h, name, move |a| {
        a.editor_file_id().as_deref() == Some(id.as_str())
    });
}

fn act(h: &mut H, act: Act) {
    let ctx = h.ctx.clone();
    h.state_mut().apply_for_test(&ctx, act);
    h.run_steps(2);
}

/// A point on the canvas, from its top-left corner.
fn at(h: &H, x: f32, y: f32) -> Pos2 {
    h.state().diagram_editor().canvas.origin() + egui::vec2(x, y)
}

/// A press-move-release with the primary button, a frame per event.
fn drag(h: &mut H, from: Pos2, to: Pos2) {
    let press = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    for event in [
        Event::PointerMoved(from),
        press(from, true),
        Event::PointerMoved(from.lerp(to, 0.5)),
        Event::PointerMoved(to),
        press(to, false),
    ] {
        h.event(event);
        h.step();
    }
    h.run_steps(2);
}

/// Draws a shape with the active tool, then Enter to finish its label (a
/// new shape opens its label editor).
fn draw(h: &mut H, from: Pos2, to: Pos2) {
    drag(h, from, to);
    h.key_press(Key::Enter);
    h.run_steps(3);
}

fn shapes(h: &H) -> usize {
    h.state().diagram_editor().diagram.shapes.len()
}

fn wait_on_disk(h: &mut H, file: PathBuf, needle: &'static str) {
    wait_for(h, needle, move |_| {
        std::fs::read_to_string(&file).is_ok_and(|s| s.contains(needle))
    });
}

#[test]
fn new_diagram_opens_the_canvas_with_blockliners_tools() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());

    // New Diagram: the New Item dialog with Diagram chosen.
    act(&mut h, Act::NewDiagram);
    assert!(h.state().has_overlay());
    h.get_by_role(Role::TextInput).type_text("Sketch");
    h.step();
    h.get_by_label("OK").click();
    wait_for(&mut h, "the new diagram", |a| {
        a.entries().iter().any(|e| e.name == "Sketch") && a.diagram_file_id().is_some()
    });
    h.run_steps(2);

    // A `.mmd` file with no frontmatter, as BlockLiner would write it.
    let file = project.join("sketch.mmd");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("flowchart"), "{text:?}");
    assert!(!text.contains("---"));
    assert_eq!(h.state().selected(), Some(id_of(&h, "Sketch")));

    // The document toolbar carries BlockLiner's tools; the note editor and
    // its preview toggle are gone.
    for label in ["Select", "Rectangle", "Arrow", "Text", "Note"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    assert!(h.query_by_label("Show More Shapes").is_some());
    assert!(h.query_by_role(Role::MultilineTextInput).is_none());
    assert!(h
        .query_by_label(&laankit_egui::panes::heading(&h.ctx, "PROPERTIES"))
        .is_some());

    // Draw a rectangle with the toolbar button: it's written to the file.
    h.get_by_label("Rectangle").click();
    h.run_steps(2);
    assert_eq!(h.state().diagram_editor().tool, Tool::Rect);
    let (from, to) = (at(&h, 100.0, 100.0), at(&h, 260.0, 180.0));
    draw(&mut h, from, to);
    assert_eq!(shapes(&h), 1);
    assert_eq!(h.state().diagram_editor().tool, Tool::Select);
    wait_on_disk(&mut h, file.clone(), "@shape");
    assert!(!std::fs::read_to_string(&file).unwrap().starts_with("---"));
}

#[test]
fn tool_keys_draw_on_the_canvas_but_not_in_a_text_field() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    open_diagram(&mut h, "Map");
    assert_eq!(shapes(&h), 1);

    // R picks the Rectangle tool; a drag draws one.
    h.key_press(Key::R);
    h.run_steps(2);
    assert_eq!(h.state().diagram_editor().tool, Tool::Rect);
    let (from, to) = (at(&h, 300.0, 200.0), at(&h, 460.0, 280.0));
    draw(&mut h, from, to);
    assert_eq!(shapes(&h), 2);
    wait_for(&mut h, "the second shape on disk", {
        let file = project.join("map.mmd");
        move |_| std::fs::read_to_string(&file).is_ok_and(|s| s.matches("@shape").count() == 2)
    });

    // Ctrl+Z undoes it; Ctrl+Shift+Z brings it back.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(shapes(&h), 1);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.run_steps(2);
    assert_eq!(shapes(&h), 2);

    // With the Search field focused, R is a letter, not a tool.
    act(&mut h, Act::ToggleSearch);
    h.run_steps(2);
    h.get_by_label("Search all files").focus();
    h.run_steps(2);
    h.key_press(Key::R);
    h.event(Event::Text("r".into()));
    h.run_steps(2);
    assert_eq!(h.state().diagram_editor().tool, Tool::Select);
    assert_eq!(shapes(&h), 2);
}

#[test]
fn a_note_tab_and_a_diagram_tab_keep_their_state() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, _project) = open_project(tmp.path());
    open_diagram(&mut h, "Map");
    h.key_press(Key::R);
    h.run_steps(2);
    let (from, to) = (at(&h, 300.0, 200.0), at(&h, 460.0, 280.0));
    draw(&mut h, from, to);
    assert_eq!(shapes(&h), 2);

    // The note's tab: the note editor and its toolbar; no diagram tools.
    open_note(&mut h, "Alpha");
    assert_eq!(h.state().tab_ids().len(), 2);
    assert_eq!(h.state().diagram_file_id(), None);
    assert!(h.query_by_role(Role::MultilineTextInput).is_some());
    assert!(h.query_by_label("Rectangle").is_none());
    assert!(h
        .query_by_label(&laankit_egui::panes::heading(&h.ctx, "PROPERTIES"))
        .is_none());
    // A tool key does nothing to the diagram now.
    h.key_press(Key::R);
    h.run_steps(2);
    assert_eq!(h.state().diagram_editor().tool, Tool::Select);

    // Back on the diagram's tab: both shapes, and the drawing can be undone.
    h.get_by_role_and_label(Role::Tab, "Map").click();
    let map = id_of(&h, "Map");
    wait_for(&mut h, "Map's tab", move |a| {
        a.diagram_file_id().as_deref() == Some(map.as_str())
    });
    h.run_steps(2);
    assert_eq!(shapes(&h), 2);
    assert!(h.query_by_label("Rectangle").is_some());
    assert!(h.state().editor_file_id().is_none());
    act(&mut h, Act::Diagram(blockliner_egui::EditAct::Undo));
    assert_eq!(shapes(&h), 1);
}

#[test]
fn export_writes_an_svg() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, _project) = open_project(tmp.path());
    let out = tmp.path().join("exports");
    h.state_mut().set_diagram_export_dir(out.clone());
    open_diagram(&mut h, "Map");

    act(&mut h, Act::ExportDiagram);
    assert!(h.state().has_overlay());
    h.get_by_label("SVG").click();
    h.run_steps(2);
    h.get_by_label("Export…").click();
    h.run_steps(3);
    assert!(!h.state().has_overlay());
    let svg = std::fs::read_to_string(out.join("map.svg")).unwrap();
    assert!(svg.contains("<svg") && svg.contains("API Gateway"), "{svg}");
}

#[test]
fn the_toolbar_moves_a_shape_behind_another_and_saves_the_order() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    open_diagram(&mut h, "Map");
    let order = |h: &H| -> Vec<String> {
        let d = &h.state().diagram_editor().diagram;
        d.shapes.iter().map(|s| s.id.clone()).collect()
    };
    // With nothing selected the four buttons are off.
    for name in ["Move to Front", "Move Up", "Move Down", "Move to Back"] {
        let button = h.get_by_role_and_label(Role::Button, name);
        assert!(button.accesskit_node().is_disabled(), "{name}");
    }

    // A second box drawn over the first (40, 40, 160 by 80) is in front of
    // it, and selected.
    h.key_press(Key::R);
    h.run_steps(2);
    let (from, to) = (at(&h, 120.0, 80.0), at(&h, 280.0, 160.0));
    draw(&mut h, from, to);
    let before = order(&h);
    assert_eq!(before.len(), 2);
    assert_eq!(before[0], "api");
    let file = project.join("map.mmd");
    wait_for(&mut h, "the second shape on disk", {
        let file = file.clone();
        move |_| std::fs::read_to_string(&file).is_ok_and(|s| s.matches("@shape").count() == 2)
    });
    let up = h.get_by_role_and_label(Role::Button, "Move Up");
    assert!(up.accesskit_node().is_disabled(), "already in front");

    h.get_by_role_and_label(Role::Button, "Move to Back")
        .click();
    h.run_steps(3);
    assert_eq!(order(&h), [before[1].clone(), "api".to_string()]);
    // The autosave writes the new order: api's place is now the last.
    wait_for(&mut h, "the new order on disk", {
        let file = file.clone();
        move |_| {
            std::fs::read_to_string(&file).is_ok_and(|s| {
                s.lines()
                    .rfind(|l| l.starts_with("%% @shape "))
                    .is_some_and(|l| l.starts_with("%% @shape api "))
            })
        }
    });

    // The keys work in a diagram tab, and Ctrl+Z undoes a move.
    h.key_press_modifiers(Modifiers::COMMAND, Key::CloseBracket);
    h.run_steps(2);
    assert_eq!(order(&h), before);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(order(&h), [before[1].clone(), "api".to_string()]);
}

#[test]
fn a_diagram_changed_on_disk_reloads_as_one_undo_step() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    let file = project.join("map.mmd");
    let original = std::fs::read_to_string(&file).unwrap();
    open_diagram(&mut h, "Map");
    h.key_press(Key::R);
    h.run_steps(2);
    let (from, to) = (at(&h, 300.0, 200.0), at(&h, 460.0, 280.0));
    draw(&mut h, from, to);
    assert_eq!(shapes(&h), 2);
    let (written, before) = (file.clone(), original.clone());
    wait_for(&mut h, "the drawing on disk", move |a| {
        a.diagram_status() == crate::editor::SaveStatus::Saved
            && std::fs::read_to_string(&written).is_ok_and(|s| s != before)
    });

    // Another program puts the file back as it was.
    std::fs::write(&file, &original).unwrap();
    h.state_mut().check_now();
    wait_for(&mut h, "the reload", |a| {
        a.diagram_editor().diagram.shapes.len() == 1
    });
    h.run_steps(2);
    assert_eq!(
        h.state().toast_texts(),
        ["“Map” changed on disk and was reloaded."]
    );
    // Undo shows the diagram as it was, and the drawing can still be undone.
    act(&mut h, Act::Undo);
    assert_eq!(shapes(&h), 2);
    assert!(h.state().diagram_editor().view().can_undo);
    act(&mut h, Act::Undo);
    assert_eq!(shapes(&h), 1);
}

/// The diagram as another program leaves it: the box, and one of its own.
const THEIRS: &str = "flowchart LR\n  api[\"API Gateway\"]\n  db[\"Their Database\"]\n%% blockliner 1 grid=20\n%% @shape api x=40 y=40 w=160 h=80\n%% @shape db x=40 y=200 w=160 h=80\n";

/// Another program writes `text` over the diagram's file.
fn write_outside(file: &Path, text: &str) {
    std::fs::write(file, text).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    let f = std::fs::File::options().write(true).open(file).unwrap();
    f.set_modified(later).unwrap();
}

/// Drags out a box with the Rectangle tool, and leaves its label open:
/// nothing is saved while it is.
fn start_box(h: &mut H, x: f32) {
    act(
        h,
        Act::Diagram(blockliner_egui::commands::EditAct::Tool(Tool::Rect)),
    );
    assert_eq!(h.state().diagram_editor().tool, Tool::Rect);
    let (from, to) = (at(h, x, 300.0), at(h, x + 100.0, 360.0));
    drag(h, from, to);
}

/// Another program writes `text` while a box is being drawn, so the app
/// has no chance to look at the file first. The box is then finished.
fn draw_box_over(h: &mut H, x: f32, file: &Path, text: &str) {
    start_box(h, x);
    write_outside(file, text);
    h.key_press(Key::Enter);
    h.run_steps(3);
}

fn settled(h: &mut H) {
    wait_for(h, "the save to settle", |a| {
        a.diagram_status() == crate::editor::SaveStatus::Saved
    });
}

fn on_disk(h: &mut H, what: &str, file: &Path, test: impl Fn(&str) -> bool + 'static) {
    let file = file.to_path_buf();
    wait_for(h, what, move |_| {
        std::fs::read_to_string(&file).is_ok_and(|s| test(&s))
    });
}

/// A diagram's save looks at the file first, and asks before it writes over
/// a change another program made.
#[test]
fn a_diagram_save_does_not_write_over_another_programs_change() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    let file = project.join("map.mmd");
    open_note(&mut h, "Alpha");
    open_diagram(&mut h, "Map");

    // The file changed, and the autosave runs with no check before it.
    draw_box_over(&mut h, 300.0, &file, THEIRS);
    assert_eq!(shapes(&h), 2);
    wait_for(&mut h, "the question", |a| a.has_overlay());
    h.run_steps(3);
    assert!(h
        .query_by_label_contains("was changed by another program")
        .is_some());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), THEIRS);
    assert_eq!(
        h.state().diagram_status(),
        crate::editor::SaveStatus::Unsaved
    );
    // Nothing is written while it waits for an answer.
    for _ in 0..40 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), THEIRS);
    // Keep Mine writes the diagram over the file.
    h.get_by_label("Keep Mine").click();
    on_disk(&mut h, "my diagram on disk", &file, |s| {
        !s.contains("Their Database") && s.matches("@shape").count() == 2
    });
    settled(&mut h);
    assert!(!h.state().has_overlay());

    // Again, and Reload takes the file as one undo step.
    draw_box_over(&mut h, 450.0, &file, THEIRS);
    assert_eq!(shapes(&h), 3);
    wait_for(&mut h, "the question again", |a| a.has_overlay());
    h.run_steps(3);
    h.get_by_label("Reload").click();
    h.run_steps(3);
    assert!(!h.state().has_overlay());
    assert_eq!(shapes(&h), 2);
    settled(&mut h);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), THEIRS);
    act(&mut h, Act::Undo);
    assert_eq!(shapes(&h), 3);
    on_disk(&mut h, "the undo on disk", &file, |s| {
        s.matches("@shape").count() == 3
    });
    settled(&mut h);

    // A file that isn't a diagram: Reload is refused, and the question
    // stays for Keep Mine.
    draw_box_over(&mut h, 600.0, &file, "# Not a diagram\n");
    wait_for(&mut h, "the question for a file that can't be read", |a| {
        a.has_overlay()
    });
    h.run_steps(3);
    h.get_by_label("Reload").click();
    h.run_steps(3);
    assert!(h.state().has_overlay());
    assert!(h
        .state()
        .toast_texts()
        .iter()
        .any(|t| t.starts_with("Couldn't reload “Map”")));
    assert_eq!(shapes(&h), 4);
    h.get_by_label("Keep Mine").click();
    on_disk(&mut h, "my diagram over the bad file", &file, |s| {
        s.matches("@shape").count() == 4
    });
    settled(&mut h);
}

/// A save refused as the diagram's tab is left asks when the tab is shown.
/// (These start from a project just opened, so the save runs before the
/// app's own look at the files.)
#[test]
fn a_diagram_tab_that_is_left_asks_on_return() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    let file = project.join("map.mmd");
    open_note(&mut h, "Alpha");
    open_diagram(&mut h, "Map");
    start_box(&mut h, 300.0);
    write_outside(&file, THEIRS);
    let ctx = h.ctx.clone();
    h.state_mut().apply_for_test(&ctx, Act::NextTab);
    let alpha = id_of(&h, "Alpha");
    wait_for(&mut h, "Alpha", move |a| {
        a.editor_file_id().as_deref() == Some(alpha.as_str())
    });
    for _ in 0..40 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!h.state().has_overlay());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), THEIRS);
    open_diagram(&mut h, "Map");
    wait_for(&mut h, "the question on return", |a| a.has_overlay());
    h.run_steps(3);
    assert_eq!(shapes(&h), 2);
    h.get_by_label("Reload").click();
    h.run_steps(3);
    assert_eq!(shapes(&h), 2);
    settled(&mut h);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), THEIRS);
    let theirs = h.state().diagram_editor().diagram.shapes.iter();
    assert!(theirs.clone().any(|s| s.label == "Their Database"));
}

/// A save refused as the diagram's tab is closed has nowhere to ask: the
/// diagram is written, and the log says so.
#[test]
fn a_diagram_tab_that_is_closed_keeps_your_version() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    let file = project.join("map.mmd");
    open_diagram(&mut h, "Map");
    start_box(&mut h, 300.0);
    write_outside(&file, THEIRS);
    let ctx = h.ctx.clone();
    h.state_mut().apply_for_test(&ctx, Act::CloseTab);
    on_disk(&mut h, "the closed tab's diagram", &file, |s| {
        !s.contains("Their Database") && s.matches("@shape").count() == 2
    });
    assert!(h
        .state()
        .log_texts()
        .iter()
        .any(|l| l.contains("Your version was kept")));
}

/// The check finds the change while a shape is being drawn: it asks once
/// the shape is done, where it used to take the file without a word.
#[test]
fn a_diagram_changed_under_unsaved_edits_asks() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    let file = project.join("map.mmd");
    open_diagram(&mut h, "Map");
    // The new shape's label is still being typed.
    h.key_press(Key::R);
    h.run_steps(2);
    let (from, to) = (at(&h, 300.0, 300.0), at(&h, 400.0, 360.0));
    drag(&mut h, from, to);
    assert_eq!(shapes(&h), 2);
    write_outside(&file, THEIRS);
    h.state_mut().check_now();
    for _ in 0..40 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!h.state().has_overlay(), "it waits for the label");
    assert_eq!(shapes(&h), 2);
    h.key_press(Key::Enter);
    wait_for(&mut h, "the question", |a| a.has_overlay());
    h.run_steps(3);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), THEIRS);
    h.get_by_label("Reload").click();
    h.run_steps(3);
    settled(&mut h);
    assert!(h
        .state()
        .diagram_editor()
        .diagram
        .shapes
        .iter()
        .any(|s| s.label == "Their Database"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), THEIRS);
}

/// The copies of your edits in the project folder.
fn copies(project: &Path) -> Vec<String> {
    std::fs::read_dir(project)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.contains("(your edits "))
        .collect()
}

/// Quitting does not write a diagram over another program's change: the
/// file is left as it is, and your diagram is written beside it.
#[test]
fn quitting_over_a_changed_diagram_keeps_both() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    let file = project.join("map.mmd");
    open_diagram(&mut h, "Map");
    start_box(&mut h, 300.0);
    write_outside(&file, THEIRS);
    h.state_mut().shutdown();
    drop(h);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), THEIRS);
    let made = copies(&project);
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(made[0].starts_with("map (your edits ") && made[0].ends_with(").mmd"));
    let mine = std::fs::read_to_string(project.join(&made[0])).unwrap();
    assert!(!mine.contains("Their Database"));
    assert_eq!(mine.matches("@shape").count(), 2);
}

/// The same for a diagram in a background tab that is waiting to ask.
#[test]
fn quitting_keeps_a_waiting_diagram_tabs_edits() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    let file = project.join("map.mmd");
    open_note(&mut h, "Alpha");
    open_diagram(&mut h, "Map");
    start_box(&mut h, 300.0);
    write_outside(&file, THEIRS);
    let ctx = h.ctx.clone();
    h.state_mut().apply_for_test(&ctx, Act::NextTab);
    let alpha = id_of(&h, "Alpha");
    wait_for(&mut h, "Alpha", move |a| {
        a.editor_file_id().as_deref() == Some(alpha.as_str())
    });
    for _ in 0..40 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), THEIRS);
    h.state_mut().shutdown();
    drop(h);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), THEIRS);
    let made = copies(&project);
    assert_eq!(made.len(), 1, "{made:?}");
    let mine = std::fs::read_to_string(project.join(&made[0])).unwrap();
    assert!(!mine.contains("Their Database"));
    assert_eq!(mine.matches("@shape").count(), 2);
}

/// The backend has found a change to a diagram, and the app has not heard
/// of it yet. A save sent in that gap is not written over the change.
#[test]
fn a_diagram_save_sent_before_the_app_hears_of_a_change_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    let file = project.join("map.mmd");
    open_note(&mut h, "Alpha");
    open_diagram(&mut h, "Map");

    // As the tab is left: it asks on return.
    start_box(&mut h, 300.0);
    write_outside(&file, THEIRS);
    h.state_mut().check_unheard_for_test();
    let ctx = h.ctx.clone();
    h.state_mut().apply_for_test(&ctx, Act::NextTab);
    let alpha = id_of(&h, "Alpha");
    wait_for(&mut h, "Alpha", move |a| {
        a.editor_file_id().as_deref() == Some(alpha.as_str())
    });
    for _ in 0..20 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), THEIRS);
    open_diagram(&mut h, "Map");
    wait_for(&mut h, "the question on return", |a| a.has_overlay());
    h.run_steps(3);
    assert_eq!(shapes(&h), 2);
    h.get_by_label("Reload").click();
    h.run_steps(3);
    settled(&mut h);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), THEIRS);

    // On quit: both versions are kept.
    const THEIRS_AGAIN: &str = "flowchart LR\n  api[\"API Gateway\"]\n  q[\"Their Queue\"]\n%% blockliner 1 grid=20\n%% @shape api x=40 y=40 w=160 h=80\n%% @shape q x=40 y=200 w=160 h=80\n";
    start_box(&mut h, 500.0);
    write_outside(&file, THEIRS_AGAIN);
    h.state_mut().check_unheard_for_test();
    h.state_mut().shutdown();
    drop(h);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), THEIRS_AGAIN);
    let made = copies(&project);
    assert_eq!(made.len(), 1, "{made:?}");
    let mine = std::fs::read_to_string(project.join(&made[0])).unwrap();
    assert!(mine.contains("Their Database") && !mine.contains("Their Queue"));
    assert_eq!(mine.matches("@shape").count(), 3);
}

/// Two boxes with a labelled arrow between them, and a third far off to
/// the right and down.
const FLOW: &str = concat!(
    "flowchart LR\n",
    "  api[\"API Gateway\"]\n",
    "  db[(\"Orders\")]\n",
    "  far[\"Far Away\"]\n",
    "  api -->|reads| db\n",
    "%% blockliner 1 grid=20\n",
    "%% @shape api x=40 y=40 w=160 h=80\n",
    "%% @shape db x=300 y=40 w=120 h=80\n",
    "%% @shape far x=5000 y=3000 w=160 h=80\n",
);

/// The project with `FLOW` as the Map diagram, the diagram open, and the
/// Outline pane shown.
fn open_flow(tmp: &Path) -> (H, PathBuf) {
    let project = tmp.join("notes");
    std::fs::create_dir_all(&project).unwrap();
    fixture(&project);
    let file = project.join("map.mmd");
    assert!(file.exists());
    std::fs::write(&file, FLOW).unwrap();
    let mut h = harness(Paths {
        user_data: tmp.join("NoteLiner"),
    });
    h.run_steps(2);
    h.state_mut().open(project.clone());
    wait_for(&mut h, "the project", |a| a.entries().len() == 2);
    open_diagram(&mut h, "Map");
    act(&mut h, Act::ToggleOutline);
    (h, file)
}

/// Folds the Shapes group, or opens it again.
fn fold_shapes(h: &mut H) {
    h.get_by_role_and_label(Role::Button, "Shapes (3)").click();
    h.run_steps(2);
}

fn selection(h: &H) -> (Vec<String>, Vec<usize>) {
    let c = &h.state().diagram_editor().canvas;
    (c.selection.clone(), c.lines.clone())
}

#[test]
fn the_outline_pane_lists_a_diagrams_objects_and_a_notes_headings() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, _file) = open_flow(tmp.path());

    assert!(h
        .query_by_role_and_label(Role::Heading, "OBJECTS")
        .is_some());
    assert!(h
        .query_by_role_and_label(Role::Heading, "OUTLINE")
        .is_none());
    for group in ["Shapes (3)", "Lines (1)"] {
        assert!(
            h.query_by_role_and_label(Role::Button, group).is_some(),
            "{group}"
        );
    }
    // No text object: no Text group.
    assert!(h.query_by_label_contains("Text (").is_none());
    // Front to back: the last one made is first.
    for row in ["Far Away", "Orders", "API Gateway"] {
        assert!(
            h.query_by_role_and_label(Role::ListItem, row).is_some(),
            "{row}"
        );
    }

    // Folding a group hides its rows, and brings the lines up into the
    // pane: only the rows in view are drawn.
    fold_shapes(&mut h);
    assert!(h
        .query_by_role_and_label(Role::ListItem, "Orders")
        .is_none());
    assert!(h
        .query_by_role_and_label(Role::ListItem, "API Gateway to Orders: reads")
        .is_some());

    // A note's tab: the same pane, with its headings.
    open_note(&mut h, "Alpha");
    h.run_steps(2);
    assert!(h
        .query_by_role_and_label(Role::Heading, "OUTLINE")
        .is_some());
    assert!(h
        .query_by_role_and_label(Role::Heading, "OBJECTS")
        .is_none());
    assert!(h.query_by_label("No headings").is_some());
}

#[test]
fn a_row_selects_on_the_canvas_and_is_not_an_edit() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, file) = open_flow(tmp.path());

    h.get_by_role_and_label(Role::ListItem, "Orders").click();
    h.run_steps(2);
    assert_eq!(selection(&h), (vec!["db".to_string()], vec![]));

    // Ctrl+click adds a line, and again takes it out.
    fold_shapes(&mut h);
    let line = "API Gateway to Orders: reads";
    h.get_by_role_and_label(Role::ListItem, line)
        .click_modifiers(Modifiers::COMMAND);
    h.run_steps(2);
    assert_eq!(selection(&h), (vec!["db".to_string()], vec![0]));
    h.get_by_role_and_label(Role::ListItem, line)
        .click_modifiers(Modifiers::COMMAND);
    h.run_steps(2);
    assert_eq!(selection(&h), (vec!["db".to_string()], vec![]));

    // Shift+click: from the top row to the row two below it.
    fold_shapes(&mut h);
    h.get_by_role_and_label(Role::ListItem, "Far Away").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::ListItem, "API Gateway")
        .click_modifiers(Modifiers::SHIFT);
    h.run_steps(2);
    assert_eq!(selection(&h).0, ["far", "db", "api"]);

    // Nothing was edited: no undo step, and the file is as it was.
    assert!(!h.state().diagram_editor().history.can_undo());
    assert!(!h.state().diagram_is_dirty_for_test());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), FLOW);
}

#[test]
fn a_row_brings_an_object_into_view_only_when_it_is_out_of_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, _file) = open_flow(tmp.path());
    let pan = |h: &H| h.state().diagram_editor().canvas.view.pan;
    let start = pan(&h);

    // In view: the canvas does not move.
    h.get_by_role_and_label(Role::ListItem, "API Gateway")
        .click();
    h.run_steps(2);
    assert_eq!(pan(&h), start);

    // Far off: it is brought to the middle, at the same zoom.
    h.get_by_role_and_label(Role::ListItem, "Far Away").click();
    h.run_steps(2);
    assert_ne!(pan(&h), start);
    let seen = h.state().diagram_editor().canvas.visible();
    let centre = (seen.x + seen.w / 2.0, seen.y + seen.h / 2.0);
    assert!((centre.0 - 5080.0).abs() < 1.0 && (centre.1 - 3040.0).abs() < 1.0);
    assert_eq!(h.state().diagram_editor().canvas.view.zoom, 1.0);
}

#[test]
fn selecting_on_the_canvas_marks_the_row() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, _file) = open_flow(tmp.path());
    let marked = |h: &H, name: &str| {
        h.get_by_role_and_label(Role::ListItem, name)
            .accesskit_node()
            .toggled()
            == Some(egui::accesskit::Toggled::True)
    };
    assert!(!marked(&h, "API Gateway"));

    // A click in the middle of the first box.
    let p = at(&h, 120.0, 80.0);
    drag(&mut h, p, p);
    assert_eq!(selection(&h).0, ["api"]);
    assert!(marked(&h, "API Gateway"));
    assert!(!marked(&h, "Orders"));
}

/// The saved preferences, as JSON.
fn saved_prefs(tmp: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(tmp.join("NoteLiner/ui-preferences.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn the_editor_font_is_chosen_in_settings_and_saved() {
    use laankit_egui::fonts::{self, EditorFont};
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, _project) = open_project(tmp.path());
    open_note(&mut h, "Alpha");
    assert_eq!(fonts::editor_font(&h.ctx), EditorFont::Inter);

    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "Editor").click();
    h.run_steps(3);
    assert!(h.query_by_label("Check spelling").is_some());
    h.get_by_role_and_label(Role::ComboBox, "Editor font")
        .click();
    h.run_steps(3);
    // Every font, under a heading for each kind.
    assert!(h.query_by_label("Proportional").is_some());
    assert!(h.query_by_label("Monospace").is_some());
    for font in EditorFont::ALL {
        assert!(
            h.query_by_role_and_label(Role::ListBoxOption, font.name())
                .is_some(),
            "{font:?}"
        );
    }
    h.get_by_role_and_label(Role::ListBoxOption, "IBM Plex Mono")
        .click();
    h.run_steps(3);
    assert_eq!(fonts::editor_font(&h.ctx), EditorFont::PlexMono);
    let saved = saved_prefs(tmp.path());
    assert_eq!(saved["editorFont"], "plex-mono");
    // The other preferences are kept, and the diagram font is its own.
    assert_eq!(saved["spellCheckEnabled"], true);
    assert_eq!(fonts::diagram_font(&h.ctx), EditorFont::Inter);
    h.key_press(Key::Escape);
    h.run_steps(3);

    // The note is unchanged, and still editable.
    assert_eq!(h.state().editor_text(), "Alpha text\n");

    // The palette's commands set it too.
    act(&mut h, Act::EditorFont(EditorFont::IaQuattro));
    assert_eq!(fonts::editor_font(&h.ctx), EditorFont::IaQuattro);
    assert_eq!(saved_prefs(tmp.path())["editorFont"], "ia-quattro");
}

#[test]
fn the_diagram_font_changes_the_canvas_and_exports_but_no_file() {
    use laankit_egui::fonts::{self, EditorFont};
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    let out = tmp.path().join("exports");
    h.state_mut().set_diagram_export_dir(out.clone());
    open_diagram(&mut h, "Map");
    let file = std::fs::read_dir(&project)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "mmd"))
        .expect("the diagram's file");
    let before = std::fs::read_to_string(&file).unwrap();

    // Settings offers the fonts about as wide as Inter, and no others.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "Diagrams").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::ComboBox, "Diagram font")
        .scroll_to_me();
    h.run_steps(3);
    h.get_by_role_and_label(Role::ComboBox, "Diagram font")
        .click();
    h.run_steps(3);
    assert!(h.query_by_label("IBM Plex Mono").is_none());
    assert!(
        h.query_by_label("Monospace").is_none(),
        "one kind: no headings"
    );
    h.get_by_role_and_label(Role::ListBoxOption, "Source Serif 4")
        .click();
    h.run_steps(3);
    assert_eq!(fonts::diagram_font(&h.ctx), EditorFont::SourceSerif);
    assert_eq!(fonts::editor_font(&h.ctx), EditorFont::Inter);
    assert_eq!(saved_prefs(tmp.path())["diagramFont"], "source-serif");
    h.key_press(Key::Escape);
    h.run_steps(3);

    // An export is in the font.
    act(&mut h, Act::ExportDiagram);
    h.get_by_label("SVG").click();
    h.run_steps(2);
    h.get_by_label("Export…").click();
    h.run_steps(3);
    let svg = std::fs::read_to_string(out.join("map.svg")).unwrap();
    assert!(
        svg.contains(r#"font-family="Source Serif 4, serif""#),
        "{svg}"
    );

    // Looking at a diagram in another font does not change its file.
    h.run_steps(60);
    std::thread::sleep(std::time::Duration::from_millis(100));
    h.run_steps(5);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);

    act(&mut h, Act::DiagramFont(EditorFont::Inter));
    assert_eq!(fonts::diagram_font(&h.ctx), EditorFont::Inter);
    assert_eq!(saved_prefs(tmp.path())["diagramFont"], "inter");
}

#[test]
fn the_editor_text_size_and_line_height_are_saved() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, _project) = open_project(tmp.path());
    open_note(&mut h, "Alpha");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "Editor").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::ComboBox, "Editor font size")
        .click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::ListBoxOption, "18 px")
        .click();
    h.run_steps(3);
    assert_eq!(saved_prefs(tmp.path())["editorFontSize"], 18);
    h.get_by_role_and_label(Role::ComboBox, "Editor line height")
        .click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::ListBoxOption, "1.6").click();
    h.run_steps(3);
    assert_eq!(saved_prefs(tmp.path())["editorLineHeight"], 1.6);
    // Back to the font's own line height.
    h.get_by_role_and_label(Role::ComboBox, "Editor line height")
        .click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::ListBoxOption, "The font's own")
        .click();
    h.run_steps(3);
    assert!(saved_prefs(tmp.path())["editorLineHeight"].is_null());
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert_eq!(h.state().editor_text(), "Alpha text\n");
}
