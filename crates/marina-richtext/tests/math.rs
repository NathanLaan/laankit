//! Math in notes: comrak's math markup (NoteLiner's `preview_html`) becomes formulas through
//! `from_note_html` (and only there), inline formulas lay out at their rendered size over
//! placeholder spaces, display formulas become their own blocks, and without a renderer (or while
//! one renders) the source shows instead, with the same characters.

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use marina_richtext::fonts::Families;
use marina_richtext::math::{self, MathImage, MathMode, MathRenderer};
use marina_richtext::view::text_job;
use marina_richtext::{
    from_article_html, from_html, from_note_html, Block, Document, Options, RichStyle, RichView,
    TextKind, WebResolver,
};

const NOTE: &str = "\
Euler: $e^{i\\pi} + 1 = 0$ and $a$$b$.

$$\\int_0^1 x^2\\,dx$$

```math
\\sum_i i
```
";

fn note(md: &str) -> Document {
    from_note_html(
        &noteliner_core::export::preview_html(md),
        &Options::default(),
    )
}

/// Draws every formula as a white box 3 points wide per source character and 12 points tall.
struct Boxes;

impl MathRenderer for Boxes {
    fn render_svg(&self, latex: &str, display: bool, size: f32) -> Result<String, String> {
        if latex.contains("\\unsupported") {
            return Err("unsupported".into());
        }
        let w = 3.0 * latex.chars().count() as f32 * if display { 2.0 } else { 1.0 };
        let h = size;
        Ok(format!(
            r#"<svg viewBox="0 0 {w} {h}" width="{w}pt" height="{h}pt" xmlns="http://www.w3.org/2000/svg"><rect width="{w}" height="{h}" fill="white"/></svg>"#
        ))
    }
}

/// Polls until the formula is rendered (the loader works on its own thread).
fn ready(ctx: &egui::Context, latex: &str, mode: MathMode, size: f32) -> MathImage {
    let start = Instant::now();
    loop {
        match math::image(ctx, latex, mode, size) {
            MathImage::Pending if start.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(5));
            }
            other => return other,
        }
    }
}

#[test]
fn comrak_math_becomes_formulas_only_in_notes() {
    let doc = note(NOTE);
    let Block::Text { spans, .. } = &doc.blocks[0] else {
        panic!("{:?}", doc.blocks[0]);
    };
    let formulas: Vec<_> = spans
        .iter()
        .filter(|s| s.style.math == Some(MathMode::Inline))
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(
        formulas,
        ["e^{i\\pi} + 1 = 0", "a", "b"],
        "adjacent formulas stay apart"
    );
    let displays: Vec<_> = doc.blocks[1..]
        .iter()
        .map(|b| match b {
            Block::Image { src, alt, .. } => {
                assert_eq!(math::parse_image_src(src).as_deref(), Some(alt.as_str()));
                alt.clone()
            }
            other => panic!("expected a display formula, got {other:?}"),
        })
        .collect();
    assert_eq!(displays, ["\\int_0^1 x^2\\,dx", "\\sum_i i"]);
    // Copying and search keep the source.
    assert!(doc.blocks[1].plain_text().contains("\\int_0^1"));

    // The same HTML through `from_html` (articles, books) stays text.
    let html = noteliner_core::export::preview_html(NOTE);
    let plain = from_html(&html, &Options::default());
    assert!(plain.blocks.iter().all(|b| match b {
        Block::Text { spans, .. } => spans.iter().all(|s| s.style.math.is_none()),
        Block::Image { .. } => false,
        _ => true,
    }));
    let article = from_article_html(&html, None);
    assert!(!article
        .blocks
        .iter()
        .any(|b| matches!(b, Block::Image { src, .. } if src.starts_with("math:"))));
}

#[test]
fn display_math_inside_text_stays_inline() {
    let doc = note("Before $$x$$ after.\n");
    let Block::Text { spans, .. } = &doc.blocks[0] else {
        panic!("{:?}", doc.blocks);
    };
    assert!(spans
        .iter()
        .any(|s| s.style.math == Some(MathMode::Display) && s.text == "x"));
}

