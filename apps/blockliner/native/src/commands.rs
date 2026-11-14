//! BlockLiner's commands and keyboard shortcuts, registered with
//! laankit-egui's command registry so the same list drives the shortcuts, the
//! command palette and Settings → Keyboard Shortcuts.
//!
//! Unlike the other apps, Ctrl+= / Ctrl+- / Ctrl+0 zoom the canvas, as in
//! other drawing tools. The UI scale is in Settings and the title bar.

use egui::{Key, Modifiers};
use laankit_egui::commands::{Binding, Command, Registry, ShortcutEntry};

use blockliner_egui::commands::{EditAct, EditCommand, When};

/// Everything a shortcut, button or palette entry can ask for.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    NewDiagram,
    RenameDiagram,
    DeleteDiagram,
    OpenFolder,
    CloseFolder,
    /// Open one or more `.mmd` files from anywhere.
    OpenFile,
    /// Close the active tab; the file stays in the Files pane…
    CloseTab,
    /// …or close the active file: its tab and its Files row.
    CloseFile,
    NextTab,
    PreviousTab,
    ToggleFiles,
    Export,
    /// An edit of the open diagram (`blockliner-egui`).
    Edit(EditAct),
    ToggleGrid,
    ToggleDimensions,
    ToggleSidebar,
    ToggleProperties,
    ToggleStatusBar,
    Settings,
    About,
    Palette,
}

