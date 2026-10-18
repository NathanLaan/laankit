//! Slide-level edits on a deck's Markdown. A port of
//! `apps/noteliner/src/renderer/lib/slideEdits.js`, egui-free and tested
//! against the JS (`tests/differential.rs`).
//!
//! Every function is pure: `(markdown, deck, …) → EditResult`. The `deck`
//! must be [`parse_deck`](crate::deck::parse_deck) of the same `markdown`;
//! slide regions come from its 1-based `source_range`s, which tile the note
//! without overlap. The host replaces the note text with
//! [`EditResult::markdown`], saves it and moves the caret to
//! [`EditResult::caret_line`].
//!
//! Two break styles survive every edit: heading breaks (`## Title`, which
//! carry their own boundary) and `---` separators (which belong to the end of
//! the previous slide's region). Reordering strips a trailing separator from
//! the block it moves and re-establishes boundaries afterwards.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::deck::{is_blank, BlockKind, Deck, Slide};

static ATX_RE: LazyLock<Regex> = LazyLock::new(|| re(r"^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$"));
static HR_RE: LazyLock<Regex> = LazyLock::new(|| re(r"^ {0,3}-{3,} *$"));
static DIRECTIVE_RE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^\s*<!--\s*slide\s*:?\s*(.*?)\s*(-->)?\s*$"));
static LAYOUT_PAIR_RE: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?i)\blayout\s*=\s*(?:"[^"]*"|'[^']*'|\S+)"#));
static SPACES_RE: LazyLock<Regex> = LazyLock::new(|| re(r"\s+"));
static NOTES_OPEN_RE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^\s*<!--\s*notes\b"));
static NOTES_CLOSE_RE: LazyLock<Regex> = LazyLock::new(|| re(r"-->\s*$"));

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid regex")
}

/// The rewritten note and where to put the caret (1-based line).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditResult {
    pub markdown: String,
    pub caret_line: Option<usize>,
}

/// Which way [`move_slide`] moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

/// An edit as a value, for UIs that pass edits around (the slides pane, the
/// app's commands). [`SlideEdit::apply`] runs the matching function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlideEdit {
    /// After slide `after` (1-based; `None` or 0 puts it first).
    Insert {
        after: Option<usize>,
        title: String,
        body: String,
    },
    Duplicate(usize),
    Delete(usize),
    Reorder {
        from: usize,
        to: usize,
    },
    Move(usize, Direction),
    MergeIntoPrevious(usize),
    SplitAtLine(usize),
    Retitle(usize, String),
    /// `None` or `"auto"` clears the directive.
    SetLayout(usize, Option<String>),
    /// 1-based, inclusive lines.
    ToggleNotes {
        from_line: usize,
        to_line: usize,
    },
}

impl SlideEdit {
    pub fn apply(&self, markdown: &str, deck: &Deck) -> EditResult {
        match self {
            SlideEdit::Insert { after, title, body } => {
                insert_slide(markdown, deck, *after, title, body)
            }
            SlideEdit::Duplicate(i) => duplicate_slide(markdown, deck, *i),
            SlideEdit::Delete(i) => delete_slide(markdown, deck, *i),
            SlideEdit::Reorder { from, to } => reorder_slide(markdown, deck, *from, *to),
            SlideEdit::Move(i, d) => move_slide(markdown, deck, *i, *d),
            SlideEdit::MergeIntoPrevious(i) => merge_into_previous(markdown, deck, *i),
            SlideEdit::SplitAtLine(line) => split_at_line(markdown, deck, *line),
            SlideEdit::Retitle(i, title) => retitle_slide(markdown, deck, *i, title),
            SlideEdit::SetLayout(i, layout) => {
                set_slide_layout(markdown, deck, *i, layout.as_deref())
            }
            SlideEdit::ToggleNotes { from_line, to_line } => {
                toggle_notes(markdown, *from_line, *to_line)
            }
        }
    }
}

type Lines = Vec<String>;

fn to_lines(markdown: &str) -> Lines {
    markdown
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(str::to_string)
        .collect()
}

