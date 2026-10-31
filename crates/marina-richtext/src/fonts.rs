//! The fonts the renderer asks for. `marina_egui::fonts` installs them from the bundled faces;
//! this crate never reads system fonts.
//!
//! | Text          | Family                                                                  |
//! |---------------|-------------------------------------------------------------------------|
//! | sans          | the face `marina_egui::fonts::Style` picks: Inter Regular, Bold, Italic, Bold Italic |
//! | serif         | Source Serif 4: `marina_egui::fonts::{SERIF, SERIF_ITALIC, SERIF_BOLD, SERIF_BOLD_ITALIC}` |
//! | code          | JetBrains Mono Regular, Bold or Italic                                  |
//!
//! Whatever isn't installed falls back to a face that is (serif to sans): italics slanted by
//! egui, and bold drawn in the strong text colour. So the view never asks egui for a family it
//! doesn't have, which would panic.

use egui::FontFamily;
use marina_egui::fonts::Style;

pub use marina_egui::fonts::{SERIF, SERIF_BOLD, SERIF_BOLD_ITALIC, SERIF_ITALIC};

/// Which reading face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Face {
    #[default]
    Sans,
    Serif,
}

/// A resolved font: the family, and what has to be faked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolved {
    pub family: FontFamily,
    pub fake_bold: bool,
    pub fake_italic: bool,
}

/// The font families a context's fonts offer. Cheap to build; [`crate::RichView`] builds one
/// per frame.
#[derive(Clone, Debug, Default)]
pub struct Families {
    names: Vec<FontFamily>,
}

impl Families {
    pub fn of(ctx: &egui::Context) -> Self {
        ctx.fonts(|f| Self {
            names: f.definitions().families.keys().cloned().collect(),
        })
    }

    fn has(&self, family: &FontFamily) -> bool {
        matches!(family, FontFamily::Proportional | FontFamily::Monospace)
            || self.names.contains(family)
    }

    /// The font for `face` in bold and/or italic, falling back as the module docs describe.
    pub fn resolve(&self, face: Face, bold: bool, italic: bool) -> Resolved {
        if face == Face::Serif && self.has(&FontFamily::Name(SERIF.into())) {
            let name = |b: bool, i: bool| match (b, i) {
                (false, false) => SERIF,
                (false, true) => SERIF_ITALIC,
                (true, false) => SERIF_BOLD,
                (true, true) => SERIF_BOLD_ITALIC,
            };
            return self.first_installed(bold, italic, |b, i| FontFamily::Name(name(b, i).into()));
        }
        self.pick(false, bold, italic)
    }

    /// The code font in bold and/or italic.
    pub fn code(&self, bold: bool, italic: bool) -> Resolved {
        self.pick(true, bold, italic)
    }

    /// The best installed sans or mono face for the style.
    fn pick(&self, mono: bool, bold: bool, italic: bool) -> Resolved {
        self.first_installed(bold, italic, |b, i| {
            Style {
                weight: b.then_some(marina_egui::fonts::BOLD),
                italic: i,
                mono,
            }
            .family()
        })
    }

    /// The first installed of the style's faces, trying to keep bold, then italic.
    fn first_installed(
        &self,
        bold: bool,
        italic: bool,
        family: impl Fn(bool, bool) -> FontFamily,
    ) -> Resolved {
        let candidates = [
            (bold, italic),
            (bold, false),
            (false, italic),
            (false, false),
        ];
        for (b, i) in candidates {
            let f = family(b, i);
            if self.has(&f) {
                return Resolved {
                    family: f,
                    fake_bold: bold && !b,
                    fake_italic: italic && !i,
                };
            }
        }
        Resolved {
            family: FontFamily::Proportional,
            fake_bold: bold,
            fake_italic: italic,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_without_marina_fonts() {
        let ctx = egui::Context::default();
        ctx.run_ui(Default::default(), |_| {})
            .drop_without_applying_deltas();
        let r = Families::of(&ctx).resolve(Face::Serif, true, true);
        assert_eq!(r.family, FontFamily::Proportional);
        assert!(r.fake_bold && r.fake_italic);
    }

    #[test]
    fn uses_marina_fonts() {
        let ctx = egui::Context::default();
        marina_egui::fonts::install(&ctx);
        ctx.run_ui(Default::default(), |_| {})
            .drop_without_applying_deltas();
        let fams = Families::of(&ctx);
        let r = fams.resolve(Face::Sans, true, true);
        assert_eq!(
            r.family,
            FontFamily::Name(marina_egui::fonts::BOLD_ITALIC.into())
        );
        assert!(!r.fake_bold && !r.fake_italic, "{r:?}");
        let code = fams.code(true, false);
        assert_eq!(
            code.family,
            FontFamily::Name(marina_egui::fonts::MONO_BOLD.into())
        );
        let serif = fams.resolve(Face::Serif, true, true);
        assert_eq!(serif.family, FontFamily::Name(SERIF_BOLD_ITALIC.into()));
        assert!(!serif.fake_bold && !serif.fake_italic);
    }
}
