//! Just enough CSS for EPUB books: the declarations a reader honours (italics, bold, underline,
//! strike-through, alignment, `display: none`, monospace) matched by simple selectors.
//!
//! Selectors are reduced to their last compound (`div.note p.first` → `p.first`), so a rule can
//! over-match; pseudo-classes, attribute selectors and `@` rules are skipped.

use crate::model::Align;

/// The declarations the converter understands; `None` means "not set here".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Decls {
    pub italic: Option<bool>,
    pub bold: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub monospace: Option<bool>,
    pub align: Option<Align>,
    pub hidden: bool,
}

impl Decls {
    /// `other` on top of `self`.
    pub fn merge(&mut self, other: &Decls) {
        macro_rules! take {
            ($($f:ident),*) => { $(if other.$f.is_some() { self.$f = other.$f; })* };
        }
        take!(italic, bold, underline, strike, monospace, align);
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
                        "left" | "start" | "justify" => Some(Align::Left),
                        _ => None,
                    }
                }
                "display" => d.hidden = value == "none",
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
}
