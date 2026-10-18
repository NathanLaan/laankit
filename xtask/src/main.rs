//! Repository tasks for packaging the native apps. See `docs/packaging.md`.
//!
//! ```bash
//! cargo run -p xtask -- icons    # packaging/generated/<app>/…
//! ```
//!
//! `icons` renders each app's `apps/<app>/assets/icon.png` into the sizes the
//! packages need, under `packaging/generated/<app>/` (git-ignored):
//!
//! | Output                                     | Used by                            |
//! |--------------------------------------------|------------------------------------|
//! | `icons/<n>x<n>/<app-id>.png` (16 … 512)    | .deb/.rpm hicolor theme, AppImage, the macOS .icns (cargo-packager builds it) |
//! | `<app-id>.ico` (16 … 256)                  | the Windows NSIS installer         |

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::FilterType;
use image::{DynamicImage, ExtendedColorType, ImageReader};

/// The native apps: (folder under `apps/`, app id).
const APPS: [(&str, &str); 3] = [
    ("threadliner", "com.threadline.app"),
    ("pageliner", "com.pageliner.app"),
    ("noteliner", "com.nathanlaan.noteliner"),
];
/// hicolor sizes for Linux (and the PNGs cargo-packager turns into an .icns).
const PNG_SIZES: [u32; 8] = [16, 24, 32, 48, 64, 128, 256, 512];
/// Sizes in the Windows .ico (256 is the largest an .ico entry can be).
const ICO_SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];

fn main() -> Result<()> {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        Some("icons") => icons(&repo_root()),
        _ => bail!("usage: cargo run -p xtask -- icons"),
    }
}

/// The workspace root (this crate lives in `<root>/xtask`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is inside the workspace")
        .to_path_buf()
}

fn icons(root: &Path) -> Result<()> {
    for (app, id) in APPS {
        let source = root.join("apps").join(app).join("assets/icon.png");
        let icon = ImageReader::open(&source)
            .with_context(|| format!("reading {}", source.display()))?
            .decode()
            .with_context(|| format!("decoding {}", source.display()))?;
        let out = root.join("packaging/generated").join(app);
        for size in PNG_SIZES {
            let dir = out.join(format!("icons/{size}x{size}"));
            std::fs::create_dir_all(&dir)?;
            square(&icon, size).save(dir.join(format!("{id}.png")))?;
        }
        write_ico(&icon, &out.join(format!("{id}.ico")))?;
        println!("{app}: {}", out.display());
    }
    Ok(())
}

/// `icon` scaled to fit `size` × `size`, centred on a transparent square.
fn square(icon: &DynamicImage, size: u32) -> DynamicImage {
    let scaled = icon.resize(size, size, FilterType::Lanczos3);
    let mut canvas = DynamicImage::new_rgba8(size, size);
    let x = (size - scaled.width()) / 2;
    let y = (size - scaled.height()) / 2;
    image::imageops::overlay(&mut canvas, &scaled, x.into(), y.into());
    canvas
}

fn write_ico(icon: &DynamicImage, path: &Path) -> Result<()> {
    let frames = ICO_SIZES
        .iter()
        .map(|&size| {
            let rgba = square(icon, size).into_rgba8();
            IcoFrame::as_png(rgba.as_raw(), size, size, ExtendedColorType::Rgba8)
        })
        .collect::<image::ImageResult<Vec<_>>>()?;
    let file = std::fs::File::create(path)?;
    IcoEncoder::new(file).encode_images(&frames)?;
    Ok(())
}
