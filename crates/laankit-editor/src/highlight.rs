//! Markdown highlighting with a per-line galley cache, for both engines
//! (the core editor lays out only the lines on screen; `TextEdit`'s layouter
//! joins them all) (Phase 0 spike 1: see `docs/plans/spikes/editor.md`).
//!
//! A line-based scanner (not a full CommonMark parse): each line is
//! highlighted from its own text plus the block state it starts in (normal,
//! inside frontmatter, inside a code fence). The layouter keeps one galley per
//! line. An edit diffs the new text against the previous one (common prefix
//! and suffix), rescans and lays out only the lines in between, plus any
//! following lines whose block state changed (for example after opening a
//! fence), and joins the line galleys with `Galley::concat`.
//!
//! Fenced code is highlighted by language through [`crate::code`]. Each line
//! remembers the fence's parse state it starts in ([`LineStart`]), so an edit
//! in a fence re-highlights only until the state settles, and an edit outside
//! one leaves it alone. Until the syntaxes have loaded, fences are one
//! colour; once they arrive, the fence lines alone are highlighted again.
//!
//! The per-line cache is what makes this usable: egui's own paragraph cache
//! drops every entry not used in the previous frame, so a single whole-text
//! `LayoutJob` re-lays out the whole note on the first keystroke after any
//! pause (127 ms at 5,000 lines).

use std::ops::Range;
use std::sync::Arc;

use egui::text::{ByteIndex, LayoutJob, LayoutSection, TextWrapping};
use egui::{Color32, FontFamily, FontId, Galley, Rect, Stroke, TextFormat, Ui};
use laankit_egui::fonts::Style;
use laankit_egui::theme::Palette;
use syntect::parsing::SyntaxSet;

use crate::code::{self, FenceCode, Token};
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
    /// Code in a fence whose language is known, by token class.
    Syntax(Token),
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

/// Everything a line's highlighting depends on besides its own text: the
/// block it starts in and, inside a fence with a known language, the parse
/// state the fence has reached.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineStart {
    pub block: BlockState,
    pub code: Option<Arc<FenceCode>>,
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
/// | selection matches | `--accent` at 15%                                             |
/// | bracket match | `--accent` at 30%; an unmatched bracket in red at 30%             |
///
/// The code colour is the one colour not in the palette: CodeMirror's string
/// green, One Dark's `#98c379` on dark themes and the default style's
/// `#116644` on light ones, as the Svelte editor shows code. Fences in a
/// language [`crate::code`] knows are coloured by token instead
/// ([`SyntaxColors`]).
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
    pub selection_match: Color32,
    pub bracket_match: Color32,
    pub bracket_unmatched: Color32,
    pub misspelled: Color32,
    pub syntax: SyntaxColors,
}

/// Colours for highlighted code, by [`Token`]. Plain code and punctuation
/// take the theme's text colours; the rest follow what the Svelte editor
/// showed: CodeMirror's One Dark (`oneDark`) on dark themes and its
/// `defaultHighlightStyle` on light ones.
///
/// | Token       | Dark (One Dark) | Light (CodeMirror default) |
/// |-------------|-----------------|----------------------------|
/// | plain       | `--text-primary`  | `--text-primary`           |
/// | punctuation | `--text-secondary`| `--text-secondary`         |
/// | comment     | `#7d8799`       | `#940` (italic in both)    |
/// | string      | `#98c379`       | `#a11`                     |
/// | number      | `#d19a66`       | `#164`                     |
/// | constant    | `#d19a66`       | `#219`                     |
/// | keyword     | `#c678dd`       | `#708`                     |
/// | operator    | `#56b6c2`       | `#708`                     |
/// | function    | `#61afef`       | `#00f`                     |
/// | type        | `#e5c07b`       | `#085`                     |
/// | tag         | `#e06c75`       | `#170`                     |
/// | attribute   | `#d19a66`       | `#00c`                     |
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyntaxColors {
    pub plain: Color32,
    pub punctuation: Color32,
    pub comment: Color32,
    pub string: Color32,
    pub number: Color32,
    pub constant: Color32,
    pub keyword: Color32,
    pub operator: Color32,
    pub function: Color32,
    pub ty: Color32,
    pub tag: Color32,
    pub attribute: Color32,
}

