//! Font Awesome 6 Free icons, as used across the Svelte UI (`<i class="fas fa-…">`).
//!
//! Each constant is the glyph's codepoint in the `fa-solid-900` face, which
//! [`crate::fonts`] registers as the [`FAMILY`] font family. Draw one with
//! [`icon`] (sized like the surrounding text) or [`icon_sized`].
//!
//! Generated from `@fortawesome/fontawesome-free` 6.7.2 (`css/all.css`); add
//! more constants the same way. Font Awesome Free fonts are under the SIL OFL
//! 1.1 and the icons under CC BY 4.0 (see `NOTICE`).

use egui::{FontFamily, FontId, RichText};

/// The font family name the solid icon face is registered under.
pub const FAMILY: &str = "fa-solid";

/// An icon as text in the icon family, at the body size.
pub fn icon(glyph: &str) -> RichText {
    icon_sized(glyph, crate::theme::BODY_SIZE)
}

/// An icon as text in the icon family, at `size` points.
pub fn icon_sized(glyph: &str, size: f32) -> RichText {
    RichText::new(glyph).font(font(size))
}

/// The icon font at `size` points, for painting glyphs directly.
pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FAMILY.into()))
}

pub const ARROW_DOWN: &str = "\u{f063}";
pub const ARROW_LEFT: &str = "\u{f060}";
pub const ARROW_RIGHT: &str = "\u{f061}";
pub const ARROWS_ROTATE: &str = "\u{f021}";
pub const ARROWS_UP_DOWN_LEFT_RIGHT: &str = "\u{f047}";
pub const ARROW_UP: &str = "\u{f062}";
pub const BARS: &str = "\u{f0c9}";
pub const BARS_STAGGERED: &str = "\u{f550}";
pub const BOLT: &str = "\u{f0e7}";
pub const BOOK: &str = "\u{f02d}";
pub const BOOKMARK: &str = "\u{f02e}";
pub const BOOK_OPEN: &str = "\u{f518}";
pub const CALENDAR: &str = "\u{f133}";
pub const CHECK: &str = "\u{f00c}";
pub const CHECK_DOUBLE: &str = "\u{f560}";
pub const CHEVRON_DOWN: &str = "\u{f078}";
pub const CHEVRON_LEFT: &str = "\u{f053}";
pub const CHEVRON_RIGHT: &str = "\u{f054}";
pub const CHEVRON_UP: &str = "\u{f077}";
pub const CIRCLE: &str = "\u{f111}";
pub const CIRCLE_CHECK: &str = "\u{f058}";
pub const CIRCLE_INFO: &str = "\u{f05a}";
pub const CIRCLE_PLAY: &str = "\u{f144}";
pub const CIRCLE_QUESTION: &str = "\u{f059}";
pub const CLOCK_ROTATE_LEFT: &str = "\u{f1da}";
pub const CLONE: &str = "\u{f24d}";
pub const CLOUD_ARROW_DOWN: &str = "\u{f0ed}";
pub const CLOUD_ARROW_UP: &str = "\u{f0ee}";
pub const CODE_BRANCH: &str = "\u{f126}";
pub const COMMENT_DOTS: &str = "\u{f4ad}";
pub const COMPRESS: &str = "\u{f066}";
pub const COPY: &str = "\u{f0c5}";
pub const EXCLAMATION_CIRCLE: &str = "\u{f06a}";
pub const EXTERNAL_LINK_ALT: &str = "\u{f35d}";
pub const EYE: &str = "\u{f06e}";
pub const FILE: &str = "\u{f15b}";
pub const FILE_AUDIO: &str = "\u{f1c7}";
pub const FILE_CIRCLE_PLUS: &str = "\u{e494}";
pub const FILE_CIRCLE_QUESTION: &str = "\u{e4ef}";
pub const FILE_CODE: &str = "\u{f1c9}";
pub const FILE_CSV: &str = "\u{f6dd}";
pub const FILE_EXCEL: &str = "\u{f1c3}";
pub const FILE_IMPORT: &str = "\u{f56f}";
pub const FILE_LINES: &str = "\u{f15c}";
pub const FILE_PDF: &str = "\u{f1c1}";
pub const FILE_VIDEO: &str = "\u{f1c8}";
pub const FILE_WORD: &str = "\u{f1c2}";
pub const FILE_ZIPPER: &str = "\u{f1c6}";
pub const FILTER: &str = "\u{f0b0}";
pub const FOLDER: &str = "\u{f07b}";
pub const FOLDER_OPEN: &str = "\u{f07c}";
pub const GEAR: &str = "\u{f013}";
pub const HIGHLIGHTER: &str = "\u{f591}";
pub const HOUSE: &str = "\u{f015}";
pub const INDENT: &str = "\u{f03c}";
pub const LAYER_GROUP: &str = "\u{f5fd}";
pub const LINK: &str = "\u{f0c1}";
pub const LIST_OL: &str = "\u{f0cb}";
pub const LIST_UL: &str = "\u{f0ca}";
pub const MAGNIFYING_GLASS: &str = "\u{f002}";
pub const MINUS: &str = "\u{f068}";
pub const OBJECT_GROUP: &str = "\u{f247}";
pub const PAPERCLIP: &str = "\u{f0c6}";
pub const PASTE: &str = "\u{f0ea}";
pub const PEN: &str = "\u{f304}";
pub const PERSON_CHALKBOARD: &str = "\u{e53d}";
pub const PLUG: &str = "\u{f1e6}";
pub const PLUS: &str = "\u{2b}";
pub const ROTATE: &str = "\u{f2f1}";
pub const ROTATE_LEFT: &str = "\u{f2ea}";
pub const RSS: &str = "\u{f09e}";
pub const SCISSORS: &str = "\u{f0c4}";
pub const SHIELD_HALVED: &str = "\u{f3ed}";
pub const SLIDERS: &str = "\u{f1de}";
pub const SPINNER: &str = "\u{f110}";
pub const SQUARE: &str = "\u{f0c8}";
pub const SQUARE_CHECK: &str = "\u{f14a}";
pub const TABLE_CELLS_LARGE: &str = "\u{f009}";
pub const TAG: &str = "\u{f02b}";
pub const TAGS: &str = "\u{f02c}";
pub const TERMINAL: &str = "\u{f120}";
pub const TIMES: &str = "\u{f00d}";
pub const TRASH: &str = "\u{f1f8}";
pub const TRASH_CAN: &str = "\u{f2ed}";
pub const TRIANGLE_EXCLAMATION: &str = "\u{f071}";
pub const USER: &str = "\u{f007}";
pub const WINDOW_MAXIMIZE: &str = "\u{f2d0}";
pub const WINDOW_MINIMIZE: &str = "\u{f2d1}";
pub const WINDOW_RESTORE: &str = "\u{f2d2}";
pub const XMARK: &str = "\u{f00d}";
