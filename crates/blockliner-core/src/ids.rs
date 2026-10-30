//! Mermaid node ids made from labels: `"Orders DB"` → `orders_db`. Ids are
//! ASCII so every Mermaid renderer accepts them, and never a Mermaid keyword
//! (a node called `end` breaks the flowchart).

const RESERVED: &[&str] = &[
    "end",
    "graph",
    "flowchart",
    "subgraph",
    "direction",
    "style",
    "class",
    "classdef",
    "click",
    "linkstyle",
    "default",
];
const MAX_LEN: usize = 32;

/// A readable id for `label`, not yet checked for uniqueness.
pub fn from_label(label: &str) -> String {
    let mut id = String::new();
    for c in label.chars() {
        if id.len() >= MAX_LEN {
            break;
        }
        if c.is_ascii_alphanumeric() {
            id.push(c.to_ascii_lowercase());
        } else if !id.is_empty() && !id.ends_with('_') {
            id.push('_');
        }
    }
    while id.ends_with('_') {
        id.pop();
    }
    if id.is_empty() {
        return "node".into();
    }
    if id.starts_with(|c: char| c.is_ascii_digit()) {
        id.insert(0, 'n');
    }
    if RESERVED.contains(&id.as_str()) {
        id.push('_');
    }
    id
}

/// `base`, or `base_2`, `base_3`, … if `taken` says it is in use.
pub fn unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|id| !taken(id))
        .expect("an unbounded range always finds a free id")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(from_label("Orders DB"), "orders_db");
        assert_eq!(from_label("  API -- Gateway! "), "api_gateway");
        assert_eq!(from_label("C#"), "c");
        assert_eq!(from_label("2nd Cache"), "n2nd_cache");
        assert_eq!(from_label("Ünïcode"), "n_code");
        assert_eq!(from_label("ÄÖÜ"), "node");
        assert_eq!(from_label(""), "node");
        assert_eq!(from_label("End"), "end_");
        assert_eq!(from_label(&"x".repeat(50)).len(), MAX_LEN);
    }

    #[test]
    fn uniqueness() {
        let taken = ["api", "api_2"];
        assert_eq!(unique("api", |id| taken.contains(&id)), "api_3");
        assert_eq!(unique("web", |id| taken.contains(&id)), "web");
    }
}
