//! Text edits beyond lists, each a pure function from (text, cursor) to a
//! [`DocEdit`]: the emphasis toggles (`toggleMarker` in `Editor.svelte`),
//! replacing the selection, CodeMirror's bracket auto-closing
//! (`closeBrackets` in `basicSetup`), its Markdown `deleteMarkupBackward`, and
//! accepting a wikilink completion. Cursors are in characters, as egui's
//! `CCursor` index is.

use crate::list_edits::{
    byte_at, chars_in, line_of, parse_item, quote_prefix_len, task_box, DocEdit,
};

/// The pairs `closeBrackets` handles in Markdown (CodeMirror's defaults).
const PAIRS: [(char, char); 5] = [('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')];
/// After these, an opening bracket still auto-closes (`closeBrackets`' `before`).
const CLOSE_BEFORE: &str = ")]}:;>";

/// Replaces the selection (or inserts at the caret) with `insert`; the caret
/// ends after it.
pub fn replace_selection(text: &str, anchor: usize, head: usize, insert: &str) -> DocEdit {
    let (from, to) = (anchor.min(head), anchor.max(head));
    let (bf, bt) = (byte_at(text, from), byte_at(text, to));
    let caret = from + chars_in(insert);
    DocEdit {
        text: format!("{}{insert}{}", &text[..bf], &text[bt..]),
        anchor: caret,
        head: caret,
    }
}

/// Ctrl+B / Ctrl+I: wraps the selection in `marker` (`**` or `*`), or strips
/// it when the selection is already wrapped in exactly that marker. With a
/// caret, inserts the pair and parks the caret between.
pub fn toggle_marker(text: &str, anchor: usize, head: usize, marker: &str) -> DocEdit {
    let (from, to) = (anchor.min(head), anchor.max(head));
    let (bf, bt) = (byte_at(text, from), byte_at(text, to));
    let len = chars_in(marker);
    if from != to && text[..bf].ends_with(marker) && text[bt..].starts_with(marker) {
        let before = &text[..bf - marker.len()];
        let after = &text[bt + marker.len()..];
        return DocEdit {
            text: format!("{before}{}{after}", &text[bf..bt]),
            anchor: from - len,
            head: to - len,
        };
    }
    if from == to {
        return DocEdit {
            text: format!("{}{marker}{marker}{}", &text[..bf], &text[bf..]),
            anchor: from + len,
            head: from + len,
        };
    }
    DocEdit {
        text: format!(
            "{}{marker}{}{marker}{}",
            &text[..bf],
            &text[bf..bt],
            &text[bt..]
        ),
        anchor: from + len,
        head: to + len,
    }
}

/// Typing `ch` with bracket auto-closing. `None` means "type it normally".
///
/// - An opening bracket wraps a selection, or inserts the pair when the next
///   character is whitespace, the end of the line or a closing bracket.
/// - A quote does the same, but not right after a word character (so
///   apostrophes type normally) and not next to another quote.
/// - A closing bracket (or quote) steps over the same character after the caret.
pub fn type_char(text: &str, anchor: usize, head: usize, ch: char) -> Option<DocEdit> {
    let (from, to) = (anchor.min(head), anchor.max(head));
    let (bf, bt) = (byte_at(text, from), byte_at(text, to));
    let next = text[bt..].chars().next();
    let prev = text[..bf].chars().next_back();
    let is_quote = ch == '"' || ch == '\'';
    if from == to && next == Some(ch) && (is_quote || PAIRS.iter().any(|p| p.1 == ch)) {
        return Some(DocEdit {
            text: text.to_owned(),
            anchor: from + 1,
            head: from + 1,
        });
    }
    let close = PAIRS.iter().find(|p| p.0 == ch)?.1;
    if from != to {
        return Some(DocEdit {
            text: format!("{}{ch}{}{close}{}", &text[..bf], &text[bf..bt], &text[bt..]),
            anchor: from + 1,
            head: to + 1,
        });
    }
    let next_ok = next.is_none_or(|c| c.is_whitespace() || CLOSE_BEFORE.contains(c));
    if !next_ok {
        return None;
    }
    if is_quote && prev.is_some_and(|c| c.is_alphanumeric() || c == ch) {
        return None;
    }
    Some(DocEdit {
        text: format!("{}{ch}{close}{}", &text[..bf], &text[bf..]),
        anchor: from + 1,
        head: from + 1,
    })
}

