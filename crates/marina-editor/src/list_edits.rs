//! List editing for the Markdown editor.
//!
//! The span functions ([`classify_span`], [`to_unordered_list`],
//! [`to_ordered_list`], [`indent_as_sub_list`]) port NoteLiner's
//! `apps/noteliner/src/renderer/lib/listEdits.js` one to one: pure functions
//! over 1-based line numbers that return only the rewritten span. Enter and
//! Tab follow what CodeMirror's Markdown keymap does in NoteLiner today
//! (`insertNewlineContinueMarkup`, `indentWithTab`).
//!
//! The `*_in_text` functions apply an edit to a whole document and a
//! cursor given in characters (egui's `CCursor` index), so the editor can
//! hand them `TextEdit`'s buffer and cursor directly.

/// One indent step for Tab / Shift+Tab (CodeMirror's default `indentUnit`).
pub const INDENT: &str = "  ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    Ordered,
    Unordered,
    None,
    Mixed,
}

/// The rewritten span, as in `listEdits.js`: `{ lines, changed }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub lines: Vec<String>,
    pub changed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item<'a> {
    pub(crate) ordered: bool,
    pub(crate) indent: &'a str,
    /// The marker as written: `-`, `*`, `+`, `3.` or `3)`.
    pub(crate) marker: &'a str,
    pub(crate) text: &'a str,
    pub(crate) content_col: usize,
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

fn body_of(line: &str) -> &str {
    line.trim_start()
}

/// `UNORDERED_RE` / `ORDERED_RE`: a marker followed by whitespace or the end of the line.
pub(crate) fn parse_item(line: &str) -> Option<Item<'_>> {
    let indent = indent_of(line);
    let rest = &line[indent.len()..];
    let b = rest.as_bytes();
    let (ordered, marker_len) = match b.first() {
        Some(b'-' | b'*' | b'+') => (false, 1),
        _ => {
            let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
            if digits == 0 || !matches!(b.get(digits), Some(b'.' | b')')) {
                return None;
            }
            (true, digits + 1)
        }
    };
    let after = &rest[marker_len..];
    let text = after.trim_start();
    let space = after.len() - text.len();
    if space == 0 && !after.is_empty() {
        return None;
    }
    Some(Item {
        ordered,
        indent,
        marker: &rest[..marker_len],
        text,
        content_col: indent.chars().count() + marker_len + space.max(1),
    })
}

fn join_item(indent: &str, marker: &str, text: &str) -> String {
    if text.is_empty() {
        format!("{indent}{marker}")
    } else {
        format!("{indent}{marker} {text}")
    }
}

fn slice_span<'a>(lines: &[&'a str], from_line: usize, to_line: usize) -> Vec<&'a str> {
    if lines.is_empty() {
        return Vec::new();
    }
    let from = from_line.clamp(1, lines.len());
    let to = to_line.min(lines.len()).max(from);
    lines[from - 1..to].to_vec()
}

fn result(before: &[&str], after: Vec<String>) -> Edit {
    let changed = after.len() != before.len() || after.iter().zip(before).any(|(a, b)| a != b);
    Edit {
        lines: after,
        changed,
    }
}

pub fn classify_span(lines: &[&str], from_line: usize, to_line: usize) -> ListKind {
    let (mut ordered, mut unordered, mut plain) = (false, false, false);
    for line in slice_span(lines, from_line, to_line) {
        if is_blank(line) {
            continue;
        }
        match parse_item(line) {
            None => plain = true,
            Some(item) if item.ordered => ordered = true,
            Some(_) => unordered = true,
        }
    }
    match (ordered, unordered, plain) {
        (false, false, _) => ListKind::None,
        (true, false, false) => ListKind::Ordered,
        (false, true, false) => ListKind::Unordered,
        _ => ListKind::Mixed,
    }
}

fn strip_span(span: &[&str]) -> Vec<String> {
    span.iter()
        .map(|line| match parse_item(line).filter(|_| !is_blank(line)) {
            Some(item) => format!("{}{}", item.indent, item.text),
            None => line.to_string(),
        })
        .collect()
}

