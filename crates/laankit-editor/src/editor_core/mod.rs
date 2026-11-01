//! The core engine: the editor's own text widget, replacing egui's
//! `TextEdit` (which stays as a fallback, [`crate::Engine::TextEdit`]).
//!
//! - **Model.** The note stays one `String` (the public API hands out `&str`,
//!   and the list, find and markup helpers work on it), indexed by a rope
//!   ([`ropey::Rope`]) for character, byte and line lookups in O(log n). The
//!   rope follows the text by splicing in what changed ([`EditorState::sync_rope`]).
//! - **Selections.** Several ranges, one primary ([`selection::Selections`]);
//!   rectangular selections are one range per line.
//! - **Layout.** Only the lines on screen are laid out and painted (their
//!   galleys come from the highlighter's per-line cache). Every line has a
//!   height: measured once laid out, estimated before; folded lines have none.
//!   A frame keeps the first visible line where it was when heights above it
//!   change ([`view`]).
//! - **Folding.** Headings, fences and list items ([`fold`]).

pub mod fold;
pub mod ops;
pub mod selection;
mod view;

use std::collections::HashMap;
use std::ops::Range;

use egui::{Id, Pos2};
use ropey::Rope;

use crate::state::EditorState;
use fold::Folds;
use selection::Selections;

/// Where the pointer drag that's in progress started.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Drag {
    /// Extends the primary range's head.
    Text,
    /// Alt+drag: one range per line from this line to the pointer's, between
    /// this column and the pointer's (character columns, as CodeMirror's
    /// rectangular selection).
    Rect { line: usize, col: usize },
}

/// The lines changed since the highlighter last looked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum HlHint {
    #[default]
    None,
    One((usize, usize, usize, usize)),
    Many,
}

impl HlHint {
    /// The single edit to pass on, if there was exactly one.
    pub fn take(&mut self) -> Option<(usize, usize, usize, usize)> {
        match std::mem::take(self) {
            HlHint::One(h) => Some(h),
            _ => None,
        }
    }
}

/// The core engine's state between frames.
#[derive(Default)]
pub(crate) struct CoreState {
    pub sel: Selections,
    pub folds: Folds,
    /// Where each line's top is, relative to the first line (plus the total height at the end).
    pub tops: Vec<f32>,
    /// What `tops` was computed from: layout version, folds, row height, wrap width.
    pub tops_key: Option<(u64, u64, u32, u32)>,
    /// The widest line (measured, or estimated), for horizontal scrolling.
    pub max_width: f32,
    /// The x a vertical move aims for (CodeMirror's "goal column"), for the primary range.
    pub goal_x: Option<f32>,
    /// The first visible line, its top, and the text version, as of last frame.
    pub anchor: Option<(usize, f32, u64)>,
    pub drag: Option<Drag>,
    /// The IME preedit text in the note: its first character and length.
    pub ime: Option<(usize, usize)>,
    /// When the caret last moved or the text changed (the caret blink restarts).
    pub last_interaction: f64,
    /// The lines painted last frame.
    pub visible: Range<usize>,
    /// The visible height of the text area last frame.
    pub viewport_h: f32,
    /// The text origin on screen last frame (top-left of line 0's text).
    pub origin: Pos2,
    /// Scroll the primary caret into view after this frame's input.
    pub reveal: bool,
    /// Foldable ranges by first line, for the text version they were computed for.
    pub fold_ends: (u64, HashMap<usize, Option<usize>>),
    /// The scroll area's id (its state is read back for the next frame).
    pub scroll_id: Option<Id>,
    /// Where the off-screen measuring left off.
    pub measured_upto: usize,
    /// The edit the highlighter hasn't seen yet.
    pub hl_hint: HlHint,
}

