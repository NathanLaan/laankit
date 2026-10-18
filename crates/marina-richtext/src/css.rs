//! Just enough CSS for EPUB books, matched by simple selectors. Honoured, because books use them
//! to carry meaning or structure:
//!
//! - `font-style`, `font-weight`, `text-decoration`, `font-family` (monospace only): emphasis.
//! - `text-align` (including `justify`): centred titles, right-aligned signatures, justified prose.
//! - `text-indent`, `margin-left`, `padding-left` (and the `margin`/`padding` shorthands):
//!   indented paragraphs, block quotes and verse built from `div`s.
//! - `font-variant: small-caps`, `text-transform: uppercase`: chapter openings and acronyms.
//! - `vertical-align: super | sub`: footnote markers styled with classes instead of `<sup>`.
//! - `width: N% | Npx` on images: frontispieces and inline figures sized by the book.
//! - `display: none`.
//!
//! Ignored, because a reflowing reader shouldn't take them from the book: colours and
//! backgrounds (the reading theme owns them), font sizes and line heights (the reader's own
//! setting), vertical margins, floats, positioning, `page-break-*` (there are no pages) and
//! everything else.
//!
//! Selectors are reduced to their last compound (`div.note p.first` → `p.first`), so a rule can
//! over-match; pseudo-classes, attribute selectors and `@` rules are skipped.

use crate::model::Align;

/// The declarations the converter understands; `None` means "not set here".
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Decls {
    pub italic: Option<bool>,
    pub bold: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub monospace: Option<bool>,
    pub align: Option<Align>,
    pub hidden: bool,
    /// `margin-left` + `padding-left`, in ems.
    pub indent_left: Option<f32>,
    /// `text-indent`, in ems.
    pub text_indent: Option<f32>,
    pub small_caps: Option<bool>,
    pub uppercase: Option<bool>,
    /// `vertical-align`: `Some(true)` super, `Some(false)` sub.
    pub superscript: Option<bool>,
    /// An image's `width`.
    pub width: Option<Length>,
}

/// A CSS length the converter keeps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    /// Ems (`em`, `rem`; `px` and `pt` converted at 16 px / 12 pt to the em).
    Em(f32),
    /// A percentage of the containing block.
    Percent(f32),
}

impl Length {
    pub fn parse(value: &str) -> Option<Length> {
        let v = value.trim();
        let num = |suffix: &str| v.strip_suffix(suffix)?.trim().parse::<f32>().ok();
        if let Some(n) = num("%") {
            Some(Length::Percent(n))
        } else if let Some(n) = num("rem").or_else(|| num("em")) {
            Some(Length::Em(n))
        } else if let Some(n) = num("px") {
            Some(Length::Em(n / 16.0))
        } else if let Some(n) = num("pt") {
            Some(Length::Em(n / 12.0))
        } else {
            v.parse::<f32>().ok().filter(|n| *n == 0.0).map(Length::Em)
        }
    }

    /// In ems, a percentage taken of a 40 em column.
    pub fn ems(self) -> f32 {
        match self {
            Length::Em(n) => n,
            Length::Percent(p) => p / 100.0 * 40.0,
        }
    }
}

impl Decls {
    /// `other` on top of `self`.
    pub fn merge(&mut self, other: &Decls) {
        macro_rules! take {
            ($($f:ident),*) => { $(if other.$f.is_some() { self.$f = other.$f; })* };
        }
        take!(
            italic,
            bold,
            underline,
            strike,
            monospace,
            align,
            indent_left,
            text_indent,
            small_caps,
            uppercase,
            superscript,
            width
        );
        self.hidden |= other.hidden;
    }

    pub fn is_empty(&self) -> bool {
        *self == Decls::default()
    }

