//! NoteLiner's commands and keyboard shortcuts (`registerCommands` in
//! `App.svelte`), registered with marina-egui's command registry so the same
//! list drives the shortcuts, the command palette and Settings → Keyboard
//! Shortcuts. Registration order matters, as in the Svelte build: the first
//! applicable match wins (so Ctrl+Shift++ adds a tag before it zooms).

use egui::{Key, Modifiers};
use marina_egui::commands::{Binding, Command, Registry, ShortcutEntry};
use marina_egui::theme::ThemeId;

/// Everything a shortcut, button or palette entry can ask for.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    NewFile,
    DeleteFile,
    DuplicateFile,
    ImportDocument,
    SaveAsTemplate,
    OpenFolder,
    PrevFile,
    NextFile,
    NewPresentation,
    ConvertToPresentation,
    ConvertToNote,
    PresentationSettings,
    ToggleSlides,
    NewSlide,
    SplitSlide,
    MoveSlideUp,
    MoveSlideDown,
    ToggleSpeakerNotes,
    /// Full-screen presenting (a native addition).
    Present,
    AddTag,
    TogglePreview,
    ToggleHistory,
    ToggleSidebar,
    ToggleToolbar,
    ToggleOutline,
    ToggleTags,
    ToggleAttachments,
    ToggleSearch,
    ToggleBacklinks,
    ToggleLog,
    ToggleStatusBar,
    ToggleSpellCheck,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Sync,
    ProjectSettings,
    GoHome,
    SetTheme(ThemeId),
    Settings,
    About,
    Help,
    Palette,
    // Palette results.
    OpenNote(String),
    OpenRecent(String),
}

/// What the commands' `when` predicates read.
#[derive(Clone, Copy, Debug, Default)]
pub struct View {
    pub project_open: bool,
    pub has_selection: bool,
    pub is_deck: bool,
    pub tags_visible: bool,
    /// A dialog is open: shortcuts other than zoom wait (the Svelte palette
    /// "owns keys while open"; the native dialogs do the same).
    pub modal_open: bool,
}

fn ready(v: &View) -> bool {
    !v.modal_open
}

fn project_open(v: &View) -> bool {
    ready(v) && v.project_open
}

fn has_selection(v: &View) -> bool {
    project_open(v) && v.has_selection
}

fn is_deck(v: &View) -> bool {
    has_selection(v) && v.is_deck
}

fn not_deck(v: &View) -> bool {
    has_selection(v) && !v.is_deck
}

fn tag_plus(v: &View) -> bool {
    has_selection(v) && v.tags_visible
}