fn result(lines: &[String], caret: Option<isize>) -> EditResult {
    EditResult {
        markdown: lines.join("\n"),
        caret_line: caret.map(|c| c.min(lines.len() as isize).max(1) as usize),
    }
}

/// 0-based inclusive `[start, end]` of a slide's lines.
fn bounds(slide: &Slide) -> (usize, usize) {
    (
        slide.source_range.from_line - 1,
        slide.source_range.to_line.saturating_sub(1),
    )
}

fn break_level(deck: &Deck) -> u8 {
    deck.config.slide_level
}

fn is_break_heading(line: Option<&str>, slide_level: u8) -> bool {
    if slide_level == 0 {
        return false;
    }
    line.and_then(|l| ATX_RE.captures(l))
        .is_some_and(|m| m[1].len() <= slide_level as usize)
}

fn is_hr(line: &str) -> bool {
    HR_RE.is_match(line)
}

fn blank_at(lines: &[String], i: usize) -> bool {
    lines.get(i).is_some_and(|l| is_blank(l))
}

fn trim_blanks(block: &[String]) -> Lines {
    let start = block.iter().position(|l| !is_blank(l));
    let end = block.iter().rposition(|l| !is_blank(l));
    match (start, end) {
        (Some(s), Some(e)) => block[s..=e].to_vec(),
        _ => Vec::new(),
    }
}

/// The region's content without blank padding or its trailing `---` (the next
/// slide's boundary).
fn content_block(lines: &[String], slide: &Slide) -> Lines {
    let (from, to) = bounds(slide);
    let end = (to + 1).min(lines.len());
    let mut block = trim_blanks(lines.get(from..end).unwrap_or(&[]));
    if block.last().is_some_and(|l| is_hr(l)) {
        block.pop();
        block = trim_blanks(&block);
    }
    block
}

/// The nearest non-blank line above `at` is a separator, or there is none.
fn has_boundary_before(lines: &[String], at: usize) -> bool {
    for i in (0..at.min(lines.len())).rev() {
        if is_blank(&lines[i]) {
            continue;
        }
        return is_hr(&lines[i]);
    }
    true
}

fn first_non_blank(lines: &[String], from: usize) -> Option<&str> {
    lines
        .iter()
        .skip(from)
        .find(|l| !is_blank(l))
        .map(String::as_str)
}

/// `Array.prototype.indexOf(value, from)`.
fn index_of(lines: &[String], value: &str, from: usize) -> isize {
    lines
        .iter()
        .enumerate()
        .skip(from)
        .find(|(_, l)| *l == value)
        .map_or(-1, |(i, _)| i as isize)
}

fn splice(lines: &mut Lines, at: usize, remove: usize, insert: Vec<String>) {
    let at = at.min(lines.len());
    let end = (at + remove).min(lines.len());
    lines.splice(at..end, insert);
}

fn strings(items: &[&str]) -> Lines {
    items.iter().map(|s| s.to_string()).collect()
}

/// Inserts a slide block at `at`, adding a separator before it when it can't
/// break on its own and after it when what follows can't. Returns the line
/// index the block's first line ends up on (or -1).
fn insert_slide_block(lines: &mut Lines, at: usize, raw_block: &[String], level: u8) -> isize {
    let block = trim_blanks(raw_block);
    if block.is_empty() {
        return at as isize;
    }
    let mut chunk = Vec::new();
    if at > 0 && !blank_at(lines, at - 1) {
        chunk.push(String::new());
    }
    if !is_break_heading(Some(&block[0]), level) && !has_boundary_before(lines, at) {
        chunk.extend(strings(&["---", ""]));
    }
    let offset = chunk.len();
    chunk.extend(block.iter().cloned());
    match first_non_blank(lines, at) {
        Some(following) => {
            let following_breaks = is_break_heading(Some(following), level) || is_hr(following);
            chunk.push(String::new());
            if !following_breaks {
                chunk.extend(strings(&["---", ""]));
            }
        }
        None => chunk.push(String::new()),
    }
    let len = chunk.len();
    splice(lines, at, 0, chunk);
    let start = at + offset;
    tidy_range(lines, at.saturating_sub(1), at + len + 1);
    index_of(lines, &block[0], start.saturating_sub(3))
}

