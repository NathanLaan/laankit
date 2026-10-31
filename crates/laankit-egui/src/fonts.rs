//! Fonts: the bundled faces, the font families the apps draw with, and the
//! system-font fallback for glyphs the bundled faces lack.
//!
//! The Svelte builds use the system UI font (`-apple-system, Segoe UI, …`),
//! which differs per platform. The native builds bundle one family instead so
//! they look the same everywhere:
//!
//! | Family                         | Face                                                |
//! |--------------------------------|-----------------------------------------------------|
//! | `FontFamily::Proportional`     | Inter Regular                                       |
//! | [`SEMIBOLD_FAMILY`]            | Inter SemiBold                                      |
//! | [`BOLD_FAMILY`]                | Inter Bold                                          |
//! | [`ITALIC`]                     | Inter Italic                                        |
//! | [`BOLD_ITALIC`]                | Inter Bold Italic                                   |
//! | `FontFamily::Monospace`        | JetBrains Mono NL Regular (no ligatures)            |
//! | [`MONO_BOLD`]                  | JetBrains Mono NL Bold                              |
//! | [`MONO_ITALIC`]                | JetBrains Mono NL Italic                            |
//! | [`SERIF`], [`SERIF_ITALIC`]    | Source Serif 4 Regular, Italic (books)              |
//! | [`SERIF_BOLD`], [`SERIF_BOLD_ITALIC`] | Source Serif 4 Bold, Bold Italic             |
//! | [`crate::icons::FAMILY`]       | Font Awesome 6 Free Solid                           |
//!
//! The faces are static instances, not variable fonts: shaping text the first
//! time is about twice as fast with them (400 paragraphs: ~40 ms against
//! ~75 ms for variable Inter), which is what a long note or article costs to
//! open. Pick a face with a [`Style`]: [`styled`] / [`bold`] / [`italic`] /
//! [`weight`] for `RichText`, [`format`] for `LayoutJob`s. Font Awesome is also
//! the last fallback of the text families, so icons can sit inside a label.
//!
//! The Inter faces are modified: their `calt` (contextual alternates) feature
//! is retagged `ss20`, because egui can't turn OpenType features off and
//! Inter's `calt` draws `->` and `-->` as arrows, which notes and code must
//! show as typed. The lookups are unchanged. To redo it after updating Inter,
//! with fonttools: for each `GSUB`/`GPOS` `FeatureRecord` whose `FeatureTag`
//! is `calt`, set it to `ss20` and save. JetBrains Mono is the no-ligatures
//! (NL) build for the same reason.
//!
//! Characters none of these have (CJK, emoji, Arabic, Devanagari, …) come from
//! the operating system's own fallback (fontconfig, DirectWrite, CoreText), via
//! eframe's `system_fonts` feature, which loads a font on demand the first time
//! a script is drawn. Contexts eframe didn't create (tests, kittest) get it from
//! [`install_system_fallback`].

use std::sync::Arc;

use egui::text::TextFormat;
use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, RichText, TextStyle};

/// Inter SemiBold (CSS `font-weight: 600`).
pub const SEMIBOLD_FAMILY: &str = "sans-semibold";
/// Inter Bold.
pub const BOLD_FAMILY: &str = "sans-bold";
/// Inter Italic.
pub const ITALIC: &str = "sans-italic";
/// Inter Bold Italic (also semibold italic).
pub const BOLD_ITALIC: &str = "sans-bold-italic";
/// JetBrains Mono Bold (also bold italic).
pub const MONO_BOLD: &str = "mono-bold";
/// JetBrains Mono Italic.
pub const MONO_ITALIC: &str = "mono-italic";
/// Source Serif 4, the reading face for books (`laankit-richtext`'s serif option).
pub const SERIF: &str = "serif";
pub const SERIF_ITALIC: &str = "serif-italic";
pub const SERIF_BOLD: &str = "serif-bold";
pub const SERIF_BOLD_ITALIC: &str = "serif-bold-italic";

/// CSS weights, for [`Style::weight`]. Each maps to the nearest bundled face.
pub const REGULAR: f32 = 400.0;
pub const SEMIBOLD: f32 = 600.0;
pub const BOLD: f32 = 700.0;