impl EditorState {
    /// Brings the rope (and the folds) up to date with the text: finds the
    /// changed span (common prefix and suffix) and splices only that.
    pub(crate) fn sync_rope(&mut self) {
        if self.rope_version == self.version && self.rope.len_bytes() == self.text.len() {
            return;
        }
        let text = self.text.as_bytes();
        let old_len = self.rope.len_bytes();
        let mut p = common_prefix(&self.rope, text);
        let max_s = old_len.min(text.len()) - p;
        let mut s = common_suffix(&self.rope, text).min(max_s);
        // Whole characters only.
        while p > 0 && !self.text.is_char_boundary(p) {
            p -= 1;
        }
        while s > 0 && !self.text.is_char_boundary(text.len() - s) {
            s -= 1;
        }
        let first_line = self.rope.byte_to_line(p);
        let old_last = self.rope.byte_to_line(old_len - s);
        // The new text's last changed line: newlines added minus removed.
        let added = crate::fastcmp::newlines(&text[p..text.len() - s]);
        let removed = self.rope.byte_slice(p..old_len - s).len_lines() - 1;
        let new_last = old_last + added - removed;
        if !self.core.folds.is_empty() {
            let rope = &self.rope;
            self.core
                .folds
                .shift(first_line, old_last, new_last, old_len - s, |l| {
                    rope.line_to_byte(l)
                });
        }
        let from = self.rope.byte_to_char(p);
        let to = self.rope.byte_to_char(old_len - s);
        self.rope.remove(from..to);
        self.rope.insert(from, &self.text[p..text.len() - s]);
        debug_assert_eq!(new_last, self.rope.byte_to_line(text.len() - s));
        // What the highlighter needs to rescan (one edit since its last
        // update; more than one and it diffs the text itself).
        let start = self.rope.line_to_byte(first_line);
        self.core.hl_hint = match self.core.hl_hint {
            HlHint::None => HlHint::One((first_line, start, old_last, new_last)),
            _ => HlHint::Many,
        };
        self.rope_version = self.version;
        if self.rope.len_bytes() != self.text.len() {
            // Shouldn't happen; rebuild rather than drift.
            self.rope = Rope::from_str(&self.text);
        }
    }

    /// Lines in the note (one more than its newlines).
    pub(crate) fn lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// The bytes of 0-based `line`, without its newline.
    pub(crate) fn line_bytes(&self, line: usize) -> Range<usize> {
        let start = self.rope.line_to_byte(line);
        let end = if line + 1 < self.rope.len_lines() {
            self.rope.line_to_byte(line + 1) - 1
        } else {
            self.text.len()
        };
        start..end
    }

    /// The characters of 0-based `line`, without its newline.
    pub(crate) fn line_chars(&self, line: usize) -> Range<usize> {
        let start = self.rope.line_to_char(line);
        let end = if line + 1 < self.rope.len_lines() {
            self.rope.line_to_char(line + 1) - 1
        } else {
            self.rope.len_chars()
        };
        start..end
    }

    pub(crate) fn line_text(&self, line: usize) -> &str {
        &self.text[self.line_bytes(line)]
    }

    /// The line holding character `c`, and `c`'s column in it.
    pub(crate) fn line_col(&self, c: usize) -> (usize, usize) {
        let c = c.min(self.rope.len_chars());
        let line = self.rope.char_to_line(c);
        (line, c - self.rope.line_to_char(line))
    }

    pub(crate) fn char_to_byte(&self, c: usize) -> usize {
        self.rope.char_to_byte(c.min(self.rope.len_chars()))
    }

    pub(crate) fn chars_len(&self) -> usize {
        self.rope.len_chars()
    }

    /// Where character `c` was painted last frame, on screen (core engine;
    /// `None` if its line isn't laid out). For tests that click or drag.
    #[doc(hidden)]
    pub fn screen_rect_of(&mut self, c: usize) -> Option<egui::Rect> {
        self.sync_rope();
        let (line, col) = self.line_col(c);
        let g = self.highlighter.cached_galley(line)?;
        let top = *self.core.tops.get(line)?;
        Some(
            g.pos_from_cursor(egui::text::CCursor::new(col))
                .translate(self.core.origin.to_vec2() + egui::vec2(0.0, top)),
        )
    }
}