/// Collapses runs of blank lines to one within `[from, to)` (a splice seam,
/// never inside fenced code).
fn tidy_range(lines: &mut Lines, from: usize, to: usize) {
    let end = lines.len().min(to);
    let mut i = from;
    while i < end && i < lines.len() {
        if !is_blank(&lines[i]) {
            i += 1;
            continue;
        }
        let mut run = 1;
        while i + run < lines.len() && is_blank(&lines[i + run]) {
            run += 1;
        }
        if run > 1 {
            lines.drain(i + 1..i + run);
        }
        i += 1;
    }
}

/// Drops a separator left dangling at the end of the note by a removal.
fn trim_orphan_tail(lines: &mut Lines, from: usize) {
    if lines.iter().skip(from).any(|l| !is_blank(l)) {
        return;
    }
    // `lines.length = from` can grow the array with holes (undefined) in JS;
    // `from` never exceeds the length here.
    lines.resize(from, String::new());
    while lines.last().is_some_and(|l| is_blank(l)) {
        lines.pop();
    }
    if lines.last().is_some_and(|l| is_hr(l)) {
        lines.pop();
    }
    while lines.last().is_some_and(|l| is_blank(l)) {
        lines.pop();
    }
    lines.push(String::new());
}

/// Makes the slide starting at `at` still start a slide; returns lines added.
fn ensure_boundary(lines: &mut Lines, at: usize, level: u8) -> usize {
    if at == 0 || at >= lines.len() {
        return 0;
    }
    if is_break_heading(Some(&lines[at]), level) || has_boundary_before(lines, at) {
        return 0;
    }
    splice(lines, at, 0, strings(&["---", ""]));
    2
}

/// Collapses the blank run at a splice junction to one line, or none at the
/// start or end of the note.
fn tidy_junction(lines: &mut Lines, at: usize) {
    let mut start = at;
    while start > 0 && blank_at(lines, start - 1) {
        start -= 1;
    }
    let mut end = at;
    while end < lines.len() && blank_at(lines, end) {
        end += 1;
    }
    let blanks = end - start;
    if blanks == 0 {
        return;
    }
    let keep = if start == 0 || end >= lines.len() {
        0
    } else {
        1
    };
    if blanks == keep {
        return;
    }
    splice(lines, start, blanks, vec![String::new(); keep]);
}

fn unchanged(markdown: &str) -> EditResult {
    result(&to_lines(markdown), None)
}

// ─── Inserting ───────────────────────────────────────────────────────────────

/// Adds a slide after slide `after` (1-based; `None` or 0 puts it first), with
/// a heading at the deck's break level and an optional body.
pub fn insert_slide(
    markdown: &str,
    deck: &Deck,
    after: Option<usize>,
    title: &str,
    body: &str,
) -> EditResult {
    let mut lines = to_lines(markdown);
    let level = break_level(deck);
    let heading_level = if level > 0 { level } else { 2 };
    let title = if title.is_empty() { "New Slide" } else { title };
    let heading = format!("{} {title}", "#".repeat(heading_level as usize));
    let mut block = vec![heading];
    if !body.is_empty() {
        block.push(String::new());
        block.extend(body.split('\n').map(str::to_string));
    }
    let at = match after.and_then(|a| deck.slide(a)) {
        Some(anchor) => bounds(anchor).1 + 1,
        None => deck
            .slides
            .first()
            .map_or(lines.len(), |first| bounds(first).0),
    };
    let start = insert_slide_block(&mut lines, at, &block, level);
    result(&lines, Some(start + 1))
}

pub fn duplicate_slide(markdown: &str, deck: &Deck, index: usize) -> EditResult {
    let Some(slide) = deck.slide(index) else {
        return unchanged(markdown);
    };
    let mut lines = to_lines(markdown);
    let block = content_block(&lines, slide);
    let start = insert_slide_block(&mut lines, bounds(slide).1 + 1, &block, break_level(deck));
    result(&lines, Some(start + 1))
}

