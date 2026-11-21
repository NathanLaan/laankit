//! Text: a slide's rich blocks as PowerPoint paragraphs and runs, their XML,
//! and an estimate of the room they take.

use laankit_richtext::math::{self, MathMode};
use laankit_richtext::{Align, Block as RichBlock, Span, TextKind};
use noteliner_slides::render::{self, SlideTheme};

use crate::xml::{esc, hex, Rels, Rgb};

/// One run of text with one look.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    /// In stage pixels.
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub mono: bool,
    pub color: Rgb,
    pub highlight: Option<Rgb>,
    /// Superscript (+) or subscript (-), in thousandths of a percent.
    pub baseline: i32,
    pub link: Option<String>,
    /// PowerPoint's slide number field: `text` is the number as exported.
    pub slide_number: bool,
}

impl Run {
    pub fn plain(text: impl Into<String>, size: f32, color: Rgb) -> Run {
        Run {
            text: text.into(),
            size,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            mono: false,
            color,
            highlight: None,
            baseline: 0,
            link: None,
            slide_number: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bullet {
    None,
    Dot,
    /// A numbered item, and its number.
    Number(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Para {
    pub runs: Vec<Run>,
    pub align: Align,
    /// The text's left edge, in stage pixels from the box's edge.
    pub left: f32,
    /// How far left of `left` the bullet hangs.
    pub hang: f32,
    pub bullet: Bullet,
    pub line_height: f32,
    /// Space above, in stage pixels.
    pub before: f32,
    /// The paragraph's own size, for an empty paragraph and the bullet.
    pub size: f32,
    pub color: Rgb,
}

impl Para {
    pub fn new(runs: Vec<Run>, size: f32, color: Rgb, line_height: f32) -> Para {
        Para {
            runs,
            align: Align::Left,
            left: 0.0,
            hang: 0.0,
            bullet: Bullet::None,
            line_height,
            before: 0.0,
            size,
            color,
        }
    }
}

/// How body text is drawn where a block is being flattened.
#[derive(Clone, Copy)]
pub struct Flow {
    pub size: f32,
    pub color: Rgb,
    pub italic: bool,
    pub bold: bool,
    pub left: f32,
    pub theme: SlideTheme,
}

impl Flow {
    pub fn new(size: f32, color: Rgb, theme: SlideTheme) -> Flow {
        Flow {
            size,
            color,
            italic: false,
            bold: false,
            left: 0.0,
            theme,
        }
    }
}

pub fn rgb(c: egui::Color32) -> Rgb {
    [c.r(), c.g(), c.b()]
}

/// Spans as runs, the way `render.rs` styles them.
pub fn runs(spans: &[Span], flow: &Flow) -> Vec<Run> {
    let theme = &flow.theme;
    let mut out = Vec::new();
    for span in spans {
        let st = span.style;
        let mut size = flow.size;
        if st.code {
            size *= 0.9;
        }
        if st.small || st.sup || st.sub {
            size *= 0.8;
        }
        let link = span.link.clone().filter(|l| linkable(l));
        let mut run = Run {
            text: span.text.clone(),
            size,
            bold: st.bold || flow.bold,
            italic: st.italic || flow.italic,
            underline: st.underline,
            strike: st.strike,
            mono: st.code,
            color: if st.mark {
                rgb(theme.bg)
            } else if span.link.is_some() {
                rgb(theme.accent)
            } else {
                flow.color
            },
            highlight: if st.mark {
                Some(rgb(theme.accent))
            } else if st.code {
                Some(rgb(theme.code_bg))
            } else {
                None
            },
            baseline: if st.sup {
                30000
            } else if st.sub {
                -25000
            } else {
                0
            },
            link,
            slide_number: false,
        };
        if let Some(checked) = st.checkbox {
            run.text = if checked { "☑ " } else { "☐ " }.to_string();
            run.highlight = None;
            run.mono = false;
        } else if let Some(mode) = st.math {
            // PowerPoint cannot hold a picture inside a line of text.
            run.text = math::fallback_text(&span.text, mode);
            run.mono = true;
            run.highlight = Some(rgb(theme.code_bg));
        }
        if !run.text.is_empty() {
            out.push(run);
        }
    }
    out
}

/// A link PowerPoint can follow.
pub fn linkable(target: &str) -> bool {
    let t = target.trim();
    let scheme_ok = ["http://", "https://", "mailto:"]
        .iter()
        .any(|p| t.len() > p.len() && t[..p.len()].eq_ignore_ascii_case(p));
    scheme_ok && !t.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Rich blocks as paragraphs. Pictures and tables inside a list or a quote
/// are reduced to their text.
pub fn paragraphs(blocks: &[RichBlock], flow: &Flow, out: &mut Vec<Para>) {
    for block in blocks {
        let gap = if out.is_empty() {
            0.0
        } else {
            render::PARAGRAPH_GAP
        };
        match block {
            RichBlock::Text {
                kind, spans, align, ..
            } => {
                let (f, lh) = match kind {
                    TextKind::Heading(level) => {
                        let (size, lh) = render::body_heading_size(*level);
                        (
                            Flow {
                                size,
                                bold: true,
                                color: rgb(flow.theme.accent),
                                ..*flow
                            },
                            lh,
                        )
                    }
                    TextKind::Caption => (
                        Flow {
                            size: flow.size * 0.8,
                            color: rgb(flow.theme.muted),
                            ..*flow
                        },
                        render::LINE_HEIGHT,
                    ),
                    TextKind::Paragraph => (*flow, render::LINE_HEIGHT),
                };
                let mut p = Para::new(runs(spans, &f), f.size, f.color, lh);
                p.align = *align;
                p.left = flow.left;
                p.before = gap;
                out.push(p);
            }
            RichBlock::List {
                ordered,
                start,
                items,
            } => {
                let indent = 1.4 * flow.size;
                let inner = Flow {
                    left: flow.left + indent,
                    ..*flow
                };
                for (k, item) in items.iter().enumerate() {
                    let first = out.len();
                    paragraphs(item, &inner, out);
                    if out.len() == first {
                        out.push(Para::new(
                            Vec::new(),
                            flow.size,
                            flow.color,
                            render::LINE_HEIGHT,
                        ));
                        out[first].left = inner.left;
                    }
                    let p = &mut out[first];
                    // A nested list's first paragraph keeps its own bullet.
                    if p.bullet == Bullet::None && p.left == inner.left {
                        p.bullet = if *ordered {
                            Bullet::Number(*start + k as u32)
                        } else {
                            Bullet::Dot
                        };
                        p.hang = indent;
                    }
                    p.before = if k == 0 { gap } else { render::ITEM_GAP };
                }
            }
            RichBlock::Quote(inner) => {
                let f = Flow {
                    left: flow.left + render::QUOTE_PAD,
                    ..*flow
                };
                let first = out.len();
                paragraphs(inner, &f, out);
                if let Some(p) = out.get_mut(first) {
                    p.before = gap;
                }
            }
            RichBlock::Code(code) => {
                let first = out.len();
                out.extend(code_paragraphs(code, flow.left, &flow.theme));
                if let Some(p) = out.get_mut(first) {
                    p.before = gap;
                }
            }
            RichBlock::Image { src, alt, .. } => {
                let text = match math::parse_image_src(src) {
                    Some(latex) => math::fallback_text(&latex, MathMode::Display),
                    None if alt.is_empty() => continue,
                    None => format!("[{alt}]"),
                };
                let mut p = Para::new(
                    vec![Run::plain(text, flow.size, rgb(flow.theme.muted))],
                    flow.size,
                    flow.color,
                    render::LINE_HEIGHT,
                );
                p.left = flow.left;
                p.before = gap;
                out.push(p);
            }
            RichBlock::Rule => {}
            RichBlock::Table { rows, .. } => {
                for (r, row) in rows.iter().enumerate() {
                    let mut all = Vec::new();
                    for (c, cell) in row.iter().enumerate() {
                        if c > 0 {
                            all.push(Run::plain("\t", flow.size, flow.color));
                        }
                        all.extend(runs(cell, flow));
                    }
                    let mut p = Para::new(all, flow.size, flow.color, render::LINE_HEIGHT);
                    p.left = flow.left;
                    p.before = if r == 0 { gap } else { 0.0 };
                    out.push(p);
                }
            }
        }
    }
}

/// A code block: one paragraph for each line, in the monospace font.
pub fn code_paragraphs(code: &str, left: f32, theme: &SlideTheme) -> Vec<Para> {
    let size = render::CODE_SIZE;
    let color = rgb(theme.fg);
    code.trim_end_matches('\n')
        .split('\n')
        .map(|line| {
            let mut run = Run::plain(line.replace('\t', "    "), size, color);
            run.mono = true;
            let runs = if run.text.is_empty() {
                Vec::new()
            } else {
                vec![run]
            };
            let mut p = Para::new(runs, size, color, render::LINE_HEIGHT);
            p.left = left;
            p
        })
        .collect()
}

// ─── Measuring ───────────────────────────────────────────────────────────────

/// A character's advance in ems. PowerPoint lays the text out itself, so this
/// only has to be near enough to stack boxes without large gaps or overlaps.
fn advance(c: char, mono: bool, bold: bool) -> f32 {
    if mono {
        return 0.6;
    }
    let w = match c {
        ' ' => 0.28,
        'i' | 'j' | 'l' | 't' | 'f' | 'r' | 'I' | '.' | ',' | ';' | ':' | '\'' | '|' | '!'
        | '(' | ')' | '[' | ']' => 0.33,
        'm' | 'w' | 'M' | 'W' | '@' | '%' => 0.88,
        'A'..='Z' | '0'..='9' => 0.66,
        c if (c as u32) >= 0x2E80 => 1.0,
        _ => 0.56,
    };
    if bold {
        w * 1.05
    } else {
        w
    }
}

/// How many lines a paragraph takes when wrapped at `width` stage pixels.
pub fn lines(p: &Para, width: f32) -> usize {
    let width = (width - p.left).max(p.size);
    let mut count = 1;
    let mut x = 0.0f32;
    let mut word = 0.0f32;
    for run in &p.runs {
        for c in run.text.chars() {
            if c == '\n' {
                if x + word > width && x > 0.0 {
                    count += 1;
                }
                count += 1;
                x = 0.0;
                word = 0.0;
                continue;
            }
            let w = advance(c, run.mono, run.bold) * run.size;
            if c.is_whitespace() {
                if x + word > width && x > 0.0 {
                    count += 1;
                    x = 0.0;
                }
                // A word longer than the line breaks inside itself.
                count += (word / width) as usize;
                x += word % width + w;
                word = 0.0;
            } else {
                word += w;
            }
        }
    }
    if x + word > width && x > 0.0 {
        count += 1;
    }
    count + (word / width) as usize
}

/// The height of paragraphs in a box `width` stage pixels wide.
pub fn height(paras: &[Para], width: f32) -> f32 {
    paras
        .iter()
        .map(|p| {
            let size = p.runs.iter().map(|r| r.size).fold(p.size, f32::max);
            p.before + lines(p, width) as f32 * size * p.line_height
        })
        .sum()
}

// ─── XML ─────────────────────────────────────────────────────────────────────

/// The fonts the file names.
#[derive(Clone, Debug)]
pub struct Fonts {
    pub text: String,
    pub code: String,
}

/// A size in stage pixels as hundredths of a point (a stage pixel is half a
/// point), within PowerPoint's limits.
fn sz(px: f32) -> i64 {
    ((px * 50.0).round() as i64).clamp(100, 400_000)
}

fn emu(px: f32) -> i64 {
    crate::xml::emu(px)
}

fn run_props(tag: &str, run: &Run, fonts: &Fonts, rels: &mut Rels) -> String {
    let mut s = format!("<a:{tag} lang=\"en-US\" sz=\"{}\"", sz(run.size));
    s.push_str(if run.bold { " b=\"1\"" } else { " b=\"0\"" });
    s.push_str(if run.italic { " i=\"1\"" } else { " i=\"0\"" });
    if run.underline {
        s.push_str(" u=\"sng\"");
    }
    if run.strike {
        s.push_str(" strike=\"sngStrike\"");
    }
    if run.baseline != 0 {
        s.push_str(&format!(" baseline=\"{}\"", run.baseline));
    }
    s.push_str(" dirty=\"0\">");
    s.push_str(&format!(
        "<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>",
        hex(run.color)
    ));
    if let Some(h) = run.highlight {
        s.push_str(&format!(
            "<a:highlight><a:srgbClr val=\"{}\"/></a:highlight>",
            hex(h)
        ));
    }
    let face = esc(if run.mono { &fonts.code } else { &fonts.text });
    s.push_str(&format!(
        "<a:latin typeface=\"{face}\"/><a:ea typeface=\"{face}\"/><a:cs typeface=\"{face}\"/>"
    ));
    if let Some(link) = &run.link {
        let id = rels.external(crate::xml::REL_HYPERLINK, link.trim());
        s.push_str(&format!("<a:hlinkClick r:id=\"{id}\"/>"));
    }
    s.push_str(&format!("</a:{tag}>"));
    s
}

/// One `<a:p>`.
pub fn para_xml(p: &Para, fonts: &Fonts, rels: &mut Rels) -> String {
    let mut s = String::from("<a:p><a:pPr");
    let left = emu(p.left.max(0.0));
    let hang = emu(p.hang.max(0.0)).min(left);
    s.push_str(&format!(" marL=\"{left}\" indent=\"{}\"", -hang));
    match p.align {
        Align::Center => s.push_str(" algn=\"ctr\""),
        Align::Right => s.push_str(" algn=\"r\""),
        Align::Justify => s.push_str(" algn=\"just\""),
        Align::Left => s.push_str(" algn=\"l\""),
    }
    s.push('>');
    s.push_str(&format!(
        "<a:lnSpc><a:spcPct val=\"{}\"/></a:lnSpc>",
        ((p.line_height * 100_000.0 / 1.2).round() as i64).clamp(50_000, 300_000)
    ));
    s.push_str(&format!(
        "<a:spcBef><a:spcPts val=\"{}\"/></a:spcBef><a:spcAft><a:spcPts val=\"0\"/></a:spcAft>",
        ((p.before.max(0.0) * 50.0).round() as i64).min(158_400)
    ));
    match p.bullet {
        Bullet::None => s.push_str("<a:buNone/>"),
        Bullet::Dot => s.push_str(&format!(
            "<a:buClr><a:srgbClr val=\"{}\"/></a:buClr><a:buFont typeface=\"Arial\"/><a:buChar char=\"•\"/>",
            hex(p.color)
        )),
        Bullet::Number(n) => s.push_str(&format!(
            "<a:buClr><a:srgbClr val=\"{}\"/></a:buClr><a:buFont typeface=\"+mj-lt\"/><a:buAutoNum type=\"arabicPeriod\" startAt=\"{}\"/>",
            hex(p.color),
            n.clamp(1, 32767)
        )),
    }
    s.push_str("</a:pPr>");
    for run in &p.runs {
        if run.slide_number {
            s.push_str(&format!(
                "<a:fld id=\"{{B6F15528-21DE-4FAA-801E-634DDDAF4B2B}}\" type=\"slidenum\">{}<a:t>{}</a:t></a:fld>",
                run_props("rPr", run, fonts, rels),
                esc(&run.text)
            ));
            continue;
        }
        let props = run_props("rPr", run, fonts, rels);
        for (i, piece) in run.text.split('\n').enumerate() {
            if i > 0 {
                s.push_str("<a:br>");
                s.push_str(&props);
                s.push_str("</a:br>");
            }
            if !piece.is_empty() {
                s.push_str(&format!("<a:r>{props}<a:t>{}</a:t></a:r>", esc(piece)));
            }
        }
    }
    let end = Run::plain("", p.size, p.color);
    let mut none = Rels::default();
    s.push_str(&run_props("endParaRPr", &end, fonts, &mut none));
    s.push_str("</a:p>");
    s
}

pub fn paras_xml(paras: &[Para], fonts: &Fonts, rels: &mut Rels) -> String {
    if paras.is_empty() {
        return "<a:p/>".to_string();
    }
    paras.iter().map(|p| para_xml(p, fonts, rels)).collect()
}
