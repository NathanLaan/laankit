//! PrintLiner's commands and keyboard shortcuts, registered with
//! laankit-egui's command registry so the same list drives the shortcuts,
//! the command palette, and Settings → Keyboard Shortcuts.

use egui::{Key, Modifiers};
use laankit_egui::commands::{Binding, Command, Registry, ShortcutEntry};

/// Everything a shortcut, button, or palette entry can ask for.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// The printer grid (leaves a printer's detail).
    Printers,
    Refresh,
    SignIn,
    SignOut,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleStatusBar,
    Settings,
    About,
    Palette,
}

/// What the commands' `when` predicates read.
pub struct View {
    pub modal_open: bool,
    pub signed_in: bool,
    /// Demo mode: no account to sign in or out of.
    pub demo: bool,
    pub detail_open: bool,
}

fn ready(v: &View) -> bool {
    !v.modal_open
}

fn in_detail(v: &View) -> bool {
    ready(v) && v.detail_open
}

fn online(v: &View) -> bool {
    ready(v) && v.signed_in
}

fn can_sign_in(v: &View) -> bool {
    ready(v) && !v.demo
}

fn can_sign_out(v: &View) -> bool {
    online(v) && !v.demo
}

pub fn registry() -> Registry<Act, View> {
    let mut r = Registry::new();
    let cmd = Modifiers::COMMAND;
    let shift = cmd | Modifiers::SHIFT;
    let add =
        |id: &str, label: &str, section: &str, act: Act| Command::new(id, label, section, act);
    r.register(
        add("view.printers", "Show All Printers", "View", Act::Printers)
            .key(Binding::logical(Modifiers::NONE, Key::Escape))
            .key(Binding::logical(Modifiers::ALT, Key::ArrowLeft))
            .shortcut("Esc")
            .when(in_detail),
    );
    r.register(
        add("printers.refresh", "Refresh", "Printers", Act::Refresh)
            .key(Binding::logical(Modifiers::NONE, Key::F5))
            .key(Binding::logical(cmd, Key::R))
            .shortcut("F5")
            .when(online),
    );
    r.register(add("account.signIn", "Sign In", "Account", Act::SignIn).when(can_sign_in));
    r.register(add("account.signOut", "Sign Out", "Account", Act::SignOut).when(can_sign_out));
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
        add("view.zoomReset", "Reset Zoom", "View", Act::ZoomReset)
            .key(Binding::logical(cmd, Key::Num0))
            .shortcut("Ctrl+0")
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

    fn view(signed_in: bool, demo: bool, detail_open: bool) -> View {
        View {
            modal_open: false,
            signed_in,
            demo,
            detail_open,
        }
    }

    #[test]
    fn account_commands_follow_the_account() {
        let r = registry();
        let enabled = |v: &View, act: Act| {
            r.commands()
                .iter()
                .any(|c| c.action == act && Registry::is_applicable(c, v))
        };
        assert!(enabled(&view(false, false, false), Act::SignIn));
        assert!(!enabled(&view(false, false, false), Act::SignOut));
        assert!(!enabled(&view(false, false, false), Act::Refresh));
        assert!(enabled(&view(true, false, false), Act::SignOut));
        assert!(!enabled(&view(true, true, false), Act::SignOut));
        assert!(!enabled(&view(true, true, false), Act::SignIn));
        assert!(!enabled(&view(true, false, false), Act::Printers));
        assert!(enabled(&view(true, false, true), Act::Printers));
    }
}
