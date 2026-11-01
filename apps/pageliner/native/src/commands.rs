//! PageLiner's commands and keyboard shortcuts (`registerCommands` and
//! `handleKeydown` in `App.svelte`), registered with laankit-egui's command
//! registry so the same list drives the shortcuts, the command palette and
//! Settings → Keyboard Shortcuts.

use egui::{Key, Modifiers};
use laankit_egui::commands::{Binding, Command, Registry, ShortcutEntry};

/// Everything a shortcut, button or palette entry can ask for.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Import,
    CloseBook,
    ToggleSidebar,
    ToggleStatusBar,
    Sync,
    Settings,
    About,
    Palette,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    /// A palette result: open this book.
    OpenBook(String),
}

/// What the commands' `when` predicates read.
pub struct View {
    pub book_open: bool,
    pub modal_open: bool,
}

/// No shortcut opens a second dialog over the first (the Svelte palette
/// "owns keys while open"; the native dialogs do the same).
fn ready(v: &View) -> bool {
    !v.modal_open
}

fn book_open(v: &View) -> bool {
    v.book_open
}

/// The registry, in `registerCommands`' order, plus zoom (`handleKeydown`).
pub fn registry() -> Registry<Act, View> {
    let mut r = Registry::new();
    let cmd = Modifiers::COMMAND;
    let shift = cmd | Modifiers::SHIFT;
    let add =
        |id: &str, label: &str, section: &str, act: Act| Command::new(id, label, section, act);
    r.register(
        add("library.import", "Import Book…", "Library", Act::Import)
            .key(Binding::logical(cmd, Key::O))
            .shortcut("Ctrl+O")
            .when(ready),
    );
    r.register(
        add(
            "library.close",
            "Close Book (Library)",
            "Library",
            Act::CloseBook,
        )
        .when(book_open),
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
        add("app.sync", "Git Sync", "App", Act::Sync)
            .key(Binding::physical(shift, Key::S))
            .shortcut("Ctrl+Shift+S")
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
    // Zoom works even with a dialog open, as in the Svelte build.
    r.register(
        add("view.zoomIn", "Zoom In", "View", Act::ZoomIn)
            .key(Binding::logical(cmd, Key::Equals))
            .key(Binding::loose(cmd, Key::Plus))
            .shortcut("Ctrl+="),
    );
    r.register(
        add("view.zoomOut", "Zoom Out", "View", Act::ZoomOut)
            .key(Binding::logical(cmd, Key::Minus))
            .shortcut("Ctrl+-"),
    );
    r.register(
        add("view.zoomReset", "Reset Zoom", "View", Act::ZoomReset)
            .key(Binding::logical(cmd, Key::Num0))
            .shortcut("Ctrl+0"),
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
    fn close_book_only_applies_with_a_book_open() {
        let r = registry();
        let open = |book_open| {
            r.applicable_commands(&View {
                book_open,
                modal_open: false,
            })
            .iter()
            .any(|c| c.id == "library.close")
        };
        assert!(open(true));
        assert!(!open(false));
    }

    #[test]
    fn lists_the_svelte_shortcuts() {
        let keys: Vec<String> = shortcut_list().into_iter().map(|s| s.keys).collect();
        for k in [
            "Ctrl+O",
            "Ctrl+E",
            "Ctrl+J",
            "Ctrl+Shift+S",
            "Ctrl+,",
            "Ctrl+Shift+I",
            "Ctrl+K",
        ] {
            assert!(keys.iter().any(|x| x == k), "missing {k}");
        }
    }
}
