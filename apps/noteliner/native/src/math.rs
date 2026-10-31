//! Math in the preview and slides: `marina-richtext` asks this renderer for
//! each formula, and it typesets them with Typst through the same warm
//! exporter the PDF export uses (`noteliner_export::Exporter::math_svg`).
//! Installed once by `Backend::attach`.

use std::sync::Arc;

use marina_richtext::math::MathRenderer;
use noteliner_export::LazyExporter;

pub struct TypstMath(pub Arc<LazyExporter>);

impl MathRenderer for TypstMath {
    fn render_svg(&self, latex: &str, display: bool, size: f32) -> Result<String, String> {
        // On the math worker thread: waiting for the exporter's warm-up is fine here.
        self.0
            .get()
            .math_svg(latex, display, size)
            .map_err(|e| e.to_string())
    }
}
