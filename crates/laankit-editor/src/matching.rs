//! Decorations and navigation that read the text but don't change it:
//! bracket matching (CodeMirror's `bracketMatching`), other occurrences of
//! the selection (`highlightSelectionMatches`), and parsing the Go to line
//! input (`gotoLine`). Pure functions over text and byte offsets, so they
//! work for any editor engine; the widget only paints what they return.

use std::ops::Range;

/// How far to look for a matching bracket (CodeMirror's `maxScanDistance`).
pub const MAX_BRACKET_SCAN: usize = 10_000;
/// At most this many selection matches are reported (`maxMatches`).
pub const MAX_SELECTION_MATCHES: usize = 100;
/// Longer selections aren't searched for.
const MAX_QUERY_CHARS: usize = 100;

/// The bracket at the caret and its partner, as byte offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BracketMatch {
    /// The bracket next to the caret.
    pub at: usize,
    /// Its partner, or `None` when it has none (shown as unmatched).
    pub other: Option<usize>,
}

/// The bracket pair around a caret at byte `caret`: the bracket just after
/// the caret, else the one just before it (CodeMirror's order). Brackets
/// inside inline code spans are skipped while scanning, unless the starting
/// bracket is in one itself.
pub fn bracket_match(text: &str, caret: usize) -> Option<BracketMatch> {
    let bytes = text.as_bytes();
    let candidates = [Some(caret), caret.checked_sub(1)];
    let at = candidates
        .into_iter()
        .flatten()
        .find(|&i| i < bytes.len() && pair(bytes[i]).is_some())?;
    let (open, close, forward) = pair(bytes[at])?;
    let in_code = |b: usize| inline_code_at(text, b);
    let skip_code = !in_code(at);
    let mut depth = 0usize;
    let mut i = at;
    for _ in 0..MAX_BRACKET_SCAN {
        i = if forward {
            i + 1
        } else {
            match i.checked_sub(1) {
                Some(j) => j,
                None => break,
            }
        };
        if i >= bytes.len() {
            break;
        }
        let c = bytes[i];
        if c != open && c != close {
            continue;
        }
        if skip_code && in_code(i) {
            continue;
        }
        if c == open {
            depth += 1;
        } else if depth == 0 {
            return Some(BracketMatch { at, other: Some(i) });
        } else {
            depth -= 1;
        }
    }
    Some(BracketMatch { at, other: None })
}

/// (opening, closing, scan forward) for a bracket byte.
fn pair(c: u8) -> Option<(u8, u8, bool)> {
    match c {
        b'(' => Some((b'(', b')', true)),
        b'[' => Some((b'[', b']', true)),
        b'{' => Some((b'{', b'}', true)),
        b')' => Some((b')', b'(', false)),
        b']' => Some((b']', b'[', false)),
        b'}' => Some((b'}', b'{', false)),
        _ => None,
    }
}

/// Whether byte `b` is inside a `` `code` `` span on its line.
fn inline_code_at(text: &str, b: usize) -> bool {
    let start = text[..b].rfind('\n').map_or(0, |i| i + 1);
    let end = text[b..].find('\n').map_or(text.len(), |i| b + i);
    inline_code_ranges(&text[start..end])
        .iter()
        .any(|r| r.contains(&(b - start)))
}

/// The byte ranges of the line's `` `code` `` spans, backticks included.
pub fn inline_code_ranges(line: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(open) = line[i..].find('`') {
        let s = i + open;
        match line[s + 1..].find('`') {
            Some(close) => {
                let e = s + 1 + close + 1;
                out.push(s..e);
                i = e;
            }
            None => break,
        }
    }
    out
}

/// What to look for when the selection `text[from..to]` is selected: the
/// selected text and whether matches must be whole words. `None` for an
/// empty, multi-line or long selection, or one with whitespace.
pub fn selection_query(text: &str, from: usize, to: usize) -> Option<(&str, bool)> {
    let (from, to) = (from.min(to), from.max(to));
    let q = text.get(from..to)?;
    if q.is_empty() || q.chars().count() > MAX_QUERY_CHARS || q.chars().any(char::is_whitespace) {
        return None;
    }
    let word = q.chars().all(is_word_char);
    // A word only highlights whole-word occurrences, and only when the
    // selection is itself the whole word (CodeMirror's `wholeWords`).
    if word {
        let before = text[..from].chars().next_back();
        let after = text[to..].chars().next();
        if before.is_some_and(is_word_char) || after.is_some_and(is_word_char) {
            return None;
        }
    }
    Some((q, word))
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The byte ranges of `query` in `line` (whole words only if `whole_word`).
pub fn find_in_line(line: &str, query: &str, whole_word: bool) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    if query.is_empty() {
        return out;
    }
    let mut i = 0;
    while let Some(off) = line[i..].find(query) {
        let s = i + off;
        let e = s + query.len();
        let ok = !whole_word
            || (!line[..s].chars().next_back().is_some_and(is_word_char)
                && !line[e..].chars().next().is_some_and(is_word_char));
        if ok {
            out.push(s..e);
        }
        i = s + query.chars().next().map_or(1, char::len_utf8);
    }
    out
}

