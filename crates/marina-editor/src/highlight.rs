//! Markdown highlighting for `TextEdit`'s layouter, with a per-line galley
//! cache (Phase 0 spike 1: see `apps/spikes/editor/RESULTS.md`).
//!
//! A line-based scanner (not a full CommonMark parse): each line is
//! highlighted from its own text plus the block state it starts in (normal,
//! inside frontmatter, inside a code fence). The layouter keeps one galley per
//! line. An edit diffs the new text against the previous one (common prefix
//! and suffix), rescans and lays out only the lines in between, plus any
//! following lines whose block state changed (for example after opening a
//! fence), and joins the line galleys with `Galley::concat`.
//!
//! The per-line cache is what makes this usable: egui's own paragraph cache
//! drops every entry not used in the previous frame, so a single whole-text
//! `LayoutJob` re-lays out the whole note on the first keystroke after any
//! pause (127 ms at 5,000 lines).

use std::ops::Range;
use std::sync::Arc;

use egui::text::{ByteIndex, LayoutJob, LayoutSection, TextWrapping};
use egui::{Color32, FontFamily, FontId, Galley, Stroke, TextFormat, Ui};
use marina_egui::fonts::Style;
use marina_egui::theme::Palette;

use crate::spell::{misspelled, SpellChecker};

/// What a run of text is, for styling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Text,
    /// Markup that isn't content: `**`, `#`, `[` `](` and the like.
    Muted,
    Heading(u8),
    Strong,
    Emphasis,
    Code,
    CodeBlock,
    LinkText,
    Url,
    WikiLink,
    ListMarker,
    Task,
    Quote,
    Frontmatter,
}

/// The block a line starts in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum BlockState {
    #[default]
    Normal,
    Frontmatter,
    Fence {
        ch: u8,
        len: u8,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// Byte range within the line.
    pub range: Range<usize>,
    pub kind: Kind,
    /// Flagged by the spell checker.
    pub misspelled: bool,
}

/// The editor's colours, derived from the app theme's [`Palette`] so all six
/// themes work:
///
/// | Colour        | From                                                              |
/// |---------------|-------------------------------------------------------------------|
/// | text          | `--text-primary`                                                  |
/// | muted markup  | `--text-muted`                                                    |
/// | headings      | `--accent`, bold                                                  |
/// | links         | `--accent` (text, underlined), `--text-muted` (URL)               |
/// | wikilinks     | `--accent-hover`                                                  |
/// | list markers  | `--accent`                                                        |
/// | quotes        | `--text-secondary`, italic                                        |
/// | inline code   | the code colour on `--code-bg`                                    |
/// | code blocks   | the code colour on `--pre-bg`                                     |
/// | frontmatter   | `--text-muted`, monospace                                         |
/// | background    | `--bg-surface` (the Svelte `.editor-wrapper`)                     |
/// | gutter        | `--bg-base`, numbers in `--text-faint` (active line `--text-muted`) |
/// | active line   | `--bg-item-hover`                                                 |
/// | find matches  | `--accent` at 25% (the current one at 45%)                        |
///
/// The code colour is the one colour not in the palette: CodeMirror's string
/// green, One Dark's `#98c379` on dark themes and the default style's
/// `#116644` on light ones, as the Svelte editor shows code.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorColors {
    pub text: Color32,
    pub muted: Color32,
    pub heading: Color32,
    pub link: Color32,
    pub wiki: Color32,
    pub marker: Color32,
    pub quote: Color32,
    pub code: Color32,
    pub code_bg: Color32,
    pub pre_bg: Color32,
    pub background: Color32,
    pub gutter: Color32,
    pub gutter_text: Color32,
    pub gutter_active: Color32,
    pub active_line: Color32,
    pub find_match: Color32,
    pub find_current: Color32,
    pub misspelled: Color32,
}

