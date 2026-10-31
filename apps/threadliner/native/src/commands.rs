//! ThreadLiner's commands and keyboard shortcuts (`App.svelte`'s
//! `handleGlobalKeydown`), registered with laankit-egui's command registry so
//! the same list drives the shortcuts, the command palette and Settings →
//! Keyboard Shortcuts.

use egui::{Key, Modifiers};
use laankit_egui::commands::{Binding, Command, Registry, ShortcutEntry};

/// Everything a shortcut, toolbar button or palette entry can ask for.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    AddFeed,
    EditFeed,
    RemoveFeed,
    RefreshFeed,
    MarkRead,
    MarkUnread,
    Tags,
    Sync,
    ToggleToolbar,
    Settings,
    About,
    Help,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Palette,
    PollNow,
    SyncNow,
    PullNow,
    /// A palette result: select this feed.
    OpenFeed(String),
}

/// What the commands' `when` predicates read.
pub struct View {
    pub setup_complete: bool,
    pub modal_open: bool,
    pub feed_selected: bool,
}

/// Shortcuts other than zoom only act once setup is done and no modal is
/// open, so a shortcut can't open a second modal on top of the first.
fn ready(v: &View) -> bool {
    v.setup_complete && !v.modal_open
}

fn ready_with_feed(v: &View) -> bool {
    ready(v) && v.feed_selected
}

/// The registry, in `handleGlobalKeydown`'s order.
pub fn registry() -> Registry<Act, View> {
    let mut r = Registry::new();
    let cmd = Modifiers::COMMAND;
    let shift = cmd | Modifiers::SHIFT;
    let none = Modifiers::NONE;
    let add =
        |id: &str, label: &str, section: &str, act: Act| Command::new(id, label, section, act);
    // Zoom works even with a modal open, as in the Svelte build.
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
    r.register(
        add("app.help", "Help", "App", Act::Help)
            .key(Binding::logical(none, Key::F1))
            .when(ready),
    );
    r.register(
        add("feed.refresh", "Refresh Feed", "Feeds", Act::RefreshFeed)
            .key(Binding::logical(none, Key::F5))
            .when(ready_with_feed),
    );
    r.register(
        add("panel.tags", "Tags", "Panels", Act::Tags)
            .key(Binding::physical(shift, Key::T))
            .when(ready),
    );
    r.register(
        add("panel.sync", "Remote Sync", "Panels", Act::Sync)
            .key(Binding::physical(shift, Key::S))
            .when(ready),
    );
    r.register(
        add(
            "view.toolbar",
            "Toggle Toolbar",
            "Panels",
            Act::ToggleToolbar,
        )
        .key(Binding::physical(shift, Key::E))
        .when(ready),
    );
    r.register(
        add(
            "entry.markUnread",
            "Mark as Unread",
            "Entries",
            Act::MarkUnread,
        )
        .key(Binding::physical(shift, Key::M))
        .when(ready_with_feed),
    );
    r.register(
        add("app.about", "About", "App", Act::About)
            .key(Binding::physical(shift, Key::I))
            .when(ready),
    );
    r.register(
        add("app.palette", "Command Palette", "App", Act::Palette)
            .key(Binding::physical(shift, Key::P))
            .when(ready),
    );
    r.register(
        add("feed.add", "Add Feed", "Feeds", Act::AddFeed)
            .key(Binding::logical(cmd, Key::N))
            .when(ready),
    );
    r.register(
        add("feed.edit", "Edit Feed", "Feeds", Act::EditFeed)
            .key(Binding::logical(cmd, Key::E))
            .when(ready_with_feed),
    );
    r.register(
        add("feed.remove", "Remove Feed", "Feeds", Act::RemoveFeed)
            .key(Binding::logical(cmd, Key::D))
            .when(ready_with_feed),
    );
    r.register(
        add("entry.markRead", "Mark as Read", "Entries", Act::MarkRead)
            .key(Binding::logical(cmd, Key::M))
            .when(ready_with_feed),
    );
    r.register(
        add("app.settings", "Settings", "App", Act::Settings)
            .key(Binding::logical(cmd, Key::Comma))
            .shortcut("Ctrl+,")
            .when(ready),
    );
    // Palette-only commands (no shortcut in the Svelte build).
    r.register(add("feed.pollAll", "Check All Feeds Now", "Feeds", Act::PollNow).when(ready));
    r.register(add("sync.now", "Sync Now", "Sync", Act::SyncNow).when(ready));
    r.register(add("sync.pull", "Pull from Remote", "Sync", Act::PullNow).when(ready));
    r
}