fn job_text(ctx: &egui::Context, doc: &Document, with_ctx: bool) -> (String, f32) {
    let Block::Text {
        spans, kind, align, ..
    } = &doc.blocks[0]
    else {
        panic!();
    };
    let style = RichStyle::from_ctx(ctx);
    let families = Families::of(ctx);
    let (job, _) = text_job(
        spans,
        *kind,
        *align,
        0.0,
        &style,
        &families,
        2000.0,
        with_ctx.then_some(ctx),
    );
    let galley = ctx.fonts_mut(|f| f.layout_job(job.clone()));
    (job.text, galley.size().x)
}

#[test]
fn inline_formulas_lay_out_at_their_rendered_width() {
    let ctx = egui::Context::default();
    marina_egui::fonts::install(&ctx);
    ctx.run_ui(Default::default(), |_| {})
        .drop_without_applying_deltas();
    let doc = note("A $abc$ B\n");

    // No renderer: the source, same characters.
    let (text, _) = job_text(&ctx, &doc, true);
    assert_eq!(text, "A abc B");

    math::install(&ctx, Arc::new(Boxes));
    let size = RichStyle::from_ctx(&ctx).size;
    let MathImage::Ready(texture) = ready(&ctx, "abc", MathMode::Inline, size) else {
        panic!("not rendered");
    };
    // `pt` read as points: 3 characters × 3 = 9 points wide, `size` tall.
    assert!((texture.size.x - 9.0).abs() < 0.01, "{:?}", texture.size);
    assert!((texture.size.y - size).abs() < 0.01, "{:?}", texture.size);

    let (text, width) = job_text(&ctx, &doc, true);
    assert_eq!(
        text,
        format!("A {} B", "\u{a0}".repeat(3)),
        "one placeholder per character"
    );
    // "A B" (HTML collapses a double space), plus the second space, plus the formula.
    let (_, plain_width) = job_text(&ctx, &note("A B\n"), true);
    let space = ctx.fonts_mut(|f| f.glyph_width(&egui::FontId::proportional(size), ' '));
    assert!(
        (width - plain_width - space - 9.0).abs() < 0.5,
        "the formula takes its own width: {width} vs {plain_width} + {space} + 9"
    );
    // Without a context (measuring only), the source again.
    assert_eq!(job_text(&ctx, &doc, false).0, "A abc B");
}

#[test]
fn failed_formulas_fall_back_to_their_source() {
    let ctx = egui::Context::default();
    math::install(&ctx, Arc::new(Boxes));
    assert!(matches!(
        ready(&ctx, "\\unsupported", MathMode::Inline, 14.0),
        MathImage::Unavailable
    ));
}

#[test]
fn display_formulas_render_or_show_their_source() {
    for installed in [false, true] {
        let doc = note("$$\\frac{a}{b}$$\n");
        let mut view = RichView::default();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 300.0))
            .with_context_setup(move |ctx| {
                marina_egui::fonts::install(ctx);
                if installed {
                    math::install(ctx, Arc::new(Boxes));
                }
            })
            .build_ui(move |ui| {
                let style = RichStyle::from_ctx(ui.ctx());
                let _ = view.show(ui, &doc, &style, &mut WebResolver);
            });
        // Let the worker render and the loader pick it up.
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            harness.run_steps(2);
            if !installed || harness.query_by_label("\\frac{a}{b}").is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if installed {
            assert!(
                harness.query_by_label("\\frac{a}{b}").is_some(),
                "the rendered formula is labelled with its source"
            );
        } else {
            assert!(
                harness.query_by_label("$$\\frac{a}{b}$$").is_some(),
                "without a renderer the source shows"
            );
        }
    }
}

#[test]
fn headings_keep_math_too() {
    let doc = note("# Area $\\pi r^2$\n");
    let Block::Text { kind, spans, .. } = &doc.blocks[0] else {
        panic!();
    };
    assert_eq!(*kind, TextKind::Heading(1));
    assert!(spans.iter().any(|s| s.style.math.is_some()));
}
