//! MediaLiner's commands and keyboard shortcuts, registered with
//! laankit-egui's command registry so the same list drives the shortcuts,
//! the command palette, and Settings → Keyboard Shortcuts.
//!
//! The arrow keys depend on where you last clicked: in the Folders tree
//! they move through the folders; in the Files pane or the grid they move
//! the photo selection; in the preview Left and Right step through photos.

use egui::{Key, Modifiers};
use laankit_egui::commands::{Binding, Command, Registry, ShortcutEntry};

/// Everything a shortcut, button, or palette entry can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    AddFolder,
    RemoveFolder,
    Refresh,
    /// The preview back to the grid.
    Back,
    OpenPreview,
    PreviousPhoto,
    NextPhoto,
    /// The selection in the grid, by one row.
    PhotoAbove,
    PhotoBelow,
    FirstPhoto,
    LastPhoto,
    FolderAbove,
    FolderBelow,
    /// Collapse, or go to the parent.
    FolderOut,
    /// Expand, or go to the first subfolder.
    FolderIn,
    /// The thumbnail size in the grid, the zoom in the preview.
    ZoomIn,
    ZoomOut,
    ZoomFit,
    ZoomActual,
    ShowInfo,
    ToggleFolders,
    ToggleFiles,
    ToggleSubfolders,
    ToggleStatusBar,
    MergeInto,
    UndoMerge,
    OpenInFolder,
    OpenWithDefault,
    CopyPath,
    Settings,
    About,
    Palette,
}

/// Where the arrow keys go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    Tree,
    #[default]
    Content,
}

/// What the commands' `when` predicates read.
#[derive(Clone, Copy, Debug, Default)]
pub struct View {
    pub modal_open: bool,
    pub focus: Focus,
    pub folder_selected: bool,
    pub root_selected: bool,
    pub photo_selected: bool,
    pub has_photos: bool,
    pub preview_open: bool,
    pub can_undo_merge: bool,
}

fn ready(v: &View) -> bool {
    !v.modal_open
}

fn in_tree(v: &View) -> bool {
    ready(v) && v.focus == Focus::Tree && v.folder_selected
}

fn in_grid(v: &View) -> bool {
    ready(v) && v.focus == Focus::Content && !v.preview_open && v.has_photos
}

fn in_preview(v: &View) -> bool {
    ready(v) && v.preview_open
}

fn photo(v: &View) -> bool {
    ready(v) && v.photo_selected
}

fn grid_photo(v: &View) -> bool {
    in_grid(v) && v.photo_selected
}

fn folder(v: &View) -> bool {
    ready(v) && v.folder_selected
}

fn root(v: &View) -> bool {
    ready(v) && v.root_selected
}

fn undo(v: &View) -> bool {
    ready(v) && v.can_undo_merge
}

