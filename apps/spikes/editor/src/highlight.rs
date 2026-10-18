//! Markdown highlighting for `TextEdit`'s layouter.
//!
//! A line-based scanner (not a full CommonMark parse): each line is
//! highlighted from its own text plus the block state it starts in (normal,
//! inside frontmatter, inside a code fence). Per-line results are cached by
//! (line text, entry state), so an edit only rescans the lines it touched,
//! and each line's galley is cached too, so an edit lays out only the lines
//! it touched before the line galleys are joined with `Galley::concat`. The
//! last document galley is reused while the text and width don't change:
//! `TextEdit` calls the layouter at least once per frame, plus once more per
//! edit.

use std::collections::HashMap;
use std::hash::BuildHasher;
use std::ops::Range;
use std::sync::Arc;

use egui::text::{ByteIndex, LayoutJob, LayoutSection, TextWrapping};
use egui::{Color32, FontFamily, FontId, Galley, Stroke, TextFormat, Ui};

/// Body text size.
pub const FONT_SIZE: f32 = 14.0;
/// H1, H2, H3 and deeper headings, relative to body text.
const HEADING_SCALE: [f32; 4] = [1.5, 1.3, 1.15, 1.0];

/// What a run of text is, for styling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}

/// Colours for each [`Kind`], from egui's dark or light visuals.
#[derive(Clone, Copy)]
pub struct Palette {
    text: Color32,
    /// No bold face ships with egui's default fonts, so strong text is brighter instead.
    strong: Color32,
    muted: Color32,
    heading: Color32,
    link: Color32,
    wiki: Color32,
    code: Color32,
    code_bg: Color32,
    marker: Color32,
    quote: Color32,
}

impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                text: Color32::from_gray(220),
                strong: Color32::WHITE,
                muted: Color32::from_gray(120),
                heading: Color32::from_rgb(130, 170, 255),
                link: Color32::from_rgb(100, 180, 255),
                wiki: Color32::from_rgb(190, 140, 255),
                code: Color32::from_rgb(230, 170, 120),
                code_bg: Color32::from_gray(40),
                marker: Color32::from_rgb(240, 190, 80),
                quote: Color32::from_gray(160),
            }
        } else {
            Self {
                text: Color32::from_gray(30),
                strong: Color32::BLACK,
                muted: Color32::from_gray(140),
                heading: Color32::from_rgb(30, 80, 190),
                link: Color32::from_rgb(20, 100, 200),
                wiki: Color32::from_rgb(120, 60, 200),
                code: Color32::from_rgb(160, 70, 20),
                code_bg: Color32::from_gray(235),
                marker: Color32::from_rgb(190, 120, 0),
                quote: Color32::from_gray(90),
            }
        }
    }

    fn format(&self, kind: Kind) -> TextFormat {
        let body = FontId::new(FONT_SIZE, FontFamily::Proportional);
        let mono = FontId::new(FONT_SIZE, FontFamily::Monospace);
        let plain = |font_id: FontId, color: Color32| TextFormat {
            font_id,
            color,
            ..Default::default()
        };
        match kind {
            Kind::Text => plain(body, self.text),
            Kind::Muted => plain(body, self.muted),
            Kind::Heading(level) => {
                let scale = HEADING_SCALE[(level as usize).saturating_sub(1).min(3)];
                plain(
                    FontId::new(FONT_SIZE * scale, FontFamily::Proportional),
                    self.heading,
                )
            }
            Kind::Strong => plain(body, self.strong),
            Kind::Emphasis => TextFormat {
                italics: true,
                ..plain(body, self.text)
            },
            Kind::Code => TextFormat {
                background: self.code_bg,
                ..plain(mono, self.code)
            },
            Kind::CodeBlock => plain(mono, self.code),
            Kind::LinkText => TextFormat {
                underline: Stroke::new(1.0, self.link),
                ..plain(body, self.link)
            },
            Kind::Url => plain(body, self.muted),
            Kind::WikiLink => plain(body, self.wiki),
            Kind::ListMarker | Kind::Task => plain(body, self.marker),
            Kind::Quote => TextFormat {
                italics: true,
                ..plain(body, self.quote)
            },
            Kind::Frontmatter => plain(mono, self.muted),
        }
    }
}

/// Counters for checking the caches work.
#[derive(Clone, Copy, Debug, Default)]
pub struct HighlightStats {
    /// Layouter calls.
    pub calls: u64,
    /// Calls that built a new galley (the text or width changed).
    pub rebuilds: u64,
    /// Lines scanned for highlighting (span cache misses).
    pub lines_scanned: u64,
    /// Lines laid out into a new galley (galley cache misses, [`Strategy::LineGalleys`] only).
    pub lines_laid_out: u64,
    /// Time spent in the layouter, split by phase: (scan, line galleys, concat).
    pub time: [std::time::Duration; 3],
}

