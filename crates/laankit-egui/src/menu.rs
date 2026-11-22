//! Rows of a pop-up menu, with the icons in a column of their own.
//!
//! Font Awesome glyphs differ in width, so a row written as one string
//! ("icon, two spaces, label") starts its label wherever its icon ends.
//! [`item`] gives the icon a box of a fixed width, which lines the icons up at the
//! left and the labels up after them.

use egui::{Align2, Atom, AtomExt as _, Button, IntoAtoms as _, RichText, TextStyle, Ui, Vec2};

/// The width of the icon column, as a multiple of the text size. The widest
/// Font Awesome glyphs are 1.25 em.
const ICON_EM: f32 = 1.3;

/// A menu row: an icon, at the left of the icon column, and a label. `size` is
/// the text size, or `None` for the size of button text.
///
/// The row is a [`Button`], so the caller adds what it needs (a shortcut, no
/// frame) and closes the menu when the row is clicked.
pub fn item<'a>(ui: &Ui, glyph: &'a str, label: &'a str, size: Option<f32>) -> Button<'a> {
    let size = size.unwrap_or_else(|| TextStyle::Button.resolve(ui.style()).size);
    let icon: Atom<'a> = RichText::new(glyph).size(size).into();
    // No taller than the label, so the icon does not change the row's height.
    let icon = icon
        .atom_size(Vec2::new(size * ICON_EM, size))
        .atom_align(Align2::LEFT_CENTER);
    Button::new((icon, RichText::new(label).size(size)).into_atoms())
}
