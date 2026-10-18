//! Deck parsing: a note's body → slides. A port of
//! `apps/noteliner/src/renderer/lib/slides.js`, kept free of egui so it can be
//! tested against the JS line for line (`tests/differential.rs` runs both on
//! the same generated notes).
//!
//! A note is a deck when its frontmatter carries a `presentation:` block (see
//! [`normalize_presentation`]). [`parse_deck`] splits the body at `---`
//! separators and at headings no deeper than `slideLevel`, reads
//! `<!-- notes -->`, `<!-- skip -->` and `<!-- slide: key=value -->`
//! comments, and infers a layout per slide. Line numbers are 1-based, like
//! the editor's caret line.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use serde_json::Value;

/// Stage sizes in px per aspect ratio (`ASPECTS`).
pub const ASPECTS: [(&str, Stage); 3] = [
    ("16:9", Stage { w: 1920, h: 1080 }),
    ("16:10", Stage { w: 1920, h: 1200 }),
    ("4:3", Stage { w: 1440, h: 1080 }),
];

/// Layouts a `layout=` directive may force (`LAYOUTS`).
const LAYOUTS: [&str; 8] = [
    "auto",
    "hero",
    "title-body",
    "split",
    "grid",
    "full-bleed",
    "quote",
    "table",
];

static FENCE_RE: LazyLock<Regex> = LazyLock::new(|| re(r"^ {0,3}(`{3,}|~{3,})"));
static HR_RE: LazyLock<Regex> = LazyLock::new(|| re(r"^ {0,3}-{3,} *$"));
static ATX_RE: LazyLock<Regex> = LazyLock::new(|| re(r"^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$"));
static COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^\s*<!--\s*(notes|skip|slide)\b\s*:?\s*"));
static LIST_RE: LazyLock<Regex> = LazyLock::new(|| re(r"^ {0,3}(?:[-*+]\s|[0-9]+[.)]\s)"));
static IMAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| re(r#"^!\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)$"#));
static YOUTUBE_RE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^(?:https?://)?(?:www\.)?(?:youtube\.com/watch\?|youtu\.be/)"));
static VIDEO_EXT_RE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\.(?:mp4|webm|mov|m4v|ogv)(?:[?#].*)?$"));
static TABLE_RULE_RE: LazyLock<Regex> = LazyLock::new(|| re(r"^[\s|:-]*-[\s|:-]*$"));
static DIRECTIVE_PAIR_RE: LazyLock<Regex> =
    LazyLock::new(|| re(r#"([A-Za-z0-9_][A-Za-z0-9_-]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|(\S+))"#));
static FLAG_RE: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-Za-z0-9_-]+$"));
static FM_NESTED_RE: LazyLock<Regex> = LazyLock::new(|| re(r"^\s+\S"));
static FM_KEY_RE: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-Za-z0-9_.$-]+\s*:(\s|$)"));
static PLACEHOLDER_RE: LazyLock<Regex> = LazyLock::new(|| re(r"\{\{([A-Za-z0-9_]+)\}\}"));
static WORD_RE: LazyLock<Regex> = LazyLock::new(|| re(r"[\p{L}\p{N}'’-]+"));

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid regex")
}

/// The fixed stage a slide is laid out on, in px.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Stage {
    pub w: u32,
    pub h: u32,
}

/// A deck's `presentation:` settings, normalized (`DEFAULT_PRESENTATION`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Presentation {
    /// `dark` or `light`; other names render as dark.
    pub theme: String,
    /// A key of [`ASPECTS`].
    pub aspect: String,
    /// Headings at this level or shallower start a slide; 0 means only `---`.
    pub slide_level: u8,
    pub header: String,
    pub footer: String,
    pub slide_numbers: bool,
    /// A leading `# heading` with at most one other block gets the hero layout.
    pub first_slide_title: bool,
}

impl Default for Presentation {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            aspect: "16:9".into(),
            slide_level: 2,
            header: String::new(),
            footer: String::new(),
            slide_numbers: true,
            first_slide_title: true,
        }
    }
}

impl Presentation {
    /// The value to store as the note's `presentation:` frontmatter block.
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("serializable")
    }
}

/// A value of a `<!-- slide: … -->` directive: `key=value`, or a bare flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirValue {
    Str(String),
    Flag,
}

impl DirValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            DirValue::Str(s) => Some(s),
            DirValue::Flag => None,
        }
    }
}

