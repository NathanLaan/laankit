//! Opening and rasterising PDFs with hayro.

use std::sync::Arc;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::page::Page;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};

/// Loads a PDF from shared bytes; each worker thread opens its own [`Pdf`] over the same bytes.
pub fn open(bytes: Arc<Vec<u8>>) -> anyhow::Result<Pdf> {
    Pdf::new(bytes).map_err(|e| anyhow::anyhow!("couldn't read the PDF: {e:?}"))
}

/// Every page's size in page units (rotation and crop box applied), in page order.
pub fn page_sizes(pdf: &Pdf) -> Vec<[f32; 2]> {
    pdf.pages()
        .iter()
        .map(|p| {
            let (w, h) = p.render_dimensions();
            [w, h]
        })
        .collect()
}

/// Renders `page` at `scale` device pixels per page unit, on white.
pub fn render_page<'a>(
    page: &'a Page<'a>,
    cache: &RenderCache<'a>,
    settings: &InterpreterSettings,
    scale: f32,
) -> egui::ColorImage {
    let pixmap = hayro::render(
        page,
        cache,
        settings,
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
        },
    );
    let size = [pixmap.width() as usize, pixmap.height() as usize];
    egui::ColorImage::from_rgba_premultiplied(size, pixmap.data_as_u8_slice())
}
