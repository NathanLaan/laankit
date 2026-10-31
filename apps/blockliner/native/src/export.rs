//! Exporting a diagram as an image: drawn once as SVG, which a PNG export
//! rasterises with `resvg` and the bundled fonts, so the two can't differ.
//!
//! Exports go into documents and wikis, so they always use the light
//! colours below, whatever the app's theme. Shapes, corners, strokes,
//! arrowheads and labels match the canvas at 100 %; labels wrap where the
//! canvas wraps them, using egui's text layout for the line breaks.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use anyhow::{anyhow, Context as _, Result};
use blockliner_core::connect::path_midpoint;
use blockliner_core::export::Scope;
use blockliner_core::geometry::{self, SUBROUTINE_INSET};
use blockliner_core::{ConnectorKind, Diagram, Point, Rect, Shape, ShapeKind};
use egui::text::LayoutJob;
use egui::{Align, Color32};
use resvg::usvg;

use crate::canvas::grid::MAJOR;
use crate::canvas::paint::{label_font, ARROW, CORNER, EDGE_LABEL_SIZE, LABEL_PAD, LABEL_SIZE};

const BACKGROUND: &str = "#ffffff";
const FILL: &str = "#f7f8fa";
const INK: &str = "#4b5563";
const TEXT: &str = "#1f2937";
const GRID_MINOR: &str = "#eef0f3";
const GRID_MAJOR: &str = "#d9dde3";
/// The canvas's stroke width at 100 %.
const STROKE: f32 = 1.5;
/// The UI font, as the bundled file names its family.
const FONT: &str = "Inter, sans-serif";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    #[default]
    Png,
    Svg,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Svg => "svg",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Svg => "SVG",
        }
    }

    /// As `exportFormat` stores it.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "png" => Some(Self::Png),
            "svg" => Some(Self::Svg),
            _ => None,
        }
    }
}

/// The scales a PNG can be exported at.
pub const SCALES: [u32; 3] = [1, 2, 3];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub format: Format,
    pub grid: bool,
    /// PNG pixels per diagram pixel; one of [`SCALES`].
    pub scale: u32,
}

/// The output's size: pixels for a PNG, user units for an SVG.
pub fn size(scope: &Scope, o: &Options) -> (u32, u32) {
    let s = match o.format {
        Format::Png => o.scale as f32,
        Format::Svg => 1.0,
    };
    let b = scope.bounds;
    ((b.w * s).round() as u32, (b.h * s).round() as u32)
}

/// The file's bytes.
pub fn render(ctx: &egui::Context, d: &Diagram, scope: &Scope, o: &Options) -> Result<Vec<u8>> {
    let svg = svg(ctx, d, scope, o.grid);
    match o.format {
        Format::Svg => Ok(svg.into_bytes()),
        Format::Png => png(&svg, o.scale as f32),
    }
}

