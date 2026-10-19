//! Sizes and colours for the renderer, built from the active Marina theme so all six themes work.

use egui::Color32;
use marina_egui::theme::{self, Palette};

use crate::fonts::Face;

/// `<mark>`'s highlight: a softer version of the browser default (`yellow`).
const MARK_BG: Color32 = Color32::from_rgb(0xff, 0xe0, 0x66);
/// Text on [`MARK_BG`], dark in every theme (the browser default is `black`).
const MARK_TEXT: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);

/// How a document looks. Build it with [`RichStyle::from_ctx`] each frame (cheap), then adjust
/// the size, face or column width for the app.
#[derive(Clone, Debug, PartialEq)]
pub struct RichStyle {
    /// Body text size, in points.
    pub size: f32,
    pub face: Face,
    /// The reading column's widest width; narrower windows use their full width.
    pub max_width: f32,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
    pub text: Color32,
    /// Bold text when the bold face is missing.
    pub strong: Color32,
    /// Captions and alt-text placeholders.
    pub muted: Color32,
    pub link: Color32,
    /// Inline `code` background.
    pub code_bg: Color32,
    /// Code block background.
    pub pre_bg: Color32,
    /// The bar beside block quotes.
    pub quote_bar: Color32,
    /// Horizontal rules and table lines.
    pub rule: Color32,
    /// `<mark>`: background and text. Like a browser's default (a yellow highlight with dark
    /// text), which reads in light and dark themes alike.
    pub mark_bg: Color32,
    pub mark_text: Color32,
    /// `<kbd>`: the key cap's outline.
    pub kbd_border: Color32,
    /// Task checkboxes: the box outline, a checked box's fill, and its tick.
    pub check_border: Color32,
    pub check_fill: Color32,
    pub check_mark: Color32,
}

impl RichStyle {
    /// The style for the palette last applied to `ctx` (`marina_egui::theme`).
    pub fn from_ctx(ctx: &egui::Context) -> Self {
        Self::from_palette(&theme::palette(ctx))
    }

    pub fn from_palette(p: &Palette) -> Self {
        Self {
            size: 16.0,
            face: Face::Sans,
            max_width: 720.0,
            line_height: 1.5,
            text: p.text_primary,
            strong: p.text_primary,
            muted: p.text_muted,
            link: p.accent,
            code_bg: p.code_bg,
            pre_bg: p.pre_bg,
            quote_bar: p.blockquote_border,
            rule: p.border,
            mark_bg: MARK_BG,
            mark_text: MARK_TEXT,
            kbd_border: p.border_hover,
            check_border: p.text_muted,
            check_fill: p.accent,
            check_mark: p.accent_on,
        }
    }
}
