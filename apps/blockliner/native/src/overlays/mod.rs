//! The dialogs: Settings, About, Export, naming a diagram, confirming a
//! delete, and a custom colour.

pub mod colour;
pub mod export;
pub mod name;
pub mod settings;

/// The open dialog.
pub enum Overlay {
    Settings(settings::SettingsState),
    About,
    Export(export::ExportState),
    Name(name::NameState),
    /// A hex value for the selected shapes' fill or stroke.
    Colour(colour::ColourState),
    /// Delete this diagram?
    ConfirmDelete(String),
}
