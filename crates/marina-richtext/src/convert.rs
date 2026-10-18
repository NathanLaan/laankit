//! HTML/XHTML → [`Document`].
//!
//! Articles are sanitized first with ThreadLiner's DOMPurify allow-list ([`sanitize`]). EPUB
//! chapters aren't (nothing is executed or rendered as HTML), but they carry CSS, which
//! [`Options::stylesheet`] supplies.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use scraper::node::Node;
use scraper::{ElementRef, Html};

use crate::css::{Decls, Length, StyleSheet};
use crate::model::{Align, Block, Document, Indent, Span, Style, TextKind};

/// ThreadLiner's `ContentViewer.svelte` `ALLOWED_TAGS`.
const ALLOWED_TAGS: &[&str] = &[
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "p",
    "br",
    "hr",
    "ul",
    "ol",
    "li",
    "a",
    "strong",
    "em",
    "b",
    "i",
    "u",
    "s",
    "blockquote",
    "pre",
    "code",
    "img",
    "picture",
    "source",
    "figure",
    "figcaption",
    "table",
    "thead",
    "tbody",
    "tr",
    "th",
    "td",
    "div",
    "span",
];
/// …and its `ALLOWED_ATTR` (plus `ALLOW_DATA_ATTR`).
const ALLOWED_ATTRS: &[&str] = &[
    "href", "src", "alt", "title", "class", "target", "rel", "width", "height", "srcset", "sizes",
    "loading", "media", "type",
];
/// The widest left indent kept, in ems (deeply nested `div`s in some books add up).
const MAX_INDENT: f32 = 6.0;
/// Images at most this big (declared size) are treated as inline glyphs: their alt text is kept.
const INLINE_IMAGE_MAX: f32 = 40.0;
/// Elements whose content is never text.
const SKIPPED: &[&str] = &[
    "script", "style", "head", "title", "template", "noscript", "iframe", "video", "audio",
    "object", "embed", "canvas", "form", "input", "button", "select", "textarea", "math",
];
/// Elements that are void in HTML; anything else written `<x/>` in XHTML must be reopened as
/// `<x></x>`, or the HTML parser would swallow what follows into it.
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// What links and images are relative to.
#[derive(Clone, Debug, Default)]
pub enum Base {
    #[default]
    None,
    /// An article: its permalink.
    Url(url::Url),
    /// An EPUB chapter: its path inside the archive (`OEBPS/text/ch01.xhtml`).
    Path(String),
}

#[derive(Clone, Debug, Default)]
pub struct Options<'a> {
    pub base: Base,
    pub stylesheet: Option<&'a StyleSheet>,
}

/// Converts an article's HTML: sanitize, then convert. `link` is the entry's permalink.
pub fn from_article_html(html: &str, link: Option<&str>) -> Document {
    let base = link
        .and_then(|l| url::Url::parse(l).ok())
        .map_or(Base::None, Base::Url);
    from_html(
        &sanitize(html),
        &Options {
            base,
            stylesheet: None,
        },
    )
}

/// Converts an XHTML document (an EPUB chapter).
pub fn from_xhtml(xhtml: &str, opts: &Options<'_>) -> Document {
    static SELF_CLOSING: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<([A-Za-z][\w:-]*)((?:\s[^<>]*?)?)\s*/>").expect("regex"));
    let fixed = SELF_CLOSING.replace_all(xhtml, |c: &regex::Captures<'_>| {
        let tag = &c[1];
        if VOID.contains(&tag.to_ascii_lowercase().as_str()) {
            c[0].to_string()
        } else {
            format!("<{tag}{}></{tag}>", &c[2])
        }
    });
    let html = Html::parse_document(&fixed);
    let root = html.root_element();
    let body = root
        .children()
        .filter_map(ElementRef::wrap)
        .find(|e| e.value().name() == "body")
        .unwrap_or(root);
    convert(body, opts)
}

/// Converts already-safe HTML (a fragment or a document).
pub fn from_html(html: &str, opts: &Options<'_>) -> Document {
    let html = Html::parse_fragment(html);
    convert(html.root_element(), opts)
}