    /// Parses a declaration list (`font-style: italic; text-align: center`).
    pub fn parse(src: &str) -> Decls {
        let mut d = Decls::default();
        for decl in src.split(';') {
            let Some((prop, value)) = decl.split_once(':') else {
                continue;
            };
            let prop = prop.trim().to_ascii_lowercase();
            let value = value
                .trim()
                .trim_end_matches("!important")
                .trim()
                .to_ascii_lowercase();
            match prop.as_str() {
                "font-style" => d.italic = Some(value == "italic" || value == "oblique"),
                "font-weight" => {
                    d.bold = Some(match value.as_str() {
                        "bold" | "bolder" => true,
                        v => v.parse::<u32>().is_ok_and(|w| w >= 600),
                    })
                }
                "text-decoration" | "text-decoration-line" => {
                    d.underline = Some(value.contains("underline"));
                    d.strike = Some(value.contains("line-through"));
                }
                "text-align" => {
                    d.align = match value.as_str() {
                        "center" => Some(Align::Center),
                        "right" | "end" => Some(Align::Right),
                        "left" | "start" => Some(Align::Left),
                        "justify" => Some(Align::Justify),
                        _ => None,
                    }
                }
                "display" => d.hidden = value == "none",
                "margin-left" | "padding-left" => {
                    if let Some(l) = Length::parse(&value) {
                        d.indent_left = Some(d.indent_left.unwrap_or(0.0) + l.ems());
                    }
                }
                "margin" | "padding" => {
                    // The left side: `a` (all), `a b`, `a b c` (left = b), `a b c d` (left = d).
                    let parts: Vec<&str> = value.split_whitespace().collect();
                    let left = match parts.len() {
                        1 => parts.first(),
                        2 | 3 => parts.get(1),
                        _ => parts.get(3),
                    };
                    if let Some(l) = left.and_then(|v| Length::parse(v)) {
                        d.indent_left = Some(d.indent_left.unwrap_or(0.0) + l.ems());
                    }
                }
                "text-indent" => d.text_indent = Length::parse(&value).map(Length::ems),
                "font-variant" | "font-variant-caps" => {
                    d.small_caps = Some(value.contains("small-caps"))
                }
                "text-transform" => d.uppercase = Some(value == "uppercase"),
                "vertical-align" => {
                    d.superscript = match value.as_str() {
                        "super" | "top" | "text-top" => Some(true),
                        "sub" | "bottom" | "text-bottom" => Some(false),
                        _ => None,
                    }
                }
                "width" => d.width = Length::parse(&value),
                "font-family" => {
                    d.monospace = Some(value.contains("mono") || value.contains("courier"))
                }
                "font" => {
                    if value.contains("italic") {
                        d.italic = Some(true);
                    }
                    if value.contains("bold") {
                        d.bold = Some(true);
                    }
                }
                _ => {}
            }
        }
        d
    }
}

/// A parsed stylesheet.
#[derive(Clone, Debug, Default)]
pub struct StyleSheet {
    rules: Vec<Rule>,
}

#[derive(Clone, Debug)]
struct Rule {
    tag: Option<String>,
    classes: Vec<String>,
    id: Option<String>,
    decls: Decls,
}

impl StyleSheet {
    pub fn parse(src: &str) -> StyleSheet {
        let src = strip_comments(src);
        let mut sheet = StyleSheet::default();
        let mut rest = src.as_str();
        while let Some(open) = rest.find('{') {
            // Leftovers before the selector: the `}` closing an `@media` block, `@import …;`.
            let head = &rest[..open];
            let selectors = head[head.rfind([';', '}']).map_or(0, |i| i + 1)..].trim();
            if selectors.starts_with("@media") || selectors.starts_with("@supports") {
                // Keep the nested rules.
                rest = &rest[open + 1..];
                continue;
            }
            let Some(close) = rest[open..].find('}') else {
                break;
            };
            let body = &rest[open + 1..open + close];
            rest = &rest[open + close + 1..];
            if selectors.starts_with('@') {
                // `@font-face`, `@page`: nothing we use.
                continue;
            }
            let decls = Decls::parse(body);
            if decls.is_empty() {
                continue;
            }
            for selector in selectors.split(',') {
                if let Some(rule) = compile(selector, decls) {
                    sheet.rules.push(rule);
                }
            }
        }
        sheet
    }

