//! Fonts: the bundled faces, the font families the apps draw with, and the
//! system-font fallback for glyphs the bundled faces lack.
//!
//! The apps bundle their fonts, so they look the same on every platform:
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
//! | [`editor_family`]              | The font chosen for the editor ([`EditorFont`])     |
//! | [`diagram_family`]             | The font chosen for diagram labels                  |
//!
//! The editor and diagram families are Inter until an app picks another
//! [`EditorFont`] with [`install_editor_font`] or [`install_diagram_font`].
//! Only the chosen fonts are registered with egui, so the others cost no
//! memory and no room in the glyph atlas.
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

/// The font definitions: bundled faces and the families built from them,
/// with Inter as the editor and diagram font.
pub fn definitions() -> FontDefinitions {
    definitions_for(Chosen::default())
}

/// The font definitions with the chosen editor and diagram fonts.
fn definitions_for(chosen: Chosen) -> FontDefinitions {
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

    // A chosen face first, then the Inter face of the same style for the
    // glyphs it lacks.
    let mut chosen_families = |font: EditorFont, names: [&str; 4]| {
        let inter = EditorFont::Inter.faces();
        for ((family, face), fallback) in names.into_iter().zip(font.faces()).zip(inter) {
            defs.font_data
                .entry(face.0.to_owned())
                .or_insert_with(|| Arc::new(FontData::from_static(face.1)));
            let mut names = chain(face.0);
            if face.0 != fallback.0 {
                names.insert(1, fallback.0.to_owned());
            }
            defs.families.insert(FontFamily::Name(family.into()), names);
        }
    };
    chosen_families(chosen.editor, EDITOR_FAMILIES);
    chosen_families(chosen.diagram, DIAGRAM_FAMILIES);
    // A family for each choice's regular face, for a list that shows each
    // font's name in that font. It is Inter until the list asks for them.
    for font in EditorFont::ALL {
        let shown = if chosen.previews {
            font
        } else {
            EditorFont::Inter
        };
        let face = shown.faces()[0];
        defs.font_data
            .entry(face.0.to_owned())
            .or_insert_with(|| Arc::new(FontData::from_static(face.1)));
        let mut names = chain(face.0);
        if face.0 != "Inter" {
            names.insert(1, "Inter".to_owned());
        }
        defs.families.insert(font.preview_family(), names);
    }
    // Code in the editor: the chosen font when it is monospace, so that
    // everything lines up, and JetBrains Mono otherwise.
    let mono = match chosen.editor.kind() {
        FontKind::Monospace => chosen.editor,
        FontKind::Proportional => EditorFont::JetBrainsMono,
    };
    let jetbrains = EditorFont::JetBrainsMono.faces();
    for ((family, face), fallback) in EDITOR_MONO_FAMILIES
        .into_iter()
        .zip(mono.faces())
        .zip(jetbrains)
    {
        defs.font_data
            .entry(face.0.to_owned())
            .or_insert_with(|| Arc::new(FontData::from_static(face.1)));
        defs.font_data
            .entry(fallback.0.to_owned())
            .or_insert_with(|| Arc::new(FontData::from_static(fallback.1)));
        let mut names = chain(face.0);
        if face.0 != fallback.0 {
            names.insert(1, fallback.0.to_owned());
        }
        defs.families.insert(FontFamily::Name(family.into()), names);
    }
    defs
}

/// The families of the font chosen for the editor: regular, bold, italic,
/// and bold italic.
const EDITOR_FAMILIES: [&str; 4] = [
    "editor",
    "editor-bold",
    "editor-italic",
    "editor-bold-italic",
];
/// The families of the editor's code font, in the same order.
const EDITOR_MONO_FAMILIES: [&str; 4] = [
    "editor-mono",
    "editor-mono-bold",
    "editor-mono-italic",
    "editor-mono-bold-italic",
];
/// The families of the font chosen for diagram labels, in the same order.
const DIAGRAM_FAMILIES: [&str; 4] = [
    "diagram",
    "diagram-bold",
    "diagram-italic",
    "diagram-bold-italic",
];