/// A slide's directives, in the order they were written. Unknown keys are kept.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Directives(Vec<(String, DirValue)>);

impl Directives {
    pub fn get(&self, key: &str) -> Option<&DirValue> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// A `key=value` directive's value (flags have none).
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(DirValue::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &(String, DirValue)> {
        self.0.iter()
    }

    /// Sets `key`, keeping its position if it's already there (a JS object assignment).
    fn set(&mut self, key: String, value: DirValue) {
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key, value)),
        }
    }

    fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
}

impl Serialize for Directives {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            match v {
                DirValue::Str(text) => map.serialize_entry(k, text)?,
                DirValue::Flag => map.serialize_entry(k, &true)?,
            }
        }
        map.end()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockKind {
    Heading,
    Para,
    List,
    Quote,
    Code,
    Table,
    Math,
    Image,
    Video,
}

/// A block of a slide. `text` is raw Markdown (a heading's text without its `#`s).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub text: String,
    pub from_line: usize,
    pub to_line: usize,
    pub kind: BlockKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    #[default]
    Auto,
    Hero,
    TitleBody,
    Split,
    Grid,
    FullBleed,
    Quote,
    Table,
}

impl Layout {
    pub fn id(self) -> &'static str {
        match self {
            Layout::Auto => "auto",
            Layout::Hero => "hero",
            Layout::TitleBody => "title-body",
            Layout::Split => "split",
            Layout::Grid => "grid",
            Layout::FullBleed => "full-bleed",
            Layout::Quote => "quote",
            Layout::Table => "table",
        }
    }

    pub fn from_id(id: &str) -> Option<Layout> {
        [
            Layout::Auto,
            Layout::Hero,
            Layout::TitleBody,
            Layout::Split,
            Layout::Grid,
            Layout::FullBleed,
            Layout::Quote,
            Layout::Table,
        ]
        .into_iter()
        .find(|l| l.id() == id)
    }
}

/// 1-based, inclusive line range a slide covers in the note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRange {
    pub from_line: usize,
    pub to_line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Slide {
    /// 1-based, counting only slides on the deck (notes-only slides are skipped).
    pub index: usize,
    pub source_range: SourceRange,
    pub title: Option<String>,
    pub heading_level: Option<u8>,
    pub blocks: Vec<Block>,
    /// Speaker notes, blocks joined by a blank line.
    pub notes: String,
    pub directives: Directives,
    pub notes_only: bool,
    pub layout: Layout,
}

impl Slide {
    /// The layout the slide declares itself (`layout=`), as opposed to the inferred one.
    pub fn explicit_layout(&self) -> Option<&str> {
        self.directives.get_str("layout")
    }

    pub fn headings(&self) -> impl Iterator<Item = &Block> {
        self.blocks.iter().filter(|b| b.kind == BlockKind::Heading)
    }

