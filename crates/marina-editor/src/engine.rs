//! Which text engine the editor runs on.

/// The editor's engine. [`Engine::Core`] is the default; [`Engine::TextEdit`]
/// is the previous engine, egui's `TextEdit`, kept as a fallback for one
/// release (set `MARINA_EDITOR_ENGINE=textedit`, or [`crate::EditorPrefs::engine`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Engine {
    /// The editor's own widget: rope index, several selections, only the
    /// visible lines laid out, folding.
    Core,
    /// egui's `TextEdit` with the highlighting layouter: one selection, the
    /// whole note laid out, no folding.
    TextEdit,
}

impl Default for Engine {
    fn default() -> Self {
        Self::from_env()
    }
}

impl Engine {
    /// `MARINA_EDITOR_ENGINE=textedit` picks the fallback; anything else (or
    /// nothing) the core engine.
    pub fn from_env() -> Self {
        match std::env::var("MARINA_EDITOR_ENGINE") {
            Ok(v) if v.eq_ignore_ascii_case("textedit") => Self::TextEdit,
            _ => Self::Core,
        }
    }
}
