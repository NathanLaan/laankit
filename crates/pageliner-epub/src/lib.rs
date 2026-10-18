//! PageLiner's native EPUB reader (plan Option C, Phase 3): no webview, no epub.js.
//!
//! - [`book`]: [`Book`], an EPUB opened from a path or bytes: metadata, spine, table of
//!   contents, and the whole book as one `marina_richtext::Document`.
//! - [`view`]: [`EpubView`], the reading screen; it returns [`EpubEvent`]s for the host to
//!   persist and act on.
//! - [`state`]: what's saved: [`NativeState`] per book (under `"native"` in `state/<id>.json`)
//!   and [`ReaderPrefs`] in the UI preferences.
//! - [`annotations`]: the Annotations pane.
//!
//! See the README for the host's side of the contract.

pub mod annotations;
pub mod book;
pub mod state;
pub mod view;

pub use book::{Book, TocEntry};
pub use state::{Bookmark, Highlight, Location, NativeState, ReaderPrefs, ReadingTheme, STATE_KEY};
pub use view::{EpubEvent, EpubView, HIGHLIGHT};