pub fn to_unordered_list(lines: &[&str], from_line: usize, to_line: usize) -> Edit {
    let span = slice_span(lines, from_line, to_line);
    if classify_span(lines, from_line, to_line) == ListKind::Unordered {
        return result(&span, strip_span(&span));
    }
    let next = span
        .iter()
        .map(|line| {
            if is_blank(line) {
                return line.to_string();
            }
            match parse_item(line) {
                Some(item) => join_item(item.indent, "-", item.text),
                None => join_item(indent_of(line), "-", body_of(line)),
            }
        })
        .collect();
    result(&span, next)
}

pub fn to_ordered_list(lines: &[&str], from_line: usize, to_line: usize) -> Edit {
    let span = slice_span(lines, from_line, to_line);
    if classify_span(lines, from_line, to_line) == ListKind::Ordered {
        return result(&span, strip_span(&span));
    }
    // One counter per indent level; returning to a shallower level discards deeper ones.
    let mut counters: Vec<(usize, usize)> = Vec::new();
    let next = span
        .iter()
        .map(|line| {
            if is_blank(line) {
                return line.to_string();
            }
            let item = parse_item(line);
            let indent = item.as_ref().map_or(indent_of(line), |i| i.indent);
            let text = item.as_ref().map_or(body_of(line), |i| i.text);
            let width = indent.chars().count();
            while counters.last().is_some_and(|c| c.0 > width) {
                counters.pop();
            }
            let n = match counters.last_mut() {
                Some(top) if top.0 == width => {
                    top.1 += 1;
                    top.1
                }
                _ => {
                    counters.push((width, 1));
                    1
                }
            };
            join_item(indent, &format!("{n}."), text)
        })
        .collect();
    result(&span, next)
}

/// The nearest earlier item at the same indent that is still in this list.
fn preceding_sibling<'a>(lines: &[&'a str], idx: usize, width: usize) -> Option<Item<'a>> {
    for line in lines[..idx].iter().rev() {
        if is_blank(line) {
            continue;
        }
        let item = parse_item(line)?;
        let w = item.indent.chars().count();
        if w == width {
            return Some(item);
        }
        if w < width {
            return None;
        }
    }
    None
}

pub fn indent_as_sub_list(lines: &[&str], line: usize) -> Edit {
    if line == 0 || line > lines.len() {
        return Edit {
            lines: Vec::new(),
            changed: false,
        };
    }
    let idx = line - 1;
    let current = lines[idx];
    let unchanged = Edit {
        lines: vec![current.to_string()],
        changed: false,
    };
    if is_blank(current) {
        return unchanged;
    }
    let Some(item) = parse_item(current) else {
        return result(
            &[current],
            vec![join_item(indent_of(current), "-", body_of(current))],
        );
    };
    let Some(parent) = preceding_sibling(lines, idx, item.indent.chars().count()) else {
        return unchanged;
    };
    let marker = if item.ordered { "1." } else { "-" };
    result(
        &[current],
        vec![join_item(
            &" ".repeat(parent.content_col),
            marker,
            item.text,
        )],
    )
}

// --- Whole-document edits with a character cursor -------------------------------------------

/// A document edit: the new text and the new cursor (anchor, head), in characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocEdit {
    pub text: String,
    pub anchor: usize,
    pub head: usize,
}

/// Byte offset of character `ci`.
pub fn byte_at(text: &str, ci: usize) -> usize {
    // Most notes are mostly ASCII: when the prefix is, characters are bytes
    // (checked a word at a time, far faster than walking `char_indices`).
    if let Some(prefix) = text.as_bytes().get(..ci) {
        if prefix.is_ascii() {
            return ci;
        }
    }
    text.char_indices().nth(ci).map_or(text.len(), |(b, _)| b)
}

pub fn chars_in(s: &str) -> usize {
    s.chars().count()
}

/// The 0-based line holding byte `b`, and that line's byte start.
pub fn line_of(text: &str, b: usize) -> (usize, usize) {
    let before = &text[..b];
    let line = crate::fastcmp::newlines(before.as_bytes());
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, start)
}