/// How the document galley is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Strategy {
    /// One galley per line, joined with `Galley::concat`. An edit diffs the
    /// new text against the last (common prefix and suffix) and redoes only
    /// the lines in between, plus any after them whose block state changed.
    #[default]
    Incremental,
    /// One galley per line, looked up by line hash on every edit (so every
    /// line is hashed per edit), joined with `Galley::concat`.
    LineGalleys,
    /// One `LayoutJob` for the whole document, relying on epaint's own
    /// per-paragraph cache. That cache drops every entry not used in the
    /// last frame, so the first edit after an idle frame lays out the
    /// whole document again (kept to measure exactly that).
    WholeJob,
}

/// The cached layouter. Keep one per editor.
#[derive(Default)]
pub struct Highlighter {
    pub strategy: Strategy,
    /// (line hash, entry state, is first line) → spans and exit state.
    lines: HashMap<LineKey, Arc<Scanned>>,
    /// (line key, look) → that line's galley.
    galleys: HashMap<(LineKey, Look), Arc<Galley>>,
    /// [`Strategy::Incremental`]: one entry per line of `last.text`.
    doc: Vec<LineEntry>,
    last: Option<Last>,
    pub stats: HighlightStats,
}

type LineKey = (u64, BlockState, bool);
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
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Look {
    /// Wrap width, rounded to whole points as epaint does.
    wrap: u32,
    dark: bool,
    plain: bool,
}

struct Last {
    text: String,
    look: Look,
    strategy: Strategy,
    galley: Arc<Galley>,
}

impl Highlighter {
    /// The layouter body: `plain` skips highlighting (for comparison).
    pub fn layout(&mut self, ui: &Ui, text: &str, wrap_width: f32, plain: bool) -> Arc<Galley> {
        self.stats.calls += 1;
        let look = Look {
            wrap: wrap_width.round().max(0.0) as u32,
            dark: ui.visuals().dark_mode,
            plain,
        };
        if let Some(last) = &self.last {
            if last.look == look && last.strategy == self.strategy && last.text == text {
                return last.galley.clone();
            }
        }
        self.stats.rebuilds += 1;
        let palette = Palette::new(look.dark);
        let galley = match self.strategy {
            Strategy::Incremental => self.incremental(ui, text, &palette, look),
            Strategy::LineGalleys => self.line_galleys(ui, text, &palette, look),
            Strategy::WholeJob => {
                let job = if plain {
                    plain_job(text, &palette, wrap_width)
                } else {
                    self.whole_job(text, &palette, wrap_width)
                };
                ui.fonts_mut(|f| f.layout_job(job))
            }
        };
        self.last = Some(Last {
            text: text.to_owned(),
            look,
            strategy: self.strategy,
            galley: galley.clone(),
        });
        galley
    }

    fn incremental(&mut self, ui: &Ui, text: &str, palette: &Palette, look: Look) -> Arc<Galley> {
        let t0 = std::time::Instant::now();
        let old = std::mem::take(&mut self.doc);
        let reusable = self
            .last
            .as_ref()
            .filter(|l| l.look == look && l.strategy == Strategy::Incremental && !old.is_empty());
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
            let scan = if look.plain {
                Arc::new((
                    if line.is_empty() {
                        vec![]
                    } else {
                        vec![Span {
                            range: 0..line.len(),
                            kind: Kind::Text,
                        }]
                    },
                    BlockState::Normal,
                ))
            } else {
                self.stats.lines_scanned += 1;
                Arc::new(scan_line(line, state, i == 0))
            };
            self.stats.lines_laid_out += 1;
            let galley = line_galley(ui, line, &scan.0, palette, look);
            doc.push(LineEntry {
                state,
                scan: scan.clone(),
                galley,
            });
            state = scan.1;
        }
        let t1 = std::time::Instant::now();
        self.stats.time[0] += t1 - t0;
        let galleys: Vec<Arc<Galley>> = doc.iter().map(|e| e.galley.clone()).collect();
        self.doc = doc;
        let job = merged_job(text, palette, look);
        let galley = Arc::new(Galley::concat(
            Arc::new(job),
            &galleys,
            ui.ctx().pixels_per_point(),
        ));
        self.stats.time[2] += t1.elapsed();
        galley
    }

    /// Spans and exit state of every line, through the span cache.
    fn scan(&mut self, text: &str, plain: bool) -> Vec<(LineKey, Arc<Scanned>)> {
        let mut fresh = HashMap::with_capacity(self.lines.len());
        let mut out = Vec::with_capacity(self.lines.len());
        let mut state = BlockState::Normal;
        for (i, line) in text.split('\n').enumerate() {
            let key = (line_hash(line), state, i == 0);
            let entry = match self.lines.get(&key) {
                Some(hit) => hit.clone(),
                None if plain => Arc::new((
                    if line.is_empty() {
                        vec![]
                    } else {
                        vec![Span {
                            range: 0..line.len(),
                            kind: Kind::Text,
                        }]
                    },
                    BlockState::Normal,
                )),
                None => {
                    self.stats.lines_scanned += 1;
                    Arc::new(scan_line(line, state, i == 0))
                }
            };
            state = entry.1;
            fresh.insert(key, entry.clone());
            out.push((key, entry));
        }
        self.lines = fresh;
        out
    }

    fn line_galleys(&mut self, ui: &Ui, text: &str, palette: &Palette, look: Look) -> Arc<Galley> {
        if look.plain != self.last.as_ref().is_some_and(|l| l.look.plain) {
            self.lines.clear();
        }
        let t0 = std::time::Instant::now();
        let lines = self.scan(text, look.plain);
        let t1 = std::time::Instant::now();
        self.stats.time[0] += t1 - t0;
        let mut fresh = HashMap::with_capacity(lines.len());
        let mut galleys = Vec::with_capacity(lines.len());
        for ((key, entry), line) in lines.iter().zip(text.split('\n')) {
            let galley = match self.galleys.get(&(*key, look)) {
                Some(hit) => hit.clone(),
                None => {
                    self.stats.lines_laid_out += 1;
                    line_galley(ui, line, &entry.0, palette, look)
                }
            };
            fresh.insert((*key, look), galley.clone());
            galleys.push(galley);
        }
        self.galleys = fresh;
        let t2 = std::time::Instant::now();
        self.stats.time[1] += t2 - t1;
        let job = merged_job(text, palette, look);
        let galley = Arc::new(Galley::concat(
            Arc::new(job),
            &galleys,
            ui.ctx().pixels_per_point(),
        ));
        self.stats.time[2] += t2.elapsed();
        galley
    }

    fn whole_job(&mut self, text: &str, palette: &Palette, wrap_width: f32) -> LayoutJob {
        let mut job = LayoutJob {
            text: text.to_owned(),
            wrap: TextWrapping::wrap_at_width(wrap_width),
            keep_trailing_whitespace: true,
            ..Default::default()
        };
        let mut offset = 0;
        for ((_, entry), line) in self.scan(text, false).iter().zip(text.split('\n')) {
            // Every span covers the line exactly, so sections tile the text.
            for span in &entry.0 {
                push_section(
                    &mut job.sections,
                    offset + span.range.start..offset + span.range.end,
                    palette.format(span.kind),
                );
            }
            // The newline gets a plain section of its own.
            let end = offset + line.len();
            if end < text.len() {
                push_section(&mut job.sections, end..end + 1, palette.format(Kind::Text));
            }
            offset = end + 1;
        }
        job
    }
}