/// Writes `bytes` to `path` through a temp file and a rename.
pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| anyhow!("no file name"))?
        .to_string_lossy();
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    {
        let mut f =
            std::fs::File::create(&tmp).with_context(|| format!("writing {}", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all().ok();
    }
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
}

// --- SVG ----------------------------------------------------------------------

pub fn svg(ctx: &egui::Context, d: &Diagram, scope: &Scope, grid: bool) -> String {
    let b = scope.bounds;
    let mut out = String::new();
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{x} {y} {w} {h}" font-family="{FONT}">"#,
        x = num(b.x),
        y = num(b.y),
        w = num(b.w),
        h = num(b.h),
    );
    let _ = writeln!(
        out,
        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{BACKGROUND}"/>"#,
        num(b.x),
        num(b.y),
        num(b.w),
        num(b.h)
    );
    if grid {
        grid_lines(&mut out, b, d.grid);
    }
    for &i in &scope.shapes {
        shape(&mut out, ctx, &d.shapes[i]);
    }
    for &i in &scope.connectors {
        let c = &d.connectors[i];
        let Some(path) = d.connector_path(c) else {
            continue;
        };
        out.push_str("<g class=\"connector\">");
        let ink = paint(c.style.stroke.as_ref()).unwrap_or_else(|| format!("\"{INK}\""));
        connector(&mut out, &path, c.kind, &ink);
        let label = c.label.as_deref().filter(|t| !t.is_empty());
        if let (Some(text), Some(at)) = (label, path_midpoint(&path)) {
            edge_label(&mut out, ctx, (at.x, at.y), text);
        }
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    out
}

/// The canvas's pattern: a line every cell, a stronger one every [`MAJOR`].
fn grid_lines(out: &mut String, b: Rect, g: f32) {
    out.push_str("<g class=\"grid\" stroke-width=\"1\">");
    let cells = |from: f32, len: f32| {
        let first = (from / g).round() as i64;
        let last = ((from + len) / g).round() as i64;
        first..=last
    };
    let color = |i: i64| {
        if i % MAJOR == 0 {
            GRID_MAJOR
        } else {
            GRID_MINOR
        }
    };
    // Minor lines first, so the major ones are drawn over them.
    for major in [false, true] {
        for i in cells(b.x, b.w).filter(|i| (i % MAJOR == 0) == major) {
            let x = num(i as f32 * g);
            let _ = write!(
                out,
                r#"<line x1="{x}" y1="{}" x2="{x}" y2="{}" stroke="{}"/>"#,
                num(b.y),
                num(b.bottom()),
                color(i)
            );
        }
        for i in cells(b.y, b.h).filter(|i| (i % MAJOR == 0) == major) {
            let y = num(i as f32 * g);
            let _ = write!(
                out,
                r#"<line x1="{}" y1="{y}" x2="{}" y2="{y}" stroke="{}"/>"#,
                num(b.x),
                num(b.right()),
                color(i)
            );
        }
    }
    out.push_str("</g>\n");
}

fn shape(out: &mut String, ctx: &egui::Context, s: &Shape) {
    let r = s.rect;
    let _ = write!(out, r#"<g id="{}">"#, escape(&s.id));
    // The stroke sits inside the shape, as on the canvas. The shape's own
    // colours where it has them, else the export's.
    let inset = STROKE / 2.0;
    let fill = paint(s.style.fill.as_ref());
    let ink = paint(s.style.stroke.as_ref());
    let body = |out: &mut String, radius: f32, fill: &str, stroke: &str| {
        let _ = write!(
            out,
            r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{}" fill={fill} stroke={stroke} stroke-width="{}"/>"#,
            num(r.x + inset),
            num(r.y + inset),
            num(r.w - STROKE),
            num(r.h - STROKE),
            num(radius),
            num(STROKE)
        );
    };
    let (or_fill, or_ink) = (
        fill.clone().unwrap_or_else(|| format!("\"{FILL}\"")),
        ink.clone().unwrap_or_else(|| format!("\"{INK}\"")),
    );
    // The outline's rectangle, the stroke's middle inside the shape.
    let inner = Rect::new(r.x + inset, r.y + inset, r.w - STROKE, r.h - STROKE);
    let stroke = |ink: &str| format!(r#"stroke={ink} stroke-width="{}""#, num(STROKE));
    match s.kind {
        ShapeKind::Rect | ShapeKind::Other { .. } => body(out, 0.0, &or_fill, &or_ink),
        ShapeKind::Diamond
        | ShapeKind::Hexagon
        | ShapeKind::Parallelogram
        | ShapeKind::Trapezoid => {
            let points: Vec<String> = geometry::polygon(&s.kind, &inner)
                .unwrap_or_default()
                .iter()
                .map(|p| format!("{},{}", num(p.x), num(p.y)))
                .collect();
            let _ = write!(
                out,
                r#"<polygon points="{}" fill={or_fill} {}/>"#,
                points.join(" "),
                stroke(&or_ink)
            );
        }
        // Sides and the two caps' outer halves, then the lid's lower edge.
        ShapeKind::Database => {
            let (rx, ry) = (num(inner.w / 2.0), num(geometry::cap_height(&r) / 2.0));
            let (left, right) = (num(inner.x), num(inner.right()));
            let top = num(inner.y + geometry::cap_height(&r) / 2.0);
            let bottom = num(inner.bottom() - geometry::cap_height(&r) / 2.0);
            let _ = write!(
                out,
                r#"<path d="M{left},{top} A{rx},{ry} 0 0 1 {right},{top} V{bottom} A{rx},{ry} 0 0 1 {left},{bottom} Z" fill={or_fill} {}/>"#,
                stroke(&or_ink)
            );
            let _ = write!(
                out,
                r#"<path d="M{left},{top} A{rx},{ry} 0 0 0 {right},{top}" fill="none" {}/>"#,
                stroke(&or_ink)
            );
        }
        ShapeKind::Subroutine => {
            body(out, 0.0, &or_fill, &or_ink);
            let k = SUBROUTINE_INSET.min(r.w / 4.0);
            for x in [inner.x + k, inner.right() - k] {
                let _ = write!(
                    out,
                    r#"<line x1="{x}" y1="{}" x2="{x}" y2="{}" {}/>"#,
                    num(inner.y),
                    num(inner.bottom()),
                    stroke(&or_ink),
                    x = num(x)
                );
            }
        }
        ShapeKind::Stadium => body(out, geometry::stadium_radius(&r) - inset, &or_fill, &or_ink),
        ShapeKind::Circle => {
            let _ = write!(
                out,
                r#"<ellipse cx="{}" cy="{}" rx="{}" ry="{}" fill={or_fill} {}/>"#,
                num(inner.x + inner.w / 2.0),
                num(inner.y + inner.h / 2.0),
                num(inner.w / 2.0),
                num(inner.h / 2.0),
                stroke(&or_ink)
            );
        }
        ShapeKind::RoundRect => body(
            out,
            CORNER.min(r.h / 2.0).min(255.0) - inset,
            &or_fill,
            &or_ink,
        ),
        // Bare text, unless it has colours of its own.
        ShapeKind::Text if fill.is_some() || ink.is_some() => {
            let none = "\"none\"".to_owned();
            body(
                out,
                0.0,
                fill.as_ref().unwrap_or(&none),
                ink.as_ref().unwrap_or(&none),
            );
        }
        ShapeKind::Text => {}
    }
    let label_ink = s
        .style
        .label_rgba()
        .map(rgb_hex)
        .unwrap_or_else(|| TEXT.to_owned());
    // In its area: inside a diamond's or hexagon's outline.
    let area = s.label_rect();
    let wrap = (area.w - 2.0 * LABEL_PAD).max(1.0);
    text(
        out,
        ctx,
        &s.label,
        area.center().x,
        area.center().y,
        LABEL_SIZE,
        Some(wrap),
        &label_ink,
    );
    out.push_str("</g>\n");
}

/// A straight connector; an arrow's line stops short of the head so the
/// tip stays sharp (as `paint::connector`).
/// A connector through `path`: a `<line>` when straight, a `<polyline>`
/// with bends. An arrow's last segment stops at the head's base.
/// `ink`: the stroke colour, quoted.
fn connector(out: &mut String, path: &[Point], kind: ConnectorKind, ink: &str) {
    let [.., a, b] = path else {
        return;
    };
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    let mut points: Vec<(f32, f32)> = path.iter().map(|p| (p.x, p.y)).collect();
    let head = ARROW.clamp(6.0, 24.0).min(len);
    let (ux, uy) = if len > 0.0 {
        (dx / len, dy / len)
    } else {
        (0.0, 0.0)
    };
    let arrow = kind == ConnectorKind::Arrow && len > 0.0;
    if arrow {
        if let Some(end) = points.last_mut() {
            *end = (b.x - ux * head * 0.8, b.y - uy * head * 0.8);
        }
    }
    if let [(x1, y1), (x2, y2)] = points[..] {
        let _ = write!(
            out,
            r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke={ink} stroke-width="{}"/>"#,
            num(x1),
            num(y1),
            num(x2),
            num(y2),
            num(STROKE)
        );
    } else {
        let list: Vec<String> = points
            .iter()
            .map(|&(x, y)| format!("{},{}", num(x), num(y)))
            .collect();
        let _ = write!(
            out,
            r#"<polyline points="{}" fill="none" stroke={ink} stroke-width="{}"/>"#,
            list.join(" "),
            num(STROKE)
        );
    }
    if arrow {
        // `views::arrowhead`: tip at `b`, base `head` back, 0.45 × head to
        // each side.
        let base = (b.x - ux * head, b.y - uy * head);
        let (nx, ny) = (-uy * head * 0.45, ux * head * 0.45);
        let _ = write!(
            out,
            r#"<polygon points="{},{} {},{} {},{}" fill={ink}/>"#,
            num(b.x),
            num(b.y),
            num(base.0 + nx),
            num(base.1 + ny),
            num(base.0 - nx),
            num(base.1 - ny)
        );
    }
}

/// An edge label on a patch of background at the midpoint.
fn edge_label(out: &mut String, ctx: &egui::Context, at: (f32, f32), label: &str) {
    let galley = layout(ctx, label, EDGE_LABEL_SIZE, None);
    let (w, h) = (galley.rect.width() + 6.0, galley.rect.height() + 6.0);
    let _ = write!(
        out,
        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="2" fill="{BACKGROUND}"/>"#,
        num(at.0 - w / 2.0),
        num(at.1 - h / 2.0),
        num(w),
        num(h)
    );
    text(out, ctx, label, at.0, at.1, EDGE_LABEL_SIZE, None, INK);
}

/// `label`, centred on (`cx`, `cy`) line by line, wrapped at `wrap` as the
/// canvas wraps it: one `<tspan>` per line.
#[allow(clippy::too_many_arguments)]
fn text(
    out: &mut String,
    ctx: &egui::Context,
    label: &str,
    cx: f32,
    cy: f32,
    size: f32,
    wrap: Option<f32>,
    fill: &str,
) {
    if label.is_empty() {
        return;
    }
    let galley = layout(ctx, label, size, wrap);
    let top = cy - galley.rect.height() / 2.0 - galley.rect.top();
    let _ = write!(
        out,
        r#"<text x="{}" font-size="{}" fill="{fill}" text-anchor="middle" dominant-baseline="central">"#,
        num(cx),
        num(size)
    );
    for row in &galley.rows {
        let y = top + row.pos.y + row.row.size.y / 2.0;
        let _ = write!(
            out,
            r#"<tspan x="{}" y="{}">{}</tspan>"#,
            num(cx),
            num(y),
            escape(row.row.text().trim_end())
        );
    }
    out.push_str("</text>");
}

fn layout(ctx: &egui::Context, text: &str, size: f32, wrap: Option<f32>) -> Arc<egui::Galley> {
    let font = label_font(size / LABEL_SIZE);
    let mut job = LayoutJob::simple(
        text.to_owned(),
        font,
        Color32::PLACEHOLDER,
        wrap.unwrap_or(f32::INFINITY),
    );
    job.halign = Align::Center;
    ctx.fonts_mut(|f| f.layout_job(job))
}

/// A style colour as SVG attribute text (quoted), with its opacity when
/// it isn't opaque; `None` when BlockLiner can't draw it.
fn paint(c: Option<&blockliner_core::style::Colour>) -> Option<String> {
    let rgba = c?.rgba?;
    Some(if rgba[3] == 255 {
        format!("\"{}\"", rgb_hex(rgba))
    } else if rgba[3] == 0 {
        "\"none\"".to_owned()
    } else {
        // `fill="#rrggbb" fill-opacity=…` would need the attribute name;
        // rgba() works for fill and stroke alike.
        format!(
            "\"rgba({},{},{},{})\"",
            rgba[0],
            rgba[1],
            rgba[2],
            num(f32::from(rgba[3]) / 255.0)
        )
    })
}

fn rgb_hex([r, g, b, _]: [u8; 4]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// A coordinate, without needless decimals.
fn num(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

// --- PNG ----------------------------------------------------------------------

/// Rasterises `svg` at `scale` and encodes it as PNG.
pub fn png(svg: &str, scale: f32) -> Result<Vec<u8>> {
    let options = usvg::Options {
        resources_dir: None,
        fontdb: fonts(),
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(svg, &options).map_err(|e| anyhow!("{e}"))?;
    let w = (tree.size().width() * scale).round() as u32;
    let h = (tree.size().height() * scale).round() as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| anyhow!("The image would be {w} × {h} pixels, which is too large."))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    let image = image::RgbaImage::from_raw(w, h, rgba).ok_or_else(|| anyhow!("bad image"))?;
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png)?;
    Ok(bytes.into_inner())
}

/// The bundled faces, so a PNG's text is the canvas's font on every system.
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            for (_, bytes) in laankit_egui::fonts::bundled_faces() {
                db.load_font_data(bytes.to_vec());
            }
            db.set_sans_serif_family("Inter");
            Arc::new(db)
        })
        .clone()
}