// ─── Removing ────────────────────────────────────────────────────────────────

pub fn delete_slide(markdown: &str, deck: &Deck, index: usize) -> EditResult {
    let Some(slide) = deck.slide(index) else {
        return unchanged(markdown);
    };
    let mut lines = to_lines(markdown);
    let (from, to) = bounds(slide);
    splice(&mut lines, from, to + 1 - from, Vec::new());
    ensure_boundary(&mut lines, from, break_level(deck));
    trim_orphan_tail(&mut lines, from);
    tidy_junction(&mut lines, from);
    if lines.is_empty() {
        lines.push(String::new());
    }
    result(&lines, Some(from as isize + 1))
}

// ─── Reordering ──────────────────────────────────────────────────────────────

/// Moves slide `from_index` to 1-based position `to_index` among the slides.
pub fn reorder_slide(
    markdown: &str,
    deck: &Deck,
    from_index: usize,
    to_index: usize,
) -> EditResult {
    let slides = &deck.slides;
    let Some(slide) = deck.slide(from_index) else {
        return unchanged(markdown);
    };
    if slides.len() < 2 {
        return unchanged(markdown);
    }
    let target = to_index.clamp(1, slides.len());
    if target == from_index {
        return unchanged(markdown);
    }
    let mut lines = to_lines(markdown);
    let level = break_level(deck);
    let (from, to) = bounds(slide);
    let block = content_block(&lines, slide);
    let removed = to - from + 1;

    let others: Vec<&Slide> = slides.iter().filter(|s| s.index != slide.index).collect();
    let shift = |line: usize| if line > to { line - removed } else { line };
    let at = if target == 1 {
        shift(bounds(others[0]).0)
    } else {
        shift(bounds(others[target - 2]).1) + 1
    };

    splice(&mut lines, from, removed, Vec::new());
    ensure_boundary(&mut lines, from, level);
    trim_orphan_tail(&mut lines, from);
    tidy_junction(&mut lines, from);

    let at = at.min(lines.len());
    let start = insert_slide_block(&mut lines, at, &block, level);
    result(&lines, Some(start + 1))
}

pub fn move_slide(markdown: &str, deck: &Deck, index: usize, direction: Direction) -> EditResult {
    let to = match direction {
        Direction::Up => index.saturating_sub(1),
        Direction::Down => index + 1,
    };
    reorder_slide(markdown, deck, index, to)
}

// ─── Merging and splitting ───────────────────────────────────────────────────

/// Removes the boundary at the top of slide `index`: a break heading is
/// demoted one level (bold at level 6), a `---` is deleted.
pub fn merge_into_previous(markdown: &str, deck: &Deck, index: usize) -> EditResult {
    let Some(slide) = deck.slide(index) else {
        return unchanged(markdown);
    };
    if index < 2 {
        return unchanged(markdown);
    }
    let mut lines = to_lines(markdown);
    let level = break_level(deck);
    let (from, _) = bounds(slide);

    if is_break_heading(lines.get(from).map(String::as_str), level) {
        let text = ATX_RE
            .captures(&lines[from])
            .map(|m| m[2].to_string())
            .unwrap_or_default();
        lines[from] = if level >= 6 {
            format!("**{text}**")
        } else {
            format!("{} {text}", "#".repeat(level as usize + 1))
        };
        return result(&lines, Some(from as isize + 1));
    }
    for i in (0..from.min(lines.len())).rev() {
        if is_blank(&lines[i]) {
            continue;
        }
        if is_hr(&lines[i]) {
            lines.remove(i);
            tidy_junction(&mut lines, i);
            return result(&lines, Some(i as isize + 1));
        }
        break;
    }
    result(&lines, Some(from as isize + 1))
}