/// ThreadLiner's sanitizer: the `<noscript><img></noscript>` unwrap, then DOMPurify's allow-list.
pub fn sanitize(html: &str) -> String {
    static NOSCRIPT_IMG: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)<noscript>\s*(<img\b[^>]*>)\s*</noscript>").expect("regex")
    });
    let html = NOSCRIPT_IMG.replace_all(html, "$1");
    ammonia::Builder::empty()
        .tags(ALLOWED_TAGS.iter().copied().collect())
        .generic_attributes(ALLOWED_ATTRS.iter().copied().collect())
        .generic_attribute_prefixes(HashSet::from(["data-"]))
        .clean_content_tags(HashSet::from(["script", "style"]))
        .url_schemes(HashSet::from(["http", "https", "mailto", "data"]))
        .url_relative(ammonia::UrlRelative::PassThrough)
        .link_rel(None)
        .clean(&html)
        .to_string()
}

fn convert(root: ElementRef<'_>, opts: &Options<'_>) -> Document {
    let mut ctx = Ctx {
        opts,
        pending_ids: Vec::new(),
    };
    let mut b = Builder::new(true);
    walk(root, Inherit::default(), None, &mut b, &mut ctx);
    b.flush(&mut ctx);
    if !ctx.pending_ids.is_empty() {
        let last = b.blocks.len().saturating_sub(1);
        for id in ctx.pending_ids.drain(..) {
            b.anchors.entry(id).or_insert(last);
        }
    }
    Document {
        blocks: b.blocks,
        anchors: b.anchors,
    }
}

struct Ctx<'a> {
    opts: &'a Options<'a>,
    /// Ids seen since the last top-level block; they anchor to the next one.
    pending_ids: Vec<String>,
}

/// What an element passes down to its content.
#[derive(Clone, Copy, Default)]
struct Inherit {
    style: Style,
    align: Align,
    indent: Indent,
    caps: Caps,
}

/// `text-transform: uppercase` / `font-variant: small-caps`, applied to the text itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Caps {
    #[default]
    None,
    Upper,
    /// Lower-case letters become small capitals.
    Small,
}

/// Collects blocks, and the inline spans of the paragraph being built.
struct Builder {
    top: bool,
    blocks: Vec<Block>,
    spans: Vec<Span>,
    align: Align,
    indent: Indent,
    anchors: HashMap<String, usize>,
}

impl Builder {
    fn new(top: bool) -> Self {
        Self {
            top,
            blocks: Vec::new(),
            spans: Vec::new(),
            align: Align::Left,
            indent: Indent::default(),
            anchors: HashMap::new(),
        }
    }

    fn push(&mut self, block: Block, ctx: &mut Ctx<'_>) {
        self.blocks.push(block);
        if self.top {
            let i = self.blocks.len() - 1;
            for id in ctx.pending_ids.drain(..) {
                self.anchors.entry(id).or_insert(i);
            }
        }
    }

    /// Whether the text so far ends at a word boundary (so a leading space can be dropped).
    fn at_space(&self) -> bool {
        self.spans
            .last()
            .and_then(|s| s.text.chars().last())
            .is_none_or(char::is_whitespace)
    }

    fn push_text(&mut self, text: &str, style: Style, caps: Caps, link: Option<&str>) {
        let mut collapsed = String::with_capacity(text.len());
        let mut space = self.at_space();
        for c in text.chars() {
            if c.is_whitespace() {
                if !space {
                    collapsed.push(' ');
                    space = true;
                }
            } else {
                collapsed.push(c);
                space = false;
            }
        }
        match caps {
            Caps::None => self.push_raw(collapsed, style, link),
            Caps::Upper => self.push_raw(upper(&collapsed), style, link),
            Caps::Small => {
                // Runs of lower-case letters, set as smaller capitals.
                let mut run = String::new();
                let mut run_small = false;
                for c in collapsed.chars() {
                    let small = c.is_lowercase() && single_upper(c).is_some();
                    if small != run_small && !run.is_empty() {
                        let st = Style {
                            small: run_small || style.small,
                            ..style
                        };
                        self.push_raw(std::mem::take(&mut run), st, link);
                    }
                    run_small = small;
                    run.push(single_upper(c).unwrap_or(c));
                }
                let st = Style {
                    small: run_small || style.small,
                    ..style
                };
                self.push_raw(run, st, link);
            }
        }
    }

    fn push_raw(&mut self, text: String, style: Style, link: Option<&str>) {
        if text.is_empty() {
            return;
        }
        match self.spans.last_mut() {
            Some(last) if last.style == style && last.link.as_deref() == link => {
                last.text.push_str(&text)
            }
            _ => self.spans.push(Span {
                text,
                style,
                link: link.map(str::to_string),
            }),
        }
    }

    fn line_break(&mut self) {
        if let Some(last) = self.spans.last_mut() {
            while last.text.ends_with(' ') {
                last.text.pop();
            }
        }
        self.push_raw("\n".into(), Style::default(), None);
    }