impl EditorColors {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            text: p.text_primary,
            muted: p.text_muted,
            heading: p.accent,
            link: p.accent,
            wiki: p.accent_hover,
            marker: p.accent,
            quote: p.text_secondary,
            code: if p.dark {
                Color32::from_rgb(0x98, 0xc3, 0x79)
            } else {
                Color32::from_rgb(0x11, 0x66, 0x44)
            },
            code_bg: p.code_bg,
            pre_bg: p.pre_bg,
            background: p.bg_surface,
            gutter: p.bg_base,
            gutter_text: p.text_faint,
            gutter_active: p.text_muted,
            active_line: p.bg_item_hover,
            find_match: p.accent.gamma_multiply(0.25),
            find_current: p.accent.gamma_multiply(0.45),
            misspelled: Color32::from_rgb(0xe0, 0x4b, 0x4b),
        }
    }

    /// For the layout cache: a change of colours re-lays out every line.
    fn key(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325_u64;
        for c in [
            self.text,
            self.muted,
            self.heading,
            self.link,
            self.wiki,
            self.marker,
            self.quote,
            self.code,
            self.code_bg,
            self.pre_bg,
            self.misspelled,
        ] {
            for b in c.to_array() {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        }
        h
    }

    fn format(&self, kind: Kind, size: f32, misspelled: bool) -> TextFormat {
        let font = |style: Style| FontId::new(size, style.family());
        let body = FontId::new(size, FontFamily::Proportional);
        let mono = FontId::new(size, FontFamily::Monospace);
        let plain = |font_id: FontId, color: Color32| TextFormat {
            font_id,
            color,
            ..Default::default()
        };
        let mut format = match kind {
            Kind::Text => plain(body, self.text),
            Kind::Muted => plain(body, self.muted),
            Kind::Heading(_) => plain(font(Style::BOLD), self.heading),
            Kind::Strong => plain(font(Style::BOLD), self.text),
            Kind::Emphasis => plain(font(Style::ITALIC), self.text),
            Kind::Code => TextFormat {
                background: self.code_bg,
                ..plain(mono, self.code)
            },
            Kind::CodeBlock => TextFormat {
                background: self.pre_bg,
                ..plain(mono, self.code)
            },
            Kind::LinkText => TextFormat {
                underline: Stroke::new(1.0, self.link),
                ..plain(body, self.link)
            },
            Kind::Url => plain(body, self.muted),
            Kind::WikiLink => plain(body, self.wiki),
            Kind::ListMarker | Kind::Task => plain(body, self.marker),
            Kind::Quote => plain(font(Style::ITALIC), self.quote),
            Kind::Frontmatter => plain(mono, self.muted),
        };
        if misspelled {
            format.underline = Stroke::new(1.5, self.misspelled);
        }
        format
    }
}

/// Counters for checking the caches work (the benchmark prints them).
#[derive(Clone, Copy, Debug, Default)]
pub struct HighlightStats {
    /// Layouter calls.
    pub calls: u64,
    /// Calls that built a new document galley (the text or look changed).
    pub rebuilds: u64,
    /// Lines scanned and laid out again.
    pub lines_laid_out: u64,
}

/// The cached layouter. One per editor.
#[derive(Default)]
pub struct Highlighter {
    /// One entry per line of `last.text`.
    doc: Vec<LineEntry>,
    /// The first row of each line in the last document galley, plus the row count at the end.
    line_rows: Vec<usize>,
    last: Option<Last>,
    pub stats: HighlightStats,
}

/// A line's spans and the block state the next line starts in.
type Scanned = (Vec<Span>, BlockState);
/// A matched inline construct: its byte length and its parts as (start, end, kind).
type Inline = (usize, Vec<(usize, usize, Kind)>);

#[derive(Clone)]
struct LineEntry {
    /// The block state the line starts in.
    state: BlockState,
    scan: Arc<Scanned>,
    galley: Arc<Galley>,
}

