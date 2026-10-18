//! Markdown → Typst markup, walking comrak's AST.
//!
//! Text is emitted as markup with every syntactically meaningful ASCII
//! character backslash-escaped ([`escape_text`]); code, URLs and paths go
//! through Typst string literals ([`string_literal`]). Structure is emitted as
//! function calls (`#list(..)`, `#table(..)`, `#heading(..)`) rather than
//! indentation-sensitive markup, so nesting can't be misread.

use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;

use comrak::nodes::{AstNode, ListType, NodeValue, TableAlignment};
use comrak::{Arena, Options};

/// Image extensions Typst can embed; anything else becomes a text placeholder
/// instead of failing the whole export.
const IMAGE_EXTENSIONS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "svg", "svgz", "webp", "pdf"];
/// Nested lists and quotes deeper than this are flattened: Typst stops at 64
/// nested show rules, which is about 31 list levels.
const MAX_NESTING: usize = 24;
/// Words longer than this get break opportunities ([`break_long_words`]).
const LONG_WORD: usize = 60;
const BREAK_EVERY: usize = 30;

/// The page and typography setup: Letter with half-inch margins (Electron's
/// `printToPDF` settings) plus the 24 px body padding of `export::pdf_document`,
/// and the same colours as its stylesheet.
const PREAMBLE: &str = r##"#set page(paper: "us-letter", margin: (y: 0.5in, x: 0.5in + 18pt))
#set text(size: 11pt, fill: rgb("#1a1a1a"), hyphenate: false)
#set par(leading: 0.75em, spacing: 1.1em)
#show heading: set block(above: 1.5em, below: 0.8em)
#show heading.where(level: 1): set text(size: 1.9em)
#show heading.where(level: 2): set text(size: 1.45em)
#show heading.where(level: 3): set text(size: 1.2em)
#show link: set text(fill: rgb("#0b62c4"))
#show raw: set text(font: "DejaVu Sans Mono", size: 0.9em)
#show raw.where(block: false): box.with(fill: rgb("#f0f0f0"), inset: (x: 4pt), outset: (y: 3pt), radius: 3pt)
#show raw.where(block: true): block.with(fill: rgb("#f0f0f0"), inset: 12pt, radius: 6pt, width: 100%)
#show quote.where(block: true): it => block(stroke: (left: 3pt + rgb("#cccccc")), inset: (left: 12pt, y: 2pt), text(fill: rgb("#555555"), it.body))
#set table(stroke: 0.75pt + rgb("#dddddd"), inset: (x: 9pt, y: 6pt), fill: (_, y) => if y == 0 { rgb("#f5f5f5") })
#show table.cell.where(y: 0): set text(weight: "bold")
#let md-image(src, alt) = layout(size => {
  let img = image(src, alt: alt)
  if measure(img).width > size.width { image(src, alt: alt, width: 100%) } else { img }
})
#let md-task(done) = box(baseline: 0.12em, width: 0.8em, height: 0.8em, stroke: 0.7pt + rgb("#555555"), radius: 1.5pt, inset: 0pt, if done { align(center + horizon, text(size: 0.75em, "✓")) })
#let md-wikilink(body) = text(fill: rgb("#0b62c4"), body)
"##;

/// How the converter resolves image URLs: returns the Typst virtual path
/// (`/…`, relative to the export root) when the file exists and is embeddable.
pub trait ImageResolver {
    fn resolve(&self, url: &str) -> Option<String>;
}

/// Images under `root` (the project folder, whose `_attachments/` holds
/// pasted images), addressed as `_attachments/x.png` or `./_attachments/x.png`.
pub struct DirImages<'a> {
    pub root: &'a Path,
}

impl ImageResolver for DirImages<'_> {
    fn resolve(&self, url: &str) -> Option<String> {
        if url.contains("://") || url.starts_with("data:") {
            return None;
        }
        let rel = percent_decode(url.trim_start_matches("./").trim_start_matches('/'));
        if rel.split(['/', '\\']).any(|s| s == "..") {
            return None;
        }
        let ext = Path::new(&rel).extension()?.to_str()?.to_ascii_lowercase();
        if !IMAGE_EXTENSIONS.contains(&ext.as_str()) || !self.root.join(&rel).is_file() {
            return None;
        }
        Some(format!("/{rel}"))
    }
}

/// No images (tests).
pub struct NoImages;

impl ImageResolver for NoImages {
    fn resolve(&self, _url: &str) -> Option<String> {
        None
    }
}