    /// Ends the paragraph being built.
    fn flush(&mut self, ctx: &mut Ctx<'_>) {
        let spans = take_spans(&mut self.spans);
        if !spans.is_empty() {
            self.push(
                Block::Text {
                    kind: TextKind::Paragraph,
                    spans,
                    align: self.align,
                    indent: self.indent,
                },
                ctx,
            );
        }
    }

    /// Everything collected, as blocks (for list items, quotes, captions).
    fn finish(mut self, ctx: &mut Ctx<'_>) -> Vec<Block> {
        self.flush(ctx);
        self.blocks
    }
}

/// `s` in upper case, letter for letter (characters whose capital is longer, like `ß`, stay as
/// they are, so positions inside the text don't move).
fn upper(s: &str) -> String {
    s.chars().map(|c| single_upper(c).unwrap_or(c)).collect()
}

fn single_upper(c: char) -> Option<char> {
    let mut up = c.to_uppercase();
    match (up.next(), up.next()) {
        (Some(u), None) => Some(u),
        _ => None,
    }
}

/// Takes `spans`, trimmed; empty when there's no visible text.
fn take_spans(spans: &mut Vec<Span>) -> Vec<Span> {
    let mut spans = std::mem::take(spans);
    if spans.iter().all(|s| s.text.trim().is_empty()) {
        return Vec::new();
    }
    while spans.first().is_some_and(|s| s.text.trim().is_empty()) {
        spans.remove(0);
    }
    while spans.last().is_some_and(|s| s.text.trim().is_empty()) {
        spans.pop();
    }
    if let Some(first) = spans.first_mut() {
        first.text = first.text.trim_start().to_string();
    }
    if let Some(last) = spans.last_mut() {
        last.text = last.text.trim_end().to_string();
    }
    spans
}

fn walk(el: ElementRef<'_>, inh: Inherit, link: Option<&str>, b: &mut Builder, ctx: &mut Ctx<'_>) {
    for child in el.children() {
        match child.value() {
            Node::Text(t) => b.push_text(t, inh.style, inh.caps, link),
            Node::Element(_) => {
                if let Some(e) = ElementRef::wrap(child) {
                    element(e, inh, link, b, ctx);
                }
            }
            _ => {}
        }
    }
}