/// Backspace: deletes an empty auto-closed pair around the caret (`(|)`),
/// or CodeMirror Markdown's `deleteMarkupBackward`: with the caret right
/// after a list marker, task box or blockquote marker (`- |`, `1. [ ] |`,
/// `> |`), removes that markup and keeps the indent. `None` means "delete
/// normally".
pub fn backspace(text: &str, anchor: usize, head: usize) -> Option<DocEdit> {
    if anchor != head || head == 0 {
        return None;
    }
    let b = byte_at(text, head);
    let prev = text[..b].chars().next_back()?;
    let next = text[b..].chars().next();
    if PAIRS.iter().any(|p| p.0 == prev && Some(p.1) == next) {
        let start = b - prev.len_utf8();
        let end = b + next.map_or(0, char::len_utf8);
        return Some(DocEdit {
            text: format!("{}{}", &text[..start], &text[end..]),
            anchor: head - 1,
            head: head - 1,
        });
    }
    let (_, start) = line_of(text, b);
    let before = &text[start..b];
    let quote = quote_prefix_len(before);
    let rest = &before[quote..];
    if let Some(item) = parse_item(rest) {
        // The caret must sit where the item's text starts (after an optional task box).
        let markup_end = rest.len() - item.text.len();
        let after_task = if task_box(item.text) {
            item.text.len() - item.text[3..].trim_start().len()
        } else {
            0
        };
        let content = &item.text[after_task..];
        if content.is_empty() && rest.len() == markup_end + after_task && rest.ends_with(' ') {
            let keep = format!("{}{}", &before[..quote], item.indent);
            let removed = chars_in(before) - chars_in(&keep);
            return Some(DocEdit {
                text: format!("{}{keep}{}", &text[..start], &text[b..]),
                anchor: head - removed,
                head: head - removed,
            });
        }
        return None;
    }
    if quote > 0 && quote == before.len() {
        // `> |` or `> > |`: drop the last marker.
        let trimmed = before.trim_end_matches(' ');
        let keep = trimmed.strip_suffix('>').unwrap_or(trimmed);
        let removed = chars_in(before) - chars_in(keep);
        return Some(DocEdit {
            text: format!("{}{keep}{}", &text[..start], &text[b..]),
            anchor: head - removed,
            head: head - removed,
        });
    }
    None
}

/// The open wikilink before the caret, `[[query` on the caret's line: the
/// character index just after `[[`, and the query typed so far.
pub fn wikilink_query(text: &str, head: usize) -> Option<(usize, String)> {
    let b = byte_at(text, head);
    let (_, start) = line_of(text, b);
    let before = &text[start..b];
    let open = before.rfind("[[")?;
    let query = &before[open + 2..];
    if query.contains(']') {
        return None;
    }
    Some((chars_in(&text[..start + open + 2]), query.to_owned()))
}

/// Accepts the completion `name` for the wikilink opened at character `from`
/// (just after `[[`), with the caret at `head`. Adds `]]` unless the text
/// after the caret already starts with it, and puts the caret after `]]`.
pub fn accept_wikilink(text: &str, from: usize, head: usize, name: &str) -> DocEdit {
    let (bf, bh) = (byte_at(text, from), byte_at(text, head));
    let has_close = text[bh..].starts_with("]]");
    let insert = if has_close {
        name.to_owned()
    } else {
        format!("{name}]]")
    };
    let caret = from + chars_in(name) + 2;
    DocEdit {
        text: format!("{}{insert}{}", &text[..bf], &text[bh..]),
        anchor: caret,
        head: caret,
    }
}

