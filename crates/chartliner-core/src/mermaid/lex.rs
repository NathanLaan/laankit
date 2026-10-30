//! Tokenises one flowchart statement (`a["A"] --> b("B")`, `a -.-> b`,
//! `a & b --> c`) into nodes and links. Links other than `---` and `-->` are
//! still recognised, as `LinkKind::Other`, so the reader can keep the
//! statement verbatim and still find the nodes it declares.

use super::label;
use crate::model::ShapeKind;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NodeRef {
    pub id: String,
    /// The shape and decoded label, when the statement gives them.
    pub shape: Option<(ShapeKind, String)>,
    pub class: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinkKind {
    Line,
    Arrow,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Token {
    Node(NodeRef),
    Link {
        kind: LinkKind,
        label: Option<String>,
    },
    Amp,
}

/// Node shape delimiters, longest opener first, with the closers each
/// accepts (the trapezoids share openers).
const SHAPES: &[(&str, &[&str])] = &[
    ("(((", &[")))"]),
    ("([", &["])"]),
    ("[[", &["]]"]),
    ("[(", &[")]"]),
    ("((", &["))"]),
    ("{{", &["}}"]),
    ("[/", &["/]", "\\]"]),
    ("[\\", &["\\]", "/]"]),
    ("[", &["]"]),
    ("(", &[")"]),
    ("{", &["}"]),
    (">", &["]"]),
];

/// Link openers that take inline text: `a -- text --> b`.
const TEXT_OPENERS: &[&str] = &["--", "==", "-.", "<--", "<==", "<-."];
const LINK_CHARS: &str = "-=.<>~";

/// `None` if `s` isn't a node/link statement this lexer understands
/// (keywords, `A@{ … }` shapes, stray text).
pub(crate) fn tokenize(s: &str) -> Option<Vec<Token>> {
    let mut lx = Lexer { s, pos: 0 };
    let mut out = Vec::new();
    loop {
        lx.skip_ws();
        out.push(Token::Node(lx.node()?));
        lx.skip_ws();
        if lx.done() {
            return Some(out);
        }
        if lx.eat("&") {
            out.push(Token::Amp);
        } else {
            out.push(lx.link()?);
        }
    }
}

fn is_id_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

struct Lexer<'a> {
    s: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.pos..]
    }

    fn done(&self) -> bool {
        self.rest().is_empty()
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) {
        if let Some(c) = self.peek() {
            self.pos += c.len_utf8();
        }
    }

    fn eat(&mut self, token: &str) -> bool {
        let found = self.rest().starts_with(token);
        if found {
            self.pos += token.len();
        }
        found
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
    }

    /// Letters, digits and `_`, plus `-` between them (`orders-db`, but the
    /// `-` in `a-->b` starts the link).
    fn id(&mut self) -> Option<String> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            let joins = c == '-' && self.pos > start && self.rest()[1..].starts_with(is_id_char);
            if !is_id_char(c) && !joins {
                break;
            }
            self.bump();
        }
        (self.pos > start).then(|| self.s[start..self.pos].to_string())
    }

    fn node(&mut self) -> Option<NodeRef> {
        let id = self.id()?;
        let shape = self.shape()?;
        let class = if self.eat(":::") {
            Some(self.id()?)
        } else {
            None
        };
        Some(NodeRef { id, shape, class })
    }

    /// `Some(None)` when no shape follows the id; `None` when one starts but
    /// never closes.
    fn shape(&mut self) -> Option<Option<(ShapeKind, String)>> {
        for &(open, closes) in SHAPES {
            if !self.eat(open) {
                continue;
            }
            let (text, close) = self.shape_label(closes)?;
            let kind = match open {
                "[" => ShapeKind::Rect,
                "(" => ShapeKind::RoundRect,
                _ => ShapeKind::Other {
                    open: open.into(),
                    close: close.into(),
                },
            };
            return Some(Some((kind, label::decode(&text))));
        }
        Some(None)
    }

    fn shape_label(&mut self, closes: &[&'static str]) -> Option<(String, &'static str)> {
        let save = self.pos;
        self.skip_ws();
        if self.eat("\"") {
            let end = self.rest().find('"')?;
            let text = self.rest()[..end].to_string();
            self.pos += end + 1;
            self.skip_ws();
            let close = closes
                .iter()
                .copied()
                .find(|c| self.rest().starts_with(c))?;
            self.pos += close.len();
            return Some((text, close));
        }
        self.pos = save;
        let (end, close) = closes
            .iter()
            .filter_map(|&c| self.rest().find(c).map(|i| (i, c)))
            .min_by_key(|&(i, _)| i)?;
        let text = self.rest()[..end].trim().to_string();
        self.pos += end + close.len();
        Some((text, close))
    }

    /// A run of link characters, at least two long, with an optional `o`/`x`
    /// head (`--o`, `--x`) when a space or `|` follows it.
    fn link_run(&mut self) -> Option<&'a str> {
        let start = self.pos;
        while self.peek().is_some_and(|c| LINK_CHARS.contains(c)) {
            self.bump();
        }
        if self.pos - start < 2 {
            self.pos = start;
            return None;
        }
        if matches!(self.peek(), Some('o' | 'x'))
            && self.rest()[1..]
                .chars()
                .next()
                .is_none_or(|c| c.is_whitespace() || c == '|')
        {
            self.bump();
        }
        Some(&self.s[start..self.pos])
    }

    fn link(&mut self) -> Option<Token> {
        let open = self.link_run()?;
        let (kind, mut label) =
            if TEXT_OPENERS.contains(&open) && self.peek().is_some_and(char::is_whitespace) {
                let (text, close) = self.inline_text()?;
                (classify(open, Some(close)), Some(text))
            } else {
                (classify(open, None), None)
            };
        let save = self.pos;
        self.skip_ws();
        if self.eat("|") {
            label = Some(self.pipe_label()?);
        } else {
            self.pos = save;
        }
        Some(Token::Link { kind, label })
    }

    /// After `--`: the text up to the closing run (`-->`, `---`, …) that
    /// starts after whitespace, so `uses-cache` stays in the text.
    fn inline_text(&mut self) -> Option<(String, &'a str)> {
        let rest = self.rest();
        let base = self.pos;
        let mut prev_ws = true;
        for (i, c) in rest.char_indices() {
            if prev_ws && "-=.".contains(c) {
                self.pos = base + i;
                if let Some(close) = self.link_run() {
                    return Some((label::decode(strip_quotes(rest[..i].trim())), close));
                }
            }
            prev_ws = c.is_whitespace();
        }
        self.pos = base;
        None
    }

    /// After the opening `|`: up to the closing one, quoted or not.
    fn pipe_label(&mut self) -> Option<String> {
        self.skip_ws();
        let text = if self.eat("\"") {
            let end = self.rest().find('"')?;
            let text = &self.rest()[..end];
            self.pos += end + 1;
            self.skip_ws();
            if !self.eat("|") {
                return None;
            }
            text
        } else {
            let end = self.rest().find('|')?;
            let text = self.rest()[..end].trim();
            self.pos += end + 1;
            text
        };
        Some(label::decode(text))
    }
}

