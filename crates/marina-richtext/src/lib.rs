//! Sanitized HTML/XHTML reduced to native rich-text blocks and laid out with egui, with no
//! webview. Shared by ThreadLiner (articles), PageLiner (EPUB chapters) and, later, the NoteLiner
//! Markdown preview. Promoted from the Phase 0 spikes 2 and 4 (see
//! `docs/plans/plan-native-rust-egui.md`).
//!
//! - [`convert`]: sanitize (ThreadLiner's DOMPurify allow-list) and convert to a [`Document`].
//! - [`css`]: the bit of CSS that EPUB books need.
//! - [`view`]: [`RichView`], progressive layout with links, images and reading positions.
//! - [`resolve`]: [`Resolver`], where images come from and what links do.
//! - [`style`]: [`RichStyle`], sizes and colours from the active Marina theme.
//! - [`fonts`]: the font family names the view asks for.

pub mod convert;
pub mod css;
pub mod fonts;
pub mod model;
pub mod resolve;
pub mod style;
pub mod view;

pub use convert::{from_article_html, from_html, from_xhtml, Base, Options};
pub use fonts::Face;
pub use model::{Align, Block, Document, Span, Style, TextKind};
pub use resolve::{BundleResolver, LinkAction, Resolver, WebResolver};
pub use style::RichStyle;
pub use view::{Action, FrameStats, Position, RichOutput, RichView};
