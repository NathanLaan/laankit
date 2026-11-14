//! The dialogs: signing in, Settings, About, and two confirmations.

pub mod settings;
pub mod signin;

/// The open dialog.
pub enum Overlay {
    SignIn(signin::SignInState),
    Settings(settings::SettingsState),
    About,
    ConfirmSignOut,
    /// The keychain refused the token: keep it in a file?
    KeychainFallback,
}