/// Everything besides the text that changes a line's galley.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Look {
    /// Wrap width, rounded to whole points as epaint does (`u32::MAX`: no wrapping).
    pub wrap: u32,
    pub font_size_bits: u32,
    pub colors: u64,
    /// Changes whenever the spell checker (or its on/off state) does.
    pub spell: u64,
}

struct Last {
    text: String,
    look: Look,
    galley: Arc<Galley>,
}

/// How lines are laid out: the colours, the font size and the spell checker.
pub struct LayoutOptions<'a> {
    pub colors: &'a EditorColors,
    pub font_size: f32,
    pub wrap: bool,
    /// The spell checker and a version number that changes with it.
    pub spell: Option<(&'a dyn SpellChecker, u64)>,
}

impl Highlighter {
    /// The layouter body.
    pub fn layout(
        &mut self,
        ui: &Ui,
        text: &str,
        wrap_width: f32,
        opts: &LayoutOptions<'_>,
    ) -> Arc<Galley> {
        self.stats.calls += 1;
        let look = Look {
            wrap: if opts.wrap {
                wrap_width.round().max(0.0) as u32
            } else {
                u32::MAX
            },
            font_size_bits: opts.font_size.to_bits(),
            colors: opts.colors.key(),
            spell: opts.spell.map_or(0, |(_, v)| v.wrapping_add(1)),
        };
        if let Some(last) = &self.last {
            if last.look == look && last.text == text {
                return last.galley.clone();
            }
        }
        self.stats.rebuilds += 1;
        let galley = self.incremental(ui, text, opts, look);
        self.last = Some(Last {
            text: text.to_owned(),
            look,
            galley: galley.clone(),
        });
        galley
    }

    /// The galley of the last layout, if the text hasn't changed since.
    pub fn galley_for(&self, text: &str) -> Option<Arc<Galley>> {
        self.last
            .as_ref()
            .filter(|l| l.text == text)
            .map(|l| l.galley.clone())
    }

    /// Lines in the last layout.
    pub fn line_count(&self) -> usize {
        self.doc.len()
    }

    /// The first row of each 0-based line in the last document galley,
    /// followed by the galley's row count; empty before the first layout.
    pub fn line_rows(&self) -> &[usize] {
        &self.line_rows
    }

    /// The 0-based line holding row `row` of the last document galley.
    pub fn line_of_row(&self, row: usize) -> usize {
        self.line_rows
            .partition_point(|&first| first <= row)
            .saturating_sub(1)
    }

    /// Spans of 0-based line `line` from the last layout.
    pub fn line_spans(&self, line: usize) -> Option<&[Span]> {
        self.doc.get(line).map(|e| e.scan.0.as_slice())
    }

    fn incremental(
        &mut self,
        ui: &Ui,
        text: &str,
        opts: &LayoutOptions<'_>,
        look: Look,
    ) -> Arc<Galley> {
        let old = std::mem::take(&mut self.doc);
        let reusable = self
            .last
            .as_ref()
            .filter(|l| l.look == look && !old.is_empty());
        // Lines [0, first) are unchanged; new lines (new_last, ..) match old lines (old_last, ..).
        let (first, start, old_last, new_last) = match reusable {
            Some(last) => changed_lines(&last.text, text, old.len()),
            None => (0, 0, old.len(), usize::MAX),
        };
        let mut doc = Vec::with_capacity(old.len() + 8);
        doc.extend_from_slice(&old[..first]);
        let mut state = doc
            .last()
            .map_or(BlockState::Normal, |e: &LineEntry| e.scan.1);
        let mut j = old_last.wrapping_add(1);
        for (k, line) in text[start..].split('\n').enumerate() {
            let i = first + k;
            if i > new_last {
                // Past the edit: the rest is reusable once a line starts in the same state.
                if j < old.len() && old[j].state == state {
                    doc.extend_from_slice(&old[j..]);
                    break;
                }
                j += 1;
            }
            let mut scan = scan_line(line, state, i == 0);
            if let Some((checker, _)) = opts.spell {
                flag_misspelled(line, &mut scan.0, checker);
            }
            let scan = Arc::new(scan);
            self.stats.lines_laid_out += 1;
            let galley = line_galley(ui, line, &scan.0, opts, look);
            doc.push(LineEntry {
                state,
                scan: scan.clone(),
                galley,
            });
            state = scan.1;
        }
        let galleys: Vec<Arc<Galley>> = doc.iter().map(|e| e.galley.clone()).collect();
        self.line_rows.clear();
        let mut rows = 0;
        for g in &galleys {
            self.line_rows.push(rows);
            rows += g.rows.len();
        }
        self.line_rows.push(rows);
        self.doc = doc;
        let job = merged_job(text, opts, look);
        Arc::new(Galley::concat(
            Arc::new(job),
            &galleys,
            ui.ctx().pixels_per_point(),
        ))
    }
}