impl SyntaxColors {
    pub fn from_palette(p: &Palette) -> Self {
        let hex = |rgb: u32| {
            let [_, r, g, b] = rgb.to_be_bytes();
            Color32::from_rgb(r, g, b)
        };
        if p.dark {
            Self {
                plain: p.text_primary,
                punctuation: p.text_secondary,
                comment: hex(0x7d8799),
                string: hex(0x98c379),
                number: hex(0xd19a66),
                constant: hex(0xd19a66),
                keyword: hex(0xc678dd),
                operator: hex(0x56b6c2),
                function: hex(0x61afef),
                ty: hex(0xe5c07b),
                tag: hex(0xe06c75),
                attribute: hex(0xd19a66),
            }
        } else {
            Self {
                plain: p.text_primary,
                punctuation: p.text_secondary,
                comment: hex(0x994400),
                string: hex(0xaa1111),
                number: hex(0x116644),
                constant: hex(0x221199),
                keyword: hex(0x770088),
                operator: hex(0x770088),
                function: hex(0x0000ff),
                ty: hex(0x008855),
                tag: hex(0x117700),
                attribute: hex(0x0000cc),
            }
        }
    }

    pub fn color(&self, token: Token) -> Color32 {
        match token {
            Token::Plain => self.plain,
            Token::Punctuation => self.punctuation,
            Token::Comment => self.comment,
            Token::String => self.string,
            Token::Number => self.number,
            Token::Constant => self.constant,
            Token::Keyword => self.keyword,
            Token::Operator => self.operator,
            Token::Function => self.function,
            Token::Type => self.ty,
            Token::Tag => self.tag,
            Token::Attribute => self.attribute,
        }
    }

    fn all(&self) -> [Color32; 12] {
        [
            self.plain,
            self.punctuation,
            self.comment,
            self.string,
            self.number,
            self.constant,
            self.keyword,
            self.operator,
            self.function,
            self.ty,
            self.tag,
            self.attribute,
        ]
    }
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
            selection_match: p.accent.gamma_multiply(0.15),
            bracket_match: p.accent.gamma_multiply(0.3),
            bracket_unmatched: Color32::from_rgb(0xe0, 0x4b, 0x4b).gamma_multiply(0.3),
            misspelled: Color32::from_rgb(0xe0, 0x4b, 0x4b),
            syntax: SyntaxColors::from_palette(p),
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
        ]
        .into_iter()
        .chain(self.syntax.all())
        {
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
            Kind::Syntax(token) => TextFormat {
                background: self.pre_bg,
                italics: token == Token::Comment,
                ..plain(mono, self.syntax.color(token))
            },
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
    /// Lines scanned (highlighted) again.
    pub lines_laid_out: u64,
    /// Line galleys built (laid out with fonts).
    pub galleys_built: u64,
}

/// The cached highlighter and layouter. One per editor.
///
/// Scanning (which spans a line has) and layout (its galley) are separate:
/// every line is scanned, because a line's highlighting depends on the block
/// it starts in, but galleys are built only when asked for. The `TextEdit`
/// engine asks for all of them ([`Self::layout`]); the core engine only for
/// the lines on screen ([`Self::line_galley`]), and keeps their measured
/// heights.
#[derive(Default)]
pub struct Highlighter {
    /// One entry per line of `text`.
    doc: Vec<LineEntry>,
    /// The first row of each line in the last document galley, plus the row count at the end.
    line_rows: Vec<usize>,
    /// The text the lines were scanned from, and the host's version of it.
    text: Option<String>,
    text_version: Option<u64>,
    /// The look the line galleys and measured heights were laid out with.
    galley_look: Option<Look>,
    /// The last document galley (`TextEdit` engine), until a line or the look changes.
    doc_galley: Option<Arc<Galley>>,
    /// Whether the last scan had the code syntaxes (fences are highlighted
    /// again, once, when they arrive).
    code_ready: bool,
    /// Bumped whenever lines change or a line's measured height is set or cleared.
    version: u64,
    /// Line galleys currently held.
    galleys: usize,
    pub stats: HighlightStats,
}

