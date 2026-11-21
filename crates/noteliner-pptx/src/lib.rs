//! NoteLiner's PowerPoint export: a [`Deck`] → the bytes of an editable
//! `.pptx`, written directly as Office Open XML.
//!
//! | Module    | Does |
//! |-----------|------|
//! | [`xml`]   | Escaping, relationships, and the parts every file has (theme, master, layout, notes master) |
//! | [`text`]  | Rich text → paragraphs and runs, their XML, and an estimate of the room they take |
//! | [`slide`] | A slide's parts placed in boxes, following `noteliner_slides::render`, and the boxes as shapes |
//!
//! The crate reads no files and draws nothing. The caller reads the pictures
//! and renders the formulas that [`needs`] lists, and passes them in
//! [`Assets`].
//!
//! Titles, text, lists and tables are PowerPoint text and tables, so the
//! slides can be edited. PowerPoint wraps the text itself and uses the fonts
//! on the computer that opens the file, so a slide is close to NoteLiner's
//! and not identical.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Write};

use laankit_richtext::math;
use laankit_richtext::Block as RichBlock;
use noteliner_slides::render::markdown_blocks;
use noteliner_slides::{BlockKind, Deck};

pub mod slide;
pub mod text;
pub mod xml;

pub use xml::Rgb;

/// A picture's file type. PowerPoint reads these three everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Gif,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpeg",
            Format::Gif => "gif",
        }
    }

    /// The type of a file, from its first bytes.
    pub fn sniff(bytes: &[u8]) -> Option<Format> {
        if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
            Some(Format::Png)
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Format::Jpeg)
        } else if bytes.starts_with(b"GIF8") {
            Some(Format::Gif)
        } else {
            None
        }
    }
}

/// A picture, as the bytes of a PNG, JPEG or GIF file.
#[derive(Clone, Debug)]
pub struct Picture {
    pub bytes: Vec<u8>,
    pub format: Format,
    /// Its size on the stage, in stage pixels: an image's size in pixels, or
    /// a formula's size at the text size it was rendered for.
    pub natural: (f32, f32),
}

/// A display formula a slide shows: its source, the text size in stage
/// pixels, and the text colour.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Formula {
    pub latex: String,
    pub size: u32,
    pub color: Rgb,
}

/// What [`needs`] asked for. A picture or a formula that is missing is
/// exported as a box with its text.
#[derive(Default)]
pub struct Assets {
    /// By the address written on the slide.
    pub pictures: HashMap<String, Picture>,
    pub formulas: HashMap<Formula, Picture>,
}

/// The pictures and formulas a deck's slides show.
#[derive(Debug, Default, PartialEq)]
pub struct Needs {
    pub pictures: Vec<String>,
    pub formulas: Vec<Formula>,
}