fn classify(open: &str, close: Option<&str>) -> LinkKind {
    match (open, close) {
        ("-->", None) | ("--", Some("-->")) => LinkKind::Arrow,
        ("---", None) | ("--", Some("---")) => LinkKind::Line,
        _ => LinkKind::Other,
    }
}

fn strip_quotes(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, shape: Option<(ShapeKind, &str)>) -> Token {
        Token::Node(NodeRef {
            id: id.into(),
            shape: shape.map(|(k, l)| (k, l.into())),
            class: None,
        })
    }

    fn link(kind: LinkKind, label: Option<&str>) -> Token {
        Token::Link {
            kind,
            label: label.map(Into::into),
        }
    }

    #[test]
    fn declarations() {
        assert_eq!(tokenize("api"), Some(vec![node("api", None)]));
        assert_eq!(
            tokenize("api[\"API Gateway\"]"),
            Some(vec![node("api", Some((ShapeKind::Rect, "API Gateway")))])
        );
        assert_eq!(
            tokenize("svc( Orders service )"),
            Some(vec![node(
                "svc",
                Some((ShapeKind::RoundRect, "Orders service"))
            )])
        );
        let other = |o: &str, c: &str| ShapeKind::Other {
            open: o.into(),
            close: c.into(),
        };
        assert_eq!(
            tokenize("db[(Orders)]"),
            Some(vec![node("db", Some((other("[(", ")]"), "Orders")))])
        );
        assert_eq!(
            tokenize("t[/Trap\\]"),
            Some(vec![node("t", Some((other("[/", "\\]"), "Trap")))])
        );
        assert_eq!(
            tokenize("c(((Stop)))"),
            Some(vec![node("c", Some((other("(((", ")))"), "Stop")))])
        );
        assert_eq!(
            tokenize("n[\"Retries #quot;soon#quot;<br>maybe\"]:::cl_text"),
            Some(vec![Token::Node(NodeRef {
                id: "n".into(),
                shape: Some((ShapeKind::Rect, "Retries \"soon\"\nmaybe".into())),
                class: Some("cl_text".into()),
            })])
        );
        assert_eq!(tokenize("orders-db"), Some(vec![node("orders-db", None)]));
    }

    #[test]
    fn links() {
        use LinkKind::*;
        assert_eq!(
            tokenize("a-->b"),
            Some(vec![node("a", None), link(Arrow, None), node("b", None)])
        );
        assert_eq!(
            tokenize("a --- b --> c"),
            Some(vec![
                node("a", None),
                link(Line, None),
                node("b", None),
                link(Arrow, None),
                node("c", None)
            ])
        );
        assert_eq!(
            tokenize("a -->|\"a|b\"| b"),
            Some(vec![
                node("a", None),
                link(Arrow, Some("a|b")),
                node("b", None)
            ])
        );
        assert_eq!(
            tokenize("a -->|events| b"),
            Some(vec![
                node("a", None),
                link(Arrow, Some("events")),
                node("b", None)
            ])
        );
        assert_eq!(
            tokenize("a -- uses-cache --> b"),
            Some(vec![
                node("a", None),
                link(Arrow, Some("uses-cache")),
                node("b", None)
            ])
        );
        assert_eq!(
            tokenize("a -- \"REST\" --- b"),
            Some(vec![
                node("a", None),
                link(Line, Some("REST")),
                node("b", None)
            ])
        );
        for other in [
            "a -.-> b",
            "a ==> b",
            "a --o b",
            "a <--> b",
            "a ----> b",
            "a -. maybe .-> b",
        ] {
            let tokens = tokenize(other).unwrap();
            assert!(
                matches!(tokens[1], Token::Link { kind: Other, .. }),
                "{other}"
            );
        }
        assert_eq!(
            tokenize("a & b --> c"),
            Some(vec![
                node("a", None),
                Token::Amp,
                node("b", None),
                link(Arrow, None),
                node("c", None)
            ])
        );
    }

    #[test]
    fn rejects() {
        for s in [
            "",
            "a b",
            "a[unclosed",
            "a -->",
            "a@{ shape: rect }",
            "accTitle: x",
        ] {
            assert_eq!(tokenize(s), None, "{s}");
        }
    }
}