/// A line's spans and what the next line starts in.
type Scanned = (Vec<Span>, LineStart);
/// A matched inline construct: its byte length and its parts as (start, end, kind).
type Inline = (usize, Vec<(usize, usize, Kind)>);

#[derive(Clone)]
struct LineEntry {
    /// What the line starts in.
    start: LineStart,
    scan: Arc<Scanned>,
    /// The line's length in bytes (without the newline).
    len: usize,
    galley: Option<Arc<Galley>>,
    /// Size of the galley last built for this line (kept when the galley is evicted).
    size: Option<egui::Vec2>,
}

impl LineEntry {
    /// Whether the line is in a fence or opens one.
    fn in_fence(&self) -> bool {
        matches!(self.start.block, BlockState::Fence { .. })
            || matches!(self.scan.1.block, BlockState::Fence { .. })
    }
}

/// Everything besides the text that changes a line's galley.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    /// Wrap width, rounded to whole points as epaint does (`u32::MAX`: no wrapping).
    pub wrap: u32,
    pub font_size_bits: u32,
    pub colors: u64,
    /// Changes whenever the spell checker (or its on/off state) does.
    pub spell: u64,
}

impl Look {
    pub fn new(opts: &LayoutOptions<'_>, wrap_width: f32) -> Self {
        Self {
            wrap: if opts.wrap {
                wrap_width.round().max(0.0) as u32
            } else {
                u32::MAX
            },
            font_size_bits: opts.font_size.to_bits(),
            colors: opts.colors.key(),
            spell: opts.spell.map_or(0, |(_, v)| v.wrapping_add(1)),
        }
    }
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
    /// The `TextEdit` layouter body: the whole note as one galley.
    pub fn layout(
        &mut self,
        ui: &Ui,
        text: &str,
        wrap_width: f32,
        opts: &LayoutOptions<'_>,
    ) -> Arc<Galley> {
        self.stats.calls += 1;
        let look = Look::new(opts, wrap_width);
        let changed = self.update(ui.ctx(), text, None);
        if !changed && self.galley_look == Some(look) {
            if let Some(g) = &self.doc_galley {
                return g.clone();
            }
        }
        self.stats.rebuilds += 1;
        self.set_look(look);
        for (i, line) in text.split('\n').enumerate() {
            self.ensure(ui, i, line, opts, look);
        }
        self.line_rows.clear();
        let mut rows = 0;
        for e in &self.doc {
            self.line_rows.push(rows);
            rows += e.galley.as_ref().map_or(1, |g| g.rows.len());
        }
        self.line_rows.push(rows);
        let galley = self.galley_from_doc(ui, text, opts, look);
        self.doc_galley = Some(galley.clone());
        galley
    }

    /// Rescans the lines that changed since the last call (and the fences,
    /// once the code syntaxes arrive). With the host's `version` of the text,
    /// an unchanged version skips comparing the text. True if anything changed.
    pub fn update(&mut self, ctx: &egui::Context, text: &str, version: Option<u64>) -> bool {
        self.update_hinted(ctx, text, version, None)
    }

    /// [`Self::update`], told which lines changed: `(first line, its byte
    /// start, last changed old line, last changed new line)`, as the core
    /// engine's rope sync knows them (so the text isn't diffed again).
    pub fn update_hinted(
        &mut self,
        ctx: &egui::Context,
        text: &str,
        version: Option<u64>,
        hint: Option<(usize, usize, usize, usize)>,
    ) -> bool {
        let syntaxes = code::syntaxes(Some(ctx));
        let refresh = syntaxes.is_some() && !self.code_ready && self.text.is_some();
        let same = match (version, self.text_version) {
            (Some(v), Some(w)) => v == w,
            _ => self.text.as_deref() == Some(text),
        };
        if same && !refresh {
            return false;
        }
        if !same {
            self.incremental(text, syntaxes, hint);
            // Reuse the allocation: a new note-sized string per keystroke
            // shows up at tens of thousands of lines.
            let t = self.text.get_or_insert_with(String::new);
            t.clear();
            t.push_str(text);
        }
        if refresh {
            self.refresh_fences(text, syntaxes);
        }
        self.text_version = version;
        self.code_ready = syntaxes.is_some();
        self.doc_galley = None;
        self.version += 1;
        true
    }

