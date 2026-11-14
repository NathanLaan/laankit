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
//! Needs `git` on the PATH, like the app.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use blockliner_egui::canvas::Tool;
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use noteliner_core::git::NoteGit;
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
        let mut p = Project::new(NoteGit::new(Arc::new(|_| {})));
        p.init(dir, None).await.unwrap();
        p.set_git_config("Test User", "test@localhost")
            .await
            .unwrap();
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

    // New Diagram: the New File dialog with Diagram chosen.
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
    assert!(h.query_by_label("PROPERTIES").is_some());

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
    assert!(h.query_by_label("PROPERTIES").is_none());
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