/// The registry, in `registerCommands`' order.
pub fn registry() -> Registry<Act, View> {
    let mut r = Registry::new();
    let cmd = Modifiers::COMMAND;
    let shift = cmd | Modifiers::SHIFT;
    let alt = cmd | Modifiers::ALT;
    let add =
        |id: &str, label: &str, section: &str, act: Act| Command::new(id, label, section, act);

    // File
    r.register(
        add("file.new", "New File", "File", Act::NewFile)
            .key(Binding::logical(cmd, Key::N))
            .shortcut("Ctrl+N")
            .when(project_open),
    );
    r.register(
        add("file.delete", "Delete File", "File", Act::DeleteFile)
            .key(Binding::logical(cmd, Key::D))
            .shortcut("Ctrl+D")
            .when(has_selection),
    );
    r.register(
        add(
            "file.duplicate",
            "Duplicate File",
            "File",
            Act::DuplicateFile,
        )
        .key(Binding::physical(shift, Key::D))
        .shortcut("Ctrl+Shift+D")
        .when(has_selection),
    );
    r.register(
        add(
            "file.import",
            "Import Document",
            "File",
            Act::ImportDocument,
        )
        .key(Binding::physical(alt, Key::I))
        .shortcut("Ctrl+Alt+I")
        .when(project_open),
    );
    r.register(
        add(
            "file.saveAsTemplate",
            "Save as Template",
            "File",
            Act::SaveAsTemplate,
        )
        .when(has_selection),
    );
    r.register(
        add("file.openFolder", "Open Folder", "File", Act::OpenFolder)
            .key(Binding::logical(cmd, Key::O))
            .shortcut("Ctrl+O")
            .when(ready),
    );
    r.register(
        add("file.prev", "Previous File", "File", Act::PrevFile)
            .key(Binding::loose(cmd, Key::PageUp))
            .shortcut("Ctrl+PgUp")
            .when(project_open),
    );
    r.register(
        add("file.next", "Next File", "File", Act::NextFile)
            .key(Binding::loose(cmd, Key::PageDown))
            .shortcut("Ctrl+PgDn")
            .when(project_open),
    );

    // Presentations: gated on the open note being a deck.
    r.register(
        add(
            "deck.new",
            "New Presentation...",
            "Presentation",
            Act::NewPresentation,
        )
        .when(project_open),
    );
    r.register(
        add(
            "deck.convert",
            "Convert to Presentation",
            "Presentation",
            Act::ConvertToPresentation,
        )
        .when(not_deck),
    );
    r.register(
        add(
            "deck.convertBack",
            "Convert to Note",
            "Presentation",
            Act::ConvertToNote,
        )
        .when(is_deck),
    );
    r.register(
        add(
            "deck.settings",
            "Presentation Settings",
            "Presentation",
            Act::PresentationSettings,
        )
        .when(is_deck),
    );
    r.register(
        add(
            "deck.toggleSlides",
            "Toggle Slides Panel",
            "Presentation",
            Act::ToggleSlides,
        )
        .key(Binding::physical(shift, Key::G))
        .shortcut("Ctrl+Shift+G")
        .when(is_deck),
    );
    r.register(
        add("deck.newSlide", "New Slide", "Presentation", Act::NewSlide)
            .key(Binding::logical(cmd, Key::Enter))
            .shortcut("Ctrl+Enter")
            .when(is_deck),
    );
    r.register(
        add(
            "deck.splitSlide",
            "Split Slide at Cursor",
            "Presentation",
            Act::SplitSlide,
        )
        .key(Binding::logical(shift, Key::Enter))
        .shortcut("Ctrl+Shift+Enter")
        .when(is_deck),
    );
    r.register(
        add(
            "deck.moveSlideUp",
            "Move Slide Up",
            "Presentation",
            Act::MoveSlideUp,
        )
        .key(Binding::logical(Modifiers::ALT, Key::ArrowUp))
        .shortcut("Alt+Up")
        .when(is_deck),
    );
    r.register(
        add(
            "deck.moveSlideDown",
            "Move Slide Down",
            "Presentation",
            Act::MoveSlideDown,
        )
        .key(Binding::logical(Modifiers::ALT, Key::ArrowDown))
        .shortcut("Alt+Down")
        .when(is_deck),
    );
    r.register(
        add(
            "deck.toggleNotes",
            "Mark as Speaker Notes",
            "Presentation",
            Act::ToggleSpeakerNotes,
        )
        .key(Binding::physical(shift, Key::V))
        .shortcut("Ctrl+Shift+V")
        .when(is_deck),
    );
    // Not in the Svelte build, which had no presenter: F5 as in most
    // presentation apps.
    r.register(
        add("deck.present", "Present", "Presentation", Act::Present)
            .key(Binding::logical(Modifiers::NONE, Key::F5))
            .shortcut("F5")
            .when(is_deck),
    );

    // Tags: before View, so Ctrl+Shift++ adds a tag (with the TAGS pane
    // open) before zoom-in sees it.
    r.register(
        add("tag.addPlus", "Add Tag", "Tags", Act::AddTag)
            .key(Binding::loose(shift, Key::Plus))
            .shortcut("Ctrl+Shift++")
            .when(tag_plus),
    );

    // View
    let view = |id: &str, label: &str, act: Act, key: Binding, text: &str| {
        add(id, label, "View", act).key(key).shortcut(text)
    };
    r.register(
        view(
            "view.togglePreview",
            "Toggle Preview",
            Act::TogglePreview,
            Binding::logical(cmd, Key::P),
            "Ctrl+P",
        )
        .when(ready),
    );
    r.register(
        view(
            "view.toggleHistory",
            "Toggle History",
            Act::ToggleHistory,
            Binding::logical(cmd, Key::H),
            "Ctrl+H",
        )
        .when(project_open),
    );
    r.register(
        view(
            "view.toggleSidebar",
            "Toggle Files Panel",
            Act::ToggleSidebar,
            Binding::logical(cmd, Key::E),
            "Ctrl+E",
        )
        .when(project_open),
    );
    r.register(
        view(
            "view.toggleToolbar",
            "Toggle Toolbar",
            Act::ToggleToolbar,
            Binding::physical(shift, Key::E),
            "Ctrl+Shift+E",
        )
        .when(ready),
    );
    r.register(
        view(
            "view.toggleOutline",
            "Toggle Outline",
            Act::ToggleOutline,
            Binding::physical(shift, Key::O),
            "Ctrl+Shift+O",
        )
        .when(project_open),
    );
    r.register(
        view(
            "view.toggleTags",
            "Toggle Tags",
            Act::ToggleTags,
            Binding::physical(shift, Key::T),
            "Ctrl+Shift+T",
        )
        .when(project_open),
    );
    r.register(
        view(
            "view.toggleAttachments",
            "Toggle Attachments",
            Act::ToggleAttachments,
            Binding::physical(shift, Key::A),
            "Ctrl+Shift+A",
        )
        .when(ready),
    );
    r.register(
        view(
            "view.toggleSearch",
            "Toggle Search",
            Act::ToggleSearch,
            Binding::logical(cmd, Key::F),
            "Ctrl+F",
        )
        .when(project_open),
    );
    r.register(
        view(
            "view.toggleBacklinks",
            "Toggle Backlinks",
            Act::ToggleBacklinks,
            Binding::physical(shift, Key::B),
            "Ctrl+Shift+B",
        )
        .when(project_open),
    );
    r.register(
        view(
            "view.toggleLog",
            "Toggle Log Panel",
            Act::ToggleLog,
            Binding::logical(cmd, Key::L),
            "Ctrl+L",
        )
        .when(ready),
    );
    r.register(
        view(
            "view.toggleStatusBar",
            "Toggle Status Bar",
            Act::ToggleStatusBar,
            Binding::logical(cmd, Key::J),
            "Ctrl+J",
        )
        .when(ready),
    );
    r.register(
        view(
            "view.toggleSpellCheck",
            "Toggle Spell Check",
            Act::ToggleSpellCheck,
            Binding::logical(Modifiers::NONE, Key::F7),
            "F7",
        )
        .when(ready),
    );
    // Zoom works with a dialog open, as in the Svelte build.
    r.register(
        add("view.zoomIn", "Zoom In", "View", Act::ZoomIn)
            .key(Binding::logical(cmd, Key::Equals))
            .key(Binding::loose(cmd, Key::Plus))
            .shortcut("Ctrl+="),
    );
    r.register(view(
        "view.zoomOut",
        "Zoom Out",
        Act::ZoomOut,
        Binding::logical(cmd, Key::Minus),
        "Ctrl+-",
    ));
    r.register(view(
        "view.zoomReset",
        "Zoom Reset",
        Act::ZoomReset,
        Binding::logical(cmd, Key::Num0),
        "Ctrl+0",
    ));

    // Tags
    r.register(
        add("tag.add", "Add Tag", "Tags", Act::AddTag)
            .key(Binding::logical(cmd, Key::T))
            .shortcut("Ctrl+T")
            .when(has_selection),
    );

    // Project
    r.register(
        add("project.sync", "Remote Sync", "Project", Act::Sync)
            .key(Binding::physical(shift, Key::S))
            .shortcut("Ctrl+Shift+S")
            .when(project_open),
    );
    r.register(
        add(
            "project.settings",
            "Project Settings",
            "Project",
            Act::ProjectSettings,
        )
        .key(Binding::physical(shift, Key::Comma))
        .shortcut("Ctrl+Shift+,")
        .when(project_open),
    );
    r.register(
        add(
            "project.goHome",
            "Close Project (Home)",
            "Project",
            Act::GoHome,
        )
        .when(project_open),
    );

    // Theme: palette-only.
    for (id, label, theme) in [
        ("theme.midnight", "Set Theme: Midnight", ThemeId::Midnight),
        ("theme.dark", "Set Theme: Dark", ThemeId::Dark),
        ("theme.light", "Set Theme: Light", ThemeId::Light),
    ] {
        r.register(add(id, label, "Theme", Act::SetTheme(theme)));
    }

    // App
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
        add("app.help", "Help", "App", Act::Help)
            .key(Binding::logical(Modifiers::NONE, Key::F1))
            .shortcut("F1")
            .when(ready),
    );
    r.register(
        add("app.commandPalette", "Command Palette", "App", Act::Palette)
            .key(Binding::logical(cmd, Key::K))
            .key(Binding::physical(shift, Key::P))
            .shortcut("Ctrl+K")
            .when(ready),
    );
    r
}

