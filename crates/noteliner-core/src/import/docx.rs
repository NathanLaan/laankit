//! Word import. The Electron build ran mammoth.convertToMarkdown with a
//! small style map and tables stripped; this converter reproduces
//! mammoth's model and Markdown writer: style-mapped headings, numbered/
//! bulleted lists (tab-indented), `__bold__` / `*italic*`, links, images,
//! bookmarks as `<a id>` anchors, and footnotes/endnotes appended as a
//! numbered list. One deliberate difference: Quote styles render as `> `
//! blockquotes (mammoth emitted their text with no separator).

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use roxmltree::{Document, Node};
use serde_json::{json, Value};

use super::{
    attach_image, ext_content_type, finish, resolve_zip_path, stats_json, tidy, Package, Stats,
};
use crate::project::{extname, Project};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn is(n: Node, local: &str) -> bool {
    n.is_element() && n.tag_name().name() == local
}

fn w_child<'a, 'i>(n: Node<'a, 'i>, local: &str) -> Option<Node<'a, 'i>> {
    n.children()
        .find(|c| is(*c, local) && c.tag_name().namespace() == Some(W))
}

fn w_val<'a>(n: Node<'a, '_>) -> Option<&'a str> {
    n.attribute((W, "val"))
}

/// `<w:b/>`-style toggles: present and not val="0"/"false".
fn toggle(rpr: Option<Node>, local: &str) -> bool {
    rpr.and_then(|r| w_child(r, local))
        .is_some_and(|t| !matches!(w_val(t), Some("0" | "false" | "none")))
}

fn escape_md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '`'
                | '*'
                | '_'
                | '{'
                | '}'
                | '['
                | ']'
                | '('
                | ')'
                | '#'
                | '+'
                | '-'
                | '.'
                | '!'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

// --- Document model -----------------------------------------------------------

#[derive(Debug, Clone)]
enum Inline {
    Text {
        text: String,
        bold: bool,
        italic: bool,
    },
    Break {
        bold: bool,
        italic: bool,
    },
    Image {
        rid: String,
        alt: String,
        bold: bool,
        italic: bool,
    },
    Link {
        href: String,
        children: Vec<Inline>,
    },
    Anchor(String),
    NoteRef {
        kind: &'static str,
        id: String,
    },
}

enum Block {
    Para {
        path: Path,
        inlines: Vec<Inline>,
    },
    TablePlaceholder,
    /// A bookmark between paragraphs (pandoc writes these).
    Anchor(String),
}

struct Styles {
    names: HashMap<String, String>,
    numbering: HashMap<String, (String, usize)>,
}

struct Numbering {
    /// numId → abstractNumId, abstractNumId → (ilvl → is_ordered)
    nums: HashMap<String, String>,
    levels: HashMap<String, HashMap<usize, bool>>,
}

impl Numbering {
    fn ordered(&self, num_id: &str, level: usize) -> bool {
        self.nums
            .get(num_id)
            .and_then(|a| self.levels.get(a))
            .and_then(|l| l.get(&level))
            .copied()
            .unwrap_or(false)
    }
}

fn read_styles(pkg: &mut Package) -> Styles {
    let mut styles = Styles {
        names: HashMap::new(),
        numbering: HashMap::new(),
    };
    let Some(xml) = pkg.text("word/styles.xml") else {
        return styles;
    };
    let Ok(doc) = Document::parse(&xml) else {
        return styles;
    };
    for s in doc.descendants().filter(|n| is(*n, "style")) {
        let Some(id) = s.attribute((W, "styleId")) else {
            continue;
        };
        if let Some(name) = w_child(s, "name").and_then(w_val) {
            styles.names.insert(id.to_string(), name.to_string());
        }
        if let Some(num_pr) = w_child(s, "pPr").and_then(|p| w_child(p, "numPr")) {
            if let Some(num_id) = w_child(num_pr, "numId").and_then(w_val) {
                let lvl = w_child(num_pr, "ilvl")
                    .and_then(w_val)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                styles
                    .numbering
                    .insert(id.to_string(), (num_id.to_string(), lvl));
            }
        }
    }
    styles
}