/// Enter: continue a list item or blockquote, or end the list on an empty
/// item: Enter twice in a row ends the list (or quote) and starts a new line
/// after a blank one, so the next paragraph isn't read as part of the last
/// item. An empty nested item moves out one level instead. `None` means "not
/// ours, insert a plain newline".
pub fn enter_in_text(text: &str, cursor: usize) -> Option<DocEdit> {
    let b = byte_at(text, cursor);
    let (_, start) = line_of(text, b);
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let line = &text[start..end];
    let col = b - start;

    let quote_len = quote_prefix_len(line);
    let (quote, rest) = line.split_at(quote_len);
    let item = parse_item(rest);
    let content_start = match &item {
        Some(it) => {
            quote_len
                + it.indent.len()
                + it.marker.len()
                + (rest.len() - it.indent.len() - it.marker.len() - it.text.len())
        }
        None if quote_len > 0 => quote_len,
        None => return None,
    };
    let (task, content_start) = match &item {
        Some(it) if task_box(it.text) => (
            true,
            content_start + 3 + usize::from(it.text[3..].starts_with(' ')),
        ),
        _ => (false, content_start),
    };
    if col < content_start.min(line.len()) {
        return None;
    }
    let content = line[content_start.min(line.len())..].trim();

    // Empty item: end the list with a blank line (or outdent a nested item
    // one level).
    if content.is_empty() && col >= line.trim_end().len() {
        let replacement = match &item {
            Some(it) if !it.indent.is_empty() => {
                let lines: Vec<&str> = text.split('\n').collect();
                let (idx, _) = line_of(text, start);
                let width = it.indent.chars().count();
                let parent_indent = lines[..idx]
                    .iter()
                    .rev()
                    .filter(|l| !is_blank(l))
                    .find_map(|l| {
                        parse_item(&l[quote_prefix_len(l)..])
                            .map(|p| p.indent)
                            .filter(|p| p.chars().count() < width)
                    })
                    .unwrap_or("");
                format!("{quote}{parent_indent}{} ", it.marker)
            }
            // The empty item's line becomes the blank line; the caret goes to
            // the new line after it.
            _ => "\n".to_string(),
        };
        let new = format!("{}{}{}", &text[..start], replacement, &text[end..]);
        let cursor = chars_in(&text[..start]) + chars_in(&replacement);
        return Some(DocEdit {
            text: new,
            anchor: cursor,
            head: cursor,
        });
    }

    let mut prefix = quote.to_string();
    if let Some(it) = &item {
        prefix.push_str(it.indent);
        prefix.push_str(&next_marker(it.marker));
        prefix.push(' ');
        if task {
            prefix.push_str("[ ] ");
        }
    }
    let tail = text[b..end].trim_start();
    // Trailing spaces before the split go, but not the space after the marker.
    let before = if col > content_start {
        text[..b].trim_end_matches(' ')
    } else {
        &text[..b]
    };
    let new = format!("{before}\n{prefix}{tail}{}", &text[end..]);
    let cursor = chars_in(before) + 1 + chars_in(&prefix);
    Some(DocEdit {
        text: new,
        anchor: cursor,
        head: cursor,
    })
}

pub(crate) fn quote_prefix_len(line: &str) -> usize {
    let mut i = 0;
    let indent = line.len() - line.trim_start().len();
    if !line[indent..].starts_with('>') {
        return 0;
    }
    i += indent;
    while line[i..].starts_with('>') {
        i += 1;
        if line[i..].starts_with(' ') {
            i += 1;
        }
    }
    i
}

pub(crate) fn task_box(text: &str) -> bool {
    (text.starts_with("[ ]") || text.starts_with("[x]") || text.starts_with("[X]"))
        && (text.len() == 3 || text[3..].starts_with(' '))
}

/// `-` stays `-`; `3.` becomes `4.`; `3)` becomes `4)`.
fn next_marker(marker: &str) -> String {
    match marker.strip_suffix(['.', ')']) {
        Some(digits) => {
            let n: u64 = digits.parse().unwrap_or(0);
            format!("{}{}", n + 1, &marker[digits.len()..])
        }
        None => marker.to_string(),
    }
}