/// Whether a font's characters all have the same width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontKind {
    Proportional,
    Monospace,
}

/// A font you can choose for the editor, and (some of them) for diagram
/// labels. Each has regular, bold, italic, and bold italic faces, and none
/// draws ligatures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EditorFont {
    #[default]
    Inter,
    SourceSerif,
    IaQuattro,
    JetBrainsMono,
    PlexMono,
    IaDuo,
}

/// A font file: its name in the font definitions, and its bytes.
type Face = (&'static str, &'static [u8]);

macro_rules! face {
    ($name:literal, $file:literal) => {
        ($name, include_bytes!(concat!("../assets/fonts/", $file)))
    };
}

impl EditorFont {
    /// Every choice, proportional fonts first.
    pub const ALL: [EditorFont; 6] = [
        EditorFont::Inter,
        EditorFont::SourceSerif,
        EditorFont::IaQuattro,
        EditorFont::JetBrainsMono,
        EditorFont::PlexMono,
        EditorFont::IaDuo,
    ];

    /// The choices for diagram labels: the fonts about as wide as Inter, so
    /// a label that fits its shape in Inter still fits.
    pub const DIAGRAM: [EditorFont; 2] = [EditorFont::Inter, EditorFont::SourceSerif];

    /// The name saved in preferences.
    pub fn id(self) -> &'static str {
        match self {
            EditorFont::Inter => "inter",
            EditorFont::SourceSerif => "source-serif",
            EditorFont::IaQuattro => "ia-quattro",
            EditorFont::JetBrainsMono => "jetbrains-mono",
            EditorFont::PlexMono => "plex-mono",
            EditorFont::IaDuo => "ia-duo",
        }
    }

    /// The font with a saved name; an unknown name is Inter.
    pub fn from_id(id: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|f| f.id() == id)
            .unwrap_or_default()
    }

    /// The diagram font with a saved name; an unknown name, or a font that
    /// is not offered for diagrams, is Inter.
    pub fn diagram_from_id(id: &str) -> Self {
        Self::DIAGRAM
            .into_iter()
            .find(|f| f.id() == id)
            .unwrap_or_default()
    }

    /// A family that draws in this font's regular face while a list of
    /// fonts is open ([`set_previews`]), and in Inter otherwise.
    pub fn preview_family(self) -> FontFamily {
        FontFamily::Name(format!("preview-{}", self.id()).into())
    }

    /// The name shown in Settings.
    pub fn name(self) -> &'static str {
        match self {
            EditorFont::Inter => "Inter",
            EditorFont::SourceSerif => "Source Serif 4",
            EditorFont::IaQuattro => "iA Writer Quattro",
            EditorFont::JetBrainsMono => "JetBrains Mono",
            EditorFont::PlexMono => "IBM Plex Mono",
            EditorFont::IaDuo => "iA Writer Duo",
        }
    }

    /// The family name inside the font files, which an SVG names and a font
    /// database looks up.
    pub fn css_family(self) -> &'static str {
        match self {
            EditorFont::Inter => "Inter",
            EditorFont::SourceSerif => "Source Serif 4",
            EditorFont::IaQuattro => "iA Writer Quattro S",
            EditorFont::JetBrainsMono => "JetBrains Mono NL",
            EditorFont::PlexMono => "IBM Plex Mono",
            EditorFont::IaDuo => "iA Writer Duo S",
        }
    }

    /// The CSS generic family to fall back to where the font is missing.
    pub fn css_generic(self) -> &'static str {
        match self {
            EditorFont::SourceSerif => "serif",
            _ if self.kind() == FontKind::Monospace => "monospace",
            _ => "sans-serif",
        }
    }

    /// iA Writer Duo counts as monospace: its characters are one width
    /// except M, W, m, and w, which are one and a half.
    pub fn kind(self) -> FontKind {
        match self {
            EditorFont::Inter | EditorFont::SourceSerif | EditorFont::IaQuattro => {
                FontKind::Proportional
            }
            EditorFont::JetBrainsMono | EditorFont::PlexMono | EditorFont::IaDuo => {
                FontKind::Monospace
            }
        }
    }

    /// The font files: regular, bold, italic, and bold italic.
    pub fn faces(self) -> [Face; 4] {
        match self {
            EditorFont::Inter => [
                face!("Inter", "Inter-Regular.ttf"),
                face!("Inter-Bold", "Inter-Bold.ttf"),
                face!("Inter-Italic", "Inter-Italic.ttf"),
                face!("Inter-BoldItalic", "Inter-BoldItalic.ttf"),
            ],
            EditorFont::SourceSerif => [
                face!("SourceSerif4", "SourceSerif4-Regular.ttf"),
                face!("SourceSerif4-Bold", "SourceSerif4-Bold.ttf"),
                face!("SourceSerif4-Italic", "SourceSerif4-It.ttf"),
                face!("SourceSerif4-BoldItalic", "SourceSerif4-BoldIt.ttf"),
            ],
            EditorFont::IaQuattro => [
                face!("iAWriterQuattro", "iAWriterQuattroS-Regular.ttf"),
                face!("iAWriterQuattro-Bold", "iAWriterQuattroS-Bold.ttf"),
                face!("iAWriterQuattro-Italic", "iAWriterQuattroS-Italic.ttf"),
                face!(
                    "iAWriterQuattro-BoldItalic",
                    "iAWriterQuattroS-BoldItalic.ttf"
                ),
            ],
            EditorFont::JetBrainsMono => [
                face!("JetBrainsMono", "JetBrainsMonoNL-Regular.ttf"),
                face!("JetBrainsMono-Bold", "JetBrainsMonoNL-Bold.ttf"),
                face!("JetBrainsMono-Italic", "JetBrainsMonoNL-Italic.ttf"),
                face!("JetBrainsMono-BoldItalic", "JetBrainsMonoNL-BoldItalic.ttf"),
            ],
            EditorFont::PlexMono => [
                face!("IBMPlexMono", "IBMPlexMono-Regular.ttf"),
                face!("IBMPlexMono-Bold", "IBMPlexMono-Bold.ttf"),
                face!("IBMPlexMono-Italic", "IBMPlexMono-Italic.ttf"),
                face!("IBMPlexMono-BoldItalic", "IBMPlexMono-BoldItalic.ttf"),
            ],
            EditorFont::IaDuo => [
                face!("iAWriterDuo", "iAWriterDuoS-Regular.ttf"),
                face!("iAWriterDuo-Bold", "iAWriterDuoS-Bold.ttf"),
                face!("iAWriterDuo-Italic", "iAWriterDuoS-Italic.ttf"),
                face!("iAWriterDuo-BoldItalic", "iAWriterDuoS-BoldItalic.ttf"),
            ],
        }
    }
}

