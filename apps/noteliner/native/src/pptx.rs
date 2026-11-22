//! Save to PowerPoint: reads the pictures a presentation shows, renders its
//! formulas, and hands them to `noteliner-pptx`, which writes the file.
//!
//! | A picture's address      | Comes from |
//! |--------------------------|------------|
//! | `_attachments/name`      | The file beside the note |
//! | `data:…`                 | The address itself |
//! | `http://`, `https://`    | The web, with [`FETCH_SECONDS`] for each |
//!
//! PNG, JPEG and GIF files go in as they are. Anything else (SVG, WebP) is
//! turned into a PNG, since older PowerPoint versions read neither.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use egui::SizeHint;
use noteliner_pptx::{Assets, Format, Formula, Options, Picture};
use serde_json::Value;

/// How long a picture at a web address may take.
const FETCH_SECONDS: u64 = 10;
/// The largest picture taken from the web.
const FETCH_BYTES: usize = 50 * 1024 * 1024;
/// Formulas and SVG pictures are drawn at this many pixels for each stage
/// pixel, so they stay sharp on a large screen.
const SCALE: f32 = 2.0;

pub struct Exported {
    pub bytes: Vec<u8>,
    /// What could not be exported in full, one line for each.
    pub warnings: Vec<String>,
}

/// The presentation in `raw` (a note with its frontmatter) as a `.pptx`.
pub async fn export(
    exporter: Arc<noteliner_export::LazyExporter>,
    raw: String,
    presentation: Value,
    name: String,
    note_dir: PathBuf,
) -> Result<Exported> {
    let deck = noteliner_slides::parse_deck(&raw, Some(&presentation));
    let needs = noteliner_pptx::needs(&deck);

    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(FETCH_SECONDS))
        .build()?;
    for src in &needs.pictures {
        let bytes = if src.starts_with("http://") || src.starts_with("https://") {
            fetch(&client, src).await
        } else {
            local(src, &note_dir)
        };
        match bytes {
            Ok(bytes) => {
                files.insert(src.clone(), bytes);
            }
            Err(e) => log::warn!("Save to PowerPoint: {}: {e}", short(src)),
        }
    }

    let vars = crate::deck::vars(&name);
    tokio::task::spawn_blocking(move || {
        let mut assets = Assets::default();
        for (src, bytes) in files {
            match picture(bytes) {
                Ok(p) => {
                    assets.pictures.insert(src, p);
                }
                Err(e) => log::warn!("Save to PowerPoint: {}: {e}", short(&src)),
            }
        }
        for f in needs.formulas {
            match formula(exporter.get(), &f) {
                Ok(p) => {
                    assets.formulas.insert(f, p);
                }
                // The slide shows the source instead, as the app does.
                Err(e) => log::warn!("Save to PowerPoint: formula {}: {e}", short(&f.latex)),
            }
        }
        let options = Options {
            title: name,
            vars,
            created: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            ..Default::default()
        };
        let out = noteliner_pptx::export(&deck, &assets, &options)?;
        Ok(Exported {
            bytes: out.bytes,
            warnings: out.warnings,
        })
    })
    .await?
}

fn short(text: &str) -> String {
    text.chars().take(80).collect()
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let response = client.get(url).send().await?.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|n| n > FETCH_BYTES as u64)
    {
        return Err(anyhow!("the picture is too large"));
    }
    let bytes = response.bytes().await?;
    if bytes.len() > FETCH_BYTES {
        return Err(anyhow!("the picture is too large"));
    }
    Ok(bytes.to_vec())
}

/// A picture in the address itself, or in `_attachments/` beside the note.
fn local(src: &str, note_dir: &Path) -> Result<Vec<u8>> {
    use base64::Engine;
    if let Some(rest) = src.strip_prefix("data:") {
        let (meta, data) = rest
            .split_once(',')
            .ok_or_else(|| anyhow!("not a data address"))?;
        return if meta.ends_with(";base64") {
            Ok(base64::engine::general_purpose::STANDARD.decode(data.trim())?)
        } else {
            Ok(data.as_bytes().to_vec())
        };
    }
    let name = crate::views::preview::attachment_name(src)
        .ok_or_else(|| anyhow!("not an attachment of this note"))?;
    Ok(std::fs::read(note_dir.join("_attachments").join(name))?)
}