/// Tab / Shift+Tab: indent or outdent every line the selection touches by
/// [`INDENT`], keeping the cursor on the same characters.
pub fn indent_in_text(text: &str, anchor: usize, head: usize, outdent: bool) -> Option<DocEdit> {
    let (a, h) = (byte_at(text, anchor), byte_at(text, head));
    let (from, to) = (a.min(h), a.max(h));
    let (_, start) = line_of(text, from);
    // A selection ending at column 0 doesn't include that line.
    let to = if to > from && text[..to].ends_with('\n') {
        to - 1
    } else {
        to
    };
    let end = text[to..].find('\n').map_or(text.len(), |i| to + i);
    let mut out = String::with_capacity(text.len() + 64);
    out.push_str(&text[..start]);
    // Character deltas per line start, to move both ends of the cursor.
    let mut shifts: Vec<(usize, isize)> = Vec::new();
    let mut line_start_chars = chars_in(&text[..start]);
    for (i, line) in text[start..end].split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let delta = if outdent {
            let n = if line.starts_with('\t') {
                1
            } else {
                line.bytes()
                    .take(INDENT.len())
                    .take_while(|&c| c == b' ')
                    .count()
            };
            out.push_str(&line[n..]);
            -(n as isize)
        } else if line.is_empty() {
            0
        } else {
            out.push_str(INDENT);
            INDENT.len() as isize
        };
        if !outdent {
            out.push_str(line);
        }
        shifts.push((line_start_chars, delta));
        line_start_chars += chars_in(line) + 1;
    }
    out.push_str(&text[end..]);
    if out == text {
        return None;
    }
    let move_cursor = |c: usize| {
        let (line_start, delta) = shifts
            .iter()
            .rev()
            .find(|(s, _)| *s <= c)
            .copied()
            .unwrap_or((0, 0));
        let col = c - line_start;
        (line_start as isize + (col as isize + delta).max(0)) as usize
    };
    Some(DocEdit {
        text: out,
        anchor: move_cursor(anchor),
        head: move_cursor(head),
    })
}

/// A toolbar list command (`applyListEdit` in `Editor.svelte`): rewrites the
/// lines the selection touches. With a bare caret, the caret keeps its
/// character; with a selection, the rewritten lines end up selected.
pub fn span_in_text(
    text: &str,
    anchor: usize,
    head: usize,
    f: fn(&[&str], usize, usize) -> Edit,
) -> Option<DocEdit> {
    let (a, h) = (byte_at(text, anchor), byte_at(text, head));
    let (from, to) = (a.min(h), a.max(h));
    let lines: Vec<&str> = text.split('\n').collect();
    let (from_line, start) = line_of(text, from);
    let (to_line, _) = line_of(text, to);
    let edit = f(&lines, from_line + 1, to_line + 1);
    if !edit.changed {
        return None;
    }
    let end = text[to..].find('\n').map_or(text.len(), |i| to + i);
    let insert = edit.lines.join("\n");
    let new = format!("{}{}{}", &text[..start], insert, &text[end..]);
    let start_c = chars_in(&text[..start]);
    if anchor == head {
        let col = anchor - start_c;
        let delta = chars_in(&edit.lines[0]) as isize - chars_in(lines[from_line]) as isize;
        let c =
            start_c + (col as isize + delta).clamp(0, chars_in(&edit.lines[0]) as isize) as usize;
        Some(DocEdit {
            text: new,
            anchor: c,
            head: c,
        })
    } else {
        Some(DocEdit {
            text: new,
            anchor: start_c,
            head: start_c + chars_in(&insert),
        })
    }
}