/// The fonts an app has chosen, kept with the context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Chosen {
    editor: EditorFont,
    diagram: EditorFont,
    /// Whether every choice is registered, for a list of them.
    previews: bool,
}

fn chosen(ctx: &egui::Context) -> Chosen {
    ctx.data(|d| d.get_temp(egui::Id::unique(CHOSEN_KEY)))
        .unwrap_or_default()
}

const CHOSEN_KEY: &str = "laankit-chosen-fonts";

/// Rebuilds the font definitions when a chosen font changes. Returns whether
/// it did.
fn choose(ctx: &egui::Context, change: impl FnOnce(&mut Chosen)) -> bool {
    let before = chosen(ctx);
    let mut after = before;
    change(&mut after);
    if after == before {
        return false;
    }
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(CHOSEN_KEY), after));
    ctx.set_fonts(definitions_for(after));
    true
}

/// Sets the editor's font ([`editor_family`]). Call it after [`install`], at
/// startup and when the setting changes. Returns whether the font changed.
pub fn install_editor_font(ctx: &egui::Context, font: EditorFont) -> bool {
    choose(ctx, |c| c.editor = font)
}

/// Sets the font of diagram labels ([`diagram_family`]), as
/// [`install_editor_font`] does for the editor.
pub fn install_diagram_font(ctx: &egui::Context, font: EditorFont) -> bool {
    choose(ctx, |c| c.diagram = font)
}

