//! Text operations the core engine's commands are made of, over plain
//! strings: word boundaries within a line (CodeMirror's "groups": a run of
//! word characters, of punctuation, or of spaces) and replacing several
//! ranges in one pass.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Punct,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Punct
    }
}

/// The column after the next group right of `col` (in characters).
pub fn word_right(line: &str, col: usize) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let mut i = col.min(chars.len());
    while i < chars.len() && class(chars[i]) == Class::Space {
        i += 1;
    }
    if i < chars.len() {
        let c = class(chars[i]);
        while i < chars.len() && class(chars[i]) == c {
            i += 1;
        }
    }
    i
}

/// The column at the start of the group left of `col`.
pub fn word_left(line: &str, col: usize) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let mut i = col.min(chars.len());
    while i > 0 && class(chars[i - 1]) == Class::Space {
        i -= 1;
    }
    if i > 0 {
        let c = class(chars[i - 1]);
        while i > 0 && class(chars[i - 1]) == c {
            i -= 1;
        }
    }
    i
}

/// The group around column `col` (a double-click selects it): the word,
/// punctuation run or space run under or just before the caret.
pub fn group_at(line: &str, col: usize) -> Range<usize> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return 0..0;
    }
    let at = if col < chars.len() {
        col
    } else {
        chars.len() - 1
    };
    // Prefer the word before the caret when the caret sits just after one.
    let at = if col > 0
        && col <= chars.len()
        && (col == chars.len() || class(chars[col]) != Class::Word)
        && class(chars[col - 1]) == Class::Word
    {
        col - 1
    } else {
        at
    };
    let c = class(chars[at]);
    let mut s = at;
    while s > 0 && class(chars[s - 1]) == c {
        s -= 1;
    }
    let mut e = at + 1;
    while e < chars.len() && class(chars[e]) == c {
        e += 1;
    }
    s..e
}

/// The whole word around `col`, if the caret touches one (for Ctrl/Cmd+D on
/// an empty selection).
pub fn word_at(line: &str, col: usize) -> Option<Range<usize>> {
    let r = group_at(line, col);
    let first = line.chars().nth(r.start)?;
    (class(first) == Class::Word).then_some(r)
}

/// The column of the first non-space character (the smart Home target).
pub fn first_non_space(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// Replaces character ranges (sorted, non-overlapping) with `reps`, given a
/// char→byte map of `text`. Returns the new text and, for each replacement,
/// its new range in characters.
pub fn replace_ranges(
    text: &str,
    ranges: &[Range<usize>],
    reps: &[String],
    char_to_byte: impl Fn(usize) -> usize,
) -> (String, Vec<Range<usize>>) {
    let extra: usize = reps.iter().map(String::len).sum();
    let mut out = String::with_capacity(text.len() + extra);
    let mut at = 0;
    let mut delta: isize = 0;
    let mut placed = Vec::with_capacity(ranges.len());
    for (r, rep) in ranges.iter().zip(reps) {
        let (bf, bt) = (char_to_byte(r.start), char_to_byte(r.end));
        out.push_str(&text[at..bf]);
        out.push_str(rep);
        at = bt;
        let n = rep.chars().count();
        let start = (r.start as isize + delta) as usize;
        placed.push(start..start + n);
        delta += n as isize - (r.end - r.start) as isize;
    }
    out.push_str(&text[at..]);
    (out, placed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c2b(text: &str) -> impl Fn(usize) -> usize + '_ {
        move |c| text.char_indices().nth(c).map_or(text.len(), |(b, _)| b)
    }

    #[test]
    fn words_move_by_group() {
        let line = "foo.bar  baz_1 ü";
        assert_eq!(word_right(line, 0), 3);
        assert_eq!(word_right(line, 3), 4);
        assert_eq!(word_right(line, 7), 14);
        assert_eq!(word_right(line, 14), 16);
        assert_eq!(word_left(line, 16), 15);
        assert_eq!(word_left(line, 14), 9);
        assert_eq!(word_left(line, 9), 4);
        assert_eq!(word_left(line, 0), 0);
    }

    #[test]
    fn groups_and_words_under_the_caret() {
        let line = "say hello, world";
        assert_eq!(group_at(line, 5), 4..9);
        assert_eq!(group_at(line, 9), 4..9, "just after a word");
        assert_eq!(group_at(line, 10), 10..11);
        assert_eq!(word_at(line, 2), Some(0..3));
        assert_eq!(word_at(line, 10), None);
        assert_eq!(first_non_space("    - x"), 4);
    }

    #[test]
    fn several_ranges_in_one_pass() {
        let text = "aé b c";
        let (out, placed) = replace_ranges(
            text,
            &[0..1, 3..4, 5..6],
            &["XY".into(), String::new(), "ü".into()],
            c2b(text),
        );
        assert_eq!(out, "XYé  ü");
        assert_eq!(placed, vec![0..2, 4..4, 5..6]);
    }
}