/// Starts a new slide at 1-based `line` with a `---` separator (0 means 1).
pub fn split_at_line(markdown: &str, deck: &Deck, line: usize) -> EditResult {
    let mut lines = to_lines(markdown);
    let at = line.max(1).saturating_sub(1).min(lines.len());
    let level = break_level(deck);
    if at == 0 {
        return result(&lines, Some(1));
    }
    if is_break_heading(lines.get(at).map(String::as_str), level) || has_boundary_before(&lines, at)
    {
        return result(&lines, Some(at as isize + 1));
    }
    let chunk = if blank_at(&lines, at - 1) {
        strings(&["---", ""])
    } else {
        strings(&["", "---", ""])
    };
    let n = chunk.len();
    splice(&mut lines, at, 0, chunk);
    result(&lines, Some((at + n) as isize + 1))
}

// ─── Slide properties ────────────────────────────────────────────────────────

/// Renames a slide, adding a heading when it has none.
pub fn retitle_slide(markdown: &str, deck: &Deck, index: usize, title: &str) -> EditResult {
    let Some(slide) = deck.slide(index) else {
        return unchanged(markdown);
    };
    let mut lines = to_lines(markdown);
    let text = crate::deck::js_trim(title);
    if let Some(heading) = slide.blocks.iter().find(|b| b.kind == BlockKind::Heading) {
        let at = heading.from_line - 1;
        let hashes = lines
            .get(at)
            .and_then(|l| ATX_RE.captures(l))
            .map(|m| m[1].to_string())
            .unwrap_or_default();
        let line = if hashes.is_empty() {
            text.to_string()
        } else {
            format!("{hashes} {text}")
        };
        if at < lines.len() {
            lines[at] = line;
        }
        return result(&lines, Some(at as isize + 1));
    }
    let level = break_level(deck);
    let heading_level = if level > 0 { level } else { 2 };
    let (from, _) = bounds(slide);
    let mut at = from;
    while at < lines.len() && (is_blank(&lines[at]) || is_hr(&lines[at])) {
        at += 1;
    }
    let heading = format!("{} {text}", "#".repeat(heading_level as usize));
    splice(&mut lines, at, 0, vec![heading, String::new()]);
    result(&lines, Some(at as isize + 1))
}

/// Sets a slide's `layout=` directive; `None` or `"auto"` clears it.
pub fn set_slide_layout(
    markdown: &str,
    deck: &Deck,
    index: usize,
    layout: Option<&str>,
) -> EditResult {
    let Some(slide) = deck.slide(index) else {
        return unchanged(markdown);
    };
    let mut lines = to_lines(markdown);
    let (from, to) = bounds(slide);
    let clearing = layout.is_none_or(|l| l.is_empty() || l == "auto");

    let mut i = from;
    while i <= to && i < lines.len() {
        let Some(m) = DIRECTIVE_RE.captures(&lines[i]) else {
            i += 1;
            continue;
        };
        let body = m.get(1).map_or("", |g| g.as_str()).to_string();
        let removed = LAYOUT_PAIR_RE.replacen(&body, 1, "");
        let without = SPACES_RE.replace_all(&removed, " ");
        let without = crate::deck::js_trim(&without).to_string();
        let next = if clearing {
            without
        } else {
            let joined = format!("{without} layout={}", layout.unwrap_or_default());
            crate::deck::js_trim(&joined).to_string()
        };
        if next.is_empty() {
            lines.remove(i);
            tidy_junction(&mut lines, i);
        } else {
            lines[i] = format!("<!-- slide: {next} -->");
        }
        return result(&lines, Some((i + 1).min(lines.len()) as isize));
    }
    if clearing {
        return result(&lines, Some(from as isize + 1));
    }
    let heading = slide.blocks.iter().find(|b| b.kind == BlockKind::Heading);
    let at = heading.map_or(from + 1, |h| h.to_line);
    let directive = format!("<!-- slide: layout={} -->", layout.unwrap_or_default());
    splice(&mut lines, at, 0, vec![String::new(), directive]);
    result(&lines, Some(at as isize + 2))
}

// ─── Speaker notes ───────────────────────────────────────────────────────────