fn element(
    el: ElementRef<'_>,
    inh: Inherit,
    link: Option<&str>,
    b: &mut Builder,
    ctx: &mut Ctx<'_>,
) {
    let e = el.value();
    let name = e.name().to_ascii_lowercase();
    let name = name.as_str();
    if SKIPPED.contains(&name) {
        return;
    }
    let mut decls = ctx
        .opts
        .stylesheet
        .map(|s| s.decls_for(name, e.id(), e.classes()))
        .unwrap_or_default();
    if let Some(style) = e.attr("style") {
        decls.merge(&Decls::parse(style));
    }
    if decls.hidden {
        return;
    }
    let inh = inherit(name, inh, &decls);
    let id = e
        .id()
        .or_else(|| if name == "a" { e.attr("name") } else { None })
        .map(str::to_string);
    let register = |ctx: &mut Ctx<'_>| {
        if let Some(id) = id.clone() {
            ctx.pending_ids.push(id);
        }
    };

    match name {
        "br" => b.line_break(),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            b.flush(ctx);
            register(ctx);
            let level = name.as_bytes()[1] - b'0';
            let mut sub = Builder::new(false);
            walk(el, inh, link, &mut sub, ctx);
            let spans = take_spans(&mut sub.spans);
            if !spans.is_empty() {
                b.push(
                    Block::Text {
                        kind: TextKind::Heading(level),
                        spans,
                        align: inh.align,
                        indent: Indent {
                            first_line: 0.0,
                            ..inh.indent
                        },
                    },
                    ctx,
                );
            }
            for block in sub.blocks {
                b.push(block, ctx);
            }
        }
        "figcaption" => {
            b.flush(ctx);
            register(ctx);
            let mut sub = Builder::new(false);
            sub.align = inh.align;
            walk(el, inh, link, &mut sub, ctx);
            let spans = take_spans(&mut sub.spans);
            for block in sub.blocks {
                b.push(block, ctx);
            }
            if !spans.is_empty() {
                b.push(
                    Block::Text {
                        kind: TextKind::Caption,
                        spans,
                        align: inh.align,
                        indent: Indent::default(),
                    },
                    ctx,
                );
            }
        }
        "ul" | "ol" | "menu" => {
            b.flush(ctx);
            register(ctx);
            let start = e
                .attr("start")
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(1);
            let mut items = Vec::new();
            for child in el.children() {
                let item = match ElementRef::wrap(child) {
                    Some(li) if li.value().name().eq_ignore_ascii_case("li") => {
                        let mut sub = Builder::new(false);
                        sub.align = inh.align;
                        sub.indent = Indent::default();
                        walk(li, inh, link, &mut sub, ctx);
                        sub.finish(ctx)
                    }
                    // Stray content between items (a nested list, text): its own item.
                    Some(other) => {
                        let mut sub = Builder::new(false);
                        element(other, inh, link, &mut sub, ctx);
                        sub.finish(ctx)
                    }
                    None => match child.value() {
                        Node::Text(t) if !t.trim().is_empty() => {
                            vec![Block::Text {
                                kind: TextKind::Paragraph,
                                spans: vec![plain(t.trim(), inh.style)],
                                align: inh.align,
                                indent: Indent::default(),
                            }]
                        }
                        _ => Vec::new(),
                    },
                };
                if !item.is_empty() {
                    items.push(item);
                }
            }
            if !items.is_empty() {
                b.push(
                    Block::List {
                        ordered: name == "ol",
                        start,
                        items,
                    },
                    ctx,
                );
            }
        }
        "blockquote" => {
            b.flush(ctx);
            register(ctx);
            let mut sub = Builder::new(false);
            walk(el, inh, link, &mut sub, ctx);
            let blocks = sub.finish(ctx);
            if !blocks.is_empty() {
                b.push(Block::Quote(blocks), ctx);
            }
        }
        "pre" => {
            b.flush(ctx);
            register(ctx);
            let text: String = el.text().collect();
            let text = text.trim_end().trim_start_matches('\n').to_string();
            if !text.is_empty() {
                b.push(Block::Code(text), ctx);
            }
        }
        "hr" => {
            b.flush(ctx);
            register(ctx);
            b.push(Block::Rule, ctx);
        }
        "img" => image(el, inh, decls.width, link, b, ctx),
        "svg" => {
            // EPUB covers: `<svg><image xlink:href="cover.jpg"/></svg>`. Other SVG is dropped.
            for img in el
                .descendants()
                .filter_map(ElementRef::wrap)
                .filter(|d| d.value().name() == "image")
            {
                let src = img
                    .value()
                    .attrs()
                    .find(|(k, _)| *k == "href" || k.ends_with(":href"))
                    .map(|(_, v)| v);
                if let Some(src) = src {
                    b.flush(ctx);
                    register(ctx);
                    let src = resolve(ctx.opts, src);
                    b.push(
                        Block::Image {
                            src,
                            alt: String::new(),
                            title: String::new(),
                            width: None,
                            height: None,
                            fraction: None,
                        },
                        ctx,
                    );
                }
            }
        }
        "table" => table(el, inh, link, b, ctx),
        "a" => {
            register(ctx);
            let href = e.attr("href").map(|h| resolve(ctx.opts, h));
            walk(el, inh, href.as_deref().or(link), b, ctx);
        }
        _ if is_block(name) => {
            b.flush(ctx);
            register(ctx);
            let saved = (b.align, b.indent);
            b.align = inh.align;
            b.indent = inh.indent;
            walk(el, inh, link, b, ctx);
            b.flush(ctx);
            (b.align, b.indent) = saved;
        }
        _ => {
            register(ctx);
            walk(el, inh, link, b, ctx);
        }
    }
}

fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "main"
            | "aside"
            | "nav"
            | "body"
            | "html"
            | "address"
            | "center"
            | "dl"
            | "dt"
            | "dd"
            | "details"
            | "summary"
            | "hgroup"
            | "figure"
            | "li"
            | "picture"
            | "tr"
            | "td"
            | "th"
            | "thead"
            | "tbody"
            | "tfoot"
    )
}