/// Registers every choice ([`EditorFont::preview_family`]) while a list of
/// them is open, and drops the ones not chosen when it closes. `list` names
/// the list, so that two lists don't undo each other.
pub fn set_previews(ctx: &egui::Context, list: egui::Id, open: bool) {
    let key = egui::Id::unique("laankit-font-previews");
    let any = ctx.data_mut(|d| {
        let lists: &mut Vec<egui::Id> = d.get_temp_mut_or_default(key);
        lists.retain(|l| *l != list);
        if open {
            lists.push(list);
        }
        !lists.is_empty()
    });
    choose(ctx, |c| c.previews = any);
}

/// The font the editor draws in.
pub fn editor_font(ctx: &egui::Context) -> EditorFont {
    chosen(ctx).editor
}

/// The font diagram labels draw in.
pub fn diagram_font(ctx: &egui::Context) -> EditorFont {
    chosen(ctx).diagram
}

/// Regular, bold, italic, or bold italic, as an index into a family list.
fn face_index(style: Style) -> usize {
    let bold = style.weight.unwrap_or(REGULAR) >= 550.0;
    usize::from(bold) + 2 * usize::from(style.italic)
}

/// The family of the editor's font for `style`. A `mono` style is the code
/// font: the chosen font when it is monospace, otherwise JetBrains Mono.
pub fn editor_family(style: Style) -> FontFamily {
    let families = if style.mono {
        EDITOR_MONO_FAMILIES
    } else {
        EDITOR_FAMILIES
    };
    FontFamily::Name(families[face_index(style)].into())
}

/// The family of the diagram label font for `style` (`mono` is ignored).
pub fn diagram_family(style: Style) -> FontFamily {
    FontFamily::Name(DIAGRAM_FAMILIES[face_index(style)].into())
}