/// A file's bytes as a picture PowerPoint reads.
fn picture(bytes: Vec<u8>) -> Result<Picture> {
    if let Some(format) = Format::sniff(&bytes) {
        let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()?
            .into_dimensions()?;
        return Ok(Picture {
            bytes,
            format,
            natural: (w as f32, h as f32),
        });
    }
    if let Some(markup) = std::str::from_utf8(&bytes)
        .ok()
        .filter(|t| t.contains("<svg"))
    {
        let image = laankit_richtext::svg::render(markup, SizeHint::Scale(SCALE.into()))
            .map_err(|e| anyhow!("{e}"))?;
        let natural = (image.source_size.x, image.source_size.y);
        return png(&image, None, natural);
    }
    // WebP, and anything else the app can read.
    let decoded = image::load_from_memory(&bytes)?;
    let (w, h) = (decoded.width(), decoded.height());
    let mut out = Vec::new();
    decoded.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)?;
    Ok(Picture {
        bytes: out,
        format: Format::Png,
        natural: (w as f32, h as f32),
    })
}

/// A display formula as a PNG in the slide's text colour.
fn formula(exporter: &noteliner_export::Exporter, f: &Formula) -> Result<Picture> {
    let svg = exporter.math_svg(&f.latex, true, f.size as f32)?;
    let image = laankit_richtext::svg::render(&svg, SizeHint::Scale(SCALE.into()))
        .map_err(|e| anyhow!("{e}"))?;
    let natural = (image.source_size.x, image.source_size.y);
    png(&image, Some(f.color), natural)
}

/// `image` as a PNG. With `tint`, its pixels take that colour and keep their
/// transparency (a formula is drawn white on nothing).
fn png(image: &egui::ColorImage, tint: Option<[u8; 3]>, natural: (f32, f32)) -> Result<Picture> {
    let mut rgba = Vec::with_capacity(image.pixels.len() * 4);
    for p in &image.pixels {
        let [r, g, b, a] = p.to_srgba_unmultiplied();
        match tint {
            Some(c) => rgba.extend_from_slice(&[c[0], c[1], c[2], a]),
            None => rgba.extend_from_slice(&[r, g, b, a]),
        }
    }
    let [w, h] = image.size;
    let buffer = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| anyhow!("the picture has no pixels"))?;
    let mut out = Vec::new();
    buffer.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)?;
    if natural.0 <= 0.0 || natural.1 <= 0.0 {
        return Err(anyhow!("the picture has no size"));
    }
    Ok(Picture {
        bytes: out,
        format: Format::Png,
        natural,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_svg_becomes_a_png_at_its_own_size() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="red"/></svg>"#;
        let p = picture(svg.to_vec()).unwrap();
        assert_eq!(p.format, Format::Png);
        assert_eq!(p.natural, (40.0, 20.0));
        assert_eq!(Format::sniff(&p.bytes), Some(Format::Png));
        let decoded = image::load_from_memory(&p.bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (80, 40));
    }

    #[test]
    fn a_data_address_is_read_and_a_path_outside_the_note_is_not() {
        let dir = Path::new("/nowhere");
        assert_eq!(local("data:image/png;base64,AQID", dir).unwrap(), [1, 2, 3]);
        assert!(local("../secret.png", dir).is_err());
        assert!(local("/etc/passwd", dir).is_err());
    }

    #[test]
    fn a_formula_is_tinted() {
        let image = egui::ColorImage::filled([2, 1], egui::Color32::from_white_alpha(128));
        let p = png(&image, Some([10, 20, 30]), (2.0, 1.0)).unwrap();
        let decoded = image::load_from_memory(&p.bytes).unwrap().to_rgba8();
        let px = decoded.get_pixel(0, 0).0;
        assert_eq!(&px[..3], &[10, 20, 30]);
        assert!(px[3] > 100 && px[3] < 160, "{px:?}");
    }
}
