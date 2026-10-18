//! Sizes and colours for the renderer, built from the active Marina theme so all six themes work.

use egui::Color32;
use marina_egui::theme::{self, Palette};

use crate::fonts::Face;

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
        }
    }
}