/// Installs the bundled fonts and the text sizes of `global.css` (14 px body).
///
/// Under eframe the system fallback is already installed (unless
/// `NativeOptions::system_font_fallback` is off); elsewhere call
/// [`install_system_fallback`] too.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions_for(chosen(ctx)));
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

    /// A context with `font` as the editor and diagram font.
    fn ctx_with(font: EditorFont) -> egui::Context {
        let ctx = egui::Context::default();
        install(&ctx);
        install_editor_font(&ctx, font);
        install_diagram_font(&ctx, font);
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        ctx
    }

    fn width(ctx: &egui::Context, family: FontFamily, text: &str) -> f32 {
        ctx.fonts_mut(|f| {
            f.layout_job(egui::text::LayoutJob::single_section(
                text.into(),
                TextFormat {
                    font_id: FontId::new(14.0, family),
                    ..Default::default()
                },
            ))
            .size()
            .x
        })
    }

    const SAMPLE: &str = "Authentication Service handles the quick brown fox, 0123456789.";

    #[test]
    fn every_choice_has_four_faces_that_cover_latin() {
        let text = "AaZz09 ÀéîõüçñßŒ “quotes” — … € £ ¥ © ™ # * _ ` [ ] | -> !=";
        for font in EditorFont::ALL {
            let faces = font.faces();
            for (i, a) in faces.iter().enumerate() {
                for b in &faces[i + 1..] {
                    assert_ne!(a.0, b.0, "{font:?} repeats a face");
                    assert_ne!(a.1, b.1, "{font:?} repeats a file");
                }
            }
            // Only the chosen font is in each family here, so a glyph found
            // is a glyph the font has.
            let mut defs = definitions_for(Chosen {
                editor: font,
                diagram: font,
                previews: false,
            });
            for family in defs.families.values_mut() {
                family.truncate(1);
            }
            let ctx = egui::Context::default();
            ctx.set_fonts(defs);
            let mut out = ctx.run_ui(Default::default(), |_| {});
            out.textures_delta.clear();
            ctx.fonts_mut(|f| {
                for style in [
                    Style::default(),
                    Style::BOLD,
                    Style::ITALIC,
                    Style::BOLD_ITALIC,
                ] {
                    let id = FontId::new(14.0, editor_family(style));
                    for c in text.chars().filter(|c| !c.is_whitespace()) {
                        assert!(f.has_glyph(&id, c), "{font:?} {style:?} lacks {c:?}");
                    }
                }
            });
        }
    }

    /// egui can't turn OpenType features off, so a face that draws `->` or
    /// `fi` as one glyph can't be offered. The faces added for the editor
    /// have no such features; a font update must keep it so.
    #[test]
    fn the_added_faces_have_no_ligature_features() {
        for font in [
            EditorFont::IaQuattro,
            EditorFont::PlexMono,
            EditorFont::IaDuo,
        ] {
            for (name, bytes) in font.faces() {
                for tag in [b"calt", b"liga", b"dlig", b"clig"] {
                    assert!(
                        !bytes.windows(4).any(|w| w == tag),
                        "{name} has a `{}` feature",
                        String::from_utf8_lossy(tag)
                    );
                }
            }
        }
    }

    #[test]
    fn no_choice_joins_characters() {
        let text = "fi fl ffi -> != == --> <=";
        for font in EditorFont::ALL {
            let ctx = ctx_with(font);
            for style in [Style::default(), Style::MONO] {
                let glyphs = ctx.fonts_mut(|f| {
                    f.layout_job(egui::text::LayoutJob::single_section(
                        text.into(),
                        TextFormat {
                            font_id: FontId::new(14.0, editor_family(style)),
                            ..Default::default()
                        },
                    ))
                    .rows[0]
                        .glyphs
                        .len()
                });
                assert_eq!(glyphs, text.chars().count(), "{font:?} {style:?}");
            }
        }
    }

    #[test]
    fn a_choice_is_saved_by_its_id() {
        for font in EditorFont::ALL {
            assert_eq!(EditorFont::from_id(font.id()), font);
        }
        assert_eq!(EditorFont::from_id("comic-sans"), EditorFont::Inter);
        assert_eq!(EditorFont::from_id(""), EditorFont::Inter);
        for font in EditorFont::DIAGRAM {
            assert_eq!(EditorFont::diagram_from_id(font.id()), font);
        }
        // A font that is too wide for diagrams.
        assert_eq!(EditorFont::diagram_from_id("plex-mono"), EditorFont::Inter);
    }

    #[test]
    fn installing_a_choice_changes_the_editor_families() {
        let ctx = ctx_with(EditorFont::Inter);
        let inter = width(&ctx, editor_family(Style::default()), SAMPLE);
        assert_eq!(inter, width(&ctx, FontFamily::Proportional, SAMPLE));
        assert_eq!(
            width(&ctx, editor_family(Style::MONO), SAMPLE),
            width(&ctx, FontFamily::Monospace, SAMPLE),
            "code is in JetBrains Mono with a proportional font"
        );

        assert!(install_editor_font(&ctx, EditorFont::PlexMono));
        assert!(
            !install_editor_font(&ctx, EditorFont::PlexMono),
            "no change"
        );
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        assert_eq!(editor_font(&ctx), EditorFont::PlexMono);
        assert_eq!(diagram_font(&ctx), EditorFont::Inter);
        let plex = width(&ctx, editor_family(Style::default()), SAMPLE);
        assert!(plex > inter * 1.1, "the editor is still in Inter");
        // Every style is the same width in a monospace font, code included.
        for style in [Style::BOLD, Style::ITALIC, Style::BOLD_ITALIC, Style::MONO] {
            assert_eq!(width(&ctx, editor_family(style), SAMPLE), plex, "{style:?}");
        }
        // The rest of the UI and the diagrams are unchanged.
        assert_eq!(width(&ctx, FontFamily::Proportional, SAMPLE), inter);
        assert_eq!(width(&ctx, diagram_family(Style::default()), SAMPLE), inter);
    }

    #[test]
    fn switching_back_leaves_no_stale_faces() {
        let names = |chosen: Chosen| {
            let defs = definitions_for(chosen);
            let mut names: Vec<String> = defs.font_data.keys().cloned().collect();
            names.sort();
            (names, defs.families)
        };
        let start = names(Chosen::default());
        let ctx = ctx_with(EditorFont::IaDuo);
        assert!(names(chosen(&ctx)).0.len() > start.0.len());
        install_editor_font(&ctx, EditorFont::Inter);
        install_diagram_font(&ctx, EditorFont::Inter);
        assert_eq!(names(chosen(&ctx)), start);
        // Only the chosen fonts are registered.
        let plex = names(Chosen {
            editor: EditorFont::PlexMono,
            diagram: EditorFont::SourceSerif,
            previews: false,
        });
        assert!(plex.0.iter().any(|n| n == "IBMPlexMono-BoldItalic"));
        assert!(!plex.0.iter().any(|n| n.starts_with("iAWriter")));
    }

    #[test]
    fn previews_are_registered_only_while_a_list_is_open() {
        let ctx = ctx_with(EditorFont::Inter);
        let step = |ctx: &egui::Context| {
            let mut out = ctx.run_ui(Default::default(), |_| {});
            out.textures_delta.clear();
        };
        let plex = EditorFont::PlexMono.preview_family();
        let inter = width(&ctx, FontFamily::Proportional, SAMPLE);
        assert_eq!(width(&ctx, plex.clone(), SAMPLE), inter);
        let (a, b) = (egui::Id::unique("a"), egui::Id::unique("b"));
        set_previews(&ctx, a, true);
        set_previews(&ctx, b, false);
        step(&ctx);
        assert!(width(&ctx, plex.clone(), SAMPLE) > inter * 1.1);
        assert_eq!(
            width(&ctx, EditorFont::Inter.preview_family(), SAMPLE),
            inter
        );
        // The chosen fonts are untouched.
        assert_eq!(width(&ctx, editor_family(Style::default()), SAMPLE), inter);
        set_previews(&ctx, a, false);
        step(&ctx);
        assert_eq!(width(&ctx, plex, SAMPLE), inter);
    }

    /// A diagram's shapes are sized for labels in Inter, so a diagram font
    /// may be narrower than Inter but not wider.
    #[test]
    fn diagram_fonts_are_about_as_wide_as_inter() {
        let ctx = ctx_with(EditorFont::Inter);
        let inter: Vec<f32> = [Style::default(), Style::BOLD, Style::ITALIC]
            .map(|s| width(&ctx, diagram_family(s), SAMPLE))
            .into();
        for font in EditorFont::ALL {
            let ctx = ctx_with(font);
            let fits = [Style::default(), Style::BOLD, Style::ITALIC]
                .into_iter()
                .zip(&inter)
                .all(|(s, inter)| {
                    let w = width(&ctx, diagram_family(s), SAMPLE);
                    w <= inter * 1.02 && w >= inter * 0.85
                });
            assert_eq!(
                fits,
                EditorFont::DIAGRAM.contains(&font),
                "{font:?} is in the wrong list"
            );
        }
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