/// The toolbar's "indent as sub-list": nests the caret's line under the item above.
pub fn sub_list_in_text(text: &str, head: usize) -> Option<DocEdit> {
    let b = byte_at(text, head);
    let (line, start) = line_of(text, b);
    let lines: Vec<&str> = text.split('\n').collect();
    let edit = indent_as_sub_list(&lines, line + 1);
    if !edit.changed {
        return None;
    }
    let end = start + lines[line].len();
    let insert = &edit.lines[0];
    let col = head - chars_in(&text[..start]);
    let delta = chars_in(insert) as isize - chars_in(lines[line]) as isize;
    let c = chars_in(&text[..start])
        + (col as isize + delta).clamp(0, chars_in(insert) as isize) as usize;
    Some(DocEdit {
        text: format!("{}{}{}", &text[..start], insert, &text[end..]),
        anchor: c,
        head: c,
    })
}

// --- Renumbering ordered lists after an edit ----------------------------------------------------

/// An ordered item's place in a list: its quote prefix, indent width,
/// delimiter (`.` or `)`) and number, plus where its digits sit in the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ordered {
    quote: usize,
    indent: usize,
    delim: u8,
    number: u64,
    digits: (usize, usize),
}

/// What a line is, for walking a list's siblings.
enum Walk {
    Ordered(Ordered),
    /// A bullet item (its indent width).
    Bullet(usize),
    Blank,
    /// Anything else, with its indent width.
    Other(usize),
}

fn walk_line(line: &str) -> Walk {
    if is_blank(line) {
        return Walk::Blank;
    }
    let quote = quote_prefix_len(line);
    let rest = &line[quote..];
    match parse_item(rest) {
        Some(item) if item.ordered => {
            let start = quote + item.indent.len();
            let end = start + item.marker.len() - 1;
            Walk::Ordered(Ordered {
                quote,
                indent: item.indent.chars().count(),
                delim: item.marker.as_bytes()[item.marker.len() - 1],
                number: line[start..end].parse().unwrap_or(0),
                digits: (start, end),
            })
        }
        Some(item) => Walk::Bullet(item.indent.chars().count()),
        None => Walk::Other(indent_of(rest).chars().count()),
    }
}

/// The next sibling of the ordered item at `from` (same quote, indent and
/// delimiter), stepping by `dir` (+1 or -1): skips blank lines, deeper
/// items and continuation lines; stops where the list ends.
fn sibling(lines: &[&str], from: usize, level: Ordered, dir: isize) -> Option<(usize, Ordered)> {
    let mut i = from as isize;
    loop {
        i += dir;
        if i < 0 || i as usize >= lines.len() {
            return None;
        }
        let idx = i as usize;
        match walk_line(lines[idx]) {
            Walk::Blank => {}
            Walk::Ordered(o) if o.quote == level.quote && o.indent == level.indent => {
                return (o.delim == level.delim).then_some((idx, o));
            }
            Walk::Ordered(o) if o.indent > level.indent => {}
            Walk::Bullet(indent) if indent > level.indent => {}
            Walk::Ordered(_) | Walk::Bullet(_) => return None,
            Walk::Other(indent) if indent > level.indent => {}
            Walk::Other(_) => {
                // A lazy continuation line belongs to the item above it, but a
                // paragraph after a blank line (or a heading or fence) ends the list.
                let line = lines[idx].trim_start();
                let above_blank = idx == 0 || is_blank(lines[idx - 1]);
                if above_blank || line.starts_with('#') || line.starts_with("```") {
                    return None;
                }
            }
        }
    }
}

