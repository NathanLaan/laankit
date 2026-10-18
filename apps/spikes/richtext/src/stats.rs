//! Measurements for spike 2: how much of an article survives conversion, and what was dropped.

use std::collections::BTreeMap;

use scraper::node::Node;
use scraper::Html;

use crate::model::Document;

/// Elements a native reader can't show; counted in the *raw* HTML, before sanitizing.
pub const DROPPED_KINDS: &[&str] = &[
    "iframe", "video", "audio", "svg", "math", "script", "form", "object", "embed", "canvas",
];
/// Elements whose text isn't content.
const NOT_TEXT: &[&str] = &["script", "style", "noscript", "template", "head", "title"];

/// `DROPPED_KINDS` elements in `html`, by tag.
pub fn dropped_elements(html: &str) -> BTreeMap<&'static str, usize> {
    let doc = Html::parse_fragment(html);
    let mut out = BTreeMap::new();
    for node in doc.tree.nodes() {
        if let Node::Element(e) = node.value() {
            if let Some(kind) = DROPPED_KINDS
                .iter()
                .find(|k| e.name().eq_ignore_ascii_case(k))
            {
                *out.entry(*kind).or_insert(0) += 1;
            }
        }
    }
    out
}

/// The visible text of `html` (what a browser would show), whitespace collapsed.
pub fn text_content(html: &str) -> String {
    let doc = Html::parse_fragment(html);
    let mut out = String::new();
    for node in doc.tree.nodes() {
        let Node::Text(t) = node.value() else {
            continue;
        };
        let hidden = node.ancestors().any(|a| match a.value() {
            Node::Element(e) => NOT_TEXT.contains(&e.name()),
            _ => false,
        });
        if !hidden {
            out.push_str(t);
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Non-whitespace characters, the unit of [`kept_ratio`].
pub fn visible_chars(s: &str) -> usize {
    s.chars().filter(|c| !c.is_whitespace()).count()
}

/// The share of the source's visible text that the document kept (1.0 when there was none).
pub fn kept_ratio(source_text: &str, doc: &Document) -> f64 {
    let src = visible_chars(source_text);
    if src == 0 {
        return 1.0;
    }
    (visible_chars(&doc.text_without_alt()) as f64 / src as f64).min(1.0)
}