pub fn registry() -> Registry<Act, View> {
    let mut r = Registry::new();
    let none = Modifiers::NONE;
    let cmd = Modifiers::COMMAND;
    let shift = cmd | Modifiers::SHIFT;
    let add =
        |id: &str, label: &str, section: &str, act: Act| Command::new(id, label, section, act);
    let key = Binding::logical;

    r.register(
        add("folders.add", "Add Folder…", "Folders", Act::AddFolder)
            .key(key(cmd, Key::O))
            .shortcut("Ctrl+O")
            .when(ready),
    );
    r.register(
        add(
            "folders.remove",
            "Remove Folder from List",
            "Folders",
            Act::RemoveFolder,
        )
        .when(root),
    );
    r.register(
        add(
            "folders.merge",
            "Merge Folder Into…",
            "Folders",
            Act::MergeInto,
        )
        .when(folder),
    );
    r.register(
        add(
            "folders.undoMerge",
            "Undo Last Merge",
            "Folders",
            Act::UndoMerge,
        )
        .key(key(cmd, Key::Z))
        .shortcut("Ctrl+Z")
        .when(undo),
    );
    r.register(
        add("folders.refresh", "Refresh", "Folders", Act::Refresh)
            .key(key(none, Key::F5))
            .shortcut("F5")
            .when(folder),
    );
    r.register(
        add(
            "folders.toggleSubfolders",
            "Toggle Include Subfolders",
            "Folders",
            Act::ToggleSubfolders,
        )
        .when(folder),
    );

    // The tree's arrows come first: they win while the tree has the keys.
    r.register(
        add("tree.up", "Previous Folder", "Navigate", Act::FolderAbove)
            .key(key(none, Key::ArrowUp))
            .shortcut("Up (Folders)")
            .when(in_tree),
    );
    r.register(
        add("tree.down", "Next Folder", "Navigate", Act::FolderBelow)
            .key(key(none, Key::ArrowDown))
            .shortcut("Down (Folders)")
            .when(in_tree),
    );
    r.register(
        add("tree.out", "Collapse Folder", "Navigate", Act::FolderOut)
            .key(key(none, Key::ArrowLeft))
            .shortcut("Left (Folders)")
            .when(in_tree),
    );
    r.register(
        add("tree.in", "Expand Folder", "Navigate", Act::FolderIn)
            .key(key(none, Key::ArrowRight))
            .shortcut("Right (Folders)")
            .when(in_tree),
    );

    r.register(
        add("photo.back", "Back to Grid", "Photos", Act::Back)
            .key(key(none, Key::Escape))
            .key(key(none, Key::Backspace))
            .key(key(Modifiers::ALT, Key::ArrowLeft))
            .shortcut("Esc")
            .when(in_preview),
    );
    r.register(
        add("photo.open", "Open Preview", "Photos", Act::OpenPreview)
            .key(key(none, Key::Enter))
            .shortcut("Enter")
            .when(grid_photo),
    );
    r.register(
        add(
            "photo.previous",
            "Previous Photo",
            "Photos",
            Act::PreviousPhoto,
        )
        .key(key(none, Key::ArrowLeft))
        .key(key(none, Key::PageUp))
        .shortcut("Left")
        .when(|v| in_preview(v) || in_grid(v)),
    );
    r.register(
        add("photo.next", "Next Photo", "Photos", Act::NextPhoto)
            .key(key(none, Key::ArrowRight))
            .key(key(none, Key::PageDown))
            .shortcut("Right")
            .when(|v| in_preview(v) || in_grid(v)),
    );
    r.register(
        add("photo.above", "Photo Above", "Photos", Act::PhotoAbove)
            .key(key(none, Key::ArrowUp))
            .shortcut("Up")
            .when(in_grid),
    );
    r.register(
        add("photo.below", "Photo Below", "Photos", Act::PhotoBelow)
            .key(key(none, Key::ArrowDown))
            .shortcut("Down")
            .when(in_grid),
    );
    r.register(
        add("photo.first", "First Photo", "Photos", Act::FirstPhoto)
            .key(key(none, Key::Home))
            .shortcut("Home")
            .when(|v| in_preview(v) || in_grid(v)),
    );
    r.register(
        add("photo.last", "Last Photo", "Photos", Act::LastPhoto)
            .key(key(none, Key::End))
            .shortcut("End")
            .when(|v| in_preview(v) || in_grid(v)),
    );
    r.register(
        add(
            "photo.openInFolder",
            "Open in Folder",
            "Photos",
            Act::OpenInFolder,
        )
        .key(Binding::physical(shift, Key::R))
        .shortcut("Ctrl+Shift+R")
        .when(photo),
    );
    r.register(
        add(
            "photo.openWith",
            "Open With Default App",
            "Photos",
            Act::OpenWithDefault,
        )
        .when(photo),
    );
    r.register(add("photo.copyPath", "Copy Path", "Photos", Act::CopyPath).when(photo));

    r.register(
        add("view.zoomIn", "Zoom In", "View", Act::ZoomIn)
            .key(key(cmd, Key::Equals))
            .key(Binding::loose(cmd, Key::Plus))
            .shortcut("Ctrl+=")
            .when(ready),
    );
    r.register(
        add("view.zoomOut", "Zoom Out", "View", Act::ZoomOut)
            .key(key(cmd, Key::Minus))
            .shortcut("Ctrl+-")
            .when(ready),
    );
    r.register(
        add("view.zoomFit", "Fit to Window", "View", Act::ZoomFit)
            .key(key(none, Key::Num0))
            .shortcut("0")
            .when(in_preview),
    );
    r.register(
        add("view.zoomActual", "Actual Size", "View", Act::ZoomActual)
            .key(key(none, Key::Num1))
            .shortcut("1")
            .when(in_preview),
    );
    r.register(
        add("view.info", "Show Info", "View", Act::ShowInfo)
            .key(key(cmd, Key::I))
            .shortcut("Ctrl+I")
            .when(ready),
    );
    r.register(
        add(
            "view.toggleFolders",
            "Toggle Folders Pane",
            "View",
            Act::ToggleFolders,
        )
        .key(key(cmd, Key::B))
        .shortcut("Ctrl+B")
        .when(ready),
    );
    r.register(
        add(
            "view.toggleFiles",
            "Toggle Files Pane",
            "View",
            Act::ToggleFiles,
        )
        .key(Binding::physical(shift, Key::F))
        .shortcut("Ctrl+Shift+F")
        .when(ready),
    );
    r.register(
        add(
            "view.toggleStatusBar",
            "Toggle Status Bar",
            "View",
            Act::ToggleStatusBar,
        )
        .key(key(cmd, Key::J))
        .shortcut("Ctrl+J")
        .when(ready),
    );
    r.register(
        add("app.settings", "Settings", "App", Act::Settings)
            .key(key(cmd, Key::Comma))
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
            .key(key(cmd, Key::K))
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
    fn arrows_follow_the_focus() {
        let r = registry();
        let first = |v: &View, k: Key| {
            r.commands()
                .iter()
                .find(|c| {
                    c.bindings
                        .iter()
                        .any(|b| b.matches(k, None, Modifiers::NONE))
                        && Registry::is_applicable(c, v)
                })
                .map(|c| c.action)
        };
        let mut v = View {
            folder_selected: true,
            has_photos: true,
            ..Default::default()
        };
        assert_eq!(first(&v, Key::ArrowDown), Some(Act::PhotoBelow));
        assert_eq!(first(&v, Key::ArrowLeft), Some(Act::PreviousPhoto));
        v.focus = Focus::Tree;
        assert_eq!(first(&v, Key::ArrowDown), Some(Act::FolderBelow));
        assert_eq!(first(&v, Key::ArrowLeft), Some(Act::FolderOut));
        v.preview_open = true;
        v.focus = Focus::Content;
        assert_eq!(first(&v, Key::ArrowRight), Some(Act::NextPhoto));
        assert_eq!(first(&v, Key::ArrowDown), None);
        assert_eq!(first(&v, Key::Escape), Some(Act::Back));
        v.modal_open = true;
        assert_eq!(first(&v, Key::Escape), None);
    }
}
