//! Label text ↔ the inside of a quoted Mermaid label. Mermaid has no
//! backslash escapes; it uses entity codes (`#quot;`, `#35;`) and `<br>` for
//! line breaks.

/// Plain text → the inside of `"…"`.
pub(crate) fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, c) in text.char_indices() {
        let rest = &text[i..];
        match c {
            '"' => out.push_str("#quot;"),
            '\n' => out.push_str("<br>"),
            // Only where it would otherwise be read back as an entity or a
            // break, so `C#` stays `C#`.
            '#' if entity(&rest[1..]).is_some() => out.push_str("#35;"),
            '<' if line_break(rest).is_some() => out.push_str("#lt;"),
            _ => out.push(c),
        }
    }
    out
}

/// The inside of a label (quoted or not) → plain text.
pub(crate) fn decode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while let Some(c) = raw[i..].chars().next() {
        let rest = &raw[i..];
        if let Some(len) = line_break(rest) {
            out.push('\n');
            i += len;
            continue;
        }
        if c == '#' {
            if let Some((name, len)) = entity(&rest[1..]) {
                if let Some(decoded) = decode_entity(name) {
                    out.push(decoded);
                    i += 1 + len;
                    continue;
                }
            }
        }
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// `name;` at the start of `s`: the name and the length including `;`.
fn entity(s: &str) -> Option<(&str, usize)> {
    let end = s.find(|c: char| !c.is_ascii_alphanumeric())?;
    (end > 0 && s[end..].starts_with(';')).then(|| (&s[..end], end + 1))
}

fn decode_entity(name: &str) -> Option<char> {
    match name {
        "quot" => Some('"'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        _ if name.bytes().all(|b| b.is_ascii_digit()) => name.parse().ok().and_then(char::from_u32),
        _ => None,
    }
}

/// `<br>`, `<br/>` or `<br />` (any case) at the start of `s`: its length.
fn line_break(s: &str) -> Option<usize> {
    let head = s.get(..3)?;
    if !head.eq_ignore_ascii_case("<br") {
        return None;
    }
    let tail = &s[3..];
    let ws = tail.len() - tail.trim_start().len();
    let tail = &tail[ws..];
    if tail.starts_with('>') {
        Some(3 + ws + 1)
    } else if tail.starts_with("/>") {
        Some(3 + ws + 2)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes() {
        assert_eq!(
            encode("Say \"hi\"\nthen go"),
            "Say #quot;hi#quot;<br>then go"
        );
        assert_eq!(encode("C# and F#"), "C# and F#");
        assert_eq!(encode("#quot; literally"), "#35;quot; literally");
        assert_eq!(encode("a <br> b < c"), "a #lt;br> b < c");
    }

    #[test]
    fn decodes() {
        assert_eq!(decode("a<br>b<BR/>c<br />d"), "a\nb\nc\nd");
        assert_eq!(
            decode("#quot;x#quot; #35; #9829; #amp;"),
            "\"x\" # \u{2665} &"
        );
        assert_eq!(decode("#unknown; #; C#"), "#unknown; #; C#");
    }

    #[test]
    fn round_trips() {
        for s in [
            "",
            "plain",
            "\"q\"",
            "#35;",
            "C#",
            "<br>",
            "<b>bold</b>",
            "a\nb",
            "#lt;",
            "ü ✓ #",
        ] {
            assert_eq!(decode(&encode(s)), s, "{s:?}");
        }
    }
}