    pub fn body(&self) -> impl Iterator<Item = &Block> {
        self.blocks.iter().filter(|b| b.kind != BlockKind::Heading)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Deck {
    pub config: Presentation,
    pub stage: Stage,
    pub slides: Vec<Slide>,
    /// Slides dropped by `<!-- slide: notes-only -->`.
    pub skipped: usize,
}

impl Deck {
    /// The deck as the JS `parseDeck` returns it.
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("serializable")
    }

    /// The 1-based slide, if there is one.
    pub fn slide(&self, index: usize) -> Option<&Slide> {
        index.checked_sub(1).and_then(|i| self.slides.get(i))
    }
}

// ─── Config ──────────────────────────────────────────────────────────────────

/// The note's `presentation:` frontmatter value, normalized; `None` when the
/// note isn't a deck (absent, `null` or `false`). `presentation: true` means
/// the defaults.
pub fn normalize_presentation(raw: Option<&Value>) -> Option<Presentation> {
    let raw = raw?;
    if raw.is_null() || *raw == Value::Bool(false) {
        return None;
    }
    let mut out = Presentation::default();
    let Value::Object(src) = raw else {
        return Some(out);
    };
    if let Some(Value::String(theme)) = src.get("theme") {
        if !js_trim(theme).is_empty() {
            out.theme = js_trim(theme).to_string();
        }
    }
    if let Some(Value::String(aspect)) = src.get("aspect") {
        let aspect = js_trim(aspect);
        if ASPECTS.iter().any(|(k, _)| *k == aspect) {
            out.aspect = aspect.to_string();
        }
    }
    // YAML may hand over a number or a numeric string; JS `Number()` semantics.
    let level = js_number(src.get("slideLevel"));
    if level.fract() == 0.0 && (0.0..=6.0).contains(&level) {
        out.slide_level = level as u8;
    }
    if let Some(Value::String(h)) = src.get("header") {
        out.header = h.clone();
    }
    if let Some(Value::String(f)) = src.get("footer") {
        out.footer = f.clone();
    }
    if let Some(v) = src.get("slideNumbers") {
        out.slide_numbers = js_truthy(v);
    }
    if let Some(v) = src.get("firstSlideTitle") {
        out.first_slide_title = js_truthy(v);
    }
    Some(out)
}

/// The stage for an aspect ratio (16:9 when unknown).
pub fn stage_size(aspect: &str) -> Stage {
    ASPECTS
        .iter()
        .find(|(k, _)| *k == aspect)
        .map_or(ASPECTS[0].1, |(_, s)| *s)
}

/// Fills `{{key}}` placeholders from `vars`, leaving unknown ones as they are
/// (`TemplateService.substitute`'s vocabulary).
pub fn substitute(text: &str, vars: &HashMap<String, String>) -> String {
    PLACEHOLDER_RE
        .replace_all(text, |c: &regex::Captures| {
            vars.get(&c[1]).cloned().unwrap_or_else(|| c[0].to_string())
        })
        .into_owned()
}

/// JS `String.prototype.trim` (Unicode whitespace and the BOM).
pub(crate) fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

pub(crate) fn is_blank(s: &str) -> bool {
    js_trim(s).is_empty()
}

fn js_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// JS `Number(value)`; a missing value is `undefined`, so NaN.
fn js_number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => js_number_str(s),
        Some(Value::Array(a)) => match a.as_slice() {
            [] => 0.0,
            [Value::Array(_) | Value::Object(_)] => f64::NAN,
            [Value::Null] => 0.0,
            [Value::String(s)] => js_number_str(s),
            [one] => js_number(Some(one)),
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
    }
}

fn js_number_str(s: &str) -> f64 {
    let t = js_trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let radix = |p: &str, r| u64::from_str_radix(p, r).map_or(f64::NAN, |n| n as f64);
    match t.get(..2) {
        Some("0x" | "0X") => return radix(&t[2..], 16),
        Some("0o" | "0O") => return radix(&t[2..], 8),
        Some("0b" | "0B") => return radix(&t[2..], 2),
        _ => {}
    }
    match t {
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ if t
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')) =>
        {
            t.parse().unwrap_or(f64::NAN)
        }
        _ => f64::NAN,
    }
}

// ─── Parsing ─────────────────────────────────────────────────────────────────

/// The note body (or a whole file: a leading YAML block is skipped) as a deck.
/// `presentation` is the frontmatter's `presentation:` value; `None` uses the
/// defaults.
pub fn parse_deck(markdown: &str, presentation: Option<&Value>) -> Deck {
    let config = normalize_presentation(presentation).unwrap_or_default();
    let stage = stage_size(&config.aspect);
    let text = markdown.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').collect();

    let start = skip_frontmatter(&lines);
    let scan = scan_lines(&lines, start, config.slide_level);
    let raw = build_slides(&lines, &scan, &config);

    let total = raw.len();
    let mut slides: Vec<Slide> = raw.into_iter().filter(|s| !s.notes_only).collect();
    for (i, s) in slides.iter_mut().enumerate() {
        s.index = i + 1;
    }
    let skipped = total - slides.len();
    Deck {
        config,
        stage,
        slides,
        skipped,
    }
}

