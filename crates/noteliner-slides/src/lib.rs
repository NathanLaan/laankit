//! NoteLiner's presentations, natively (plan Option C, Phase 4; see
//! `docs/plans/plan-native-rust-egui.md`).
//!
//! | Module      | Svelte / JS original                                   |
//! |-------------|--------------------------------------------------------|
//! | [`deck`]    | `lib/slides.js`: a note's body → a [`Deck`] (egui-free) |
//! | [`edits`]   | `lib/slideEdits.js`: pure slide edits (egui-free)      |
//! | [`render`]  | `Slide.svelte`: [`SlideView`] draws one slide at any size |
//! | [`pane`]    | `SlidesPane.svelte` ([`SlidesPane`]) and `DeckPreview.svelte` ([`DeckPreview`]) |
//! | [`settings`] | `PresentationSettingsModal.svelte` ([`PresentationSettings`]) |
//! | [`presenter`] | Full-screen presenting ([`Presenter`]); new in the native build |
//!
//! The parsing and edit modules are tested against the JS originals on
//! thousands of generated notes (`tests/differential.rs`).

mod color;
pub mod deck;
pub mod edits;
pub mod pane;
pub mod presenter;
pub mod render;
pub mod settings;

pub use deck::{
    estimate_duration, normalize_presentation, parse_deck, slide_at_line, stage_size, substitute,
    Block, BlockKind, Deck, DirValue, Directives, Layout, Presentation, Slide, SourceRange, Stage,
    ASPECTS,
};
pub use edits::{Direction, EditResult, SlideEdit};
pub use pane::{DeckPreview, PaneAction, SlidesPane};
pub use presenter::{Presenter, PresenterAction};
pub use render::{ImageSource, PassThrough, SlideTheme, SlideView};
pub use settings::{PresentationSettings, SettingsOutcome};
