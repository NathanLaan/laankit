//! Spell checking: the [`SpellChecker`] the editor asks, and, with the
//! `spellcheck` feature, [`Hunspell`], a pure-Rust Hunspell checker
//! (`spellbook`) over a dictionary the host supplies.
//!
//! The Svelte editor uses the webview's spell checker (`spellcheck="true"`
//! on CodeMirror's content), with the system locale's dictionary. The native
//! editor has no system checker, so the host loads a Hunspell `.aff`/`.dic`
//! pair (bundled, or from the system's `hunspell` folder) and hands it to
//! [`crate::EditorState::set_spell_checker`].

use std::ops::Range;

/// A word checker. [`crate::EditorState`] underlines the words it rejects
/// and offers its suggestions in the context menu.
pub trait SpellChecker: Send + Sync {
    fn check(&self, word: &str) -> bool;
    fn suggest(&self, word: &str) -> Vec<String>;
}

/// The byte ranges of the words in `text` that `checker` rejects.
pub fn misspelled(checker: &dyn SpellChecker, text: &str) -> Vec<Range<usize>> {
    words(text)
        .filter(|r| !checker.check(&text[r.clone()]))
        .collect()
}

/// The words of `text` worth checking, as byte ranges: runs of letters with
/// inner apostrophes, skipping anything with digits, underscores or a mix of
/// cases after the first letter (identifiers, acronyms such as `HTTP`).
pub fn words(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut chars = text.char_indices().peekable();
    std::iter::from_fn(move || loop {
        let (start, c) = chars.next()?;
        if !c.is_alphanumeric() {
            continue;
        }
        let mut end = start + c.len_utf8();
        let mut skip = !c.is_alphabetic();
        let mut upper_after_first = false;
        while let Some(&(i, c)) = chars.peek() {
            if c.is_alphanumeric() || c == '_' {
                skip |= !c.is_alphabetic();
                upper_after_first |= c.is_uppercase();
                end = i + c.len_utf8();
                chars.next();
            } else if c == '\'' || c == '’' {
                // An apostrophe inside a word (don't), not a closing quote.
                let mut ahead = chars.clone();
                ahead.next();
                if ahead.peek().is_some_and(|&(_, n)| n.is_alphabetic()) {
                    end = i + c.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        if !skip && !upper_after_first {
            return Some(start..end);
        }
    })
}

/// The word at byte `at` of `text`, if any (for the context menu).
pub fn word_at(text: &str, at: usize) -> Option<Range<usize>> {
    words(text).find(|r| r.start <= at && at <= r.end)
}

/// A Hunspell dictionary, plus words the user added this session.
#[cfg(feature = "spellcheck")]
pub struct Hunspell {
    dict: spellbook::Dictionary,
}

#[cfg(feature = "spellcheck")]
impl Hunspell {
    /// Parses a dictionary from the contents of its `.aff` and `.dic` files.
    pub fn new(aff: &str, dic: &str) -> Result<Self, String> {
        spellbook::Dictionary::new(aff, dic)
            .map(|dict| Self { dict })
            .map_err(|e| e.to_string())
    }

    /// Accepts `word` from now on (the host persists it, e.g. in a personal word list).
    pub fn add_word(&mut self, word: &str) {
        if let Err(e) = self.dict.add(word) {
            log::warn!("Couldn't add {word:?} to the dictionary: {e}");
        }
    }
}

#[cfg(feature = "spellcheck")]
impl SpellChecker for Hunspell {
    fn check(&self, word: &str) -> bool {
        self.dict.check(word)
    }

    fn suggest(&self, word: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.dict.suggest(word, &mut out);
        out.truncate(6);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(text: &str) -> Vec<&str> {
        words(text).map(|r| &text[r]).collect()
    }

    #[test]
    fn words_skip_code_like_tokens() {
        assert_eq!(
            list("Don't parse HTTP or v2 or snake_case, 'quoted' café."),
            ["Don't", "parse", "or", "or", "quoted", "café"]
        );
        assert_eq!(word_at("one two", 5), Some(4..7));
        assert_eq!(word_at("one  two", 4), None);
    }

    #[cfg(feature = "spellcheck")]
    #[test]
    fn hunspell_checks_and_suggests() {
        let aff = "SET UTF-8\nTRY esianrtolcdugmphbyfvkwz\n";
        let dic = "3\nhello\nworld\nnote\n";
        let mut h = Hunspell::new(aff, dic).unwrap();
        assert!(h.check("hello"));
        assert!(!h.check("helo"));
        assert!(h.suggest("helo").contains(&"hello".to_string()));
        assert_eq!(misspelled(&h, "hello wrld note"), vec![6..10]);
        h.add_word("wrld");
        assert!(h.check("wrld"));
    }
}