/// Lines a leading YAML block takes up; 0 unless it really looks like YAML, so
/// a body that opens with a `---` separator isn't swallowed.
fn skip_frontmatter(lines: &[&str]) -> usize {
    if lines.first().map(|l| js_trim(l)) != Some("---") {
        return 0;
    }
    let mut saw_key = false;
    for (i, line) in lines.iter().enumerate().skip(1) {
        if js_trim(line) == "---" {
            return if saw_key { i + 1 } else { 0 };
        }
        if js_trim(line).is_empty() || FM_NESTED_RE.is_match(line) {
            continue;
        }
        if FM_KEY_RE.is_match(line) {
            saw_key = true;
            continue;
        }
        return 0;
    }
    0
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommentKind {
    Notes,
    Skip,
    Slide,
}

struct Comment {
    kind: CommentKind,
    from_line: usize,
    to_line: usize,
    body: String,
}

struct Scan {
    start: usize,
    /// Line → heading level, or 0 for a `---` separator.
    breaks: HashMap<usize, u8>,
    comments: Vec<Comment>,
    drop: HashSet<usize>,
    setext: HashMap<usize, u8>,
}

fn fence_marker(line: &str) -> Option<char> {
    FENCE_RE.captures(line).and_then(|c| c[1].chars().next())
}

fn fence_closes(line: &str, marker: char) -> bool {
    fence_marker(line) == Some(marker)
}

/// `Text` + `---` is a setext H2, not a separator: true when the line above is
/// plain text.
fn is_setext_underline(lines: &[&str], i: usize) -> bool {
    if i == 0 {
        return false;
    }
    let prev = lines[i - 1];
    if prev.is_empty() || is_blank(prev) {
        return false;
    }
    if ATX_RE.is_match(prev) || FENCE_RE.is_match(prev) || LIST_RE.is_match(prev) {
        return false;
    }
    if prev.trim_start().starts_with('>') {
        return false;
    }
    !prev.contains('|')
}

fn scan_lines(lines: &[&str], start: usize, slide_level: u8) -> Scan {
    let mut scan = Scan {
        start,
        breaks: HashMap::new(),
        comments: Vec::new(),
        drop: HashSet::new(),
        setext: HashMap::new(),
    };
    let mut fence: Option<char> = None;
    let mut i = start;
    while i < lines.len() {
        let line = lines[i];
        if let Some(marker) = fence {
            if fence_closes(line, marker) {
                fence = None;
            }
            i += 1;
            continue;
        }
        if let Some(marker) = fence_marker(line) {
            fence = Some(marker);
            i += 1;
            continue;
        }
        if let Some(m) = COMMENT_RE.captures(line) {
            let region = read_comment(lines, i, &m);
            for j in region.from_line..=region.to_line {
                scan.drop.insert(j);
            }
            i = region.to_line + 1;
            scan.comments.push(region);
            continue;
        }
        if let Some(atx) = ATX_RE.captures(line) {
            let level = atx[1].len() as u8;
            if slide_level > 0 && level <= slide_level {
                scan.breaks.insert(i, level);
            }
            i += 1;
            continue;
        }
        if HR_RE.is_match(line) {
            if is_setext_underline(lines, i) {
                scan.setext.insert(i - 1, 2);
                if slide_level >= 2 {
                    scan.breaks.insert(i - 1, 2);
                }
            } else {
                scan.breaks.insert(i, 0);
            }
            scan.drop.insert(i);
        }
        i += 1;
    }
    scan
}

/// A notes / skip / slide comment, single- or multi-line; an unterminated one
/// runs to the end of the note.
fn read_comment(lines: &[&str], i: usize, m: &regex::Captures) -> Comment {
    let kind = match m[1].to_lowercase().as_str() {
        "notes" => CommentKind::Notes,
        "skip" => CommentKind::Skip,
        _ => CommentKind::Slide,
    };
    let first = &lines[i][m.get(0).map_or(0, |g| g.end())..];
    let mut collected: Vec<&str> = Vec::new();
    let mut to_line = i;
    if let Some(close) = first.find("-->") {
        collected.push(&first[..close]);
    } else {
        collected.push(first);
        for (j, line) in lines.iter().enumerate().skip(i + 1) {
            to_line = j;
            if let Some(close) = line.find("-->") {
                collected.push(&line[..close]);
                break;
            }
            collected.push(line);
        }
    }
    let joined = collected.join("\n");
    let body = joined
        .trim_start_matches('\n')
        .trim_end_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    Comment {
        kind,
        from_line: i,
        to_line,
        body: body.to_string(),
    }
}

/// `layout=split bg=#101418 class="big quote" notes-only` → directives.
fn parse_directives(body: &str) -> Directives {
    let mut out = Directives::default();
    for m in DIRECTIVE_PAIR_RE.captures_iter(body) {
        let key = m[1].to_lowercase();
        // `obj.__proto__ = "string"` is a no-op in JS.
        if key == "__proto__" {
            continue;
        }
        let value = m
            .get(2)
            .or_else(|| m.get(3))
            .or_else(|| m.get(4))
            .map_or("", |g| g.as_str());
        out.set(key, DirValue::Str(value.to_string()));
    }
    for word in body.split(char::is_whitespace) {
        let flag = js_trim(word).to_lowercase();
        // `flag in out` also sees Object.prototype's lower-case names.
        let inherited = flag == "constructor" || flag == "__proto__";
        if !flag.is_empty()
            && !flag.contains('=')
            && FLAG_RE.is_match(&flag)
            && !out.contains(&flag)
            && !inherited
        {
            out.set(flag, DirValue::Flag);
        }
    }
    out
}

struct Token<'a> {
    line: usize,
    text: &'a str,
    setext_level: Option<u8>,
}