/// A Go to line request, from CodeMirror's `gotoLine` input: `42`, `42:7`
/// (line and column), `+5` / `-5` (relative to the caret's line) or `50%`
/// (of the document). Lines and columns are 1-based; the result is clamped
/// to the document. `current` is the caret's 1-based line.
pub fn parse_goto(
    input: &str,
    current: usize,
    line_count: usize,
) -> Option<(usize, Option<usize>)> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    let last = line_count.max(1);
    let (line_part, col) = match s.split_once(':') {
        Some((l, c)) => (l.trim(), Some(c.trim().parse::<usize>().ok()?.max(1))),
        None => (s, None),
    };
    let line = if let Some(pct) = line_part.strip_suffix('%') {
        let pct: f64 = pct.trim().parse().ok()?;
        ((pct.clamp(0.0, 100.0) / 100.0) * last as f64).round() as usize
    } else if let Some(n) = line_part.strip_prefix('+') {
        current.saturating_add(n.trim().parse::<usize>().ok()?)
    } else if let Some(n) = line_part.strip_prefix('-') {
        current.saturating_sub(n.trim().parse::<usize>().ok()?)
    } else {
        line_part.parse::<usize>().ok()?
    };
    Some((line.clamp(1, last), col))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bm(doc: &str) -> Option<(usize, Option<usize>)> {
        let caret = doc.find('|').unwrap();
        let text = doc.replacen('|', "", 1);
        bracket_match(&text, caret).map(|m| (m.at, m.other))
    }

    #[test]
    fn brackets_after_then_before_the_caret() {
        assert_eq!(bm("a |(b [c]) d"), Some((2, Some(8))));
        assert_eq!(bm("a (b [c])| d"), Some((8, Some(2))));
        assert_eq!(bm("a (b [|c]) d"), Some((5, Some(7))));
        assert_eq!(bm("{\n  x: [1, 2],\n|}"), Some((15, Some(0))));
        assert_eq!(bm("no| brackets"), None);
        // Unmatched.
        assert_eq!(bm("|(open"), Some((0, None)));
        assert_eq!(bm("close)|"), Some((5, None)));
    }

    #[test]
    fn brackets_in_inline_code_are_skipped() {
        // The `)` inside code doesn't close the outer `(`.
        assert_eq!(bm("|(see `f(x)` here)"), Some((0, Some(16))));
        // But a bracket in code matches within it.
        assert_eq!(bm("`f|(x)` (y)"), Some((2, Some(4))));
        assert_eq!(inline_code_ranges("a `b` c `d"), vec![2..5]);
    }

    #[test]
    fn selection_queries() {
        let text = "let foo = foo_bar + foo; a->b ->";
        assert_eq!(selection_query(text, 4, 7), Some(("foo", true)));
        // Part of a word: nothing.
        assert_eq!(selection_query(text, 4, 6), None);
        assert_eq!(selection_query(text, 26, 28), Some(("->", false)));
        assert_eq!(selection_query(text, 3, 7), None, "whitespace");
        assert_eq!(selection_query(text, 4, 4), None);
        assert_eq!(selection_query("a\nb", 0, 3), None);
    }

    #[test]
    fn finds_whole_words_or_substrings() {
        let line = "foo foo_bar barfoo foo. (foo)";
        assert_eq!(find_in_line(line, "foo", true), vec![0..3, 19..22, 25..28]);
        assert_eq!(find_in_line(line, "foo", false).len(), 5);
        assert_eq!(find_in_line("é é", "é", true), vec![0..2, 3..5]);
        assert_eq!(find_in_line("aaa", "aa", false), vec![0..2, 1..3]);
    }

    #[test]
    fn goto_forms() {
        assert_eq!(parse_goto("42", 1, 100), Some((42, None)));
        assert_eq!(parse_goto(" 42:7 ", 1, 100), Some((42, Some(7))));
        assert_eq!(parse_goto("+5", 10, 100), Some((15, None)));
        assert_eq!(parse_goto("-5", 3, 100), Some((1, None)));
        assert_eq!(parse_goto("50%", 1, 200), Some((100, None)));
        assert_eq!(parse_goto("999", 1, 100), Some((100, None)));
        assert_eq!(parse_goto("0", 5, 100), Some((1, None)));
        assert_eq!(parse_goto("abc", 1, 100), None);
        assert_eq!(parse_goto("", 1, 100), None);
    }
}