/// After an edit from `old` to `new`, renumbers the ordered items that
/// follow it so their run stays sequential (CodeMirror's `renumberList`,
/// for insertions and deletions alike): the first ordered item after the
/// changed lines becomes its previous sibling's number + 1, and so do the
/// following siblings, as long as they were sequential before the edit. A
/// list that wasn't sequential there (`1. 1. 1.`) is left alone. `None`
/// when nothing changes.
pub fn renumber_after_edit(old: &str, new: &str) -> Option<String> {
    if old == new {
        return None;
    }
    let new_lines: Vec<&str> = new.split('\n').collect();
    let old_lines: Vec<&str> = old.split('\n').collect();
    // Whole lines: the edit's lines are the ones between the common leading
    // and trailing lines.
    let same_start = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(a, b)| a == b)
        .count();
    let max_end = old_lines.len().min(new_lines.len()) - same_start;
    let same_end = old_lines
        .iter()
        .rev()
        .zip(new_lines.iter().rev())
        .take(max_end)
        .take_while(|(a, b)| a == b)
        .count();
    // The lines from `after` on are old lines shifted by `delta`.
    let after = new_lines.len() - same_end;
    let delta = new_lines.len() as isize - old_lines.len() as isize;

    // The first ordered item after the edit, at the level it sits at.
    let (first, level) = new_lines
        .iter()
        .enumerate()
        .skip(after)
        .find_map(|(i, l)| match walk_line(l) {
            Walk::Blank | Walk::Other(_) => None,
            Walk::Ordered(o) => Some(Some((i, o))),
            Walk::Bullet(_) => Some(None),
        })
        .flatten()?;
    let (_, prev) = sibling(&new_lines, first, level, -1)?;
    let old_first = (first as isize - delta) as usize;
    let Walk::Ordered(old_level) = walk_line(old_lines.get(old_first)?) else {
        return None;
    };
    let (_, old_prev) = sibling(&old_lines, old_first, old_level, -1)?;
    // Only a run that was sequential before the edit is renumbered.
    if old_level.number != old_prev.number + 1 || level.number == prev.number + 1 {
        return None;
    }

    let mut out: Vec<String> = new_lines.iter().map(|l| l.to_string()).collect();
    let (mut at, mut item) = (first, level);
    let (mut old_at, mut old_item) = (old_first, old_level);
    let mut number = prev.number + 1;
    loop {
        let line = new_lines[at];
        out[at] = format!(
            "{}{number}{}",
            &line[..item.digits.0],
            &line[item.digits.1..]
        );
        let Some((next, next_item)) = sibling(&new_lines, at, item, 1) else {
            break;
        };
        let Some((old_next, old_next_item)) = sibling(&old_lines, old_at, old_item, 1) else {
            break;
        };
        // The same line (shifted by the edit) and still in sequence before it.
        if old_next as isize + delta != next as isize || old_next_item.number != old_item.number + 1
        {
            break;
        }
        (at, item, old_at, old_item) = (next, next_item, old_next, old_next_item);
        number += 1;
    }
    let text = out.join("\n");
    (text != new).then_some(text)
}