const FACES: [(&str, &[u8]); 13] = [
    ("Inter", include_bytes!("../assets/fonts/Inter-Regular.ttf")),
    (
        "Inter-SemiBold",
        include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    ),
    (
        "Inter-Bold",
        include_bytes!("../assets/fonts/Inter-Bold.ttf"),
    ),
    (
        "Inter-Italic",
        include_bytes!("../assets/fonts/Inter-Italic.ttf"),
    ),
    (
        "Inter-BoldItalic",
        include_bytes!("../assets/fonts/Inter-BoldItalic.ttf"),
    ),
    (
        "JetBrainsMono",
        include_bytes!("../assets/fonts/JetBrainsMonoNL-Regular.ttf"),
    ),
    (
        "JetBrainsMono-Bold",
        include_bytes!("../assets/fonts/JetBrainsMonoNL-Bold.ttf"),
    ),
    (
        "JetBrainsMono-Italic",
        include_bytes!("../assets/fonts/JetBrainsMonoNL-Italic.ttf"),
    ),
    (
        "SourceSerif4",
        include_bytes!("../assets/fonts/SourceSerif4-Regular.ttf"),
    ),
    (
        "SourceSerif4-Italic",
        include_bytes!("../assets/fonts/SourceSerif4-It.ttf"),
    ),
    (
        "SourceSerif4-Bold",
        include_bytes!("../assets/fonts/SourceSerif4-Bold.ttf"),
    ),
    (
        "SourceSerif4-BoldItalic",
        include_bytes!("../assets/fonts/SourceSerif4-BoldIt.ttf"),
    ),
    (
        crate::icons::FAMILY,
        include_bytes!("../assets/fonts/fa-solid-900.ttf"),
    ),
];

/// The bundled font files as (name, bytes), for code that lays out text
/// outside egui with the same faces (e.g. `noteliner-export`'s PDFs).
pub fn bundled_faces() -> &'static [(&'static str, &'static [u8])] {
    &FACES
}

/// egui's own icon font (`egui::special_emojis`), kept as a fallback because
/// some built-in widgets draw with it.
const EGUI_ICONS: &str = "egui-icons";

/// The font definitions: bundled faces and the families built from them.
pub fn definitions() -> FontDefinitions {
    let defaults = FontDefinitions::default();
    let mut defs = FontDefinitions::empty();
    for (name, bytes) in FACES {
        defs.font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    if let Some(icons) = defaults.font_data.get(EGUI_ICONS) {
        defs.font_data.insert(EGUI_ICONS.to_owned(), icons.clone());
    }
    let has_egui_icons = defs.font_data.contains_key(EGUI_ICONS);

    let chain = |primary: &str| {
        let mut names = vec![primary.to_owned(), crate::icons::FAMILY.to_owned()];
        if has_egui_icons {
            names.push(EGUI_ICONS.to_owned());
        }
        names
    };
    let families = [
        (FontFamily::Proportional, "Inter"),
        (FontFamily::Name(SEMIBOLD_FAMILY.into()), "Inter-SemiBold"),
        (FontFamily::Name(BOLD_FAMILY.into()), "Inter-Bold"),
        (FontFamily::Name(ITALIC.into()), "Inter-Italic"),
        (FontFamily::Name(BOLD_ITALIC.into()), "Inter-BoldItalic"),
        (FontFamily::Monospace, "JetBrainsMono"),
        (FontFamily::Name(MONO_BOLD.into()), "JetBrainsMono-Bold"),
        (FontFamily::Name(MONO_ITALIC.into()), "JetBrainsMono-Italic"),
        (FontFamily::Name(SERIF.into()), "SourceSerif4"),
        (FontFamily::Name(SERIF_ITALIC.into()), "SourceSerif4-Italic"),
        (FontFamily::Name(SERIF_BOLD.into()), "SourceSerif4-Bold"),
        (
            FontFamily::Name(SERIF_BOLD_ITALIC.into()),
            "SourceSerif4-BoldItalic",
        ),
    ];
    for (family, face) in families {
        defs.families.insert(family, chain(face));
    }
    defs.families.insert(
        FontFamily::Name(crate::icons::FAMILY.into()),
        vec![crate::icons::FAMILY.to_owned()],
    );
    defs
}

/// Installs the bundled fonts and the text sizes of `global.css` (14 px body).
///
/// Under eframe the system fallback is already installed (unless
/// `NativeOptions::system_font_fallback` is off); elsewhere call
/// [`install_system_fallback`] too.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
    ctx.all_styles_mut(|style| {
        use FontFamily::{Monospace, Proportional};
        let body = crate::theme::BODY_SIZE;
        style.text_styles = [
            (TextStyle::Small, FontId::new(11.0, Proportional)),
            (TextStyle::Body, FontId::new(body, Proportional)),
            (TextStyle::Button, FontId::new(body, Proportional)),
            (TextStyle::Heading, FontId::new(18.0, Proportional)),
            (TextStyle::Monospace, FontId::new(13.0, Monospace)),
        ]
        .into();
    });
}