/// What the commands' `when` predicates read.
pub struct View {
    pub modal_open: bool,
    pub project_open: bool,
    /// A diagram is open on the canvas.
    pub doc_open: bool,
    /// …and it's one of the project folder's diagrams.
    pub doc_in_project: bool,
    /// How many tabs are open.
    pub tabs: usize,
    /// The open diagram has something to export.
    pub has_content: bool,
    pub has_selection: bool,
    /// Selected connectors include a straight one…
    pub straight_selected: bool,
    /// …or a right-angle one.
    pub elbow_selected: bool,
    /// Selected shapes or lines can take a stroke…
    pub stylable: bool,
    /// …and selected shapes a fill.
    pub fillable: bool,
    /// The selection is one shape or Mermaid edge whose label can be edited.
    pub labellable: bool,
    /// A selected shape can change kind (one BlockLiner declares).
    pub changeable: bool,
    /// Selected lines include one without an arrowhead…
    pub plain_line_selected: bool,
    /// …or one with.
    pub arrow_selected: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// No shortcut opens a second dialog over the first.
fn ready(v: &View) -> bool {
    !v.modal_open
}

fn in_project(v: &View) -> bool {
    ready(v) && v.project_open
}

fn editing(v: &View) -> bool {
    ready(v) && v.doc_open
}

fn project_doc(v: &View) -> bool {
    editing(v) && v.doc_in_project
}

fn several_tabs(v: &View) -> bool {
    ready(v) && v.tabs > 1
}

fn exportable(v: &View) -> bool {
    editing(v) && v.has_content
}

fn selected(v: &View) -> bool {
    ready(v) && v.has_selection
}

fn has_straight(v: &View) -> bool {
    ready(v) && v.straight_selected
}

fn has_elbow(v: &View) -> bool {
    ready(v) && v.elbow_selected
}

fn stylable(v: &View) -> bool {
    ready(v) && v.stylable
}

fn fillable(v: &View) -> bool {
    ready(v) && v.fillable
}

fn labellable(v: &View) -> bool {
    ready(v) && v.labellable
}

fn changeable(v: &View) -> bool {
    ready(v) && v.changeable
}

fn has_line(v: &View) -> bool {
    ready(v) && (v.straight_selected || v.elbow_selected)
}

fn has_plain_line(v: &View) -> bool {
    ready(v) && v.plain_line_selected
}

fn has_arrow(v: &View) -> bool {
    ready(v) && v.arrow_selected
}

fn undoable(v: &View) -> bool {
    ready(v) && v.can_undo
}

fn redoable(v: &View) -> bool {
    ready(v) && v.can_redo
}

/// An edit command in BlockLiner's registry: no dialog open, and what its
/// [`When`] asks of the selection.
fn edit_command(c: EditCommand) -> Command<Act, View> {
    c.into_command(Act::Edit, |when| match when {
        When::Ready => ready,
        When::Editing => editing,
        When::Selected => selected,
        When::HasStraight => has_straight,
        When::HasElbow => has_elbow,
        When::Stylable => stylable,
        When::Fillable => fillable,
        When::Labellable => labellable,
        When::Changeable => changeable,
        When::HasLine => has_line,
        When::HasPlainLine => has_plain_line,
        When::HasArrow => has_arrow,
        When::Undoable => undoable,
        When::Redoable => redoable,
    })
}

pub fn registry() -> Registry<Act, View> {
    let mut r = Registry::new();
    let cmd = Modifiers::COMMAND;
    let shift = cmd | Modifiers::SHIFT;
    let add =
        |id: &str, label: &str, section: &str, act: Act| Command::new(id, label, section, act);
    r.register(
        add("file.new", "New Diagram", "File", Act::NewDiagram)
            .key(Binding::logical(cmd, Key::N))
            .shortcut("Ctrl+N")
            .when(in_project),
    );
    r.register(
        add("file.rename", "Rename Diagram", "File", Act::RenameDiagram)
            .key(Binding::logical(Modifiers::NONE, Key::F2))
            .shortcut("F2")
            .when(project_doc),
    );
    r.register(add("file.delete", "Delete Diagram", "File", Act::DeleteDiagram).when(project_doc));
    r.register(
        add("file.openFolder", "Open Folder", "File", Act::OpenFolder)
            .key(Binding::logical(cmd, Key::O))
            .shortcut("Ctrl+O")
            .when(ready),
    );
    r.register(
        add("file.export", "Export…", "File", Act::Export)
            .key(Binding::physical(shift, Key::E))
            .shortcut("Ctrl+Shift+E")
            .when(exportable),
    );
    r.register(add("file.closeFolder", "Close Folder", "File", Act::CloseFolder).when(in_project));
    r.register(
        add("file.openFile", "Open File", "File", Act::OpenFile)
            .key(Binding::physical(shift, Key::O))
            .shortcut("Ctrl+Shift+O")
            .when(ready),
    );
    r.register(
        add("file.closeTab", "Close Tab", "File", Act::CloseTab)
            .key(Binding::logical(cmd, Key::W))
            .shortcut("Ctrl+W")
            .when(editing),
    );
    r.register(add("file.closeFile", "Close File", "File", Act::CloseFile).when(editing));
    r.register(
        add("view.nextTab", "Next Tab", "View", Act::NextTab)
            .key(Binding::logical(cmd, Key::Tab))
            .key(Binding::logical(cmd, Key::PageDown))
            .shortcut("Ctrl+Tab")
            .when(several_tabs),
    );
    r.register(
        add("view.previousTab", "Previous Tab", "View", Act::PreviousTab)
            .key(Binding::logical(shift, Key::Tab))
            .key(Binding::logical(cmd, Key::PageUp))
            .shortcut("Ctrl+Shift+Tab")
            .when(several_tabs),
    );
    r.register(
        add(
            "view.toggleFiles",
            "Toggle Files Pane",
            "View",
            Act::ToggleFiles,
        )
        .when(ready),
    );
    for c in blockliner_egui::commands::commands() {
        r.register(edit_command(c));
    }
    r.register(
        add("view.zoomIn", "Zoom In", "View", Act::Edit(EditAct::ZoomIn))
            .key(Binding::logical(cmd, Key::Equals))
            .key(Binding::loose(cmd, Key::Plus))
            .shortcut("Ctrl+=")
            .when(ready),
    );
    r.register(
        add(
            "view.zoomOut",
            "Zoom Out",
            "View",
            Act::Edit(EditAct::ZoomOut),
        )
        .key(Binding::logical(cmd, Key::Minus))
        .shortcut("Ctrl+-")
        .when(ready),
    );
    r.register(
        add(
            "view.zoomReset",
            "Zoom to 100%",
            "View",
            Act::Edit(EditAct::ZoomReset),
        )
        .key(Binding::logical(cmd, Key::Num0))
        .shortcut("Ctrl+0")
        .when(ready),
    );
    r.register(
        add("view.toggleGrid", "Toggle Grid", "View", Act::ToggleGrid)
            .key(Binding::logical(cmd, Key::Quote))
            .shortcut("Ctrl+'")
            .when(ready),
    );
    r.register(
        add(
            "view.toggleDimensions",
            "Toggle Dimension Lines",
            "View",
            Act::ToggleDimensions,
        )
        .key(Binding::physical(shift, Key::D))
        .shortcut("Ctrl+Shift+D")
        .when(ready),
    );
    r.register(
        add(
            "view.toggleSidebar",
            "Toggle Sidebar",
            "View",
            Act::ToggleSidebar,
        )
        .key(Binding::logical(cmd, Key::E))
        .shortcut("Ctrl+E")
        .when(in_project),
    );
    r.register(
        add(
            "view.toggleProperties",
            "Toggle Properties",
            "View",
            Act::ToggleProperties,
        )
        .key(Binding::logical(cmd, Key::I))
        .shortcut("Ctrl+I")
        .when(ready),
    );
    r.register(
        add(
            "view.toggleStatusBar",
            "Toggle Status Bar",
            "View",
            Act::ToggleStatusBar,
        )
        .key(Binding::logical(cmd, Key::J))
        .shortcut("Ctrl+J")
        .when(ready),
    );
    r.register(
        add("app.settings", "Settings", "App", Act::Settings)
            .key(Binding::logical(cmd, Key::Comma))
            .shortcut("Ctrl+,")
            .when(ready),
    );
    r.register(
        add("app.about", "About", "App", Act::About)
            .key(Binding::physical(shift, Key::I))
            .shortcut("Ctrl+Shift+I")
            .when(ready),
    );
    r.register(
        add("app.commandPalette", "Command Palette", "App", Act::Palette)
            .key(Binding::logical(cmd, Key::K))
            .shortcut("Ctrl+K")
            .when(ready),
    );
    r
}

/// Settings → Keyboard Shortcuts.
pub fn shortcut_list() -> Vec<ShortcutEntry> {
    registry().shortcut_list()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Tool;

    #[test]
    fn every_tool_has_a_command() {
        let r = registry();
        let view = View {
            modal_open: false,
            project_open: true,
            doc_open: true,
            doc_in_project: true,
            tabs: 2,
            has_content: true,
            has_selection: true,
            straight_selected: true,
            elbow_selected: true,
            stylable: true,
            fillable: true,
            labellable: true,
            changeable: true,
            plain_line_selected: true,
            arrow_selected: true,
            can_undo: true,
            can_redo: true,
        };
        for tool in Tool::ALL {
            assert!(
                r.applicable_commands(&view)
                    .iter()
                    .any(|c| c.action == Act::Edit(EditAct::Tool(tool))),
                "{tool:?}"
            );
        }
        assert!(r
            .applicable_commands(&View {
                modal_open: true,
                project_open: true,
                doc_open: true,
                doc_in_project: true,
                tabs: 2,
                has_content: true,
                has_selection: true,
                straight_selected: true,
                elbow_selected: true,
                stylable: true,
                fillable: true,
                labellable: true,
                changeable: true,
                plain_line_selected: true,
                arrow_selected: true,
                can_undo: true,
                can_redo: true,
            })
            .is_empty());
    }

    #[test]
    fn tools_list_their_letter_and_number() {
        let r = registry();
        let label = |id: &str| r.get(id).and_then(|c| c.shortcut_label());
        assert_eq!(label("tool.select").as_deref(), Some("V, 1"));
        assert_eq!(label("tool.rectangle").as_deref(), Some("R, 2"));
        assert_eq!(label("tool.roundedrectangle").as_deref(), Some("O, 3"));
        assert_eq!(label("tool.diamond").as_deref(), Some("D"));
        assert_eq!(label("tool.hexagon").as_deref(), Some("H"));
        assert_eq!(label("tool.database").as_deref(), Some("C"));
        assert_eq!(label("tool.subroutine").as_deref(), Some("S"));
        assert_eq!(label("tool.moreShapes").as_deref(), Some("4"));
        assert_eq!(label("tool.line").as_deref(), Some("L, 5"));
        assert_eq!(label("tool.rightangleline").as_deref(), Some("J, 7"));
        assert_eq!(label("tool.rightanglearrow").as_deref(), Some("E, 8"));
        assert_eq!(label("tool.text").as_deref(), Some("T, 9"));
    }

    #[test]
    fn shortcuts_are_unique() {
        let keys: Vec<String> = shortcut_list().into_iter().map(|s| s.keys).collect();
        for (i, k) in keys.iter().enumerate() {
            assert!(!keys[i + 1..].contains(k), "{k} is bound twice");
        }
        assert!(keys.iter().any(|k| k == "Ctrl+'"));
    }
}