fn inherit(name: &str, mut inh: Inherit, decls: &Decls) -> Inherit {
    let s = &mut inh.style;
    match name {
        "b" | "strong" | "th" | "dt" => s.bold = true,
        "i" | "em" | "cite" | "dfn" | "var" => s.italic = true,
        "u" | "ins" => s.underline = true,
        "s" | "strike" | "del" => s.strike = true,
        "code" | "kbd" | "samp" | "tt" => s.code = true,
        "small" => s.small = true,
        "sup" => s.sup = true,
        "sub" => s.sub = true,
        "center" => inh.align = Align::Center,
        _ => {}
    }
    if let Some(up) = decls.superscript {
        (s.sup, s.sub) = (up, !up);
    }
    if let Some(left) = decls.indent_left {
        if is_block(name) || matches!(name, "blockquote" | "ul" | "ol" | "figcaption") {
            inh.indent.left = (inh.indent.left + left).clamp(0.0, MAX_INDENT);
        }
    }
    if let Some(first) = decls.text_indent {
        // A negative (hanging) indent would need the other lines indented instead.
        inh.indent.first_line = first.clamp(0.0, MAX_INDENT);
    }
    if decls.small_caps == Some(true) {
        inh.caps = Caps::Small;
    } else if decls.uppercase == Some(true) {
        inh.caps = Caps::Upper;
    } else if decls.small_caps == Some(false) && decls.uppercase.is_some() {
        inh.caps = Caps::None;
    }
    if let Some(v) = decls.bold {
        s.bold = v;
    }
    if let Some(v) = decls.italic {
        s.italic = v;
    }
    if let Some(v) = decls.underline {
        s.underline = v;
    }
    if let Some(v) = decls.strike {
        s.strike = v;
    }
    if let Some(v) = decls.monospace {
        s.code = v;
    }
    if let Some(a) = decls.align {
        inh.align = a;
    }
    inh
}

fn plain(text: &str, style: Style) -> Span {
    Span {
        text: text.to_string(),
        style,
        link: None,
    }
}

fn image(
    el: ElementRef<'_>,
    inh: Inherit,
    css_width: Option<Length>,
    link: Option<&str>,
    b: &mut Builder,
    ctx: &mut Ctx<'_>,
) {
    let e = el.value();
    let alt = e.attr("alt").unwrap_or("").trim().to_string();
    let dim = |a: &str| {
        e.attr(a)
            .and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok())
    };
    let (mut width, height) = (dim("width"), dim("height"));
    let fraction = match css_width {
        Some(Length::Percent(p)) if p > 0.0 => Some((p / 100.0).min(1.0)),
        Some(Length::Em(em)) if em > 0.0 => {
            width = Some(em * 16.0);
            None
        }
        _ => None,
    };
    let emoji = e
        .classes()
        .any(|c| c.contains("emoji") || c.contains("smiley"));
    if emoji
        || (width.is_some_and(|w| w <= INLINE_IMAGE_MAX)
            && height.is_some_and(|h| h <= INLINE_IMAGE_MAX))
    {
        // Tracking pixels and emoji: keep the alt text inline.
        if !alt.is_empty() {
            b.push_text(&alt, inh.style, Caps::None, link);
        }
        return;
    }
    let Some(src) = image_src(el) else { return };
    b.flush(ctx);
    if let Some(id) = e.id() {
        ctx.pending_ids.push(id.to_string());
    }
    let src = resolve(ctx.opts, &src);
    let title = e.attr("title").unwrap_or("").trim().to_string();
    b.push(
        Block::Image {
            src,
            alt,
            title,
            width,
            height,
            fraction,
        },
        ctx,
    );
}

/// `src`, fixed for lazy loaders as ThreadLiner does (`data-src` over a placeholder), else the
/// first `srcset` candidate (the `img`'s or a sibling `<source>`'s in a `<picture>`).
fn image_src(el: ElementRef<'_>) -> Option<String> {
    let e = el.value();
    let src = e.attr("src").unwrap_or("").trim();
    let lazy = ["data-src", "data-lazy-src", "data-original"]
        .iter()
        .find_map(|a| e.attr(a))
        .map(str::trim);
    let placeholder = src.is_empty()
        || src.starts_with("data:")
        || src.contains("placeholder")
        || src.contains("1x1");
    if let Some(lazy) = lazy.filter(|l| placeholder && !l.is_empty()) {
        return Some(lazy.to_string());
    }
    if !src.is_empty() {
        return Some(src.to_string());
    }
    let srcset = e
        .attr("srcset")
        .or_else(|| e.attr("data-srcset"))
        .map(str::to_string)
        .or_else(|| {
            let picture = el
                .parent()
                .and_then(ElementRef::wrap)
                .filter(|p| p.value().name() == "picture")?;
            picture
                .children()
                .filter_map(ElementRef::wrap)
                .find_map(|s| s.value().attr("srcset").map(str::to_string))
        })?;
    srcset
        .split(',')
        .next()
        .and_then(|c| c.split_whitespace().next())
        .map(str::to_string)
}

