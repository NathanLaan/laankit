//! Find and replace: the search side of CodeMirror's search panel
//! (`basicSetup`'s `searchKeymap`, opened with Ctrl+F or Ctrl+Shift+F in
//! `Editor.svelte`): match case, regular expression and whole-word options,
//! next/previous, select all, replace and replace all.
//!
//! Matches are byte ranges into the text, recomputed only when the text,
//! the query or an option changes.

use std::ops::Range;

use regex::{Regex, RegexBuilder};

/// Matches found at most (the rest are ignored, as a guard on huge notes).
pub const MAX_MATCHES: usize = 10_000;

#[derive(Clone, Debug, Default)]
pub struct FindState {
    pub open: bool,
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
    /// Focus the query field next frame.
    pub(crate) focus_query: bool,
    cache: Option<Cache>,
}

#[derive(Clone, Debug)]
struct Cache {
    key: (u64, String, bool, bool, bool),
    matches: Vec<Range<usize>>,
    /// The same matches in characters (egui's cursor index).
    chars: Vec<Range<usize>>,
    /// The query is an invalid regular expression.
    invalid: bool,
}

impl FindState {
    /// All matches in `text`, whose content version is `version`.
    pub fn matches(&mut self, text: &str, version: u64) -> &[Range<usize>] {
        let key = (
            version,
            self.query.clone(),
            self.case_sensitive,
            self.regex,
            self.whole_word,
        );
        if self.cache.as_ref().is_none_or(|c| c.key != key) {
            let (matches, invalid) = match self.compile() {
                Some(Ok(re)) => (
                    re.find_iter(text)
                        .filter(|m| !m.is_empty())
                        .take(MAX_MATCHES)
                        .map(|m| m.range())
                        .collect(),
                    false,
                ),
                Some(Err(_)) => (Vec::new(), true),
                None => (Vec::new(), false),
            };
            let chars = char_ranges(text, &matches);
            self.cache = Some(Cache {
                key,
                matches,
                chars,
                invalid,
            });
        }
        &self.cache.as_ref().expect("cache").matches
    }

    /// The matches of the last [`Self::matches`] call, in characters.
    pub fn char_matches(&self) -> &[Range<usize>] {
        self.cache.as_ref().map_or(&[], |c| c.chars.as_slice())
    }

    /// The query is a regular expression that doesn't compile.
    pub fn invalid(&self) -> bool {
        self.cache.as_ref().is_some_and(|c| c.invalid)
    }

    fn compile(&self) -> Option<Result<Regex, regex::Error>> {
        if self.query.is_empty() {
            return None;
        }
        let mut pattern = if self.regex {
            self.query.clone()
        } else {
            regex::escape(&self.query)
        };
        if self.whole_word {
            pattern = format!(r"\b(?:{pattern})\b");
        }
        Some(
            RegexBuilder::new(&pattern)
                .case_insensitive(!self.case_sensitive)
                .multi_line(true)
                .build(),
        )
    }

    /// The replacement for match `m` of `text`: with `$1`-style groups
    /// expanded when searching by regular expression.
    pub fn replacement_for(&self, text: &str, m: &Range<usize>) -> String {
        if !self.regex {
            return self.replacement.clone();
        }
        match self.compile() {
            Some(Ok(re)) => match re.captures(&text[m.clone()]) {
                Some(caps) => {
                    let mut out = String::new();
                    caps.expand(&self.replacement, &mut out);
                    out
                }
                None => self.replacement.clone(),
            },
            _ => self.replacement.clone(),
        }
    }
}

/// Byte ranges to character ranges, in one pass over the text.
fn char_ranges(text: &str, matches: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut out = Vec::with_capacity(matches.len());
    let (mut byte, mut chars) = (0, 0);
    let mut to_chars = |b: usize| {
        chars += text[byte..b].chars().count();
        byte = b;
        chars
    };
    for m in matches {
        let s = to_chars(m.start);
        let e = to_chars(m.end);
        out.push(s..e);
    }
    out
}

/// The match to go to from a selection `sel` (bytes): the first one starting
/// at or after its end going forward, or the last one ending at or before its
/// start going back, wrapping around.
pub fn next_match(matches: &[Range<usize>], sel: Range<usize>, forward: bool) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    if forward {
        let i = matches.partition_point(|m| m.start < sel.end);
        Some(if i == matches.len() { 0 } else { i })
    } else {
        let i = matches.partition_point(|m| m.end <= sel.start);
        Some(if i == 0 { matches.len() - 1 } else { i - 1 })
    }
}

/// `text` with every match replaced, back to front; returns the new text
/// and how many were replaced.
pub fn replace_all(find: &FindState, text: &str, matches: &[Range<usize>]) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for m in matches {
        out.push_str(&text[at..m.start]);
        out.push_str(&find.replacement_for(text, m));
        at = m.end;
    }
    out.push_str(&text[at..]);
    (out, matches.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(query: &str) -> FindState {
        FindState {
            query: query.into(),
            ..Default::default()
        }
    }

    #[test]
    fn options_change_the_matches() {
        let text = "Note note notebook";
        let mut f = find("note");
        assert_eq!(f.matches(text, 1), [0..4, 5..9, 10..14]);
        assert_eq!(find("b").matches("éb", 1), vec![2..3]);
        f.case_sensitive = true;
        assert_eq!(f.matches(text, 1), [5..9, 10..14]);
        f.whole_word = true;
        assert_eq!(f.matches(text, 1), vec![5..9]);
        let mut r = find("n.te");
        assert_eq!(r.matches(text, 1).len(), 0);
        r.regex = true;
        assert_eq!(r.matches(text, 1).len(), 3);
        let mut bad = find("(");
        bad.regex = true;
        assert!(bad.matches(text, 1).is_empty());
        assert!(bad.invalid());
    }

    #[test]
    fn next_and_previous_wrap() {
        let m = [0..2, 5..7, 9..11];
        assert_eq!(next_match(&m, 0..2, true), Some(1));
        assert_eq!(next_match(&m, 9..11, true), Some(0));
        assert_eq!(next_match(&m, 5..7, false), Some(0));
        assert_eq!(next_match(&m, 0..0, false), Some(2));
        assert_eq!(next_match(&[], 0..0, true), None);
    }

    #[test]
    fn replace_all_expands_groups() {
        let text = "a1 b2 c3";
        let mut f = find(r"([a-z])(\d)");
        f.regex = true;
        f.replacement = "$2$1".into();
        let m = f.matches(text, 1).to_vec();
        assert_eq!(replace_all(&f, text, &m), ("1a 2b 3c".into(), 3));
    }
}