/// Settings → Keyboard Shortcuts: the Svelte `SettingsModal` list, plus the
/// native command palette.
pub fn shortcut_list() -> Vec<ShortcutEntry> {
    [
        ("Esc", "Close modal", "General"),
        ("Ctrl+N", "Add Feed", "Feeds"),
        ("Ctrl+E", "Edit Feed", "Feeds"),
        ("Ctrl+D", "Remove Feed", "Feeds"),
        ("F5", "Refresh Feed", "Feeds"),
        ("Ctrl+M", "Mark as Read", "Entries"),
        ("Ctrl+Shift+M", "Mark as Unread", "Entries"),
        ("Ctrl+Shift+T", "Tags", "Panels"),
        ("Ctrl+Shift+S", "Remote Sync", "Panels"),
        ("Ctrl+Shift+E", "Toggle Toolbar", "Panels"),
        ("Ctrl+,", "Settings", "App"),
        ("Ctrl+Shift+I", "About", "App"),
        ("Ctrl+Shift+P", "Command Palette", "App"),
        ("F1", "Help", "App"),
        ("Ctrl+=", "Zoom in", "View"),
        ("Ctrl+-", "Zoom out", "View"),
        ("Ctrl+0", "Reset zoom", "View"),
        ("Ctrl+MouseWheelUp", "Zoom in", "View"),
        ("Ctrl+MouseWheelDown", "Zoom out", "View"),
    ]
    .into_iter()
    .map(|(keys, action, section)| ShortcutEntry {
        keys: keys.into(),
        action: action.into(),
        section: section.into(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(r: &mut Registry<Act, View>, view: &View, key: Key, modifiers: Modifiers) -> Vec<Act> {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        });
        let mut out = Vec::new();
        let mut frame = ctx.run_ui(input, |ui| out = r.dispatch_keys(ui.ctx(), view));
        frame.textures_delta.clear();
        out
    }

    #[test]
    fn shortcuts_respect_setup_modals_and_selection() {
        let mut r = registry();
        let ready = View {
            setup_complete: true,
            modal_open: false,
            feed_selected: true,
        };
        assert_eq!(
            press(&mut r, &ready, Key::N, Modifiers::COMMAND),
            [Act::AddFeed]
        );
        assert_eq!(
            press(
                &mut r,
                &ready,
                Key::M,
                Modifiers::COMMAND | Modifiers::SHIFT
            ),
            [Act::MarkUnread]
        );
        assert_eq!(
            press(&mut r, &ready, Key::F5, Modifiers::NONE),
            [Act::RefreshFeed]
        );

        let no_feed = View {
            feed_selected: false,
            ..ready
        };
        assert!(press(&mut r, &no_feed, Key::E, Modifiers::COMMAND).is_empty());
        let modal = View {
            modal_open: true,
            ..ready
        };
        assert!(press(&mut r, &modal, Key::N, Modifiers::COMMAND).is_empty());
        // Zoom still works with a modal open.
        assert_eq!(
            press(&mut r, &modal, Key::Minus, Modifiers::COMMAND),
            [Act::ZoomOut]
        );
    }

    #[test]
    fn shortcut_list_matches_settings_modal() {
        let list = shortcut_list();
        assert_eq!(list.len(), 19);
        assert_eq!(list[0].keys, "Esc");
        assert!(list
            .iter()
            .any(|s| s.action == "Remote Sync" && s.keys == "Ctrl+Shift+S"));
    }
}