/// Adds the on-demand system-font fallback to a context eframe didn't create.
pub fn install_system_fallback(ctx: &egui::Context) {
    ctx.add_font_provider(Arc::new(
        egui_system_font_provider::SystemFontProvider::new(),
    ));
}

/// `text` in the face for `style`, at its text style's size.
pub fn styled(text: impl Into<RichText>, style: Style) -> RichText {
    text.into().family(style.family())
}

/// `text` in Inter Bold.
pub fn bold(text: impl Into<RichText>) -> RichText {
    styled(text, Style::BOLD)
}

/// `text` at the bundled face nearest a CSS weight (e.g. [`SEMIBOLD`]).
pub fn weight(text: impl Into<RichText>, weight: f32) -> RichText {
    styled(
        text,
        Style {
            weight: Some(weight),
            ..Default::default()
        },
    )
}

/// `text` in the italic face (not egui's synthetic slant).
pub fn italic(text: impl Into<RichText>) -> RichText {
    styled(text, Style::ITALIC)
}

/// The text face at `size` points.
pub fn sans(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// The italic text face at `size` points.
pub fn sans_italic(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ITALIC.into()))
}

/// The monospace face at `size` points.
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// The italic monospace face at `size` points.
pub fn mono_italic(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MONO_ITALIC.into()))
}

/// How a run of text is set.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Style {
    /// A CSS weight; `None` is [`REGULAR`]. Below 550 is regular, below 650
    /// semibold, otherwise bold (monospace and italics have no semibold).
    pub weight: Option<f32>,
    pub italic: bool,
    pub mono: bool,
}

impl Style {
    pub const BOLD: Style = Style {
        weight: Some(BOLD),
        italic: false,
        mono: false,
    };
    pub const ITALIC: Style = Style {
        weight: None,
        italic: true,
        mono: false,
    };
    pub const BOLD_ITALIC: Style = Style {
        weight: Some(BOLD),
        italic: true,
        mono: false,
    };
    pub const MONO: Style = Style {
        weight: None,
        italic: false,
        mono: true,
    };

    /// The font family of the bundled face for this style.
    pub fn family(&self) -> FontFamily {
        let w = self.weight.unwrap_or(REGULAR);
        let name = match (self.mono, self.italic) {
            (false, false) if w < 550.0 => return FontFamily::Proportional,
            (false, false) if w < 650.0 => SEMIBOLD_FAMILY,
            (false, false) => BOLD_FAMILY,
            (false, true) if w < 550.0 => ITALIC,
            (false, true) => BOLD_ITALIC,
            (true, _) if w >= 550.0 => MONO_BOLD,
            (true, false) => return FontFamily::Monospace,
            (true, true) => MONO_ITALIC,
        };
        FontFamily::Name(name.into())
    }

    /// The font for this style at `size` points.
    pub fn font(&self, size: f32) -> FontId {
        FontId::new(size, self.family())
    }
}

