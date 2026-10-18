//! The text layer: every glyph's character(s) and box in page units, collected by a hayro
//! [`Device`] that ignores everything but text. Selection and highlight rectangles are computed
//! from these boxes, the way pdf.js's text layer is used by the Svelte reader.

use hayro::hayro_interpret::font::{Glyph, GlyphRun};
use hayro::hayro_interpret::hayro_syntax::page::Page;
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_interpret::{
    interpret_page, BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image,
    ImageDrawProps, InterpreterCache, InterpreterSettings, SoftMask,
};
use hayro::kurbo::{Affine, BezPath, Point, Rect};

/// Glyph box in glyph space (1000 units per em): pdf.js-like ascent and descent.
const ASCENT: f64 = 800.0;
const DESCENT: f64 = -200.0;
/// Advance assumed when a font doesn't report one (Type 3 glyphs).
const FALLBACK_ADVANCE: f64 = 500.0;
/// A horizontal gap wider than this share of the line height becomes a space.
const SPACE_GAP: f32 = 0.2;

/// One glyph of the page's text.
#[derive(Clone, Debug)]
pub struct Char {
    pub text: String,
    /// `[x, y, w, h]` in page units.
    pub rect: [f32; 4],
}

impl Char {
    fn center(&self) -> [f32; 2] {
        [
            self.rect[0] + self.rect[2] / 2.0,
            self.rect[1] + self.rect[3] / 2.0,
        ]
    }
}

/// A page's text in content-stream order (pdf.js's `getTextContent` order).
#[derive(Clone, Debug, Default)]
pub struct PageText {
    pub chars: Vec<Char>,
}

/// A selection resolved against the text layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    /// One rectangle per line, in page units (`[x, y, w, h]`).
    pub rects: Vec<[f32; 4]>,
    pub text: String,
}

/// Collects the text of `page` (all render modes, including invisible OCR text).
pub fn extract<'a>(
    page: &Page<'a>,
    cache: &InterpreterCache<'a>,
    settings: &InterpreterSettings,
) -> PageText {
    let (w, h) = page.render_dimensions();
    let transform = page.initial_transform(true).to_kurbo();
    let mut ctx = Context::new(
        transform,
        Rect::new(0.0, 0.0, w as f64, h as f64),
        cache,
        page.xref(),
        settings.clone(),
    );
    let mut device = TextDevice::default();
    interpret_page(page, &mut ctx, &mut device);
    PageText {
        chars: device.chars,
    }
}

impl PageText {
    /// The text between two points (in page units), like a drag in a browser's text layer: from
    /// the glyph nearest `from` to the glyph nearest `to`, in content-stream order.
    pub fn select(&self, from: [f32; 2], to: [f32; 2]) -> Option<Selection> {
        let a = self.nearest(from)?;
        let b = self.nearest(to)?;
        let (start, end) = (a.min(b), a.max(b));
        Some(self.range(start, end))
    }

    /// Merges the glyphs `start..=end` into line rectangles and text.
    pub fn range(&self, start: usize, end: usize) -> Selection {
        let mut rects: Vec<[f32; 4]> = Vec::new();
        let mut text = String::new();
        let mut prev: Option<&Char> = None;
        for c in &self.chars[start..=end] {
            match prev {
                Some(p) if same_line(p, c) => {
                    let gap = c.rect[0] - (p.rect[0] + p.rect[2]);
                    if gap > SPACE_GAP * c.rect[3]
                        && !text.ends_with(' ')
                        && !c.text.starts_with(' ')
                    {
                        text.push(' ');
                    }
                    let r = rects.last_mut().expect("a line is open");
                    let (x0, y0) = (r[0].min(c.rect[0]), r[1].min(c.rect[1]));
                    let (x1, y1) = (
                        (r[0] + r[2]).max(c.rect[0] + c.rect[2]),
                        (r[1] + r[3]).max(c.rect[1] + c.rect[3]),
                    );
                    *r = [x0, y0, x1 - x0, y1 - y0];
                }
                _ => {
                    if prev.is_some() && !text.ends_with(' ') {
                        text.push(' ');
                    }
                    rects.push(c.rect);
                }
            }
            text.push_str(&c.text);
            prev = Some(c);
        }
        Selection {
            rects,
            text: text.split_whitespace().collect::<Vec<_>>().join(" "),
        }
    }

