//! The dialogs: merging a folder, Settings, and About.

pub mod merge;
pub mod settings;

/// The open dialog.
pub enum Overlay {
    Merge(Box<merge::MergeState>),
    Settings(settings::SettingsState),
    About,
}
