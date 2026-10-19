//! Folding for the core engine: which lines can fold (headings to the next
//! heading of the same or a higher level, fenced code blocks, and list items
//! with nested lines under them), and the folds that are closed, kept per
//! open note in memory and moved along with edits.

use std::ops::Range;

use crate::highlight::BlockState;

/// The last line a fold starting at `line` would hide, if it can fold.
///
/// `text_of(i)` is line `i`'s text and `blocks(i)` the blocks it starts and
/// ends in (from the highlighter), so lines in fences and frontmatter don't
/// count as headings or list items.
pub fn foldable_end<'a>(
    line: usize,
    lines: usize,
    text_of: impl Fn(usize) -> &'a str,
    blocks: impl Fn(usize) -> Option<(BlockState, BlockState)>,
) -> Option<usize> {
    let (start, end) = blocks(line)?;
    if start != BlockState::Normal {
        return None;
    }
    // A fence opener: to its closing line (or the end of an unclosed fence).
    if matches!(end, BlockState::Fence { .. }) {
        let close = (line + 1..lines)
            .find(|&j| {
                blocks(j).is_some_and(|(s, e)| {
                    matches!(s, BlockState::Fence { .. }) && e == BlockState::Normal
                })
            })
            .unwrap_or(lines.saturating_sub(1));
        return (close > line).then_some(close);
    }
    let text = text_of(line);
    if let Some(level) = heading_level(text) {
        let mut j = line + 1;
        while j < lines {
            let normal = blocks(j).is_some_and(|(s, _)| s == BlockState::Normal);
            if normal && heading_level(text_of(j)).is_some_and(|l| l <= level) {
                break;
            }
            j += 1;
        }
        let mut last = j - 1;
        while last > line && text_of(last).trim().is_empty() {
            last -= 1;
        }
        return (last > line).then_some(last);
    }
    if is_list_item(text) {
        let indent = indent_width(text);
        let mut last = line;
        for j in line + 1..lines {
            let t = text_of(j);
            if t.trim().is_empty() {
                continue;
            }
            if indent_width(t) > indent {
                last = j;
            } else {
                break;
            }
        }
        return (last > line).then_some(last);
    }
    None
}

/// `#` to `######` followed by a space or the end of the line.
fn heading_level(line: &str) -> Option<usize> {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    match line.as_bytes().get(hashes) {
        None | Some(b' ' | b'\t') => Some(hashes),
        _ => None,
    }
}

fn is_list_item(line: &str) -> bool {
    let rest = line.trim_start_matches([' ', '\t']);
    let b = rest.as_bytes();
    let marker = if matches!(b.first(), Some(b'-' | b'*' | b'+')) {
        1
    } else {
        let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || digits > 9 || !matches!(b.get(digits), Some(b'.' | b')')) {
            return false;
        }
        digits + 1
    };
    matches!(b.get(marker), None | Some(b' ' | b'\t'))
}

/// Leading whitespace width, a tab counting as four.
fn indent_width(line: &str) -> usize {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// The closed folds: (first line, last hidden line), sorted by first line.
/// The first line stays visible; the lines after it, up to the last, are hidden.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folds {
    ranges: Vec<(usize, usize)>,
    /// Bumped on every change (the view's line positions depend on it).
    rev: u64,
}