/// The job behind a `Galley::concat` of line galleys. The rows already carry
/// the formatting, but `Galley::is_empty` checks for sections, and the
/// painter skips an "empty" galley, so give it one covering the text.
fn merged_job(text: &str, opts: &LayoutOptions<'_>, look: Look) -> LayoutJob {
    let mut job = LayoutJob {
        text: text.to_owned(),
        wrap: wrapping(look),
        keep_trailing_whitespace: true,
        ..Default::default()
    };
    push_section(
        &mut job.sections,
        0..text.len(),
        opts.colors.format(Kind::Text, opts.font_size, false),
    );
    job
}

fn wrapping(look: Look) -> TextWrapping {
    if look.wrap == u32::MAX {
        TextWrapping::no_max_width()
    } else {
        TextWrapping::wrap_at_width(look.wrap as f32)
    }
}

fn line_galley(
    ui: &Ui,
    line: &str,
    spans: &[Span],
    opts: &LayoutOptions<'_>,
    look: Look,
) -> Arc<Galley> {
    let mut job = LayoutJob {
        text: line.to_owned(),
        wrap: wrapping(look),
        keep_trailing_whitespace: true,
        ..Default::default()
    };
    for span in spans {
        push_section(
            &mut job.sections,
            span.range.clone(),
            opts.colors
                .format(span.kind, opts.font_size, span.misspelled),
        );
    }
    if line.is_empty() {
        // An empty line still needs a font for its row height.
        push_section(
            &mut job.sections,
            0..0,
            opts.colors.format(Kind::Text, opts.font_size, false),
        );
    }
    ui.fonts_mut(|f| f.layout_job(job))
}

/// Splits prose spans at the words `checker` rejects and flags those.
fn flag_misspelled(line: &str, spans: &mut Vec<Span>, checker: &dyn SpellChecker) {
    let prose = |k: Kind| {
        matches!(
            k,
            Kind::Text
                | Kind::Heading(_)
                | Kind::Strong
                | Kind::Emphasis
                | Kind::LinkText
                | Kind::Quote
        )
    };
    if !spans.iter().any(|s| prose(s.kind)) {
        return;
    }
    let mut out = Vec::with_capacity(spans.len() + 2);
    for span in spans.drain(..) {
        if !prose(span.kind) {
            out.push(span);
            continue;
        }
        let base = span.range.start;
        let mut at = base;
        for bad in misspelled(checker, &line[span.range.clone()]) {
            let (s, e) = (base + bad.start, base + bad.end);
            if s > at {
                out.push(Span {
                    range: at..s,
                    kind: span.kind,
                    misspelled: false,
                });
            }
            out.push(Span {
                range: s..e,
                kind: span.kind,
                misspelled: true,
            });
            at = e;
        }
        if at < span.range.end {
            out.push(Span {
                range: at..span.range.end,
                kind: span.kind,
                misspelled: false,
            });
        }
    }
    *spans = out;
}