fn table(el: ElementRef<'_>, inh: Inherit, link: Option<&str>, b: &mut Builder, ctx: &mut Ctx<'_>) {
    let rows: Vec<ElementRef<'_>> = el
        .descendants()
        .filter_map(ElementRef::wrap)
        .filter(|r| {
            r.value().name() == "tr"
                && r.ancestors()
                    .filter_map(ElementRef::wrap)
                    .find(|a| a.value().name() == "table")
                    == Some(el)
        })
        .collect();
    let columns = rows.iter().map(|r| cells(*r).len()).max().unwrap_or(0);
    // (colspans aren't counted: a one-cell row spanning two columns is still a data table.)
    let nested = el
        .descendants()
        .filter_map(ElementRef::wrap)
        .skip(1)
        .any(|d| d.value().name() == "table");
    if columns <= 1 || nested {
        // A layout table (newsletters, old blogs): its content is the document's.
        b.flush(ctx);
        let saved = b.align;
        walk(el, inh, link, b, ctx);
        b.flush(ctx);
        b.align = saved;
        return;
    }
    b.flush(ctx);
    let mut out = Vec::new();
    let mut header_rows = 0;
    // Rows still covered by a `rowspan` from above, per column.
    let mut covered: Vec<usize> = Vec::new();
    for row in &rows {
        let cs = cells(*row);
        let in_head = row
            .parent()
            .and_then(ElementRef::wrap)
            .is_some_and(|p| p.value().name() == "thead");
        if out.len() == header_rows && (in_head || cs.iter().all(|c| c.value().name() == "th")) {
            header_rows += 1;
        }
        let mut cells_out: Vec<Vec<Span>> = Vec::new();
        // Pads the columns a cell above still covers, from the current column on.
        let pad = |cells_out: &mut Vec<Vec<Span>>, covered: &mut Vec<usize>| {
            while covered.get(cells_out.len()).is_some_and(|n| *n > 0) {
                covered[cells_out.len()] -= 1;
                cells_out.push(Vec::new());
            }
        };
        for cell in cs {
            pad(&mut cells_out, &mut covered);
            let mut sub = Builder::new(false);
            walk(cell, inh, link, &mut sub, ctx);
            let mut spans = take_spans(&mut sub.spans);
            for block in sub.blocks {
                let text = block.plain_text();
                if !text.is_empty() {
                    spans.push(plain(&format!(" {text}"), inh.style));
                }
            }
            // `colspan`: pad with empty cells so later columns stay aligned.
            let span = cell
                .value()
                .attr("colspan")
                .and_then(|c| c.trim().parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 16);
            let rows_down = cell
                .value()
                .attr("rowspan")
                .and_then(|c| c.trim().parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 64);
            let col = cells_out.len();
            if covered.len() < col + span {
                covered.resize(col + span, 0);
            }
            covered[col..col + span].fill(rows_down - 1);
            cells_out.push(spans);
            cells_out.extend(std::iter::repeat_n(Vec::new(), span - 1));
        }
        // Trailing columns covered from above.
        while cells_out.len() < covered.len() {
            if covered[cells_out.len()] > 0 {
                covered[cells_out.len()] -= 1;
            }
            cells_out.push(Vec::new());
        }
        out.push(cells_out);
    }
    if !out.is_empty() {
        b.push(
            Block::Table {
                rows: out,
                header_rows,
            },
            ctx,
        );
    }
}

fn cells(row: ElementRef<'_>) -> Vec<ElementRef<'_>> {
    row.children()
        .filter_map(ElementRef::wrap)
        .filter(|c| matches!(c.value().name(), "td" | "th"))
        .collect()
}

/// Resolves a link or image reference against the document's base.
pub fn resolve(opts: &Options<'_>, href: &str) -> String {
    let href = href.trim();
    let has_scheme = href.split_once(':').is_some_and(|(s, _)| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    });
    if has_scheme {
        return href.to_string();
    }
    match &opts.base {
        Base::None => href.to_string(),
        Base::Url(base) => base
            .join(href)
            .map_or_else(|_| href.to_string(), |u| u.to_string()),
        Base::Path(doc) => {
            let (path, frag) = href
                .split_once('#')
                .map_or((href, None), |(p, f)| (p, Some(f)));
            let path = percent_encoding::percent_decode_str(path).decode_utf8_lossy();
            let joined = if path.is_empty() {
                doc.clone()
            } else {
                join_path(doc, &path)
            };
            match frag {
                Some(f) => format!("{joined}#{f}"),
                None => joined,
            }
        }
    }
}

