//! BlockLiner's commands and keyboard shortcuts, registered with
//! marina-egui's command registry so the same list drives the shortcuts, the
//! command palette and Settings → Keyboard Shortcuts.
//!
//! Unlike the other apps, Ctrl+= / Ctrl+- / Ctrl+0 zoom the canvas, as in
//! other drawing tools. The UI scale is in Settings and the title bar.

use egui::{Key, Modifiers};
use marina_egui::commands::{Binding, Command, Registry, ShortcutEntry};

use crate::canvas::Tool;

/// Everything a shortcut, button or palette entry can ask for.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    NewDiagram,
    RenameDiagram,
    DeleteDiagram,
    OpenFolder,
    Tool(Tool),
    Undo,
    Redo,
    SelectAll,
    Delete,
    /// Move the selection by this many grid steps (the arrow keys go
    /// through `App::nudge_keys`, which also reads Shift and Alt).
    Nudge(i8, i8),
    /// Cancel the drag in progress, else clear the selection.
    Escape,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleGrid,
    ToggleSidebar,
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
    pub has_selection: bool,
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

fn selected(v: &View) -> bool {
    ready(v) && v.has_selection
}

fn undoable(v: &View) -> bool {
    ready(v) && v.can_undo
}

fn redoable(v: &View) -> bool {
    ready(v) && v.can_redo
}

/// A tool's single-key shortcut (Visio-style; never fires while a text field
/// has the keyboard).
pub fn tool_key(tool: Tool) -> (Key, &'static str) {
    match tool {
        Tool::Select => (Key::V, "V"),
        Tool::Rect => (Key::R, "R"),
        Tool::RoundRect => (Key::O, "O"),
        Tool::Line => (Key::L, "L"),
        Tool::Arrow => (Key::A, "A"),
        Tool::Text => (Key::T, "T"),
    }
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
            .when(editing),
    );
    r.register(add("file.delete", "Delete Diagram", "File", Act::DeleteDiagram).when(editing));
    r.register(
        add("file.openFolder", "Open Folder", "File", Act::OpenFolder)
            .key(Binding::logical(cmd, Key::O))
            .shortcut("Ctrl+O")
            .when(ready),
    );
    for tool in Tool::ALL {
        let (key, text) = tool_key(tool);
        let id = format!("tool.{}", tool.name().to_lowercase().replace(' ', ""));
        r.register(
            add(
                &id,
                &format!("{} Tool", tool.name()),
                "Tools",
                Act::Tool(tool),
            )
            .key(Binding::logical(Modifiers::NONE, key))
            // With Shift the tool stays picked after each shape.
            .key(Binding::logical(Modifiers::SHIFT, key))
            .shortcut(text)
            .when(ready),
        );
    }
    r.register(
        add("edit.undo", "Undo", "Edit", Act::Undo)
            .key(Binding::logical(cmd, Key::Z))
            .shortcut("Ctrl+Z")
            .when(undoable),
    );
    r.register(
        add("edit.redo", "Redo", "Edit", Act::Redo)
            .key(Binding::logical(shift, Key::Z))
            .key(Binding::logical(cmd, Key::Y))
            .shortcut("Ctrl+Shift+Z")
            .when(redoable),
    );
    r.register(
        add("edit.selectAll", "Select All", "Edit", Act::SelectAll)
            .key(Binding::logical(cmd, Key::A))
            .shortcut("Ctrl+A")
            .when(ready),
    );
    // No key bindings: the arrow keys are read in `App::nudge_keys`, which
    // also handles Shift (5 steps) and Alt (1 px). These entries put them in
    // the palette and the shortcuts list.
    for (id, label, keys, (dx, dy)) in [
        ("edit.nudgeLeft", "Nudge Left", "←", (-1, 0)),
        ("edit.nudgeRight", "Nudge Right", "→", (1, 0)),
        ("edit.nudgeUp", "Nudge Up", "↑", (0, -1)),
        ("edit.nudgeDown", "Nudge Down", "↓", (0, 1)),
    ] {
        r.register(
            add(id, label, "Edit", Act::Nudge(dx, dy))
                .shortcut(keys)
                .when(selected),
        );
    }
    r.register(
        add("edit.delete", "Delete", "Edit", Act::Delete)
            .key(Binding::logical(Modifiers::NONE, Key::Delete))
            .key(Binding::logical(Modifiers::NONE, Key::Backspace))
            .shortcut("Delete")
            .when(selected),
    );
    r.register(
        add("edit.escape", "Clear Selection", "Edit", Act::Escape)
            .key(Binding::logical(Modifiers::NONE, Key::Escape))
            .shortcut("Esc")
            .when(ready),
    );
    r.register(
        add("view.zoomIn", "Zoom In", "View", Act::ZoomIn)
            .key(Binding::logical(cmd, Key::Equals))
            .key(Binding::loose(cmd, Key::Plus))
            .shortcut("Ctrl+=")
            .when(ready),
    );
    r.register(
        add("view.zoomOut", "Zoom Out", "View", Act::ZoomOut)
            .key(Binding::logical(cmd, Key::Minus))
            .shortcut("Ctrl+-")
            .when(ready),
    );
    r.register(
        add("view.zoomReset", "Zoom to 100%", "View", Act::ZoomReset)
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

    #[test]
    fn every_tool_has_a_command() {
        let r = registry();
        let view = View {
            modal_open: false,
            project_open: true,
            doc_open: true,
            has_selection: true,
            can_undo: true,
            can_redo: true,
        };
        for tool in Tool::ALL {
            assert!(
                r.applicable_commands(&view)
                    .iter()
                    .any(|c| c.action == Act::Tool(tool)),
                "{tool:?}"
            );
        }
        assert!(r
            .applicable_commands(&View {
                modal_open: true,
                project_open: true,
                doc_open: true,
                has_selection: true,
                can_undo: true,
                can_redo: true,
            })
            .is_empty());
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
