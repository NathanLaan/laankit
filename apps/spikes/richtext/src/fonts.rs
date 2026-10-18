//! Reading fonts with real bold and italic faces. egui's default fonts have neither, so the spike
//! borrows the system's (DejaVu/Liberation on Linux, Segoe UI/Georgia on Windows, Arial/Georgia
//! on macOS). Phase 1 bundles OFL fonts instead, as ArtCraft does.

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// Which reading face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Face {
    #[default]
    Sans,
    Serif,
}

/// The four styles of one face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

const VARIANTS: [Variant; 4] = [
    Variant::Regular,
    Variant::Bold,
    Variant::Italic,
    Variant::BoldItalic,
];

/// Candidate files per face, in the order of [`VARIANTS`]; the first complete set wins.
const SANS: &[[&str; 4]] = &[
    [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Oblique.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-BoldOblique.ttf",
    ],
    [
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Italic.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-BoldItalic.ttf",
    ],
    [
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\segoeuib.ttf",
        "C:\\Windows\\Fonts\\segoeuii.ttf",
        "C:\\Windows\\Fonts\\segoeuiz.ttf",
    ],
    [
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        "/System/Library/Fonts/Supplemental/Arial Italic.ttf",
        "/System/Library/Fonts/Supplemental/Arial Bold Italic.ttf",
    ],
];
const SERIF: &[[&str; 4]] = &[
    [
        "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSerif-Bold.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSerif-Italic.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSerif-BoldItalic.ttf",
    ],
    [
        "/usr/share/fonts/truetype/liberation/LiberationSerif-Regular.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSerif-Bold.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSerif-Italic.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSerif-BoldItalic.ttf",
    ],
    [
        "C:\\Windows\\Fonts\\georgia.ttf",
        "C:\\Windows\\Fonts\\georgiab.ttf",
        "C:\\Windows\\Fonts\\georgiai.ttf",
        "C:\\Windows\\Fonts\\georgiaz.ttf",
    ],
    [
        "/System/Library/Fonts/Supplemental/Georgia.ttf",
        "/System/Library/Fonts/Supplemental/Georgia Bold.ttf",
        "/System/Library/Fonts/Supplemental/Georgia Italic.ttf",
        "/System/Library/Fonts/Supplemental/Georgia Bold Italic.ttf",
    ],
];

/// The font family of a face and variant. Always valid once [`install`] has run: without system
/// fonts it falls back to egui's proportional font.
pub fn family(face: Face, variant: Variant) -> FontFamily {
    FontFamily::Name(family_name(face, variant).into())
}

fn family_name(face: Face, variant: Variant) -> String {
    format!("rt-{face:?}-{variant:?}").to_ascii_lowercase()
}

/// Whether real bold/italic faces were found (otherwise the renderer fakes them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Installed {
    pub sans: bool,
    pub serif: bool,
}

/// Adds the reading families to `ctx`'s fonts. Takes effect from the next frame.
pub fn install(ctx: &egui::Context) -> Installed {
    let mut defs = FontDefinitions::default();
    let fallback = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut installed = Installed::default();
    for (face, candidates) in [(Face::Sans, SANS), (Face::Serif, SERIF)] {
        let set = candidates.iter().find_map(|paths| {
            let data: Option<Vec<Vec<u8>>> = paths.iter().map(|p| std::fs::read(p).ok()).collect();
            data
        });
        let found = set.is_some();
        for (i, variant) in VARIANTS.iter().enumerate() {
            let name = family_name(face, *variant);
            let mut chain = Vec::new();
            if let Some(set) = &set {
                defs.font_data
                    .insert(name.clone(), Arc::new(FontData::from_owned(set[i].clone())));
                chain.push(name.clone());
            }
            chain.extend(fallback.iter().cloned());
            defs.families.insert(FontFamily::Name(name.into()), chain);
        }
        match face {
            Face::Sans => installed.sans = found,
            Face::Serif => installed.serif = found,
        }
    }
    ctx.set_fonts(defs);
    installed
}
