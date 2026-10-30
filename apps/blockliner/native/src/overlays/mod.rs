//! The dialogs: Settings, About, Export, naming a diagram and confirming a
//! delete.

pub mod export;
pub mod name;
pub mod settings;

/// The open dialog.
pub enum Overlay {
    Settings(settings::SettingsState),
    About,
    Export(export::ExportState),
    Name(name::NameState),
    /// Delete this diagram?
    ConfirmDelete(String),
}
