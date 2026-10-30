//! The dialogs: Settings and About.

pub mod settings;

/// The open dialog.
pub enum Overlay {
    Settings(settings::SettingsState),
    About,
}
