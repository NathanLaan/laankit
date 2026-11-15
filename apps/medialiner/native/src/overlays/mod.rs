//! The dialogs: importing from a device, merging a folder, Settings, and
//! About.

pub mod import;
pub mod merge;
pub mod settings;

/// The open dialog.
pub enum Overlay {
    Import(Box<import::ImportState>),
    Merge(Box<merge::MergeState>),
    Settings(settings::SettingsState),
    About,
}