fn build_slides(lines: &[&str], scan: &Scan, config: &Presentation) -> Vec<Slide> {
    let comments: HashMap<usize, &Comment> =
        scan.comments.iter().map(|c| (c.from_line, c)).collect();

    // Slice at the breaks: a heading keeps its line, a separator is dropped.
    let mut bounds: Vec<(usize, isize)> = Vec::new();
    let mut cursor = scan.start;
    for i in scan.start..lines.len() {
        let Some(&level) = scan.breaks.get(&i) else {
            continue;
        };
        let from = if level > 0 { i } else { i + 1 };
        if from > cursor {
            bounds.push((cursor, from as isize - 1));
        }
        cursor = from;
    }
    bounds.push((cursor, lines.len() as isize - 1));

    let mut slides = Vec::new();
    for (from, to) in bounds {
        let slide = build_slide(lines, from, to, scan, &comments);
        if slide.blocks.is_empty() && slide.notes.is_empty() && slide.directives.is_empty() {
            continue;
        }
        slides.push(slide);
    }
    if config.first_slide_title {
        if let Some(first) = slides.first_mut() {
            apply_title_slide(first);
        }
    }
    for slide in &mut slides {
        slide.layout = pick_layout_for(slide);
    }
    slides
}

fn build_slide(
    lines: &[&str],
    from: usize,
    to: isize,
    scan: &Scan,
    comments: &HashMap<usize, &Comment>,
) -> Slide {
    let mut content = Vec::new();
    let mut notes: Vec<&str> = Vec::new();
    let mut directives = Directives::default();
    let mut i = from as isize;
    while i <= to {
        let at = i as usize;
        if let Some(comment) = comments.get(&at) {
            match comment.kind {
                CommentKind::Notes if !comment.body.is_empty() => notes.push(&comment.body),
                CommentKind::Slide => {
                    for (k, v) in parse_directives(&comment.body).0 {
                        directives.set(k, v);
                    }
                }
                _ => {}
            }
            i = comment.to_line as isize + 1;
            continue;
        }
        if !scan.drop.contains(&at) {
            content.push(Token {
                line: at,
                text: lines[at],
                setext_level: scan.setext.get(&at).copied(),
            });
        }
        i += 1;
    }

    let blocks = split_blocks(&content);
    let heading = blocks.iter().find(|b| b.kind == BlockKind::Heading);
    let notes_only = match directives.get("notes-only") {
        Some(DirValue::Flag) => true,
        Some(DirValue::Str(s)) => s == "true",
        None => false,
    };
    Slide {
        index: 0,
        source_range: SourceRange {
            from_line: from + 1,
            to_line: (to + 1) as usize,
        },
        title: heading.map(|h| h.text.clone()),
        heading_level: heading.and_then(|h| h.level),
        notes: notes.join("\n\n"),
        directives,
        notes_only,
        layout: Layout::Auto,
        blocks,
    }
}

/// Blank lines separate blocks; fenced code keeps its own; headings stand alone.
fn split_blocks(content: &[Token]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut cur: Vec<&Token> = Vec::new();
    let mut fence: Option<char> = None;
    let flush = |cur: &mut Vec<&Token>, blocks: &mut Vec<Block>| {
        if !cur.is_empty() {
            blocks.push(make_block(cur));
            cur.clear();
        }
    };
    for tok in content {
        if let Some(marker) = fence {
            cur.push(tok);
            if fence_closes(tok.text, marker) {
                fence = None;
                flush(&mut cur, &mut blocks);
            }
            continue;
        }
        if let Some(marker) = fence_marker(tok.text) {
            flush(&mut cur, &mut blocks);
            fence = Some(marker);
            cur.push(tok);
            continue;
        }
        if is_blank(tok.text) {
            flush(&mut cur, &mut blocks);
            continue;
        }
        if ATX_RE.is_match(tok.text) || tok.setext_level.is_some() {
            flush(&mut cur, &mut blocks);
            cur.push(tok);
            flush(&mut cur, &mut blocks);
            continue;
        }
        cur.push(tok);
    }
    flush(&mut cur, &mut blocks);
    blocks
}