/// Settings → Keyboard Shortcuts: the registry's shortcuts plus the ones
/// handled elsewhere (`extraShortcuts`), sorted by section then action.
pub fn shortcut_list() -> Vec<ShortcutEntry> {
    let mut list = registry().shortcut_list();
    for (keys, action, section) in [
        ("Ctrl+Shift+7", "Ordered List", "Editor"),
        ("Ctrl+Shift+8", "Unordered List", "Editor"),
        ("Ctrl+MouseWheelUp", "Zoom In", "View"),
        ("Ctrl+MouseWheelDown", "Zoom Out", "View"),
    ] {
        list.push(ShortcutEntry {
            keys: keys.into(),
            action: action.into(),
            section: section.into(),
        });
    }
    list.sort_by(|a, b| {
        a.section
            .cmp(&b.section)
            .then_with(|| a.action.cmp(&b.action))
    });
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    fn applicable(view: View) -> Vec<String> {
        registry()
            .applicable_commands(&view)
            .iter()
            .map(|c| c.id.clone())
            .collect()
    }

    #[test]
    fn gating_follows_the_svelte_when_predicates() {
        let home = applicable(View::default());
        assert!(home.contains(&"file.openFolder".into()));
        assert!(!home.contains(&"file.new".into()));
        let open = applicable(View {
            project_open: true,
            ..Default::default()
        });
        assert!(open.contains(&"file.new".into()));
        assert!(!open.contains(&"file.delete".into()));
        let deck = applicable(View {
            project_open: true,
            has_selection: true,
            is_deck: true,
            ..Default::default()
        });
        assert!(deck.contains(&"deck.convertBack".into()));
        assert!(!deck.contains(&"deck.convert".into()));
        let modal = applicable(View {
            project_open: true,
            modal_open: true,
            ..Default::default()
        });
        assert!(modal.contains(&"view.zoomIn".into()));
        assert!(!modal.contains(&"file.new".into()));
    }

    #[test]
    fn lists_every_svelte_shortcut() {
        let keys: Vec<String> = shortcut_list().into_iter().map(|s| s.keys).collect();
        for k in [
            "Ctrl+N",
            "Ctrl+D",
            "Ctrl+Shift+D",
            "Ctrl+Alt+I",
            "Ctrl+O",
            "Ctrl+PgUp",
            "Ctrl+PgDn",
            "Ctrl+Shift+G",
            "Ctrl+Enter",
            "Ctrl+Shift+Enter",
            "Alt+Up",
            "Alt+Down",
            "Ctrl+Shift+V",
            "Ctrl+Shift++",
            "Ctrl+P",
            "Ctrl+H",
            "Ctrl+E",
            "Ctrl+Shift+E",
            "Ctrl+Shift+O",
            "Ctrl+Shift+T",
            "Ctrl+Shift+A",
            "Ctrl+F",
            "Ctrl+Shift+B",
            "Ctrl+L",
            "Ctrl+J",
            "F7",
            "Ctrl+=",
            "Ctrl+-",
            "Ctrl+0",
            "Ctrl+T",
            "Ctrl+Shift+S",
            "Ctrl+Shift+,",
            "Ctrl+,",
            "Ctrl+Shift+I",
            "F1",
            "Ctrl+K",
        ] {
            assert!(keys.iter().any(|x| x == k), "missing {k}");
        }
    }
}