/// Where `new` differs from `old` (which has `old_lines` lines), by line:
/// (first changed line, byte offset of that line, last changed old line,
/// last changed new line). Lines after the last changed ones are identical.
fn changed_lines(old: &str, new: &str, old_lines: usize) -> (usize, usize, usize, usize) {
    let (o, n) = (old.as_bytes(), new.as_bytes());
    let newlines = |b: &[u8]| b.iter().filter(|&&c| c == b'\n').count();
    let p = o.iter().zip(n).take_while(|(a, b)| a == b).count();
    let max_s = o.len().min(n.len()) - p;
    let s = o
        .iter()
        .rev()
        .zip(n.iter().rev())
        .take(max_s)
        .take_while(|(a, b)| a == b)
        .count();
    let first = newlines(&o[..p]);
    let start = o[..p]
        .iter()
        .rposition(|&c| c == b'\n')
        .map_or(0, |i| i + 1);
    let new_lines = old_lines + newlines(&n[p..n.len() - s]) - newlines(&o[p..o.len() - s]);
    let old_last = old_lines - 1 - newlines(&o[o.len() - s..]);
    let new_last = (new_lines - 1 - newlines(&n[n.len() - s..])).max(first.saturating_sub(1));
    (first, start, old_last, new_last)
}

fn push_section(sections: &mut Vec<LayoutSection>, range: Range<usize>, format: TextFormat) {
    sections.push(LayoutSection {
        leading_space: 0.0,
        byte_range: ByteIndex(range.start)..ByteIndex(range.end),
        format,
    });
}

/// Highlights one line that starts in `state`; returns its spans (tiling the
/// whole line, in order) and the state the next line starts in.
pub fn scan_line(line: &str, state: BlockState, first_line: bool) -> (Vec<Span>, BlockState) {
    let whole = |kind| {
        if line.is_empty() {
            vec![]
        } else {
            vec![Span {
                range: 0..line.len(),
                kind,
                misspelled: false,
            }]
        }
    };
    let trimmed = line.trim_start();
    match state {
        BlockState::Frontmatter => {
            let end = matches!(line.trim_end(), "---" | "...");
            return (
                whole(Kind::Frontmatter),
                if end {
                    BlockState::Normal
                } else {
                    BlockState::Frontmatter
                },
            );
        }
        BlockState::Fence { ch, len } => {
            let run = trimmed.bytes().take_while(|&b| b == ch).count();
            let closes = run >= len as usize && trimmed[run..].trim().is_empty();
            return (
                whole(if closes { Kind::Muted } else { Kind::CodeBlock }),
                if closes { BlockState::Normal } else { state },
            );
        }
        BlockState::Normal => {}
    }
    if first_line && line.trim_end() == "---" {
        return (whole(Kind::Frontmatter), BlockState::Frontmatter);
    }
    if let Some(ch) = trimmed.bytes().next().filter(|b| *b == b'`' || *b == b'~') {
        let run = trimmed.bytes().take_while(|&b| b == ch).count();
        if run >= 3 && line.len() - trimmed.len() < 4 {
            return (
                whole(Kind::Muted),
                BlockState::Fence {
                    ch,
                    len: run.min(255) as u8,
                },
            );
        }
    }
    let mut spans = Vec::new();
    let indent = line.len() - trimmed.len();
    push(&mut spans, 0..indent, Kind::Text);
    // Heading.
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&hashes)
        && indent < 4
        && (trimmed.len() == hashes || trimmed.as_bytes()[hashes] == b' ')
    {
        let kind = Kind::Heading(hashes as u8);
        push(&mut spans, indent..indent + hashes, Kind::Muted);
        push(&mut spans, indent + hashes..line.len(), kind);
        return (spans, BlockState::Normal);
    }
    let mut pos = indent;
    let mut base = Kind::Text;
    // Blockquote markers.
    while line[pos..].starts_with('>') {
        let end = pos + 1 + usize::from(line[pos + 1..].starts_with(' '));
        push(&mut spans, pos..end, Kind::Muted);
        pos = end;
        base = Kind::Quote;
    }
    // List marker and task box.
    if let Some(marker_len) = list_marker_len(&line[pos..]) {
        push(&mut spans, pos..pos + marker_len, Kind::ListMarker);
        pos += marker_len;
        let rest = &line[pos..];
        if rest.len() >= 3
            && (rest.starts_with("[ ]") || rest.starts_with("[x]") || rest.starts_with("[X]"))
        {
            push(&mut spans, pos..pos + 3, Kind::Task);
            pos += 3;
        }
    }
    // Table rows: pipes are markup.
    let table = trimmed.starts_with('|');
    inline(line, pos, base, table, &mut spans);
    (spans, BlockState::Normal)
}

