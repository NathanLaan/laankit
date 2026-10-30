//! The dialogs: Settings, About, naming a diagram and confirming a delete.

pub mod name;
pub mod settings;

/// The open dialog.
pub enum Overlay {
    Settings(settings::SettingsState),
    About,
    Name(name::NameState),
    /// Delete this diagram?
    ConfirmDelete(String),
}