/// Wraps 1-based lines `[from_line, to_line]` in a `<!-- notes … -->` block,
/// or unwraps them when the selection is exactly one. 0 means "not given".
pub fn toggle_notes(markdown: &str, from_line: usize, to_line: usize) -> EditResult {
    let mut lines = to_lines(markdown);
    let first = if from_line > 0 { from_line } else { 1 };
    let from = first - 1;
    let last = if to_line > 0 { to_line } else { first };
    let to = from.max((last - 1).min(lines.len().saturating_sub(1)));
    let line = |i: usize| lines.get(i).map_or("", String::as_str);

    let opens = NOTES_OPEN_RE.is_match(line(from));
    let closes = NOTES_CLOSE_RE.is_match(line(to));
    if opens && closes && to > from {
        lines.remove(to);
        lines.remove(from);
        return result(&lines, Some(from as isize + 1));
    }
    // Already inside a notes block: nesting comments isn't possible.
    for i in (0..=from).rev() {
        if i < from && line(i).contains("-->") {
            break;
        }
        if NOTES_OPEN_RE.is_match(line(i)) {
            return result(&lines, Some(from as isize + 1));
        }
    }
    splice(&mut lines, to + 1, 0, strings(&["-->"]));
    splice(&mut lines, from, 0, strings(&["<!-- notes"]));
    result(&lines, Some(from as isize + 2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deck::parse_deck;
    use serde_json::json;

    fn titles(md: &str) -> Vec<Option<String>> {
        parse_deck(md, None)
            .slides
            .into_iter()
            .map(|s| s.title)
            .collect()
    }

    fn apply(md: &str, edit: SlideEdit) -> EditResult {
        edit.apply(md, &parse_deck(md, None))
    }

    const MD: &str = "## Alpha\n\nFirst body.\n\n## Beta\n\nSecond body.\n\n## Gamma\n\nThird.\n";

    #[test]
    fn insert_after_and_first() {
        let r = apply(
            MD,
            SlideEdit::Insert {
                after: Some(1),
                title: "New".into(),
                body: String::new(),
            },
        );
        assert_eq!(
            titles(&r.markdown),
            [Some("Alpha"), Some("New"), Some("Beta"), Some("Gamma")].map(|s| s.map(String::from))
        );
        assert_eq!(r.caret_line, Some(5));
    }

    #[test]
    fn reorder_and_delete_keep_boundaries() {
        let r = apply(MD, SlideEdit::Reorder { from: 3, to: 1 });
        assert_eq!(
            titles(&r.markdown),
            [Some("Gamma"), Some("Alpha"), Some("Beta")].map(|s| s.map(String::from))
        );
        let sep = "Intro\n\n---\n\nMiddle\n\n---\n\nEnd\n";
        let deck = parse_deck(sep, Some(&json!({ "slideLevel": 0 })));
        let r = delete_slide(sep, &deck, 3);
        let after = parse_deck(&r.markdown, Some(&json!({ "slideLevel": 0 })));
        assert_eq!(after.slides.len(), 2);
        assert!(!r.markdown.trim_end().ends_with("---"));
    }

    #[test]
    fn merge_split_retitle_layout_notes() {
        let merged = apply(MD, SlideEdit::MergeIntoPrevious(2));
        assert!(merged.markdown.contains("### Beta"));
        let split = apply(MD, SlideEdit::SplitAtLine(3));
        assert_eq!(parse_deck(&split.markdown, None).slides.len(), 4);
        let renamed = apply(MD, SlideEdit::Retitle(2, " Bee ".into()));
        assert!(renamed.markdown.contains("## Bee\n"));
        let laid = apply(MD, SlideEdit::SetLayout(1, Some("split".into())));
        assert!(laid.markdown.contains("<!-- slide: layout=split -->"));
        let cleared = apply(&laid.markdown, SlideEdit::SetLayout(1, None));
        assert_eq!(cleared.markdown, MD);
        let notes = toggle_notes(MD, 3, 3);
        assert!(notes.markdown.contains("<!-- notes\nFirst body.\n-->"));
        assert_eq!(toggle_notes(&notes.markdown, 3, 5).markdown, MD);
    }
}