    pub fn append(&mut self, other: StyleSheet) {
        self.rules.extend(other.rules);
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The declarations for an element, in source order (later rules win).
    pub fn decls_for<'a>(
        &self,
        tag: &str,
        id: Option<&str>,
        classes: impl Iterator<Item = &'a str> + Clone,
    ) -> Decls {
        let mut d = Decls::default();
        for rule in &self.rules {
            if rule.tag.as_deref().is_some_and(|t| t != tag) {
                continue;
            }
            if rule.id.is_some() && rule.id.as_deref() != id {
                continue;
            }
            if !rule.classes.iter().all(|c| classes.clone().any(|k| k == c)) {
                continue;
            }
            d.merge(&rule.decls);
        }
        d
    }
}

fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start..].find("*/") {
            Some(end) => rest = &rest[start + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Compiles the last compound of `selector`; `None` for selectors we can't honour.
fn compile(selector: &str, decls: Decls) -> Option<Rule> {
    let last = selector
        .split(|c: char| c.is_whitespace() || c == '>' || c == '+' || c == '~')
        .rfind(|s| !s.is_empty())?;
    if last.contains([':', '[', '*']) {
        return None;
    }
    let mut rule = Rule {
        tag: None,
        classes: Vec::new(),
        id: None,
        decls,
    };
    let mut part = String::new();
    let mut kind = ' ';
    let flush = |kind: char, part: &mut String, rule: &mut Rule| {
        if !part.is_empty() {
            let p = std::mem::take(part);
            match kind {
                '.' => rule.classes.push(p),
                '#' => rule.id = Some(p),
                _ => rule.tag = Some(p.to_ascii_lowercase()),
            }
        }
    };
    for c in last.chars() {
        if c == '.' || c == '#' {
            flush(kind, &mut part, &mut rule);
            kind = c;
        } else {
            part.push(c);
        }
    }
    flush(kind, &mut part, &mut rule);
    Some(rule)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_simple_selectors() {
        let sheet = StyleSheet::parse(
            "/* c */ p.center, .ctr { text-align: center } em.x { font-style: normal } \
             div p.i { font-style: italic } a:hover { color: red } @font-face { src: url(x) }",
        );
        let d = sheet.decls_for("p", None, ["center"].into_iter());
        assert_eq!(d.align, Some(Align::Center));
        let d = sheet.decls_for("p", None, ["i", "other"].into_iter());
        assert_eq!(d.italic, Some(true));
        let d = sheet.decls_for("span", None, std::iter::empty());
        assert!(d.is_empty());
    }

    #[test]
    fn parses_inline_styles() {
        let d =
            Decls::parse("font-weight: 700; display:none; text-decoration: underline line-through");
        assert_eq!(d.bold, Some(true));
        assert!(d.hidden);
        assert_eq!((d.underline, d.strike), (Some(true), Some(true)));
    }

    #[test]
    fn parses_book_layout() {
        let d = Decls::parse(
            "text-align: justify; text-indent: 1.5em; margin: 0 0 0 24px; padding-left: 1em; \
             font-variant: small-caps; vertical-align: super; width: 50%; page-break-before: always",
        );
        assert_eq!(d.align, Some(Align::Justify));
        assert_eq!(d.text_indent, Some(1.5));
        assert_eq!(d.indent_left, Some(2.5));
        assert_eq!(d.small_caps, Some(true));
        assert_eq!(d.superscript, Some(true));
        assert_eq!(d.width, Some(Length::Percent(50.0)));
        assert_eq!(Length::parse("12pt"), Some(Length::Em(1.0)));
        assert_eq!(Length::parse("0"), Some(Length::Em(0.0)));
        assert_eq!(Length::parse("auto"), None);
        assert_eq!(Decls::parse("margin: 1em 2em").indent_left, Some(2.0));
    }
}