/// `- `, `* `, `+ `, `1. `, `1) ` (or the marker alone at the end of the line).
fn list_marker_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let marker = if matches!(b.first(), Some(b'-' | b'*' | b'+')) {
        1
    } else {
        let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || digits > 9 || !matches!(b.get(digits), Some(b'.' | b')')) {
            return None;
        }
        digits + 1
    };
    match b.get(marker) {
        None => Some(marker),
        Some(b' ' | b'\t') => Some(marker + 1),
        _ => None,
    }
}

/// Inline markup from `start` to the end of the line.
fn inline(line: &str, start: usize, base: Kind, table: bool, spans: &mut Vec<Span>) {
    let b = line.as_bytes();
    let mut i = start;
    let mut run = start;
    let flush = |spans: &mut Vec<Span>, run: usize, to: usize| push(spans, run..to, base);
    while i < b.len() {
        let rest = &line[i..];
        let matched: Option<Inline> = if rest.starts_with("[[") {
            rest.find("]]").map(|end| {
                (
                    end + 2,
                    vec![
                        (0, 2, Kind::Muted),
                        (2, end, Kind::WikiLink),
                        (end, end + 2, Kind::Muted),
                    ],
                )
            })
        } else if b[i] == b'`' {
            rest[1..]
                .find('`')
                .map(|end| (end + 2, vec![(0, end + 2, Kind::Code)]))
        } else if rest.starts_with("**") || rest.starts_with("__") {
            let delim = &rest[..2];
            rest[2..].find(delim).filter(|&n| n > 0).map(|n| {
                (
                    n + 4,
                    vec![
                        (0, 2, Kind::Muted),
                        (2, n + 2, Kind::Strong),
                        (n + 2, n + 4, Kind::Muted),
                    ],
                )
            })
        } else if (b[i] == b'*' || b[i] == b'_') && rest.len() > 1 && !rest[1..].starts_with(' ') {
            let delim = &rest[..1];
            rest[1..].find(delim).filter(|&n| n > 0).map(|n| {
                (
                    n + 2,
                    vec![
                        (0, 1, Kind::Muted),
                        (1, n + 1, Kind::Emphasis),
                        (n + 1, n + 2, Kind::Muted),
                    ],
                )
            })
        } else if b[i] == b'[' {
            rest.find("](")
                .and_then(|mid| rest[mid..].find(')').map(|close| (mid, mid + close)))
                .map(|(mid, close)| {
                    (
                        close + 1,
                        vec![
                            (0, 1, Kind::Muted),
                            (1, mid, Kind::LinkText),
                            (mid, mid + 2, Kind::Muted),
                            (mid + 2, close, Kind::Url),
                            (close, close + 1, Kind::Muted),
                        ],
                    )
                })
        } else if table && b[i] == b'|' {
            Some((1, vec![(0, 1, Kind::Muted)]))
        } else {
            None
        };
        match matched {
            Some((len, parts)) => {
                flush(spans, run, i);
                for (s, e, kind) in parts {
                    push(spans, i + s..i + e, kind);
                }
                i += len;
                run = i;
            }
            None => i += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    flush(spans, run, b.len());
}

fn push(spans: &mut Vec<Span>, range: Range<usize>, kind: Kind) {
    if range.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut() {
        if last.kind == kind && last.range.end == range.start {
            last.range.end = range.end;
            return;
        }
    }
    spans.push(Span {
        range,
        kind,
        misspelled: false,
    });
}

/// The link under byte `col` of `line`, if any: a wikilink target
/// (`[[Name]]` or `[[Name|alias]]`) or a Markdown link URL (`[text](url)`).
pub fn link_at(line: &str, col: usize) -> Option<LinkTarget> {
    let mut i = 0;
    while let Some(off) = line[i..].find('[') {
        let s = i + off;
        let rest = &line[s..];
        if rest.starts_with("[[") {
            if let Some(end) = rest.find("]]") {
                if (s..=s + end + 2).contains(&col) {
                    let inner = &rest[2..end];
                    let name = inner.split('|').next().unwrap_or(inner).trim();
                    return Some(LinkTarget::Wiki(name.to_string()));
                }
                i = s + end + 2;
                continue;
            }
        } else if let Some(mid) = rest.find("](") {
            if let Some(close) = rest[mid..].find(')') {
                let end = s + mid + close + 1;
                if (s..=end).contains(&col) {
                    return Some(LinkTarget::Url(rest[mid + 2..mid + close].to_string()));
                }
                i = end;
                continue;
            }
        }
        i = s + 1;
    }
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    Wiki(String),
    Url(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(&str, Kind)> {
        scan_line(line, BlockState::Normal, false)
            .0
            .into_iter()
            .map(|s| (&line[s.range], s.kind))
            .collect()
    }

    fn tiles(line: &str, state: BlockState) {
        let (spans, _) = scan_line(line, state, false);
        let mut at = 0;
        for s in &spans {
            assert_eq!(s.range.start, at, "{line:?}: {spans:?}");
            at = s.range.end;
        }
        assert_eq!(at, line.len(), "{line:?}: {spans:?}");
    }

    fn opts(colors: &EditorColors) -> LayoutOptions<'_> {
        LayoutOptions {
            colors,
            font_size: 14.0,
            wrap: true,
            spell: None,
        }
    }

    #[test]
    fn headings_lists_and_inline() {
        assert_eq!(
            kinds("## Title"),
            [("##", Kind::Muted), (" Title", Kind::Heading(2))]
        );
        assert_eq!(kinds("#nope")[0].1, Kind::Text);
        assert_eq!(
            kinds("  - [x] done"),
            [
                ("  ", Kind::Text),
                ("- ", Kind::ListMarker),
                ("[x]", Kind::Task),
                (" done", Kind::Text)
            ]
        );
        assert!(kinds("a [[Page]] b").contains(&("Page", Kind::WikiLink)));
        assert!(kinds("see [t](u) x").contains(&("u", Kind::Url)));
        assert!(kinds("x **b** *i* `c`").contains(&("i", Kind::Emphasis)));
        assert_eq!(kinds("---")[0].1, Kind::Text);
    }

    #[test]
    fn fences_and_frontmatter_carry_state() {
        let (_, s) = scan_line("```rust", BlockState::Normal, false);
        assert_eq!(s, BlockState::Fence { ch: b'`', len: 3 });
        let (spans, s2) = scan_line("# not a heading", s, false);
        assert_eq!(spans[0].kind, Kind::CodeBlock);
        assert_eq!(scan_line("```", s2, false).1, BlockState::Normal);
        assert_eq!(
            scan_line("---", BlockState::Normal, true).1,
            BlockState::Frontmatter
        );
        assert_eq!(
            scan_line("---", BlockState::Frontmatter, false).1,
            BlockState::Normal
        );
    }

    #[test]
    fn incremental_matches_a_full_rebuild() {
        let ctx = egui::Context::default();
        marina_egui::fonts::install(&ctx);
        let colors = EditorColors::from_palette(marina_egui::theme::ThemeId::Midnight.palette());
        let doc = crate::gen::document(300);
        let edits: [(&str, &str); 6] = [
            ("title: Generated", "title: Edited"),
            ("```rust", "```"),
            ("\n```\n", "\n"),
            ("- ", "-- "),
            ("\n\n", "\n"),
            ("## ", "\n---\n## "),
        ];
        let mut h = Highlighter::default();
        let mut text = doc.clone();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            h.layout(ui, &text, 600.0, &opts(&colors));
            for (from, to) in edits {
                text = text.replacen(from, to, 1);
                let inc = h.layout(ui, &text, 600.0, &opts(&colors));
                let mut fresh = Highlighter::default();
                let full = fresh.layout(ui, &text, 600.0, &opts(&colors));
                let spans = |h: &Highlighter| {
                    h.doc
                        .iter()
                        .map(|e| (e.state, e.scan.0.clone()))
                        .collect::<Vec<_>>()
                };
                assert_eq!(spans(&h), spans(&fresh), "after {from:?} → {to:?}");
                assert_eq!(inc.rows.len(), full.rows.len());
                assert_eq!(inc.text(), text);
            }
        });
        out.textures_delta.clear();
    }

    #[test]
    fn rows_map_to_lines_with_wrapping() {
        let ctx = egui::Context::default();
        marina_egui::fonts::install(&ctx);
        let colors = EditorColors::from_palette(marina_egui::theme::ThemeId::Light.palette());
        let mut h = Highlighter::default();
        let long = "word ".repeat(40);
        let text = format!("one\n{long}\nthree");
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let g = h.layout(ui, &text, 120.0, &opts(&colors));
            let rows = h.line_rows().to_vec();
            assert_eq!(rows.len(), 4);
            assert_eq!(*rows.last().unwrap(), g.rows.len());
            // The long line wraps onto several rows, all mapped to line 1.
            assert!(rows[2] - rows[1] > 1, "{rows:?}");
            for r in rows[1]..rows[2] {
                assert_eq!(h.line_of_row(r), 1);
            }
            assert_eq!(h.line_of_row(rows[2]), 2);
            assert_eq!(h.line_of_row(0), 0);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn spans_tile_every_generated_line() {
        let doc = crate::gen::document(400);
        let mut state = BlockState::Normal;
        for (i, line) in doc.split('\n').enumerate() {
            tiles(line, state);
            state = scan_line(line, state, i == 0).1;
        }
        for odd in [
            "**",
            "*",
            "[a](",
            "`",
            "[[x",
            "> > - [ ]",
            "| a | b",
            "é **ü** `ß`",
        ] {
            tiles(odd, BlockState::Normal);
        }
    }

    struct Rejects(&'static str);
    impl SpellChecker for Rejects {
        fn check(&self, word: &str) -> bool {
            word != self.0
        }
        fn suggest(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
    }

    #[test]
    fn misspelled_words_split_prose_spans_only() {
        let line = "a teh `teh` **teh**";
        let (mut spans, _) = scan_line(line, BlockState::Normal, false);
        flag_misspelled(line, &mut spans, &Rejects("teh"));
        let bad: Vec<_> = spans
            .iter()
            .filter(|s| s.misspelled)
            .map(|s| (s.range.clone(), s.kind))
            .collect();
        assert_eq!(bad, [(2..5, Kind::Text), (14..17, Kind::Strong)]);
        // Still tiles.
        let mut at = 0;
        for s in &spans {
            assert_eq!(s.range.start, at);
            at = s.range.end;
        }
        assert_eq!(at, line.len());
    }

    #[test]
    fn links_under_the_pointer() {
        let line = "see [[Page|alias]] and [docs](https://x.y) end";
        assert_eq!(link_at(line, 7), Some(LinkTarget::Wiki("Page".into())));
        assert_eq!(
            link_at(line, 25),
            Some(LinkTarget::Url("https://x.y".into()))
        );
        assert_eq!(link_at(line, 1), None);
        assert_eq!(link_at(line, 45), None);
    }
}