/// The full Typst document for exporting note `name` (body without
/// frontmatter): the preamble, the note name as the title heading (like
/// `export::pdf_document`), then the body.
///
/// `family` is the body font (e.g. the app's bundled sans), falling back to
/// Typst's embedded Libertinus Serif.
pub fn document(
    name: &str,
    body_md: &str,
    family: Option<&str>,
    images: &dyn ImageResolver,
) -> String {
    let mut out = String::with_capacity(PREAMBLE.len() + body_md.len() * 2);
    out.push_str(PREAMBLE);
    if let Some(family) = family {
        let _ = writeln!(
            out,
            "#set text(font: ({}, \"Libertinus Serif\"))",
            string_literal(family)
        );
    }
    let _ = writeln!(out, "#set document(title: {})", string_literal(name));
    let _ = writeln!(out, "#heading(level: 1)[{}]\n", escape_text(name));
    out.push_str(&markdown_to_typst(body_md, images));
    out
}

/// The comrak options NoteLiner's export uses (`export::markdown_to_html`),
/// plus footnotes and `[[Real Name|Label]]` wikilinks.
pub fn options() -> Options<'static> {
    let mut opts = Options::default();
    opts.extension.table = true;
    opts.extension.strikethrough = true;
    opts.extension.autolink = true;
    opts.extension.tasklist = true;
    opts.extension.footnotes = true;
    opts.extension.superscript = false;
    opts.extension.wikilinks_title_after_pipe = true;
    opts
}

/// The body markup only (no preamble).
pub fn markdown_to_typst(md: &str, images: &dyn ImageResolver) -> String {
    let arena = Arena::new();
    let root = comrak::parse_document(&arena, md, &options());
    let mut footnotes = HashMap::new();
    for node in root.descendants() {
        if let NodeValue::FootnoteDefinition(def) = &node.data.borrow().value {
            footnotes.insert(def.name.clone(), node);
        }
    }
    let mut w = Writer {
        out: String::new(),
        images,
        footnotes,
        footnotes_used: HashMap::new(),
        nesting: 0,
    };
    w.blocks(root);
    w.out
}

struct Writer<'a, 'n> {
    out: String,
    images: &'a dyn ImageResolver,
    footnotes: HashMap<String, &'n AstNode<'n>>,
    /// Footnotes already placed, by name → their label (later references point back).
    footnotes_used: HashMap<String, String>,
    /// Current list/quote nesting depth.
    nesting: usize,
}