/// Applies [`renumber_after_edit`] to an edit of `old`. Renumbering only
/// rewrites lines after the edit, so the cursor stays put.
pub fn with_renumbering(old: &str, mut edit: DocEdit) -> DocEdit {
    if let Some(text) = renumber_after_edit(old, &edit.text) {
        edit.text = text;
    }
    edit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<&str> {
        s.split('\n').collect()
    }

    /// `|` marks the cursor.
    fn enter(doc: &str) -> String {
        let cursor = doc.find('|').unwrap();
        let text = doc.replace('|', "");
        let c = text[..cursor].chars().count();
        match enter_in_text(&text, c) {
            Some(e) => {
                let b = byte_at(&e.text, e.head);
                format!("{}|{}", &e.text[..b], &e.text[b..])
            }
            None => "<plain>".into(),
        }
    }

    // Cases from tests/integration/list-edits.test.mjs.
    #[test]
    fn classify_and_toggle() {
        assert_eq!(classify_span(&lines("- a\n- b"), 1, 2), ListKind::Unordered);
        assert_eq!(
            classify_span(&lines("1. a\n\n2. b"), 1, 3),
            ListKind::Ordered
        );
        assert_eq!(classify_span(&lines("- a\ntext"), 1, 2), ListKind::Mixed);
        assert_eq!(classify_span(&lines("---\n-Alpha"), 1, 2), ListKind::None);
        assert_eq!(
            to_unordered_list(&lines("a\n  b\n\n* c"), 1, 4).lines,
            ["- a", "  - b", "", "- c"]
        );
        assert_eq!(
            to_unordered_list(&lines("- a\n- b"), 1, 2).lines,
            ["a", "b"]
        );
        assert_eq!(
            to_ordered_list(&lines("a\n  b\n  c\nd\n  e"), 1, 5).lines,
            ["1. a", "  1. b", "  2. c", "2. d", "  1. e"]
        );
        assert_eq!(to_ordered_list(&lines("3) a"), 1, 1).lines, ["a"]);
        assert!(!to_unordered_list(&lines(""), 1, 1).changed);
    }

    #[test]
    fn indent_sub_list_uses_parent_content_column() {
        assert_eq!(
            indent_as_sub_list(&lines("1. Alpha\n2. Beta"), 2).lines,
            ["   1. Beta"]
        );
        assert_eq!(indent_as_sub_list(&lines("- a\n- b"), 2).lines, ["  - b"]);
        assert!(!indent_as_sub_list(&lines("- a"), 1).changed);
        assert!(!indent_as_sub_list(&lines("text\n- a"), 2).changed);
        assert_eq!(indent_as_sub_list(&lines("plain"), 1).lines, ["- plain"]);
    }

    #[test]
    fn enter_continues_items() {
        assert_eq!(enter("- one|"), "- one\n- |");
        assert_eq!(enter("* one|"), "* one\n* |");
        assert_eq!(enter("  - one|\nnext"), "  - one\n  - |\nnext");
        assert_eq!(enter("9. one|"), "9. one\n10. |");
        assert_eq!(enter("1) one|"), "1) one\n2) |");
        assert_eq!(enter("- [x] done|"), "- [x] done\n- [ ] |");
        assert_eq!(enter("> quoted|"), "> quoted\n> |");
        assert_eq!(enter("> - in quote|"), "> - in quote\n> - |");
        assert_eq!(enter("- spl|it"), "- spl\n- |it");
        assert_eq!(enter("plain|"), "<plain>");
        assert_eq!(enter("-|- rule"), "<plain>");
        assert_eq!(enter("é - ü|"), "<plain>");
        assert_eq!(enter("- ü|"), "- ü\n- |");
    }

    #[test]
    fn enter_on_empty_item_ends_or_outdents() {
        // Enter twice ends the list: a blank line, then the caret on a new one.
        assert_eq!(enter("- a\n- |"), "- a\n\n|");
        assert_eq!(enter("1. a\n2. |"), "1. a\n\n|");
        assert_eq!(enter("- a\n- [ ] |"), "- a\n\n|");
        assert_eq!(enter("- a\n- |\nnext"), "- a\n\n|\nnext");
        // A nested empty item moves out one level instead.
        assert_eq!(enter("- a\n  - b\n  - |"), "- a\n  - b\n- |");
        assert_eq!(enter("> a\n> |"), "> a\n\n|");
        assert_eq!(enter("- |abc"), "- \n- |abc");
    }

    #[test]
    fn tab_indents_selected_lines() {
        let e = indent_in_text("- a\n- b\n- c", 4, 6, false).unwrap();
        assert_eq!(e.text, "- a\n  - b\n- c");
        assert_eq!((e.anchor, e.head), (6, 8));
        let e = indent_in_text("- a\n  - b\n- c", 0, 9, false).unwrap();
        assert_eq!(e.text, "  - a\n    - b\n- c");
        let e = indent_in_text("  - a\n\t- b", 0, 9, true).unwrap();
        assert_eq!(e.text, "- a\n- b");
        assert_eq!(indent_in_text("- a", 0, 0, true), None);
        let e = indent_in_text(" x", 1, 1, true).unwrap();
        assert_eq!((e.text.as_str(), e.head), ("x", 0));
    }

    #[test]
    fn toolbar_commands_keep_the_caret() {
        let e = span_in_text("alpha\nbeta", 8, 8, to_unordered_list).unwrap();
        assert_eq!(e.text, "alpha\n- beta");
        assert_eq!(e.head, 10);
        let e = span_in_text("a\nb", 0, 3, to_ordered_list).unwrap();
        assert_eq!((e.text.as_str(), e.anchor, e.head), ("1. a\n2. b", 0, 9));
        let e = sub_list_in_text("- a\n- b", 7).unwrap();
        assert_eq!((e.text.as_str(), e.head), ("- a\n  - b", 9));
    }

    fn renum(old: &str, new: &str) -> String {
        renumber_after_edit(old, new).unwrap_or_else(|| "<none>".into())
    }

    #[test]
    fn renumbers_after_insertions_and_deletions() {
        // Enter after item 1 inserts "2.": the rest move up by one.
        assert_eq!(
            renum("1. a\n2. b\n3. c", "1. a\n2. x\n2. b\n3. c"),
            "1. a\n2. x\n3. b\n4. c"
        );
        // Deleting item 2: the rest move down.
        assert_eq!(
            renum("1. a\n2. b\n3. c\n4. d", "1. a\n3. c\n4. d"),
            "1. a\n2. c\n3. d"
        );
        // Pasting two items shifts by two; widths can change.
        assert_eq!(
            renum("1. a\n2. b\n3. c", "1. a\n2. p\n3. q\n2. b\n3. c"),
            "1. a\n2. p\n3. q\n4. b\n5. c"
        );
        assert_eq!(renum("8. a\n9. b", "8. a\n9. x\n9. b"), "8. a\n9. x\n10. b");
        // A list starting at 5 keeps its numbering.
        assert_eq!(renum("5. a\n6. b", "5. a\n6. x\n6. b"), "5. a\n6. x\n7. b");
    }

    #[test]
    fn renumbering_respects_levels_delimiters_and_quotes() {
        // Only the nested list where the edit happened.
        assert_eq!(
            renum(
                "1. a\n   1. x\n   2. y\n2. b\n3. c",
                "1. a\n   1. x\n   2. n\n   2. y\n2. b\n3. c"
            ),
            "1. a\n   1. x\n   2. n\n   3. y\n2. b\n3. c"
        );
        // Tab nests item 2: the top level closes the gap.
        assert_eq!(
            renum("1. a\n2. b\n3. c", "1. a\n  2. b\n3. c"),
            "1. a\n  2. b\n2. c"
        );
        // `)` lists renumber; a `.` item after them is another list.
        assert_eq!(
            renum("1) a\n2) b\n3. c", "1) a\n2) x\n2) b\n3. c"),
            "1) a\n2) x\n3) b\n3. c"
        );
        assert_eq!(
            renum("> 1. a\n> 2. b", "> 1. a\n> 2. x\n> 2. b"),
            "> 1. a\n> 2. x\n> 3. b"
        );
        // Continuation lines and nested bullets don't break the run.
        assert_eq!(
            renum(
                "1. a\n   more\n   - sub\n2. b\n3. c",
                "1. a\n   more\n   - sub\n2. x\n2. b\n3. c"
            ),
            "1. a\n   more\n   - sub\n2. x\n3. b\n4. c"
        );
    }

    #[test]
    fn leaves_lazy_and_separate_lists_alone() {
        assert_eq!(
            renum("1. a\n1. b\n1. c", "1. a\n2. x\n1. b\n1. c"),
            "<none>"
        );
        // A paragraph after a blank line starts a new list.
        assert_eq!(
            renum("1. a\n\nText\n\n2. b", "1. a\n2. x\n\nText\n\n2. b"),
            "<none>"
        );
        assert_eq!(renum("- a\n- b", "- a\n- x\n- b"), "<none>");
        assert_eq!(renum("plain", "plain text"), "<none>");
        // Already sequential: nothing to do.
        assert_eq!(renum("1. a\n2. b", "1. a\n2. b\n3. c"), "<none>");
    }

    #[test]
    fn enter_and_tab_renumber_through_the_text_helpers() {
        let old = "1. a\n2. b\n3. c";
        let e = with_renumbering(old, enter_in_text(old, 4).unwrap());
        assert_eq!(e.text, "1. a\n2. \n3. b\n4. c");
        assert_eq!((e.anchor, e.head), (8, 8));
        let e = with_renumbering(old, indent_in_text(old, 6, 6, false).unwrap());
        assert_eq!(e.text, "1. a\n  2. b\n2. c");
    }
}