/// Bytes `rope` and `text` share at the start.
fn common_prefix(rope: &Rope, text: &[u8]) -> usize {
    let mut n = 0;
    for chunk in rope.chunks() {
        let cb = chunk.as_bytes();
        let m = cb.len().min(text.len() - n);
        let k = crate::fastcmp::common_prefix(&cb[..m], &text[n..n + m]);
        n += k;
        if k < cb.len() {
            break;
        }
    }
    n
}

/// Bytes `rope` and `text` share at the end.
fn common_suffix(rope: &Rope, text: &[u8]) -> usize {
    let mut n = 0;
    let (mut chunks, ..) = rope.chunks_at_byte(rope.len_bytes());
    while let Some(chunk) = chunks.prev() {
        let cb = chunk.as_bytes();
        let m = cb.len().min(text.len() - n);
        let k = crate::fastcmp::common_suffix(&cb[cb.len() - m..], &text[..text.len() - n], m);
        n += k;
        if k < cb.len() {
            break;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rope_follows_edits_and_moves_folds() {
        let mut s = EditorState::new("rope");
        let edit = |s: &mut EditorState, text: &str| {
            s.text = text.to_owned();
            s.version += 1;
            s.sync_rope();
            assert_eq!(s.rope.to_string(), text);
        };
        edit(&mut s, "# a\nb\nc\n# d\ne");
        s.core.folds.fold(3, 4);
        edit(&mut s, "new\n# a\nb\nc\n# d\ne");
        assert_eq!(s.core.folds.ranges(), &[(4, 5)]);
        edit(&mut s, "new\n# a\nb\nc\n# dd\ne");
        assert_eq!(
            s.core.folds.ranges(),
            &[(4, 5)],
            "typing in the fold's first line"
        );
        edit(&mut s, "new\n# a\nb\nc\n# dd\nX");
        assert!(s.core.folds.is_empty(), "an edit inside the fold opens it");
        edit(&mut s, "ü€\nnew\n# a");
        edit(&mut s, "");
        assert_eq!(s.lines(), 1);
    }

    /// The highlighter, told the edited lines by the rope sync, ends up with
    /// the same highlighting as scanning the final text from scratch, over
    /// random edits (fences, headings, frontmatter, multi-line deletions).
    #[test]
    fn hinted_highlighting_matches_a_full_scan() {
        crate::code::load_blocking();
        let ctx = egui::Context::default();
        let mut s = EditorState::new("hint");
        s.text = crate::gen::document_with_fences(120, 2, 12);
        s.version += 1;
        s.sync_rope();
        s.highlighter.update(&ctx, &s.text, Some(s.version));
        s.core.hl_hint = Default::default();
        let snippets = [
            "x",
            "\n",
            "```rust\n",
            "```\n",
            "# H\n",
            "---\n",
            "é",
            "- [ ] t\n",
            "/* ",
            "*/",
            "",
        ];
        let mut seed: u64 = 0x5eed;
        let mut next = |n: usize| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % n.max(1)
        };
        for round in 0..150 {
            let chars = s.chars_len();
            let at = next(chars + 1);
            let len = if next(4) == 0 { next(80) } else { 0 };
            let to = (at + len).min(chars);
            let (bf, bt) = (s.char_to_byte(at), s.char_to_byte(to));
            let snippet = snippets[next(snippets.len())];
            s.text.replace_range(bf..bt, snippet);
            s.version += 1;
            s.sync_rope();
            // Sometimes two edits before the highlighter looks (it then diffs).
            if next(5) == 0 {
                s.text.push('z');
                s.version += 1;
                s.sync_rope();
            }
            let hint = s.core.hl_hint.take();
            s.highlighter
                .update_hinted(&ctx, &s.text, Some(s.version), hint);
            let mut fresh = crate::highlight::Highlighter::default();
            fresh.update(&ctx, &s.text, None);
            assert_eq!(
                s.highlighter.line_count(),
                fresh.line_count(),
                "round {round}"
            );
            for i in 0..fresh.line_count() {
                assert_eq!(
                    s.highlighter.line_spans(i),
                    fresh.line_spans(i),
                    "round {round}, line {i}"
                );
                assert_eq!(s.highlighter.line_blocks(i), fresh.line_blocks(i));
            }
        }
    }
}