    /// The galley of 0-based `line` (whose text is `text`), laid out with
    /// `look` (a different look than last time drops every galley and height).
    pub fn line_galley(
        &mut self,
        ui: &Ui,
        line: usize,
        text: &str,
        opts: &LayoutOptions<'_>,
        look: Look,
    ) -> Option<Arc<Galley>> {
        if line >= self.doc.len() {
            return None;
        }
        self.set_look(look);
        Some(self.ensure(ui, line, text, opts, look))
    }

    /// `line`'s galley if it's currently held.
    pub fn cached_galley(&self, line: usize) -> Option<Arc<Galley>> {
        self.doc.get(line)?.galley.clone()
    }

    /// The size of `line`'s galley, if it has been laid out with the current look.
    pub fn measured_size(&self, line: usize) -> Option<egui::Vec2> {
        self.doc.get(line)?.size
    }

    /// The length of `line` in bytes.
    pub fn line_len(&self, line: usize) -> usize {
        self.doc.get(line).map_or(0, |e| e.len)
    }

    /// The blocks `line` starts and ends in (code fences, frontmatter).
    pub fn line_blocks(&self, line: usize) -> Option<(BlockState, BlockState)> {
        self.doc.get(line).map(|e| (e.start.block, e.scan.1.block))
    }

    /// Bumped whenever lines change or a measured height is set or cleared.
    pub fn layout_version(&self) -> u64 {
        self.version
    }

    /// Line galleys currently held.
    pub fn galley_count(&self) -> usize {
        self.galleys
    }

    /// Drops the galleys of lines outside `keep` (their measured sizes stay).
    pub fn evict_galleys(&mut self, keep: Range<usize>) {
        for (i, e) in self.doc.iter_mut().enumerate() {
            if !keep.contains(&i) && e.galley.take().is_some() {
                self.galleys -= 1;
            }
        }
        self.doc_galley = None;
    }

