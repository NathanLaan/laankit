//! Spikes 2 and 4 (see `docs/plans/plan-native-rust-egui.md`, Phase 0): HTML and XHTML reduced
//! to native rich-text blocks and laid out with egui, with no webview.
//!
//! - [`convert`]: sanitize (ThreadLiner's DOMPurify allow-list) and convert to a [`Document`].
//! - [`css`]: the bit of CSS that EPUB books need.
//! - [`render`]: viewport-culled layout, links, images, reading positions.
//! - [`stats`]: what survived conversion.

pub mod convert;
pub mod css;
pub mod fonts;
pub mod headless;
pub mod model;
pub mod render;
pub mod stats;

pub use model::{Align, Block, Document, Span, Style, TextKind};