fn is_table(toks: &[&Token]) -> bool {
    toks.len() >= 2
        && toks.iter().all(|t| t.text.contains('|'))
        && TABLE_RULE_RE.is_match(toks[1].text)
        && toks[1].text.contains('-')
}

fn make_block(toks: &[&Token]) -> Block {
    let text = toks.iter().map(|t| t.text).collect::<Vec<_>>().join("\n");
    let mut block = Block {
        text,
        from_line: toks[0].line + 1,
        to_line: toks[toks.len() - 1].line + 1,
        kind: BlockKind::Para,
        level: None,
        alt: None,
        src: None,
    };
    let first = toks[0].text;
    let trimmed = js_trim(first);

    if toks.len() == 1 {
        if let Some(atx) = ATX_RE.captures(first) {
            block.kind = BlockKind::Heading;
            block.level = Some(atx[1].len() as u8);
            block.text = atx[2].to_string();
            return block;
        }
        if let Some(level) = toks[0].setext_level {
            block.kind = BlockKind::Heading;
            block.level = Some(level);
            block.text = trimmed.to_string();
            return block;
        }
        if let Some(img) = IMAGE_RE.captures(trimmed) {
            let src = img[2].to_string();
            block.kind = if VIDEO_EXT_RE.is_match(&src) {
                BlockKind::Video
            } else {
                BlockKind::Image
            };
            block.alt = Some(img[1].to_string());
            block.src = Some(src);
            return block;
        }
        if YOUTUBE_RE.is_match(trimmed) {
            block.kind = BlockKind::Video;
            block.src = Some(trimmed.to_string());
            block.alt = Some(String::new());
            return block;
        }
    }
    block.kind = if FENCE_RE.is_match(first) {
        BlockKind::Code
    } else if trimmed.starts_with('>') {
        BlockKind::Quote
    } else if LIST_RE.is_match(first) {
        BlockKind::List
    } else if is_table(toks) {
        BlockKind::Table
    } else if trimmed.starts_with("$$") {
        BlockKind::Math
    } else {
        BlockKind::Para
    };
    block
}

/// A leading H1 with at most one other block is a title slide (a layout hint only).
fn apply_title_slide(slide: &mut Slide) {
    let Some(first) = slide.blocks.first() else {
        return;
    };
    if first.kind == BlockKind::Heading && first.level == Some(1) && slide.blocks.len() <= 2 {
        let mut d = Directives(vec![("layout".into(), DirValue::Str("hero".into()))]);
        for (k, v) in slide.directives.0.drain(..) {
            d.set(k, v);
        }
        slide.directives = d;
    }
}

/// The layout from the block count and kinds, unless `layout=` forces one.
pub fn pick_layout_for(slide: &Slide) -> Layout {
    if let Some(forced) = slide.directives.get_str("layout") {
        if forced != "auto" && LAYOUTS.contains(&forced) {
            return Layout::from_id(forced).unwrap_or_default();
        }
    }
    let body: Vec<&Block> = slide.body().collect();
    let has_heading = slide.headings().next().is_some();
    match body.as_slice() {
        [] => Layout::Hero,
        [only] => match only.kind {
            BlockKind::Image | BlockKind::Video if has_heading => Layout::TitleBody,
            BlockKind::Image | BlockKind::Video => Layout::FullBleed,
            BlockKind::Quote => Layout::Quote,
            BlockKind::Table => Layout::Table,
            _ if has_heading => Layout::TitleBody,
            _ => Layout::Hero,
        },
        [_, _] => Layout::Split,
        _ => Layout::Grid,
    }
}

// ─── Helpers for the UI ──────────────────────────────────────────────────────