    /// The galley of the last layout, if the text hasn't changed since.
    pub fn galley_for(&self, text: &str) -> Option<Arc<Galley>> {
        self.doc_galley
            .as_ref()
            .filter(|_| self.text.as_deref() == Some(text))
            .cloned()
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

    /// The rectangle of characters `cols` (in characters) of 0-based line
    /// `line`, relative to the document galley `galley` (the last layout).
    /// Uses the line's own galley, so it doesn't walk the document's rows.
    /// A range that wraps onto another row is cut at the end of its first row.
    pub fn char_rect(&self, galley: &Galley, line: usize, cols: Range<usize>) -> Option<Rect> {
        let entry = self.doc.get(line)?;
        let first_row = *self.line_rows.get(line)?;
        let top = galley.rows.get(first_row)?.pos.y;
        let g = entry.galley.as_ref()?;
        let r = cols_rect(g, cols)?;
        Some(r.translate(egui::vec2(0.0, top)))
    }

    /// Drops every galley and height if `look` differs from theirs.
    pub fn set_look(&mut self, look: Look) {
        if self.galley_look == Some(look) {
            return;
        }
        for e in &mut self.doc {
            e.galley = None;
            e.size = None;
        }
        self.galleys = 0;
        self.galley_look = Some(look);
        self.doc_galley = None;
        self.version += 1;
    }

    /// The galley of `line`, built if missing.
    fn ensure(
        &mut self,
        ui: &Ui,
        line: usize,
        text: &str,
        opts: &LayoutOptions<'_>,
        look: Look,
    ) -> Arc<Galley> {
        let e = &mut self.doc[line];
        if let Some(g) = &e.galley {
            return g.clone();
        }
        let mut spans = e.scan.0.clone();
        if let Some((checker, _)) = opts.spell {
            flag_misspelled(text, &mut spans, checker);
        }
        let g = line_galley(ui, text, &spans, opts, look);
        if e.size != Some(g.size()) {
            self.version += 1;
        }
        e.size = Some(g.size());
        e.galley = Some(g.clone());
        self.galleys += 1;
        self.stats.galleys_built += 1;
        g
    }

    fn incremental(
        &mut self,
        text: &str,
        syntaxes: Option<&SyntaxSet>,
        hint: Option<(usize, usize, usize, usize)>,
    ) {
        let old_len = self.doc.len();
        // Lines [0, first) are unchanged; new lines (new_last, ..) match old lines (old_last, ..).
        let (first, start, old_last, new_last) = match self.text.as_deref() {
            Some(last) if old_len > 0 => hint.unwrap_or_else(|| changed_lines(last, text, old_len)),
            _ => (0, 0, old_len, usize::MAX),
        };
        let first = first.min(old_len);
        let mut state = match first {
            0 => LineStart::default(),
            f => self.doc[f - 1].scan.1.clone(),
        };
        let mut fresh = Vec::new();
        let mut j = old_last.wrapping_add(1);
        let mut reuse_from = old_len;
        for (k, line) in text[start..].split('\n').enumerate() {
            let i = first + k;
            if i > new_last {
                // Past the edit: the rest is reusable once a line starts in the same state.
                if j < old_len && self.doc[j].start == state {
                    reuse_from = j;
                    break;
                }
                j += 1;
            }
            let entry = self.scan_entry(line, state, i == 0, syntaxes);
            state = entry.scan.1.clone();
            fresh.push(entry);
        }
        // One splice: the unchanged lines on both sides move at most once,
        // and aren't cloned.
        let end = reuse_from.max(first);
        let dropped = self.doc[first..end]
            .iter()
            .filter(|e| e.galley.is_some())
            .count();
        self.galleys -= dropped;
        self.doc.splice(first..end, fresh);
    }

    /// Scans one line (its galley is built later, when asked for).
    fn scan_entry(
        &mut self,
        line: &str,
        start: LineStart,
        first_line: bool,
        syntaxes: Option<&SyntaxSet>,
    ) -> LineEntry {
        let scan = scan_line_with_code(line, &start, first_line, syntaxes);
        self.stats.lines_laid_out += 1;
        LineEntry {
            start,
            scan: Arc::new(scan),
            len: line.len(),
            galley: None,
            size: None,
        }
    }

    /// The syntaxes just arrived: highlights the fence lines of the current
    /// document again and keeps every other line as it is.
    fn refresh_fences(&mut self, text: &str, syntaxes: Option<&SyntaxSet>) {
        let old = std::mem::take(&mut self.doc);
        let mut doc = Vec::with_capacity(old.len());
        let mut state = LineStart::default();
        for (i, (line, entry)) in text.split('\n').zip(old).enumerate() {
            if !entry.in_fence() && entry.start == state {
                state = entry.scan.1.clone();
                doc.push(entry);
                continue;
            }
            let entry = self.scan_entry(line, state, i == 0, syntaxes);
            state = entry.scan.1.clone();
            doc.push(entry);
        }
        self.set_doc(doc);
    }

    /// Keeps the lines (and counts their galleys).
    fn set_doc(&mut self, doc: Vec<LineEntry>) {
        self.galleys = doc.iter().filter(|e| e.galley.is_some()).count();
        self.doc = doc;
    }

    /// Joins the line galleys into the document galley.
    fn galley_from_doc(
        &self,
        ui: &Ui,
        text: &str,
        opts: &LayoutOptions<'_>,
        look: Look,
    ) -> Arc<Galley> {
        let galleys: Vec<Arc<Galley>> = self.doc.iter().filter_map(|e| e.galley.clone()).collect();
        let job = merged_job(text, opts, look);
        Arc::new(Galley::concat(
            Arc::new(job),
            &galleys,
            ui.ctx().pixels_per_point(),
        ))
    }
}

/// The rectangle of characters `cols` of one line's galley. A range that
/// wraps onto another row is cut at the end of its first row.
pub fn cols_rect(g: &Galley, cols: Range<usize>) -> Option<Rect> {
    let a = g.pos_from_cursor(egui::text::CCursor::new(cols.start));
    let b = g.pos_from_cursor(egui::text::CCursor::new(cols.end));
    let right = if (a.top() - b.top()).abs() < 1.0 {
        b.left()
    } else {
        g.rows
            .get(
                g.layout_from_cursor(egui::text::CCursor::new(cols.start))
                    .row,
            )
            .map_or(a.left() + 6.0, |r| r.rect().right())
    };
    Some(Rect::from_min_max(
        a.min,
        egui::pos2(right.max(a.left() + 2.0), a.bottom()),
    ))
}

/// The job behind a `Galley::concat` of line galleys. The rows already carry
/// the formatting, but `Galley::is_empty` checks for sections, and the
/// painter skips an "empty" galley, so give it one covering the text.
pub(crate) fn merged_job(text: &str, opts: &LayoutOptions<'_>, look: Look) -> LayoutJob {
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
    let newlines = crate::fastcmp::newlines;
    let p = crate::fastcmp::common_prefix(o, n);
    let max_s = o.len().min(n.len()) - p;
    let s = crate::fastcmp::common_suffix(o, n, max_s);
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

/// [`scan_line`] plus code: a fence opener looks up its language, and lines
/// inside a fence with a known language are highlighted by token (when the
/// syntaxes have loaded; otherwise fences stay one colour).
pub fn scan_line_with_code(
    line: &str,
    start: &LineStart,
    first_line: bool,
    syntaxes: Option<&SyntaxSet>,
) -> Scanned {
    let (spans, block) = scan_line(line, start.block, first_line);
    let code = match (start.block, block) {
        // A fence opens: start its language, if known.
        (BlockState::Normal, BlockState::Fence { ch, .. }) => syntaxes.and_then(|set| {
            let info = line.trim_start().trim_start_matches(ch as char);
            let syntax = code::find_syntax(set, info)?;
            FenceCode::new(set, syntax).map(Arc::new)
        }),
        // Inside a fence (not its closing line): highlight by token.
        (BlockState::Fence { .. }, BlockState::Fence { .. }) => {
            match (start.code.as_ref(), syntaxes) {
                (Some(code), Some(set)) => match code.line(set, line) {
                    Some((runs, next)) => {
                        let spans = runs
                            .into_iter()
                            .map(|(range, token)| Span {
                                range,
                                kind: Kind::Syntax(token),
                                misspelled: false,
                            })
                            .collect();
                        return (
                            spans,
                            LineStart {
                                block,
                                code: Some(Arc::new(next)),
                            },
                        );
                    }
                    // syntect gave up: the rest of the fence is plain.
                    None => None,
                },
                _ => None,
            }
        }
        _ => None,
    };
    (spans, LineStart { block, code })
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
        crate::code::load_blocking();
        let ctx = egui::Context::default();
        laankit_egui::fonts::install(&ctx);
        let colors = EditorColors::from_palette(laankit_egui::theme::ThemeId::Midnight.palette());
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
                        .map(|e| (e.start.clone(), e.scan.0.clone()))
                        .collect::<Vec<_>>()
                };
                assert_eq!(spans(&h), spans(&fresh), "after {from:?} → {to:?}");
                assert_eq!(inc.rows.len(), full.rows.len());
                assert_eq!(inc.text(), text);
            }
        });
        out.textures_delta.clear();
    }

    /// A note with prose around two highlighted fences.
    fn fenced_doc() -> String {
        let mut lines = vec!["# Title".to_string(), "Some text.".into(), "```rust".into()];
        for i in 0..20 {
            lines.push(format!("fn f{i}() -> u8 {{ {i} }} // c"));
        }
        lines.push("```".into());
        lines.push("More text.".into());
        lines.push("```python".into());
        for i in 0..10 {
            lines.push(format!("def g{i}(): return '{i}'"));
        }
        lines.push("```".into());
        lines.push("End.".into());
        lines.join("\n")
    }

    #[test]
    fn fences_are_highlighted_and_edits_stay_local() {
        crate::code::load_blocking();
        let ctx = egui::Context::default();
        laankit_egui::fonts::install(&ctx);
        let colors = EditorColors::from_palette(laankit_egui::theme::ThemeId::Dark.palette());
        let mut h = Highlighter::default();
        let text = fenced_doc();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            h.layout(ui, &text, 600.0, &opts(&colors));
            // Line 3 is `fn f0() -> u8 { 0 } // c`.
            let spans = h.line_spans(3).unwrap();
            let kind_of = |needle: &str| {
                let line = text.split('\n').nth(3).unwrap();
                let at = line.find(needle).unwrap();
                spans.iter().find(|s| s.range.contains(&at)).unwrap().kind
            };
            assert_eq!(kind_of("fn"), Kind::Syntax(Token::Keyword));
            assert_eq!(kind_of("// c"), Kind::Syntax(Token::Comment));
            assert_eq!(kind_of("0 }"), Kind::Syntax(Token::Number));

            // Editing prose doesn't touch either fence.
            let before = h.stats.lines_laid_out;
            let edited = text.replacen("More text.", "More text, edited.", 1);
            h.layout(ui, &edited, 600.0, &opts(&colors));
            assert_eq!(h.stats.lines_laid_out - before, 1);

            // Editing inside the Rust fence re-highlights only that line (the
            // parse state after it is unchanged).
            let before = h.stats.lines_laid_out;
            let edited2 = edited.replacen("fn f5()", "fn f5x()", 1);
            h.layout(ui, &edited2, 600.0, &opts(&colors));
            assert_eq!(h.stats.lines_laid_out - before, 1);

            // Opening a block comment changes the state: the rest of the fence
            // is highlighted again (as a comment), and the Python fence isn't.
            let before = h.stats.lines_laid_out;
            let edited3 = edited2.replacen("fn f10()", "/* fn f10()", 1);
            h.layout(ui, &edited3, 600.0, &opts(&colors));
            let relaid = h.stats.lines_laid_out - before;
            assert!((10..=14).contains(&relaid), "{relaid}");
            let mut fresh = Highlighter::default();
            fresh.layout(ui, &edited3, 600.0, &opts(&colors));
            for i in 0..h.line_count() {
                assert_eq!(h.line_spans(i), fresh.line_spans(i), "line {i}");
            }
        });
        out.textures_delta.clear();
    }

    #[test]
    fn syntaxes_arriving_re_highlight_only_fences() {
        crate::code::load_blocking();
        let ctx = egui::Context::default();
        laankit_egui::fonts::install(&ctx);
        let colors = EditorColors::from_palette(laankit_egui::theme::ThemeId::Light.palette());
        let text = fenced_doc();
        let mut h = Highlighter::default();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            h.layout(ui, &text, 600.0, &opts(&colors));
            // As if the last layout had run before the syntaxes loaded.
            h.code_ready = false;
            let before = h.stats.lines_laid_out;
            h.layout(ui, &text, 600.0, &opts(&colors));
            let fence_lines = 2 + 20 + 2 + 10;
            assert_eq!(h.stats.lines_laid_out - before, fence_lines);
            assert!(h.code_ready);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn unknown_languages_stay_one_colour() {
        let set = crate::code::load_blocking();
        let start = LineStart::default();
        let (_, opened) = scan_line_with_code("```klingon", &start, false, Some(set));
        assert_eq!(opened.block, BlockState::Fence { ch: b'`', len: 3 });
        assert!(opened.code.is_none());
        let (spans, _) = scan_line_with_code("qapla'", &opened, false, Some(set));
        assert_eq!(spans[0].kind, Kind::CodeBlock);
        // Before the syntaxes load, a known language is one colour too.
        let (_, pending) = scan_line_with_code("```rust", &start, false, None);
        assert!(pending.code.is_none());
    }

    #[test]
    fn syntax_colours_follow_the_theme() {
        use laankit_egui::theme::ThemeId;
        for id in ThemeId::ALL {
            let p = id.palette();
            let c = SyntaxColors::from_palette(p);
            assert_eq!(c.plain, p.text_primary);
            assert_eq!(c.punctuation, p.text_secondary);
            assert_ne!(c.keyword, c.string);
            let one_dark_keyword = Color32::from_rgb(0xc6, 0x78, 0xdd);
            assert_eq!(c.keyword == one_dark_keyword, p.dark, "{}", id.name());
        }
    }

    #[test]
    fn rows_map_to_lines_with_wrapping() {
        let ctx = egui::Context::default();
        laankit_egui::fonts::install(&ctx);
        let colors = EditorColors::from_palette(laankit_egui::theme::ThemeId::Light.palette());
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