pub fn needs(deck: &Deck) -> Needs {
    let mut out = Needs::default();
    let mut seen = HashSet::new();
    let mut seen_formulas = HashSet::new();
    for s in &deck.slides {
        let theme = slide::theme_of(s, deck);
        let layout = slide::layout_of(s);
        for block in s.body() {
            match block.kind {
                BlockKind::Image => {
                    if let Some(src) = &block.src {
                        if seen.insert(src.clone()) {
                            out.pictures.push(src.clone());
                        }
                    }
                }
                BlockKind::Video => {}
                _ => {
                    for rich in markdown_blocks(&block.text) {
                        let RichBlock::Image { src, .. } = rich else {
                            continue;
                        };
                        match math::parse_image_src(&src) {
                            Some(latex) => {
                                let f = slide::formula(&latex, layout, &theme);
                                if seen_formulas.insert(f.clone()) {
                                    out.formulas.push(f);
                                }
                            }
                            None => {
                                if seen.insert(src.clone()) {
                                    out.pictures.push(src);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

pub struct Options {
    /// The presentation's name, for the file's properties.
    pub title: String,
    /// What `{{name}}`, `{{date}}` and the others in a header or a footer
    /// become. `{{page}}` and `{{total}}` are filled in here.
    pub vars: HashMap<String, String>,
    /// The font the file names for text.
    pub text_font: String,
    /// The font the file names for code.
    pub code_font: String,
    /// The time the file says it was made, as `2026-11-21T10:00:00Z`.
    pub created: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            title: String::new(),
            vars: HashMap::new(),
            text_font: "Inter".to_string(),
            code_font: "JetBrains Mono".to_string(),
            created: None,
        }
    }
}

pub struct Output {
    /// The `.pptx` file.
    pub bytes: Vec<u8>,
    /// What could not be exported in full, one line for each.
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// The deck as a PowerPoint file.
pub fn export(deck: &Deck, assets: &Assets, options: &Options) -> Result<Output, Error> {
    let fonts = text::Fonts {
        text: options.text_font.clone(),
        code: options.code_font.clone(),
    };
    let (w, h) = (xml::emu(deck.stage.w as f32), xml::emu(deck.stage.h as f32));
    let deck_theme = noteliner_slides::SlideTheme::named(&deck.config.theme);
    let palette = xml::Palette {
        bg: text::rgb(deck_theme.bg),
        fg: text::rgb(deck_theme.fg),
        muted: text::rgb(deck_theme.muted),
        accent: text::rgb(deck_theme.accent),
        rule: text::rgb(deck_theme.rule),
        code_bg: text::rgb(deck_theme.code_bg),
    };

    let mut warnings = Vec::new();
    let mut media = slide::Media::default();
    let mut built = Vec::new();
    for s in &deck.slides {
        let before = warnings.len();
        let mut cx = slide::Cx {
            deck,
            assets,
            vars: &options.vars,
            media: &mut media,
            warnings: &mut warnings,
        };
        built.push(slide::build(s, &mut cx));
        for line in &mut warnings[before..] {
            *line = format!("Slide {}: {line}", s.index);
        }
    }
    let media_ext: Vec<&str> = media.items.iter().map(|p| p.format.ext()).collect();

    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    let mut add = |name: &str, body: String| parts.push((name.to_string(), body.into_bytes()));

    let mut pres_rels = xml::Rels::default();
    add(
        "ppt/presentation.xml",
        xml::presentation(deck.slides.len(), w, h, &mut pres_rels),
    );
    add("ppt/_rels/presentation.xml.rels", pres_rels.xml());
    add("ppt/tableStyles.xml", xml::table_styles());
    add(
        "ppt/theme/theme1.xml",
        xml::theme("NoteLiner", &palette, &fonts.text, &fonts.code),
    );
    add(
        "ppt/theme/theme2.xml",
        xml::theme("Notes", &xml::Palette::light(), &fonts.text, &fonts.code),
    );

    let mut master_rels = xml::Rels::default();
    master_rels.part(xml::REL_LAYOUT, "../slideLayouts/slideLayout1.xml");
    master_rels.part(xml::REL_THEME, "../theme/theme1.xml");
    add(
        "ppt/slideMasters/slideMaster1.xml",
        xml::slide_master(&palette, w, h),
    );
    add(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        master_rels.xml(),
    );
    let mut layout_rels = xml::Rels::default();
    layout_rels.part(xml::REL_MASTER, "../slideMasters/slideMaster1.xml");
    add("ppt/slideLayouts/slideLayout1.xml", xml::slide_layout(w));
    add(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        layout_rels.xml(),
    );
    let mut notes_master_rels = xml::Rels::default();
    notes_master_rels.part(xml::REL_THEME, "../theme/theme2.xml");
    add("ppt/notesMasters/notesMaster1.xml", xml::notes_master());
    add(
        "ppt/notesMasters/_rels/notesMaster1.xml.rels",
        notes_master_rels.xml(),
    );

    for (i, (s, b)) in deck.slides.iter().zip(&built).enumerate() {
        let n = i + 1;
        let mut rels = xml::Rels::default();
        rels.part(xml::REL_LAYOUT, "../slideLayouts/slideLayout1.xml");
        rels.part(xml::REL_NOTES, &format!("../notesSlides/notesSlide{n}.xml"));
        let body = slide::slide_xml(b, &fonts, &mut rels, &media_ext);
        add(&format!("ppt/slides/slide{n}.xml"), body);
        add(&format!("ppt/slides/_rels/slide{n}.xml.rels"), rels.xml());

        let mut notes_rels = xml::Rels::default();
        notes_rels.part(xml::REL_NOTES_MASTER, "../notesMasters/notesMaster1.xml");
        notes_rels.part(xml::REL_SLIDE, &format!("../slides/slide{n}.xml"));
        add(
            &format!("ppt/notesSlides/notesSlide{n}.xml"),
            xml::notes_slide(&slide::notes_paragraphs(&s.notes)),
        );
        add(
            &format!("ppt/notesSlides/_rels/notesSlide{n}.xml.rels"),
            notes_rels.xml(),
        );
    }
    add(
        "docProps/core.xml",
        xml::core_props(&options.title, options.created.as_deref()),
    );
    add("docProps/app.xml", xml::app_props(deck.slides.len()));

    let names: Vec<String> = parts.iter().map(|(n, _)| n.clone()).collect();
    let fail =
        |e: &dyn std::fmt::Display| Error(format!("Couldn't write the PowerPoint file: {e}"));
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // A fixed time, so the same deck always gives the same file.
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    let stored = deflated.compression_method(zip::CompressionMethod::Stored);
    let mut put = |name: &str, bytes: &[u8], options| -> Result<(), Error> {
        zip.start_file(name, options).map_err(|e| fail(&e))?;
        zip.write_all(bytes).map_err(|e| fail(&e))
    };
    put(
        "[Content_Types].xml",
        xml::content_types(&names).as_bytes(),
        deflated,
    )?;
    put("_rels/.rels", xml::root_rels().as_bytes(), deflated)?;
    for (name, bytes) in &parts {
        put(name, bytes, deflated)?;
    }
    for (i, picture) in media.items.iter().enumerate() {
        // Pictures are compressed already.
        put(
            &format!("ppt/media/image{}.{}", i + 1, picture.format.ext()),
            &picture.bytes,
            stored,
        )?;
    }
    let bytes = zip.finish().map_err(|e| fail(&e))?.into_inner();
    Ok(Output { bytes, warnings })
}