/// The slide containing a 1-based caret line (`slideAtLine`).
pub fn slide_at_line(deck: &Deck, line: usize) -> Option<&Slide> {
    if line == 0 {
        return None;
    }
    let mut active = None;
    for slide in &deck.slides {
        if slide.source_range.from_line <= line {
            active = Some(slide);
        } else {
            break;
        }
    }
    active
}

/// Words the presenter says (visible text plus notes) and the minutes at `wpm`.
pub fn estimate_duration(deck: &Deck, wpm: f64) -> (usize, f64) {
    let words: usize = deck
        .slides
        .iter()
        .map(|s| {
            s.blocks.iter().map(|b| count_words(&b.text)).sum::<usize>() + count_words(&s.notes)
        })
        .sum();
    (words, words as f64 / wpm)
}

fn count_words(text: &str) -> usize {
    WORD_RE.find_iter(text).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn titles(md: &str, cfg: Value) -> Vec<Option<String>> {
        parse_deck(md, Some(&cfg))
            .slides
            .into_iter()
            .map(|s| s.title)
            .collect()
    }

    #[test]
    fn headings_and_separators_split_slides() {
        let md = "# Deck\n\nIntro\n\n## One\n\nBody\n\n---\n\nTwo body\n";
        assert_eq!(
            titles(md, json!({})),
            [Some("Deck".into()), Some("One".into()), None]
        );
        let deck = parse_deck(md, None);
        assert_eq!(deck.slides[0].layout, Layout::Hero);
        assert_eq!(deck.slides[1].layout, Layout::TitleBody);
    }

    #[test]
    fn code_fences_hide_breaks_and_comments() {
        let md = "## A\n\n```\n## not a slide\n---\n<!-- notes x -->\n```\n";
        let deck = parse_deck(md, None);
        assert_eq!(deck.slides.len(), 1);
        assert_eq!(deck.slides[0].blocks[1].kind, BlockKind::Code);
        assert!(deck.slides[0].notes.is_empty());
    }

    #[test]
    fn notes_skip_and_directives() {
        let md = "## A\n<!-- notes\nSay hi.\n-->\n<!-- slide: layout=quote bg=\"#000\" -->\n> q\n\n## B\n<!-- slide: notes-only -->\nhidden\n";
        let deck = parse_deck(md, None);
        assert_eq!(deck.slides.len(), 1);
        assert_eq!(deck.skipped, 1);
        let a = &deck.slides[0];
        assert_eq!(a.notes, "Say hi.");
        assert_eq!(a.directives.get_str("bg"), Some("#000"));
        assert_eq!(a.layout, Layout::Quote);
    }

    #[test]
    fn setext_heading_is_not_a_separator() {
        let deck = parse_deck("Intro\n\nRevenue\n---\n\nUp 10%\n", None);
        assert_eq!(deck.slides.len(), 2);
        assert_eq!(deck.slides[1].title.as_deref(), Some("Revenue"));
    }

    #[test]
    fn config_normalization() {
        assert_eq!(normalize_presentation(None), None);
        assert_eq!(normalize_presentation(Some(&json!(false))), None);
        assert_eq!(
            normalize_presentation(Some(&json!(true))),
            Some(Presentation::default())
        );
        let p = normalize_presentation(Some(&json!({
            "theme": " light ", "aspect": "4:3", "slideLevel": "3", "slideNumbers": 0
        })))
        .unwrap();
        assert_eq!(
            (
                p.theme.as_str(),
                p.aspect.as_str(),
                p.slide_level,
                p.slide_numbers
            ),
            ("light", "4:3", 3, false)
        );
        let bad = normalize_presentation(Some(&json!({ "slideLevel": 9 }))).unwrap();
        assert_eq!(bad.slide_level, 2);
    }

    #[test]
    fn substitutes_known_placeholders() {
        let vars = HashMap::from([("page".to_string(), "3".to_string())]);
        assert_eq!(substitute("{{page}} / {{total}}", &vars), "3 / {{total}}");
    }

    #[test]
    fn caret_maps_to_slide_and_duration_counts_words() {
        let deck = parse_deck(
            "## A\n\none two\n\n## B\n\nthree\n<!-- notes four five -->\n",
            None,
        );
        assert_eq!(slide_at_line(&deck, 3).map(|s| s.index), Some(1));
        assert_eq!(slide_at_line(&deck, 6).map(|s| s.index), Some(2));
        assert_eq!(estimate_duration(&deck, 130.0).0, 7);
    }
}