    /// The glyph under `p`, or else the closest one (vertical distance weighs more, so a point in
    /// a line's margin picks that line).
    fn nearest(&self, p: [f32; 2]) -> Option<usize> {
        let inside = self.chars.iter().position(|c| {
            p[0] >= c.rect[0]
                && p[0] <= c.rect[0] + c.rect[2]
                && p[1] >= c.rect[1]
                && p[1] <= c.rect[1] + c.rect[3]
        });
        inside.or_else(|| {
            self.chars
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let [cx, cy] = c.center();
                    let dx = (p[0] - cx).abs() - c.rect[2] / 2.0;
                    let dy = (p[1] - cy).abs() - c.rect[3] / 2.0;
                    (i, dx.max(0.0) + 4.0 * dy.max(0.0))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        })
    }
}

/// Whether `c` continues `p`'s line: overlapping vertically and not running backwards.
fn same_line(p: &Char, c: &Char) -> bool {
    let overlap = (p.rect[1] + p.rect[3]).min(c.rect[1] + c.rect[3]) - p.rect[1].max(c.rect[1]);
    overlap > 0.5 * p.rect[3].min(c.rect[3]) && c.rect[0] + 0.5 * c.rect[2] >= p.rect[0]
}

#[derive(Default)]
struct TextDevice {
    chars: Vec<Char>,
    /// The previous run's first glyph box, to drop the second pass of fill-then-stroke text.
    last_run: Option<(usize, [f32; 4])>,
}

impl<'a> Device<'a> for TextDevice {
    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        let start = self.chars.len();
        for glyph in run.glyphs() {
            let advance = match &**glyph {
                Glyph::Outline(g) => g
                    .advance_width()
                    .map(f64::from)
                    .filter(|a| *a > 0.0)
                    .unwrap_or(FALLBACK_ADVANCE),
                Glyph::Type3(_) => FALLBACK_ADVANCE,
            };
            let Some(text) = glyph.as_unicode().map(|u| match u {
                hayro::hayro_interpret::hayro_cmap::BfString::Char(c) => c.to_string(),
                hayro::hayro_interpret::hayro_cmap::BfString::String(s) => s,
            }) else {
                continue;
            };
            let rect = glyph_box(props.transform * glyph.transform(), advance);
            self.chars.push(Char { text, rect });
        }
        // FillStroke text arrives twice with the same glyphs.
        if let (Some((len, first)), Some(c)) = (self.last_run, self.chars.get(start)) {
            if self.chars.len() - start == len && c.rect == first && start >= len {
                self.chars.truncate(start);
                return;
            }
        }
        self.last_run = self
            .chars
            .get(start)
            .map(|c| (self.chars.len() - start, c.rect));
    }

    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// The axis-aligned box of a glyph `advance` wide, mapped to page units.
fn glyph_box(t: Affine, advance: f64) -> [f32; 4] {
    let corners = [
        Point::new(0.0, DESCENT),
        Point::new(advance, DESCENT),
        Point::new(0.0, ASCENT),
        Point::new(advance, ASCENT),
    ]
    .map(|p| t * p);
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in corners {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    [x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(text: &str, x: f32, y: f32) -> Char {
        Char {
            text: text.into(),
            rect: [x, y, 6.0, 12.0],
        }
    }

    #[test]
    fn selection_merges_lines_and_inserts_spaces() {
        let text = PageText {
            chars: vec![
                ch("a", 10.0, 10.0),
                ch("b", 16.0, 10.0),
                ch("c", 30.0, 10.0),
                ch("d", 10.0, 30.0),
                ch("e", 16.0, 30.0),
            ],
        };
        let sel = text.select([11.0, 15.0], [17.0, 35.0]).unwrap();
        assert_eq!(sel.text, "ab c de");
        assert_eq!(
            sel.rects,
            vec![[10.0, 10.0, 26.0, 12.0], [10.0, 30.0, 12.0, 12.0]]
        );
    }

    #[test]
    fn drag_backwards_selects_the_same_range() {
        let text = PageText {
            chars: vec![
                ch("a", 10.0, 10.0),
                ch("b", 16.0, 10.0),
                ch("c", 22.0, 10.0),
            ],
        };
        assert_eq!(
            text.select([23.0, 15.0], [11.0, 15.0]),
            text.select([11.0, 15.0], [23.0, 15.0])
        );
    }

    #[test]
    fn point_in_the_margin_snaps_to_the_nearest_line() {
        let text = PageText {
            chars: vec![ch("a", 10.0, 10.0), ch("b", 10.0, 30.0)],
        };
        assert_eq!(text.select([200.0, 33.0], [200.0, 33.0]).unwrap().text, "b");
    }
}