fn read_numbering(pkg: &mut Package) -> Numbering {
    let mut n = Numbering {
        nums: HashMap::new(),
        levels: HashMap::new(),
    };
    let Some(xml) = pkg.text("word/numbering.xml") else {
        return n;
    };
    let Ok(doc) = Document::parse(&xml) else {
        return n;
    };
    for a in doc.descendants().filter(|x| is(*x, "abstractNum")) {
        let Some(id) = a.attribute((W, "abstractNumId")) else {
            continue;
        };
        let mut levels = HashMap::new();
        for l in a.children().filter(|c| is(*c, "lvl")) {
            let ilvl = l
                .attribute((W, "ilvl"))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let fmt = w_child(l, "numFmt").and_then(w_val).unwrap_or("bullet");
            levels.insert(ilvl, fmt != "bullet");
        }
        n.levels.insert(id.to_string(), levels);
    }
    for num in doc.descendants().filter(|x| is(*x, "num")) {
        if let (Some(id), Some(abs)) = (
            num.attribute((W, "numId")),
            w_child(num, "abstractNumId").and_then(w_val),
        ) {
            n.nums.insert(id.to_string(), abs.to_string());
        }
    }
    n
}

struct Reader<'r> {
    links: &'r HashMap<String, String>,
    styles: &'r Styles,
    nums: &'r Numbering,
    notes: Vec<(&'static str, String)>,
    tables: usize,
    warnings: Vec<String>,
}

/// Run styles mammoth's default map handles (all but Strong map to nothing).
const KNOWN_RUN_STYLES: [&str; 8] = [
    "Strong",
    "footnote reference",
    "endnote reference",
    "annotation reference",
    "Footnote anchor",
    "Endnote anchor",
    "Hyperlink",
    "Default Paragraph Font",
];

