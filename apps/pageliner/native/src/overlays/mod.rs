//! The dialogs: Settings, Git Sync, About, and the remove-book confirmation
//! (`confirm(…)` in `LibraryGrid.svelte`).

pub mod settings;
pub mod sync;

use marina_egui::dialog::{self, Confirm};

/// The open dialog.
pub enum Overlay {
    Settings(settings::SettingsState),
    Sync(sync::SyncState),
    About,
    RemoveBook { id: String, title: String },
}

/// `Remove "<title>" from the library? The file will be deleted.`
pub fn remove_book(ctx: &egui::Context, title: &str) -> Option<Confirm> {
    dialog::confirm(
        ctx,
        "pageliner-remove-book",
        "Remove Book",
        &format!("Remove \"{title}\" from the library?"),
        Some("The file will be deleted."),
        "Remove",
    )
}