/// A `TextFormat` for `style` at `size` points in `color`.
pub fn format(style: Style, size: f32, color: Color32) -> TextFormat {
    TextFormat {
        font_id: style.font(size),
        color,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> egui::Context {
        let ctx = egui::Context::default();
        install(&ctx);
        // Fonts are loaded at the start of a pass.
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        ctx
    }

    #[test]
    fn bundled_faces_cover_latin_and_icons() {
        let ctx = ctx();
        let text = "AaZz09 ÀéîõüçñßŒ “quotes” — … € £ ¥ © ™ ΑΩ αω ЖЯжя";
        ctx.fonts_mut(|f| {
            for family in [
                FontFamily::Proportional,
                FontFamily::Name(ITALIC.into()),
                FontFamily::Name(SEMIBOLD_FAMILY.into()),
                FontFamily::Name(BOLD_FAMILY.into()),
                FontFamily::Name(BOLD_ITALIC.into()),
                FontFamily::Monospace,
                FontFamily::Name(MONO_BOLD.into()),
                FontFamily::Name(MONO_ITALIC.into()),
            ] {
                let font = FontId::new(14.0, family.clone());
                for c in text.chars().filter(|c| !c.is_whitespace()) {
                    assert!(f.has_glyph(&font, c), "{family:?} lacks {c:?}");
                }
            }
            let icons = crate::icons::font(14.0);
            for glyph in [
                crate::icons::BARS,
                crate::icons::XMARK,
                crate::icons::WINDOW_MINIMIZE,
                crate::icons::WINDOW_MAXIMIZE,
                crate::icons::WINDOW_RESTORE,
                crate::icons::GEAR,
            ] {
                let c = glyph.chars().next().unwrap();
                assert!(f.has_glyph(&icons, c), "icon font lacks {c:?}");
                assert!(
                    f.has_glyph(&sans(14.0), c),
                    "text family can't fall back to icons"
                );
            }
        });
    }

    #[test]
    fn bold_is_wider_than_regular() {
        let ctx = ctx();
        ctx.fonts_mut(|f| {
            let regular = f.layout_job(egui::text::LayoutJob::single_section(
                "Bold text".into(),
                format(Style::default(), 14.0, Color32::WHITE),
            ));
            let bold = f.layout_job(egui::text::LayoutJob::single_section(
                "Bold text".into(),
                format(Style::BOLD, 14.0, Color32::WHITE),
            ));
            assert!(bold.size().x > regular.size().x, "the bold face isn't used");
        });
    }

    #[test]
    fn style_picks_the_family() {
        assert_eq!(Style::default().font(12.0), sans(12.0));
        assert_eq!(Style::ITALIC.font(12.0), sans_italic(12.0));
        assert_eq!(Style::MONO.font(12.0), mono(12.0));
        let both = Style {
            italic: true,
            mono: true,
            ..Default::default()
        };
        assert_eq!(both.font(12.0), mono_italic(12.0));
        let family = |name: &str| FontFamily::Name(name.into());
        assert_eq!(Style::BOLD.family(), family(BOLD_FAMILY));
        assert_eq!(Style::BOLD_ITALIC.family(), family(BOLD_ITALIC));
        let semibold = Style {
            weight: Some(SEMIBOLD),
            ..Default::default()
        };
        assert_eq!(semibold.family(), family(SEMIBOLD_FAMILY));
        let mono_bold = Style {
            mono: true,
            ..Style::BOLD_ITALIC
        };
        assert_eq!(mono_bold.family(), family(MONO_BOLD));
        assert_eq!(
            Style {
                weight: Some(500.0),
                ..Default::default()
            }
            .family(),
            FontFamily::Proportional
        );
    }

    /// The bundled Inter has its `calt` feature retagged (see the module
    /// docs), so `->` and `-->` stay as typed. A font update must redo that.
    #[test]
    fn inter_has_no_contextual_arrows() {
        for (name, bytes) in bundled_faces()
            .iter()
            .filter(|(n, _)| n.starts_with("Inter"))
        {
            assert!(
                !bytes.windows(4).any(|w| w == b"calt"),
                "{name} still has a `calt` feature"
            );
        }
    }
}