impl Reader<'_> {
    fn blocks(&mut self, parent: Node, out: &mut Vec<Block>) {
        for n in parent.children().filter(|c| c.is_element()) {
            match n.tag_name().name() {
                "p" => out.push(self.paragraph(n)),
                "bookmarkStart" => {
                    if let Some(name) = n.attribute((W, "name")).filter(|n| *n != "_GoBack") {
                        out.push(Block::Anchor(name.to_string()));
                    }
                }
                "tbl" => {
                    self.tables += 1;
                    out.push(Block::TablePlaceholder);
                }
                "sdt" => {
                    if let Some(c) = w_child(n, "sdtContent") {
                        self.blocks(c, out);
                    }
                }
                "customXml" | "ins" | "smartTag" => self.blocks(n, out),
                "AlternateContent" => {
                    if let Some(fb) = n.children().find(|c| is(*c, "Fallback")) {
                        self.blocks(fb, out);
                    }
                }
                _ => {}
            }
        }
    }

    fn paragraph(&mut self, p: Node) -> Block {
        let ppr = w_child(p, "pPr");
        let style_id = ppr
            .and_then(|x| w_child(x, "pStyle"))
            .and_then(w_val)
            .map(str::to_string);
        let numbering = ppr.and_then(|x| w_child(x, "numPr")).and_then(|np| {
            let id = w_child(np, "numId").and_then(w_val)?;
            let lvl = w_child(np, "ilvl")
                .and_then(w_val)
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            Some((id.to_string(), lvl))
        });
        let path = html_path(
            style_id.as_deref(),
            self.styles,
            numbering,
            self.nums,
            &mut self.warnings,
        );
        let mut inlines = Vec::new();
        let mut field = FieldState::default();
        self.inlines(p, &mut inlines, &mut field);
        Block::Para { path, inlines }
    }

    fn inlines(&mut self, parent: Node, out: &mut Vec<Inline>, field: &mut FieldState) {
        for n in parent.children().filter(|c| c.is_element()) {
            match n.tag_name().name() {
                "r" => self.run(n, out, field),
                "hyperlink" => {
                    let href = n
                        .attribute((R, "id"))
                        .and_then(|id| self.links.get(id).cloned())
                        .or_else(|| n.attribute((W, "anchor")).map(|a| format!("#{a}")));
                    let mut children = Vec::new();
                    self.inlines(n, &mut children, field);
                    match href {
                        Some(href) => out.push(Inline::Link { href, children }),
                        None => out.extend(children),
                    }
                }
                "bookmarkStart" => {
                    if let Some(name) = n.attribute((W, "name")).filter(|n| *n != "_GoBack") {
                        out.push(Inline::Anchor(name.to_string()));
                    }
                }
                "ins" | "smartTag" | "customXml" | "fldSimple" => self.inlines(n, out, field),
                "sdt" => {
                    if let Some(c) = w_child(n, "sdtContent") {
                        self.inlines(c, out, field);
                    }
                }
                "AlternateContent" => {
                    if let Some(fb) = n.children().find(|c| is(*c, "Fallback")) {
                        self.inlines(fb, out, field);
                    }
                }
                _ => {}
            }
        }
    }

    fn run(&mut self, r: Node, out: &mut Vec<Inline>, field: &mut FieldState) {
        let rpr = w_child(r, "rPr");
        let rstyle = rpr.and_then(|x| w_child(x, "rStyle")).and_then(w_val);
        let rstyle_name = rstyle
            .and_then(|id| self.styles.names.get(id))
            .map(String::as_str);
        if let Some(id) = rstyle {
            if !rstyle_name
                .is_some_and(|n| KNOWN_RUN_STYLES.iter().any(|k| k.eq_ignore_ascii_case(n)))
            {
                self.warnings.push(format!(
                    "Unrecognised run style: '{}' (Style ID: {id})",
                    rstyle_name.unwrap_or("undefined")
                ));
            }
        }
        // Direct formatting only (mammoth ignores style-inherited bold/italic),
        // plus the default "Strong => strong" run mapping.
        let bold =
            toggle(rpr, "b") || rstyle_name.is_some_and(|n| n.eq_ignore_ascii_case("Strong"));
        let italic = toggle(rpr, "i");
        let target: &mut Vec<Inline> = if field.link.is_some() {
            &mut field.children
        } else {
            out
        };
        for c in r.children().filter(|c| c.is_element()) {
            match c.tag_name().name() {
                "t" => target.push(Inline::Text {
                    text: c.text().unwrap_or("").to_string(),
                    bold,
                    italic,
                }),
                "tab" => target.push(Inline::Text {
                    text: "\t".into(),
                    bold,
                    italic,
                }),
                "noBreakHyphen" => target.push(Inline::Text {
                    text: "\u{2011}".into(),
                    bold,
                    italic,
                }),
                "softHyphen" => target.push(Inline::Text {
                    text: "\u{ad}".into(),
                    bold,
                    italic,
                }),
                "br" if !matches!(c.attribute((W, "type")), Some("page" | "column")) => {
                    target.push(Inline::Break { bold, italic })
                }
                "cr" => target.push(Inline::Break { bold, italic }),
                "drawing" | "pict" => {
                    for (rid, alt) in images_in(c) {
                        target.push(Inline::Image {
                            rid,
                            alt,
                            bold,
                            italic,
                        });
                    }
                }
                "footnoteReference" | "endnoteReference" => {
                    let kind = if c.tag_name().name() == "footnoteReference" {
                        "footnote"
                    } else {
                        "endnote"
                    };
                    if let Some(id) = c.attribute((W, "id")) {
                        self.notes.push((kind, id.to_string()));
                        target.push(Inline::NoteRef {
                            kind,
                            id: id.to_string(),
                        });
                    }
                }
                "fldChar" => match c.attribute((W, "fldCharType")) {
                    Some("begin") => field.instr.clear(),
                    Some("separate") => field.link = parse_hyperlink_instr(&field.instr),
                    Some("end") => {
                        if let Some(href) = field.link.take() {
                            out.push(Inline::Link {
                                href,
                                children: std::mem::take(&mut field.children),
                            });
                        }
                        return;
                    }
                    _ => {}
                },
                "instrText" => field.instr.push_str(c.text().unwrap_or("")),
                "AlternateContent" => {
                    if let Some(fb) = c.children().find(|x| is(*x, "Fallback")) {
                        for (rid, alt) in images_in(fb) {
                            target.push(Inline::Image {
                                rid,
                                alt,
                                bold,
                                italic,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

#[derive(Default)]
struct FieldState {
    instr: String,
    link: Option<String>,
    children: Vec<Inline>,
}

fn parse_hyperlink_instr(instr: &str) -> Option<String> {
    let rest = instr.trim().strip_prefix("HYPERLINK")?.trim();
    let quoted = |s: &str| s.split('"').nth(1).map(str::to_string);
    if let Some(anchor) = rest.strip_prefix("\\l") {
        return quoted(anchor).map(|a| format!("#{a}"));
    }
    quoted(rest)
}

/// (relationship id, alt text) for images in a drawing/VML element.
fn images_in(n: Node) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for d in n
        .descendants()
        .filter(|d| is(*d, "inline") || is(*d, "anchor"))
    {
        let alt = d
            .children()
            .find(|c| is(*c, "docPr"))
            .and_then(|p| {
                p.attribute("descr")
                    .filter(|s| !s.is_empty())
                    .or_else(|| p.attribute("title"))
            })
            .unwrap_or("")
            .to_string();
        if let Some(blip) = d.descendants().find(|b| is(*b, "blip")) {
            if let Some(rid) = blip.attribute((R, "embed")) {
                out.push((rid.to_string(), alt));
            }
        }
    }
    for img in n.descendants().filter(|d| is(*d, "imagedata")) {
        if let Some(rid) = img.attribute((R, "id")) {
            let alt = img
                .attribute(("urn:schemas-microsoft-com:office:office", "title"))
                .unwrap_or("")
                .to_string();
            out.push((rid.to_string(), alt));
        }
    }
    out
}

// --- Markdown writer (mammoth's markdown-writer) --------------------------------

enum Path {
    Heading(usize),
    Para,
    Quote,
    List { ordered: bool, level: usize },
}

struct ListState {
    ordered: bool,
    count: usize,
}

#[derive(Default)]
struct Writer {
    out: String,
    lists: Vec<ListState>,
    /// mammoth shares one listItem object across all <li>: once any item
    /// closes, enclosing items close silently until a new one opens.
    li_has_closed: bool,
}

impl Writer {
    fn close_li(&mut self) {
        if !self.li_has_closed {
            self.li_has_closed = true;
            self.out.push('\n');
        }
    }

    fn close_lists_to(&mut self, depth: usize) {
        while self.lists.len() > depth {
            self.close_li();
            self.lists.pop();
            if self.lists.is_empty() {
                self.out.push('\n');
            }
        }
    }

    fn open_list(&mut self, ordered: bool) {
        if !self.lists.is_empty() {
            self.out.push('\n');
        }
        self.lists.push(ListState { ordered, count: 0 });
    }

    fn open_li(&mut self) {
        let indent = self.lists.len() - 1;
        let list = self.lists.last_mut().unwrap();
        list.count += 1;
        let bullet = if list.ordered {
            format!("{}.", list.count)
        } else {
            "-".into()
        };
        self.li_has_closed = false;
        self.out.push_str(&"\t".repeat(indent));
        self.out.push_str(&bullet);
        self.out.push(' ');
    }

    fn list_item(&mut self, ordered: bool, level: usize) {
        if self.lists.len() >= level {
            self.close_lists_to(level);
            if self.lists[level - 1].ordered != ordered {
                self.close_lists_to(level - 1);
                self.open_list(ordered);
            } else {
                self.close_li();
            }
        } else {
            while self.lists.len() < level - 1 {
                self.open_list(false);
                self.open_li();
            }
            self.open_list(ordered);
        }
        self.open_li();
    }
}

struct Ctx<'a> {
    images: &'a HashMap<String, String>,
}

fn render_inlines(items: &[Inline], ctx: &Ctx, out: &mut String) {
    // Formatting stack: strong outside em, adjacent runs collapse.
    let mut open: Vec<&'static str> = Vec::new();
    let set = |want: Vec<&'static str>, open: &mut Vec<&'static str>, out: &mut String| {
        let common = open.iter().zip(&want).take_while(|(a, b)| a == b).count();
        while open.len() > common {
            out.push_str(open.pop().unwrap());
        }
        for m in &want[common..] {
            out.push_str(m);
            open.push(m);
        }
    };
    let marks = |bold: bool, italic: bool| {
        let mut v = Vec::new();
        if bold {
            v.push("__");
        }
        if italic {
            v.push("*");
        }
        v
    };
    for item in items {
        match item {
            Inline::Text { text, bold, italic } => {
                set(marks(*bold, *italic), &mut open, out);
                out.push_str(&escape_md(text));
            }
            Inline::Break { bold, italic } => {
                set(marks(*bold, *italic), &mut open, out);
                out.push_str("  \n");
            }
            Inline::Image {
                rid,
                alt,
                bold,
                italic,
            } => {
                set(marks(*bold, *italic), &mut open, out);
                let src = ctx.images.get(rid).cloned().unwrap_or_default();
                if !src.is_empty() || !alt.is_empty() {
                    out.push_str(&format!("![{alt}]({src})"));
                }
            }
            Inline::Link { href, children } => {
                set(Vec::new(), &mut open, out);
                out.push('[');
                render_inlines(children, ctx, out);
                out.push_str(&format!("]({href})"));
            }
            Inline::Anchor(id) => {
                set(Vec::new(), &mut open, out);
                out.push_str(&format!("<a id=\"{id}\"></a>"));
            }
            Inline::NoteRef { .. } => {}
        }
    }
    set(Vec::new(), &mut open, out);
}

fn has_content(items: &[Inline]) -> bool {
    !items.is_empty()
}

// --- Entry point ----------------------------------------------------------------

/// Paragraph → output block, by NoteLiner's style map then mammoth's
/// defaults (style names compare case-insensitively, as in mammoth).
fn html_path(
    style_id: Option<&str>,
    styles: &Styles,
    numbering: Option<(String, usize)>,
    nums: &Numbering,
    warnings: &mut Vec<String>,
) -> Path {
    let name = style_id
        .and_then(|id| styles.names.get(id))
        .map(String::as_str);
    let lname = name.map(str::to_lowercase);
    let is_name = |n: &str| lname.as_deref() == Some(n);
    if is_name("title") {
        return Path::Heading(1);
    }
    if is_name("subtitle") {
        return Path::Heading(2);
    }
    if is_name("quote") || is_name("intense quote") {
        return Path::Quote;
    }
    for lvl in 1..=6 {
        if is_name(&format!("heading {lvl}")) || style_id == Some(&format!("Heading{lvl}")) {
            return Path::Heading(lvl);
        }
    }
    if style_id == Some("Heading") || is_name("heading") {
        return Path::Heading(1);
    }
    let numbering = numbering.or_else(|| style_id.and_then(|id| styles.numbering.get(id).cloned()));
    if let Some((num_id, ilvl)) = numbering.filter(|(id, _)| id != "0") {
        let level = ilvl + 1;
        if level <= 5 {
            return Path::List {
                ordered: nums.ordered(&num_id, ilvl),
                level,
            };
        }
    }
    let known = [
        "normal",
        "body",
        "footnote text",
        "endnote text",
        "annotation text",
        "footnote",
        "endnote",
    ]
    .iter()
    .any(|k| is_name(k))
        || style_id == Some("Body");
    if let (Some(id), false) = (style_id, known) {
        warnings.push(format!(
            "Unrecognised paragraph style: '{}' (Style ID: {id})",
            name.unwrap_or("undefined")
        ));
    }
    Path::Para
}

pub async fn import(project: &mut Project, bytes: &[u8], stem: &str) -> Result<Value> {
    let mut pkg = Package::open(bytes).map_err(|e| anyhow!("Conversion failed: {e}"))?;
    let entry = project
        .create_file(stem, &json!([]), Some(String::new()), None)
        .await?;
    let file_id = entry["id"].as_str().unwrap_or("").to_string();
    let mut stats = Stats::default();

    let markdown = convert(project, &mut pkg, &file_id, stem, &mut stats)
        .await
        .map_err(|e| anyhow!("Conversion failed: {e}"))?;
    let filename = entry["filename"].as_str().unwrap_or("").to_string();
    project.write_file(&filename, &markdown).await?;
    Ok(json!({ "entry": entry, "stats": stats_json(&stats, true) }))
}

async fn convert(
    project: &mut Project,
    pkg: &mut Package,
    file_id: &str,
    stem: &str,
    stats: &mut Stats,
) -> Result<String> {
    let doc_xml = pkg
        .text("word/document.xml")
        .ok_or_else(|| anyhow!("Could not find main document part"))?;
    let styles = read_styles(pkg);
    let nums = read_numbering(pkg);
    let rels = pkg.rels("word/_rels/document.xml.rels");
    let links: HashMap<String, String> = rels
        .iter()
        .filter(|r| r.1.ends_with("/hyperlink"))
        .map(|r| (r.0.clone(), r.2.clone()))
        .collect();
    let content_types = read_content_types(pkg);

    let doc = Document::parse(&doc_xml)?;
    let body = doc
        .descendants()
        .find(|n| is(*n, "body"))
        .ok_or_else(|| anyhow!("document has no body"))?;
    let mut reader = Reader {
        links: &links,
        styles: &styles,
        nums: &nums,
        notes: Vec::new(),
        tables: 0,
        warnings: Vec::new(),
    };
    let mut blocks = Vec::new();
    reader.blocks(body, &mut blocks);
    stats.tables_stripped = reader.tables;

    // Notes bodies (footnotes.xml / endnotes.xml), in reference order.
    let mut note_blocks: Vec<(&'static str, String, Vec<Block>)> = Vec::new();
    let referenced = std::mem::take(&mut reader.notes);
    for kind in ["footnote", "endnote"] {
        let Some(xml) = pkg.text(&format!("word/{kind}s.xml")) else {
            continue;
        };
        let Ok(ndoc) = Document::parse(&xml) else {
            continue;
        };
        for (k, id) in referenced.iter().filter(|(k, _)| *k == kind) {
            if let Some(n) = ndoc
                .descendants()
                .find(|n| is(*n, kind) && n.attribute((W, "id")) == Some(id))
            {
                let mut nb = Vec::new();
                reader.blocks(n, &mut nb);
                note_blocks.push((k, id.clone(), nb));
            }
        }
    }

    // Attach images in document order.
    let mut images: HashMap<String, String> = HashMap::new();
    let mut rids = Vec::new();
    let collect = |items: &[Inline], rids: &mut Vec<String>| {
        fn walk(items: &[Inline], rids: &mut Vec<String>) {
            for i in items {
                match i {
                    Inline::Image { rid, .. } => rids.push(rid.clone()),
                    Inline::Link { children, .. } => walk(children, rids),
                    _ => {}
                }
            }
        }
        walk(items, rids)
    };
    for b in blocks
        .iter()
        .chain(note_blocks.iter().flat_map(|(_, _, b)| b.iter()))
    {
        if let Block::Para { inlines, .. } = b {
            collect(inlines, &mut rids);
        }
    }
    for rid in rids {
        if images.contains_key(&rid) {
            stats.images += 1;
            continue;
        }
        let Some(rel) = rels.iter().find(|r| r.0 == rid) else {
            continue;
        };
        if rel.3 {
            continue; // linked (external) image
        }
        let part = resolve_zip_path("word/", &rel.2);
        let ct = content_types
            .get(&format!("/{part}"))
            .cloned()
            .or_else(|| {
                content_types
                    .get(&extname(&part).trim_start_matches('.').to_lowercase())
                    .cloned()
            })
            .unwrap_or_else(|| ext_content_type(&extname(&part).to_lowercase()).to_string());
        let Some(data) = pkg.bytes(&part) else {
            stats
                .warnings
                .push(format!("Failed to read embedded image: missing {part}"));
            continue;
        };
        let src = attach_image(project, file_id, &data, &ct, stats)
            .await
            .map(|f| format!("./_attachments/{f}"))
            .unwrap_or_default();
        images.insert(rid, src);
    }

    let ctx = Ctx { images: &images };
    let mut w = Writer::default();
    let mut note_numbers: HashMap<(&str, String), usize> = HashMap::new();
    for (i, (k, id)) in referenced.iter().enumerate() {
        note_numbers.entry((k, id.clone())).or_insert(i + 1);
    }
    // mammoth reports each distinct message once, in first-seen order.
    for warning in std::mem::take(&mut reader.warnings) {
        if !stats.warnings.contains(&warning) {
            stats.warnings.push(warning);
        }
    }
    write_blocks(&mut w, &blocks, &ctx, &note_numbers);
    w.close_lists_to(0);

    if !note_blocks.is_empty() {
        // <ol><li id="footnote-N">…<p> <a href="#footnote-ref-N">↑</a></p></li></ol>
        for (i, (kind, id, nb)) in note_blocks.iter().enumerate() {
            w.out
                .push_str(&format!("{}. <a id=\"{kind}-{id}\"></a>", i + 1));
            let mut inner = Writer::default();
            write_blocks(&mut inner, nb, &ctx, &note_numbers);
            inner.close_lists_to(0);
            let text = inner.out.trim_end_matches('\n').to_string();
            w.out.push_str(&text);
            w.out.push_str(&format!(" [↑](#{kind}-ref-{id})\n\n\n"));
        }
        w.out.push('\n');
    }

    Ok(post_process(&w.out, stem))
}

fn write_blocks(
    w: &mut Writer,
    blocks: &[Block],
    ctx: &Ctx,
    note_numbers: &HashMap<(&str, String), usize>,
) {
    for b in blocks {
        let (path, inlines): (&Path, Vec<Inline>) = match b {
            Block::TablePlaceholder => (
                &Path::Para,
                vec![Inline::Text {
                    text: "[Table omitted during import]".into(),
                    bold: false,
                    italic: false,
                }],
            ),
            Block::Anchor(id) => {
                w.close_lists_to(0);
                w.out.push_str(&format!("<a id=\"{id}\"></a>"));
                continue;
            }
            Block::Para { path, inlines } => (path, inlines.clone()),
        };
        if !has_content(&inlines) {
            continue; // mammoth ignores empty paragraphs
        }
        let mut text = String::new();
        render_with_notes(&inlines, ctx, note_numbers, &mut text);
        match path {
            Path::List { ordered, level } => {
                w.list_item(*ordered, *level);
                w.out.push_str(&text);
            }
            other => {
                w.close_lists_to(0);
                match other {
                    Path::Heading(n) => w.out.push_str(&format!("{} {text}\n\n", "#".repeat(*n))),
                    Path::Quote => w.out.push_str(&format!("> {text}\n\n")),
                    _ => w.out.push_str(&format!("{text}\n\n")),
                }
            }
        }
    }
}

/// Inline rendering plus note references (`<a id>[\[n\]](#footnote-id)`).
fn render_with_notes(
    items: &[Inline],
    ctx: &Ctx,
    numbers: &HashMap<(&str, String), usize>,
    out: &mut String,
) {
    let mut chunk: Vec<Inline> = Vec::new();
    for i in items {
        if let Inline::NoteRef { kind, id } = i {
            render_inlines(&chunk, ctx, out);
            chunk.clear();
            let n = numbers.get(&(*kind, id.clone())).copied().unwrap_or(0);
            out.push_str(&format!(
                "<a id=\"{kind}-ref-{id}\"></a>[\\[{n}\\]](#{kind}-{id})"
            ));
        } else {
            chunk.push(i.clone());
        }
    }
    render_inlines(&chunk, ctx, out);
}

fn read_content_types(pkg: &mut Package) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(xml) = pkg.text("[Content_Types].xml") else {
        return map;
    };
    let Ok(doc) = Document::parse(&xml) else {
        return map;
    };
    for n in doc.descendants().filter(|n| n.is_element()) {
        match n.tag_name().name() {
            "Default" => {
                if let (Some(e), Some(ct)) = (n.attribute("Extension"), n.attribute("ContentType"))
                {
                    map.insert(e.to_lowercase(), ct.to_string());
                }
            }
            "Override" => {
                if let (Some(p), Some(ct)) = (n.attribute("PartName"), n.attribute("ContentType")) {
                    map.insert(p.to_string(), ct.to_string());
                }
            }
            _ => {}
        }
    }
    map
}

/// Same tidy-up as the Electron importer, plus an H1 from the filename
/// when the first lines have none.
fn post_process(md: &str, stem: &str) -> String {
    let mut md = tidy(md);
    let head: Vec<&str> = md.split('\n').take(5).collect();
    if !head
        .iter()
        .any(|l| l.starts_with('#') && l[1..].starts_with(char::is_whitespace))
    {
        md = format!("# {stem}\n\n{}", md.trim_start_matches('\n'));
    }
    finish(&md)
}
