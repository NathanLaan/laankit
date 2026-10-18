//! NoteLiner's Markdown editor for the native (egui) builds: the counterpart
//! of `apps/noteliner/src/renderer/components/Editor.svelte` (CodeMirror 6).
//! See `README.md` for the API, the host contract and parity.
//!
//! ```ignore
//! let mut state = EditorState::new("note-editor");
//! state.set_text(ctx, note_markdown);
//! // every frame:
//! let out = Editor::new(&mut state).show(ui);
//! for event in out.events {
//!     match event {
//!         EditorEvent::Changed => schedule_save(state.text()),
//!         EditorEvent::PasteImage(image) => { /* save, then state.run(Command::Insert(md)) */ }
//!         _ => {}
//!     }
//! }
//! ```

mod editor;
pub mod edits;
pub mod find;
#[doc(hidden)]
pub mod gen;
pub mod highlight;
pub mod history;
pub mod list_edits;
pub mod spell;
mod state;
pub mod toolbar;

pub use editor::{Editor, EditorEvent, EditorOutput, SaveFormat};
pub use edits::attachment_markdown;
pub use highlight::EditorColors;
pub use list_edits::ListKind;
#[cfg(feature = "spellcheck")]
pub use spell::Hunspell;
pub use spell::SpellChecker;
pub use state::{Command, CursorStatus, EditorPrefs, EditorState, DEFAULT_FONT_SIZE};