/// The job behind a `Galley::concat` of line galleys. The rows already carry
/// the formatting, but `Galley::is_empty` checks for sections, and the
/// painter skips an "empty" galley, so give it one covering the text.
fn merged_job(text: &str, palette: &Palette, look: Look) -> LayoutJob {
    let mut job = LayoutJob {
        text: text.to_owned(),
        wrap: TextWrapping::wrap_at_width(look.wrap as f32),
        keep_trailing_whitespace: true,
        ..Default::default()
    };
    push_section(&mut job.sections, 0..text.len(), palette.format(Kind::Text));
    job
}

fn line_galley(ui: &Ui, line: &str, spans: &[Span], palette: &Palette, look: Look) -> Arc<Galley> {
    let mut job = LayoutJob {
        text: line.to_owned(),
        wrap: TextWrapping::wrap_at_width(look.wrap as f32),
        keep_trailing_whitespace: true,
        ..Default::default()
    };
    for span in spans {
        push_section(
            &mut job.sections,
            span.range.clone(),
            palette.format(span.kind),
        );
    }
    if line.is_empty() {
        // An empty line still needs a font for its row height.
        push_section(&mut job.sections, 0..0, palette.format(Kind::Text));
    }
    ui.fonts_mut(|f| f.layout_job(job))
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

fn plain_job(text: &str, palette: &Palette, wrap_width: f32) -> LayoutJob {
    let mut job = LayoutJob::simple(
        text.to_owned(),
        FontId::new(FONT_SIZE, FontFamily::Proportional),
        palette.text,
        wrap_width,
    );
    job.keep_trailing_whitespace = true;
    job
}

fn push_section(sections: &mut Vec<LayoutSection>, range: Range<usize>, format: TextFormat) {
    sections.push(LayoutSection {
        leading_space: 0.0,
        byte_range: ByteIndex(range.start)..ByteIndex(range.end),
        format,
    });
}

fn line_hash(line: &str) -> u64 {
    // Fixed keys: the cache only lives in this process.
    std::hash::BuildHasherDefault::<std::collections::hash_map::DefaultHasher>::default()
        .hash_one(line)
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
    spans.push(Span { range, kind });
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
            h.layout(ui, &text, 600.0, false);
            for (from, to) in edits {
                text = text.replacen(from, to, 1);
                let inc = h.layout(ui, &text, 600.0, false);
                let full = Highlighter::default().layout(ui, &text, 600.0, false);
                let spans = |h: &Highlighter| {
                    h.doc
                        .iter()
                        .map(|e| (e.state, e.scan.0.clone()))
                        .collect::<Vec<_>>()
                };
                let mut fresh = Highlighter::default();
                fresh.layout(ui, &text, 600.0, false);
                assert_eq!(spans(&h), spans(&fresh), "after {from:?} → {to:?}");
                assert_eq!(inc.rows.len(), full.rows.len());
                assert_eq!(inc.text(), text);
            }
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
}