/// `rel` relative to the directory of `doc`, with `.` and `..` folded.
pub fn join_path(doc: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = if rel.starts_with('/') {
        Vec::new()
    } else {
        doc.split('/').collect()
    };
    if !rel.starts_with('/') {
        parts.pop();
    }
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(doc: &Document) -> Vec<String> {
        doc.blocks.iter().map(Block::plain_text).collect()
    }

    #[test]
    fn paragraphs_headings_and_inline_styles() {
        let doc = from_article_html(
            "<h2 id=intro>Intro</h2><p>Some <b>bold</b>, <em>italic</em> and <a href=\"/x\">a  link</a>.</p>\
             <div>Loose <code>code</code><br>next line</div>",
            Some("https://example.com/post/1"),
        );
        assert_eq!(
            texts(&doc),
            [
                "Intro",
                "Some bold, italic and a link.",
                "Loose code\nnext line"
            ]
        );
        let Block::Text {
            kind: TextKind::Heading(2),
            ..
        } = &doc.blocks[0]
        else {
            panic!("{:?}", doc.blocks[0])
        };
        let Block::Text { spans, .. } = &doc.blocks[1] else {
            panic!()
        };
        assert!(spans.iter().any(|s| s.style.bold && s.text == "bold"));
        assert!(spans.iter().any(|s| s.style.italic && s.text == "italic"));
        let link = spans.iter().find(|s| s.link.is_some()).unwrap();
        assert_eq!(
            (link.text.as_str(), link.link.as_deref()),
            ("a link", Some("https://example.com/x"))
        );
        // DOMPurify's ALLOWED_ATTR (and so the sanitizer) drops ids.
        assert!(doc.anchors.is_empty());
    }

    #[test]
    fn sanitizer_drops_scripts_and_embeds() {
        let doc = from_article_html(
            "<p>Before</p><script>alert(1)</script><iframe src=x>fallback</iframe><style>p{}</style><p>After</p>",
            None,
        );
        assert_eq!(texts(&doc), ["Before", "fallback", "After"]);
    }

    #[test]
    fn lists_quotes_code_rules() {
        let doc = from_html(
            "<ol start=3><li>one</li><li>two<ul><li>nested</li></ul></li></ol><blockquote><p>q</p></blockquote>\
             <pre><code>fn main() {\n    x();\n}</code></pre><hr>",
            &Options::default(),
        );
        let Block::List {
            ordered: true,
            start: 3,
            items,
        } = &doc.blocks[0]
        else {
            panic!("{:?}", doc.blocks[0])
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(items[1][1], Block::List { ordered: false, .. }));
        assert!(matches!(&doc.blocks[1], Block::Quote(b) if b.len() == 1));
        assert_eq!(
            doc.blocks[2],
            Block::Code("fn main() {\n    x();\n}".into())
        );
        assert_eq!(doc.blocks[3], Block::Rule);
    }

    #[test]
    fn images_lazy_sources_and_figures() {
        let doc = from_article_html(
            "<figure><img src=\"data:image/gif;base64,R0\" data-src=\"/a.jpg\" alt=\"A\" width=800>\
             <figcaption>Cap</figcaption></figure><p>x <img class=emoji alt=\"🙂\" src=e.png> y</p>\
             <picture><source srcset=\"b.webp 1x, b2.webp 2x\"><img alt=B></picture>",
            Some("https://ex.com/p/"),
        );
        assert_eq!(
            doc.blocks[0],
            Block::Image {
                src: "https://ex.com/a.jpg".into(),
                alt: "A".into(),
                title: String::new(),
                width: Some(800.0),
                height: None,
                fraction: None,
            }
        );
        assert!(matches!(
            &doc.blocks[1],
            Block::Text {
                kind: TextKind::Caption,
                ..
            }
        ));
        assert_eq!(doc.blocks[2].plain_text(), "x 🙂 y");
        assert!(
            matches!(&doc.blocks[3], Block::Image { src, .. } if src == "https://ex.com/p/b.webp")
        );
    }

    #[test]
    fn row_and_column_spans_line_up() {
        let doc = from_html(
            "<table><tr><th>A</th><th>B</th><th>C</th></tr>\
             <tr><td rowspan=2>a</td><td colspan=2>bc</td></tr>\
             <tr><td>b2</td><td>c2</td></tr></table>",
            &Options::default(),
        );
        let Block::Table { rows, .. } = &doc.blocks[0] else {
            panic!("{:?}", doc.blocks[0])
        };
        let text = |r: usize| -> Vec<String> {
            rows[r]
                .iter()
                .map(|c| c.iter().map(|s| s.text.as_str()).collect::<String>())
                .collect()
        };
        assert_eq!(text(1), ["a", "bc", ""]);
        // The `rowspan` cell's column is padded, so `b2` stays under B.
        assert_eq!(text(2), ["", "b2", "c2"]);
    }

    #[test]
    fn book_css_indents_caps_and_scripts() {
        let sheet = StyleSheet::parse(
            "p.body { text-indent: 1.5em; text-align: justify } div.verse { margin-left: 2em } \
             span.sc { font-variant: small-caps } span.fn { vertical-align: super }",
        );
        let opts = Options {
            base: Base::None,
            stylesheet: Some(&sheet),
        };
        let doc = from_html(
            "<p class=body>Prose<span class=fn>1</span> <sup>2</sup> H<sub>2</sub>O</p>\
             <div class=verse><p>Verse</p></div><p><span class=sc>Hello</span> World</p>\
             <p><img src=a.png style='width: 50%'></p>",
            &opts,
        );
        let Block::Text {
            align,
            indent,
            spans,
            ..
        } = &doc.blocks[0]
        else {
            panic!()
        };
        assert_eq!(*align, Align::Justify);
        assert_eq!(indent.first_line, 1.5);
        assert!(spans.iter().any(|s| s.text == "1" && s.style.sup));
        assert!(spans.iter().any(|s| s.text == "2" && s.style.sup));
        assert!(spans.iter().any(|s| s.text == "2" && s.style.sub));
        let Block::Text { indent, .. } = &doc.blocks[1] else {
            panic!()
        };
        assert_eq!(indent.left, 2.0);
        let Block::Text { spans, .. } = &doc.blocks[2] else {
            panic!()
        };
        assert_eq!(doc.blocks[2].plain_text(), "HELLO World");
        assert!(spans.iter().any(|s| s.text == "ELLO" && s.style.small));
        assert!(matches!(
            &doc.blocks[3],
            Block::Image {
                fraction: Some(f),
                ..
            } if (*f - 0.5).abs() < 1e-6
        ));
    }

    #[test]
    fn text_parts_follow_layout_order() {
        let doc = from_html(
            "<ul><li>one</li><li>two<blockquote>q</blockquote></li></ul>",
            &Options::default(),
        );
        assert_eq!(doc.blocks[0].text_parts(), ["one", "two", "q"]);
        assert_eq!(doc.blocks[0].part_text(), "one\ntwo\nq");
    }

    #[test]
    fn tables_data_and_layout() {
        let doc = from_html(
            "<table><thead><tr><th>A</th><th>B</th></tr></thead><tr><td>1</td><td><p>2</p></td></tr></table>\
             <table><tr><td><p>layout para</p></td></tr></table>",
            &Options::default(),
        );
        let Block::Table {
            rows,
            header_rows: 1,
        } = &doc.blocks[0]
        else {
            panic!("{:?}", doc.blocks[0])
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[1][1]
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
                .trim(),
            "2"
        );
        assert_eq!(doc.blocks[1].plain_text(), "layout para");
    }

    #[test]
    fn xhtml_css_anchors_and_paths() {
        let sheet = StyleSheet::parse(
            ".c { text-align: center } .it { font-style: italic } .gone { display: none }",
        );
        let opts = Options {
            base: Base::Path("OEBPS/text/ch1.xhtml".into()),
            stylesheet: Some(&sheet),
        };
        let doc = from_xhtml(
            "<?xml version=\"1.0\"?><html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>T</title></head><body>\
             <p class=\"c\">Centred</p><a id=\"p12\"/><p>Text <span class=\"it\">it</span> \
             <a href=\"ch2.xhtml#s%201\">next</a></p><p class=\"gone\">hidden</p>\
             <img src=\"../images/a%20b.png\" alt=\"\"/></body></html>",
            &opts,
        );
        assert_eq!(texts(&doc), ["Centred", "Text it next", ""]);
        assert!(matches!(
            &doc.blocks[0],
            Block::Text {
                align: Align::Center,
                ..
            }
        ));
        let Block::Text { spans, .. } = &doc.blocks[1] else {
            panic!()
        };
        assert!(spans.iter().any(|s| s.style.italic && s.text == "it"));
        assert_eq!(
            spans.last().unwrap().link.as_deref(),
            Some("OEBPS/text/ch2.xhtml#s%201")
        );
        assert_eq!(doc.anchors.get("p12"), Some(&1));
        assert!(
            matches!(&doc.blocks[2], Block::Image { src, .. } if src == "OEBPS/images/a b.png")
        );
    }
}