/// The Markdown `insertMarkdownReference` adds for an attachment: an image
/// embed for image files, a link otherwise, then a newline.
pub fn attachment_markdown(original_name: &str, filename: &str) -> String {
    const IMAGES: [&str; 6] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"];
    let path = format!("./_attachments/{filename}");
    let lower = original_name.to_lowercase();
    if IMAGES.iter().any(|e| lower.ends_with(e)) {
        format!("![{original_name}]({path})\n")
    } else {
        format!("[{original_name}]({path})\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `|` marks a caret, `[` … `]` written as `«` … `»` marks a selection.
    fn run(doc: &str, f: impl Fn(&str, usize, usize) -> Option<DocEdit>) -> String {
        let (text, a, h) = parse(doc);
        match f(&text, a, h) {
            Some(e) => render(&e),
            None => "<none>".into(),
        }
    }

    fn parse(doc: &str) -> (String, usize, usize) {
        let mut text = String::new();
        let (mut a, mut h) = (None, None);
        for c in doc.chars() {
            match c {
                '|' => {
                    a = Some(text.chars().count());
                    h = a;
                }
                '«' => a = Some(text.chars().count()),
                '»' => h = Some(text.chars().count()),
                _ => text.push(c),
            }
        }
        (text, a.unwrap(), h.unwrap())
    }

    fn render(e: &DocEdit) -> String {
        let mut out = String::new();
        for (i, c) in e.text.chars().enumerate() {
            if e.anchor == e.head && i == e.head {
                out.push('|');
            } else if e.anchor != e.head && i == e.anchor.min(e.head) {
                out.push('«');
            } else if e.anchor != e.head && i == e.anchor.max(e.head) {
                out.push('»');
            }
            out.push(c);
        }
        let n = e.text.chars().count();
        if e.anchor == e.head && e.head == n {
            out.push('|');
        } else if e.anchor != e.head && e.anchor.max(e.head) == n {
            out.push('»');
        }
        out
    }

    #[test]
    fn toggle_marker_wraps_strips_and_inserts() {
        let t = |d: &str, m: &'static str| run(d, |t, a, h| Some(toggle_marker(t, a, h, m)));
        assert_eq!(t("a «bold» c", "**"), "a **«bold»** c");
        assert_eq!(t("a **«bold»** c", "**"), "a «bold» c");
        assert_eq!(t("x|", "*"), "x*|*");
        assert_eq!(t("«é»", "*"), "*«é»*");
    }

    #[test]
    fn brackets_pair_wrap_and_step_over() {
        let ty = |d: &str, c: char| run(d, |t, a, h| type_char(t, a, h, c));
        assert_eq!(ty("x |", '('), "x (|)");
        assert_eq!(ty("x |y", '('), "<none>");
        assert_eq!(ty("«sel»", '['), "[«sel»]");
        assert_eq!(ty("[[|]]", ']'), "[[]|]");
        assert_eq!(ty("don|", '\''), "<none>");
        assert_eq!(ty("say |", '"'), "say \"|\"");
        assert_eq!(ty("[|]", '['), "[[|]]");
        assert_eq!(ty("a|", 'b'), "<none>");
    }

    #[test]
    fn backspace_removes_pairs_and_markup() {
        let bs = |d: &str| run(d, backspace);
        assert_eq!(bs("(|)"), "|");
        assert_eq!(bs("- |"), "|");
        assert_eq!(bs("  - [ ] |"), "  |");
        assert_eq!(bs("1. |"), "|");
        assert_eq!(bs("> |"), "|");
        assert_eq!(bs("> > |"), "> |");
        assert_eq!(bs("- a|"), "<none>");
        assert_eq!(bs("plain|"), "<none>");
        assert_eq!(bs("-|"), "<none>");
    }

    #[test]
    fn wikilink_query_and_accept() {
        let text = "see [[Rea";
        assert_eq!(wikilink_query(text, 9), Some((6, "Rea".into())));
        assert_eq!(wikilink_query("a [[x]] b", 9), None);
        let e = accept_wikilink(text, 6, 9, "Reading list");
        assert_eq!(render(&e), "see [[Reading list]]|");
        // closeBrackets already added the `]]`.
        let e = accept_wikilink("[[Re]] x", 2, 4, "Reading");
        assert_eq!(render(&e), "[[Reading]]| x");
    }

    #[test]
    fn attachment_markdown_embeds_images() {
        assert_eq!(
            attachment_markdown("Shot.PNG", "att-1.png"),
            "![Shot.PNG](./_attachments/att-1.png)\n"
        );
        assert_eq!(
            attachment_markdown("doc.pdf", "att-2.pdf"),
            "[doc.pdf](./_attachments/att-2.pdf)\n"
        );
    }
}