impl Folds {
    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn ranges(&self) -> &[(usize, usize)] {
        &self.ranges
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub fn clear(&mut self) {
        if !self.ranges.is_empty() {
            self.ranges.clear();
            self.rev += 1;
        }
    }

    /// Whether a fold starts at `line`.
    pub fn folded_at(&self, line: usize) -> bool {
        self.ranges.iter().any(|&(s, _)| s == line)
    }

    /// Closes a fold over lines `start + 1 ..= end`.
    pub fn fold(&mut self, start: usize, end: usize) {
        if end <= start || self.folded_at(start) {
            return;
        }
        let at = self.ranges.partition_point(|&(s, _)| s < start);
        self.ranges.insert(at, (start, end));
        self.rev += 1;
    }

    /// Opens the fold starting at `line`; true if there was one.
    pub fn unfold_at(&mut self, line: usize) -> bool {
        let n = self.ranges.len();
        self.ranges.retain(|&(s, _)| s != line);
        let changed = self.ranges.len() != n;
        if changed {
            self.rev += 1;
        }
        changed
    }

    /// Opens every fold that hides `line` (so a caret moved there shows).
    pub fn unfold_hiding(&mut self, line: usize) -> bool {
        let n = self.ranges.len();
        self.ranges.retain(|&(s, e)| !(s < line && line <= e));
        let changed = self.ranges.len() != n;
        if changed {
            self.rev += 1;
        }
        changed
    }

    /// Whether `line` is hidden by a fold.
    pub fn hidden(&self, line: usize) -> bool {
        self.ranges
            .iter()
            .take_while(|&&(s, _)| s < line)
            .any(|&(_, e)| line <= e)
    }

    /// The hidden lines as merged, sorted ranges.
    pub fn hidden_runs(&self) -> Vec<Range<usize>> {
        let mut out: Vec<Range<usize>> = Vec::new();
        for &(s, e) in &self.ranges {
            let r = s + 1..e + 1;
            match out.last_mut() {
                Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
                _ => out.push(r),
            }
        }
        out
    }

    /// Moves the folds along with an edit that replaced old lines
    /// `first ..= old_last` with new lines `first ..= new_last`, ending at old
    /// byte `old_end` (`line_start` maps an old line to its first byte). A
    /// fold moves when the edit ends before its first line starts; the edit
    /// opens it if it reaches the hidden lines, or changes the number of
    /// lines from inside its first line.
    pub fn shift(
        &mut self,
        first: usize,
        old_last: usize,
        new_last: usize,
        old_end: usize,
        line_start: impl Fn(usize) -> usize,
    ) {
        let delta = new_last as isize - old_last as isize;
        let before = self.ranges.clone();
        self.ranges = before
            .iter()
            .filter_map(|&(s, e)| {
                let moved = || ((s as isize + delta) as usize, (e as isize + delta) as usize);
                if e < first {
                    Some((s, e))
                } else if s > old_last || old_end <= line_start(s) {
                    Some(moved())
                } else if s == first && old_last == first && new_last == first {
                    Some((s, e))
                } else {
                    None
                }
            })
            .collect();
        if self.ranges != before {
            self.rev += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight::scan_line;

    struct Doc {
        lines: Vec<String>,
        blocks: Vec<(BlockState, BlockState)>,
    }

    impl Doc {
        fn new(text: &str) -> Self {
            let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
            let mut state = BlockState::Normal;
            let mut blocks = Vec::new();
            for (i, l) in lines.iter().enumerate() {
                let next = scan_line(l, state, i == 0).1;
                blocks.push((state, next));
                state = next;
            }
            Self { lines, blocks }
        }

        fn end(&self, line: usize) -> Option<usize> {
            foldable_end(
                line,
                self.lines.len(),
                |i| self.lines[i].as_str(),
                |i| self.blocks.get(i).copied(),
            )
        }
    }

    #[test]
    fn headings_fold_to_the_next_heading_of_their_level() {
        let d = Doc::new("# A\ntext\n## B\nmore\n\n# C\nlast\n#tag");
        assert_eq!(d.end(0), Some(3), "to before `# C`, trailing blank dropped");
        assert_eq!(d.end(2), Some(3));
        assert_eq!(d.end(5), Some(7), "`#tag` isn't a heading");
        assert_eq!(d.end(1), None);
    }

    #[test]
    fn fences_and_list_items() {
        let d = Doc::new("```rust\n# not a heading\nfn x() {}\n```\n- a\n  - b\n\n  c\n- d\n- e");
        assert_eq!(d.end(0), Some(3));
        assert_eq!(d.end(1), None, "inside the fence");
        assert_eq!(d.end(4), Some(7));
        assert_eq!(d.end(8), None, "no children");
    }

    #[test]
    fn hidden_lines_and_shifting() {
        let mut f = Folds::default();
        f.fold(2, 5);
        f.fold(8, 9);
        assert!(!f.hidden(2) && f.hidden(3) && f.hidden(5) && !f.hidden(6) && f.hidden(9));
        assert_eq!(f.hidden_runs(), vec![3..6, 9..10]);
        // Lines start every 10 bytes here.
        let start = |l: usize| l * 10;
        // Typing on the first line of a fold keeps it.
        f.shift(2, 2, 2, 25, start);
        assert_eq!(f.ranges(), &[(2, 5), (8, 9)]);
        // Two lines added before both folds move them.
        f.shift(0, 0, 2, 5, start);
        assert_eq!(f.ranges(), &[(4, 7), (10, 11)]);
        // A newline typed at the very start of a fold's first line moves it.
        f.shift(4, 4, 5, 40, start);
        assert_eq!(f.ranges(), &[(5, 8), (11, 12)]);
        // A newline typed inside its first line opens it.
        f.shift(5, 5, 6, 53, start);
        assert_eq!(f.ranges(), &[(12, 13)]);
        f.fold(4, 7);
        // An edit inside a fold opens it.
        f.shift(5, 6, 6, 61, start);
        assert_eq!(f.ranges(), &[(12, 13)]);
        assert!(f.unfold_hiding(13));
        assert!(f.is_empty());
    }
}
