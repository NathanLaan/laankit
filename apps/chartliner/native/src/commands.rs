//! ChartLiner's commands and keyboard shortcuts, registered with
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
    Tool(Tool),
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleGrid,
    ToggleStatusBar,
    Settings,
    About,
    Palette,
}

/// What the commands' `when` predicates read.
pub struct View {
    pub modal_open: bool,
}

/// No shortcut opens a second dialog over the first.
fn ready(v: &View) -> bool {
    !v.modal_open
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
            .shortcut(text)
            .when(ready),
        );
    }
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
        let view = View { modal_open: false };
        for tool in Tool::ALL {
            assert!(
                r.applicable_commands(&view)
                    .iter()
                    .any(|c| c.action == Act::Tool(tool)),
                "{tool:?}"
            );
        }
        assert!(r.applicable_commands(&View { modal_open: true }).is_empty());
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