impl<'n> Writer<'_, 'n> {
    /// Block children, separated by paragraph breaks.
    fn blocks(&mut self, node: &'n AstNode<'n>) {
        let mut first = true;
        for child in node.children() {
            if matches!(
                child.data.borrow().value,
                NodeValue::FootnoteDefinition(_) | NodeValue::FrontMatter(_)
            ) {
                continue;
            }
            if !first {
                self.out.push_str("\n\n");
            }
            first = false;
            self.block(child);
        }
    }

    fn block(&mut self, node: &'n AstNode<'n>) {
        let value = node.data.borrow().value.clone();
        match value {
            NodeValue::Paragraph => self.inlines(node),
            NodeValue::Heading(h) => {
                // The note name is the level-1 title, so Markdown headings nest under it.
                let _ = write!(self.out, "#heading(level: {})[", (h.level + 1).min(6));
                self.inlines(node);
                self.out.push(']');
            }
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) | NodeValue::Alert(_)
                if self.nesting >= MAX_NESTING =>
            {
                self.blocks(node)
            }
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) | NodeValue::Alert(_) => {
                self.nesting += 1;
                self.out.push_str("#quote(block: true)[");
                self.blocks(node);
                self.out.push(']');
                self.nesting -= 1;
            }
            NodeValue::List(list) => self.list(node, list.list_type, list.start, list.tight),
            NodeValue::CodeBlock(code) => {
                let lang = code.info.split_whitespace().next().unwrap_or("");
                let _ = write!(self.out, "#raw(block: true, ");
                if !lang.is_empty() {
                    let _ = write!(self.out, "lang: {}, ", string_literal(lang));
                }
                let _ = write!(
                    self.out,
                    "{})",
                    string_literal(code.literal.trim_end_matches('\n'))
                );
            }
            NodeValue::HtmlBlock(html) => {
                // Raw HTML can't be laid out; keep its text.
                let text = strip_tags(&html.literal);
                if !text.trim().is_empty() {
                    self.out.push_str(&escape_text(text.trim()));
                }
            }
            NodeValue::ThematicBreak => self
                .out
                .push_str("#line(length: 100%, stroke: 0.75pt + rgb(\"#cccccc\"))"),
            NodeValue::Table(table) => self.table(node, &table.alignments),
            NodeValue::DescriptionList => {
                self.out.push_str("#terms(");
                for item in node.children() {
                    let mut parts = item.children();
                    let (Some(term), Some(details)) = (parts.next(), parts.next()) else {
                        continue;
                    };
                    self.out.push_str("terms.item([");
                    self.inline_or_blocks(term);
                    self.out.push_str("], [");
                    self.blocks(details);
                    self.out.push_str("]), ");
                }
                self.out.push(')');
            }
            // Anything else at block level (e.g. a stray inline) is laid out as inline content.
            _ => self.inline(node),
        }
    }

    fn list(&mut self, node: &'n AstNode<'n>, kind: ListType, start: usize, tight: bool) {
        if self.nesting >= MAX_NESTING {
            return self.flat_list(node, kind, start);
        }
        self.nesting += 1;
        match kind {
            ListType::Bullet => {
                let _ = write!(self.out, "#list(tight: {tight}, ");
            }
            ListType::Ordered => {
                let _ = write!(self.out, "#enum(tight: {tight}, start: {start}, ");
            }
        }
        for item in node.children() {
            self.out.push('[');
            if let NodeValue::TaskItem(mark) = item.data.borrow().value {
                let done = mark.is_some_and(|c| c != ' ');
                let _ = write!(self.out, "#md-task({done}) ");
            }
            self.blocks(item);
            self.out.push_str("], ");
        }
        self.out.push(')');
        self.nesting -= 1;
    }

    /// A list past [`MAX_NESTING`]: its items as paragraphs with text markers.
    fn flat_list(&mut self, node: &'n AstNode<'n>, kind: ListType, start: usize) {
        for (i, item) in node.children().enumerate() {
            if i > 0 {
                self.out.push_str("\n\n");
            }
            match kind {
                ListType::Bullet => self.out.push_str("• "),
                ListType::Ordered => {
                    let _ = write!(self.out, "{}\\. ", start + i);
                }
            }
            self.blocks(item);
        }
    }

    fn table(&mut self, node: &'n AstNode<'n>, alignments: &[TableAlignment]) {
        let align: Vec<&str> = alignments
            .iter()
            .map(|a| match a {
                TableAlignment::Center => "center",
                TableAlignment::Right => "right",
                _ => "left",
            })
            .collect();
        let _ = write!(
            self.out,
            "#table(columns: {}, align: ({},), ",
            align.len(),
            align.join(", ")
        );
        for row in node.children() {
            let header = matches!(row.data.borrow().value, NodeValue::TableRow(true));
            if header {
                self.out.push_str("table.header(");
            }
            for cell in row.children() {
                self.out.push('[');
                self.inlines(cell);
                self.out.push_str("], ");
            }
            // comrak omits trailing empty cells; pad so rows stay aligned.
            for _ in row.children().count()..align.len() {
                self.out.push_str("[], ");
            }
            if header {
                self.out.push_str("), ");
            }
        }
        self.out.push(')');
    }

    fn inline_or_blocks(&mut self, node: &'n AstNode<'n>) {
        let block_children = node.children().any(|c| c.data.borrow().value.block());
        if block_children {
            self.blocks(node);
        } else {
            self.inlines(node);
        }
    }

    fn inlines(&mut self, node: &'n AstNode<'n>) {
        for child in node.children() {
            self.inline(child);
        }
    }

    fn wrap(&mut self, func: &str, node: &'n AstNode<'n>) {
        let _ = write!(self.out, "#{func}[");
        self.inlines(node);
        self.out.push(']');
    }

    fn inline(&mut self, node: &'n AstNode<'n>) {
        let value = node.data.borrow().value.clone();
        match value {
            NodeValue::Text(text) => self.out.push_str(&escape_text(&break_long_words(&text))),
            NodeValue::SoftBreak => self.out.push(' '),
            NodeValue::LineBreak => self.out.push_str("#linebreak()"),
            NodeValue::Code(code) => {
                let _ = write!(self.out, "#raw({})", string_literal(&code.literal));
            }
            NodeValue::Emph => self.wrap("emph", node),
            NodeValue::Strong => self.wrap("strong", node),
            NodeValue::Strikethrough => self.wrap("strike", node),
            NodeValue::Underline => self.wrap("underline", node),
            NodeValue::Superscript => self.wrap("super", node),
            NodeValue::Subscript => self.wrap("sub", node),
            NodeValue::Link(link) => {
                if link.url.is_empty() || link.url.starts_with('#') {
                    self.inlines(node);
                } else {
                    let _ = write!(self.out, "#link({})[", string_literal(&link.url));
                    self.inlines(node);
                    self.out.push(']');
                }
            }
            NodeValue::WikiLink(_) => {
                // A standalone PDF has no notes to link to; show the label styled as a link.
                self.wrap("md-wikilink", node);
            }
            NodeValue::Image(link) => {
                let alt: String = node
                    .descendants()
                    .filter_map(|d| {
                        if let NodeValue::Text(t) = &d.data.borrow().value {
                            Some(t.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                match self.images.resolve(&link.url) {
                    Some(path) => {
                        let _ = write!(
                            self.out,
                            "#md-image({}, {})",
                            string_literal(&path),
                            string_literal(&alt)
                        );
                    }
                    None => {
                        let label = if alt.is_empty() {
                            "image".to_string()
                        } else {
                            format!("image: {alt}")
                        };
                        let _ = write!(
                            self.out,
                            "#text(fill: rgb(\"#777777\"))[\\[{}\\]]",
                            escape_text(&label)
                        );
                    }
                }
            }
            NodeValue::FootnoteReference(r) => self.footnote(&r.name),
            NodeValue::HtmlInline(html) => {
                let tag = html.trim().to_ascii_lowercase();
                if tag.starts_with("<br") {
                    self.out.push_str("#linebreak()");
                }
                // Other tags are dropped; the text between them still flows.
            }
            NodeValue::Raw(raw) => self.out.push_str(&escape_text(&raw)),
            NodeValue::Escaped | NodeValue::SpoileredText => self.inlines(node),
            NodeValue::Math(math) => {
                let _ = write!(self.out, "#raw({})", string_literal(&math.literal));
            }
            _ if node.first_child().is_some() => self.inline_or_blocks(node),
            _ => {}
        }
    }

    /// First reference: the footnote with its body and a label. Later references point back.
    fn footnote(&mut self, name: &str) {
        if let Some(label) = self.footnotes_used.get(name) {
            let _ = write!(self.out, "#footnote(<{label}>)");
            return;
        }
        let Some(def) = self.footnotes.get(name).copied() else {
            let _ = write!(self.out, "\\[^{}\\]", escape_text(name));
            return;
        };
        let label = format!("fn-{}", self.footnotes_used.len() + 1);
        self.footnotes_used.insert(name.to_string(), label.clone());
        self.out.push_str("#footnote[");
        self.blocks(def);
        let _ = write!(self.out, "]<{label}>");
    }
}

/// Escapes `text` for Typst markup: every ASCII character with markup meaning
/// gets a backslash, whitespace runs become one space (as HTML does), and
/// straight quotes are left for Typst's smart quotes.
pub fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut in_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
            continue;
        }
        in_space = false;
        if c.is_ascii_punctuation()
            && !matches!(
                c,
                '\'' | '"' | ',' | ';' | '!' | '?' | '(' | ')' | '%' | '&' | '|' | '^'
            )
        {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Lets unbreakable runs (long URLs, hashes) wrap instead of running off the
/// page, by putting a zero-width space every [`BREAK_EVERY`] characters of
/// any word longer than [`LONG_WORD`].
fn break_long_words(text: &str) -> std::borrow::Cow<'_, str> {
    if !text
        .split_whitespace()
        .any(|w| w.chars().count() > LONG_WORD)
    {
        return text.into();
    }
    let mut out = String::with_capacity(text.len() + text.len() / BREAK_EVERY * 3);
    let mut run = 0;
    for c in text.chars() {
        run = if c.is_whitespace() { 0 } else { run + 1 };
        if run > BREAK_EVERY {
            out.push('\u{200B}');
            run = 1;
        }
        out.push(c);
    }
    out.into()
}

/// A Typst string literal (`"…"`) holding exactly `s`.
pub fn string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use typst::syntax::{parse, SyntaxKind, SyntaxNode};

    /// Characters that mean something in Typst markup, Markdown, or both.
    const TRICKY: &[&str] = &[
        "#",
        "*",
        "_",
        "`",
        "$",
        "[",
        "]",
        "<",
        ">",
        "@",
        "=",
        "-",
        "+",
        "/",
        "~",
        "\\",
        "'",
        "\"",
        ".",
        "...",
        "--",
        "---",
        "1.",
        ":",
        "http://x.y",
        "//",
        "/*",
        "*/",
        "\\u{41}",
        "{",
        "}",
        " ",
        "\t",
        "é",
        "😀",
        "a",
        "Z",
        "9",
        "<label>",
        "@ref",
        "= h",
        "- l",
        "+ e",
        "/ t: d",
        "-?",
    ];

    /// The visible text of escaped markup, failing on anything that isn't
    /// plain text (markup syntax that slipped through).
    fn plain_text(node: &SyntaxNode, out: &mut String) -> Result<(), String> {
        match node.kind() {
            SyntaxKind::Markup => {
                for child in node.children() {
                    plain_text(child, out)?;
                }
                Ok(())
            }
            SyntaxKind::Text => {
                out.push_str(node.leaf_text());
                Ok(())
            }
            SyntaxKind::Escape => {
                out.push_str(&node.leaf_text()[1..]);
                Ok(())
            }
            SyntaxKind::SmartQuote => {
                out.push_str(node.leaf_text());
                Ok(())
            }
            SyntaxKind::Space => {
                out.push(' ');
                Ok(())
            }
            other => Err(format!("{other:?} in {:?}", node.leaf_text())),
        }
    }

    fn normalize_space(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn assert_roundtrip(input: &str) {
        let escaped = escape_text(input);
        let tree = parse(&escaped);
        assert!(
            tree.errors_and_warnings().0.is_empty(),
            "{input:?} → {escaped:?}: {:?}",
            tree.errors_and_warnings().0
        );
        let mut text = String::new();
        if let Err(e) = plain_text(&tree, &mut text) {
            panic!("{input:?} → {escaped:?}: {e}");
        }
        // Leading/trailing spaces are dropped by the parser's own handling; compare trimmed.
        assert_eq!(
            text.trim(),
            normalize_space(input).trim(),
            "{input:?} → {escaped:?}"
        );
    }

    #[test]
    fn escapes_each_tricky_token() {
        for t in TRICKY {
            assert_roundtrip(t);
            assert_roundtrip(&format!("x{t}y"));
            assert_roundtrip(&format!("{t} {t}"));
        }
    }

    #[test]
    fn fuzz_escaping() {
        // xorshift: deterministic, no extra dependency.
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..20_000 {
            let len = (next() % 12) as usize;
            let s: String = (0..len)
                .map(|_| TRICKY[(next() % TRICKY.len() as u64) as usize])
                .collect();
            assert_roundtrip(&s);
        }
        // Every printable ASCII character, alone and in pairs.
        for a in 0x20u8..0x7f {
            assert_roundtrip(&(a as char).to_string());
            for b in 0x20u8..0x7f {
                assert_roundtrip(&format!("{}{}", a as char, b as char));
            }
        }
    }

    #[test]
    fn string_literals_roundtrip() {
        for t in TRICKY
            .iter()
            .chain(["a\nb", "\r\n", "\"quoted\"", "back\\slash"].iter())
        {
            let lit = string_literal(t);
            let code = typst::syntax::parse_code(&lit);
            assert!(code.errors_and_warnings().0.is_empty(), "{t:?} → {lit:?}");
        }
    }

    #[test]
    fn markdown_structures_emit_calls() {
        let md = "# H\n\n- [ ] todo\n- [x] done\n\n1. one\n2. two\n\n> quote\n\n| a | b |\n|:-|-:|\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n\nSee [[Real Name|Label]] and [[Plain]].\n\nNote[^1] and again[^1].\n\n[^1]: The footnote.\n\n---\n";
        let t = markdown_to_typst(md, &NoImages);
        for needle in [
            "#heading(level: 2)[H]",
            "#md-task(false)",
            "#md-task(true)",
            "#enum(tight: true, start: 1, ",
            "#quote(block: true)[",
            "#table(columns: 2, align: (left, right,), table.header(",
            "#raw(block: true, lang: \"rust\", \"fn main() {}\")",
            "#md-wikilink[Label]",
            "#md-wikilink[Plain]",
            "#footnote[The footnote\\.]<fn-1>",
            "#footnote(<fn-1>)",
            "#line(length: 100%",
        ] {
            assert!(t.contains(needle), "missing {needle:?} in:\n{t}");
        }
        let tree = parse(&t);
        assert!(
            tree.errors_and_warnings().0.is_empty(),
            "{:?}",
            tree.errors_and_warnings().0
        );
    }

    #[test]
    fn missing_and_remote_images_become_placeholders() {
        let t = markdown_to_typst(
            "![Cat](_attachments/none.png) ![Web](https://x.y/a.png)",
            &DirImages {
                root: Path::new("/nonexistent"),
            },
        );
        assert!(
            t.contains("image\\: Cat") && t.contains("image\\: Web"),
            "{t}"
        );
        assert!(!t.contains("md-image"));
    }

    #[test]
    fn long_words_get_break_opportunities() {
        assert_eq!(break_long_words("short words stay"), "short words stay");
        let long = "a".repeat(70);
        assert_eq!(break_long_words(&long).matches('\u{200B}').count(), 2);
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("a%20b%2Fc%"), "a b/c%");
    }
}
