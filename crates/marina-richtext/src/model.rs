//! The rich-text model: what's left of an HTML document once it's been reduced to things a native
//! reader can lay out. ThreadLiner articles, PageLiner EPUB chapters and (later) the NoteLiner
//! preview share it.

use std::collections::HashMap;

/// A converted document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
    /// Element ids (`<a id>`, `<h2 id>`, …) → the top-level block that contains them, for
    /// internal links (`#note-3`, `chapter2.xhtml#sec1`).
    pub anchors: HashMap<String, usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Text {
        kind: TextKind,
        spans: Vec<Span>,
        align: Align,
        indent: Indent,
    },
    List {
        ordered: bool,
        start: u32,
        items: Vec<Vec<Block>>,
    },
    Quote(Vec<Block>),
    Code(String),
    /// `title` is the hover text (xkcd's punchline lives there).
    Image {
        src: String,
        alt: String,
        title: String,
        width: Option<f32>,
        height: Option<f32>,
        /// A CSS `width: N%`: the share of the column the image fills.
        fraction: Option<f32>,
    },
    Rule,
    /// Rows of cells; each cell is inline text (block content inside a cell is flattened).
    /// `colspan`s and `rowspan`s are padded with empty cells, so every row lines up.
    Table {
        rows: Vec<Vec<Vec<Span>>>,
        header_rows: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextKind {
    Paragraph,
    /// `h1`–`h6`.
    Heading(u8),
    /// `figcaption`.
    Caption,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    /// `text-align: justify`. Kept so the book's intent survives, but laid out ragged-right
    /// for now (see `view::text_job`).
    Justify,
}

/// A paragraph's CSS indents, in ems.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Indent {
    /// `margin-left` + `padding-left`, of the paragraph and its block ancestors.
    pub left: f32,
    /// `text-indent` (first line only; negative "hanging" indents are dropped).
    pub first_line: f32,
}

/// A run of text with one style.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    /// The resolved link target.
    pub link: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    pub small: bool,
    /// `<sup>` / `vertical-align: super`.
    pub sup: bool,
    /// `<sub>` / `vertical-align: sub`.
    pub sub: bool,
}

impl Block {
    /// The block's text, for search, statistics and positions.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        self.push_text(&mut out, true);
        out
    }

    fn push_text(&self, out: &mut String, alt_text: bool) {
        let sep = |out: &mut String| {
            if !out.is_empty() && !out.ends_with(char::is_whitespace) {
                out.push(' ');
            }
        };
        match self {
            Block::Text { spans, .. } => {
                sep(out);
                spans.iter().for_each(|s| out.push_str(&s.text));
            }
            Block::List { items, .. } => items
                .iter()
                .flatten()
                .for_each(|b| b.push_text(out, alt_text)),
            Block::Quote(blocks) => blocks.iter().for_each(|b| b.push_text(out, alt_text)),
            Block::Code(code) => {
                sep(out);
                out.push_str(code);
            }
            Block::Image { alt, .. } => {
                if alt_text && !alt.is_empty() {
                    sep(out);
                    out.push_str(alt);
                }
            }
            Block::Rule => {}
            Block::Table { rows, .. } => {
                for cell in rows.iter().flatten() {
                    sep(out);
                    cell.iter().for_each(|s| out.push_str(&s.text));
                }
            }
        }
    }

    /// The block's runs of selectable text, in the order the view lays them out: one per
    /// paragraph (nested ones included), code block and table cell. A position's offset inside a
    /// block counts through them, one extra character between parts (see [`Self::part_text`]).
    pub fn text_parts(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.push_parts(&mut out);
        out
    }

    fn push_parts(&self, out: &mut Vec<String>) {
        match self {
            Block::Text { spans, .. } => out.push(spans.iter().map(|s| s.text.as_str()).collect()),
            Block::List { items, .. } => items.iter().flatten().for_each(|b| b.push_parts(out)),
            Block::Quote(blocks) => blocks.iter().for_each(|b| b.push_parts(out)),
            Block::Code(code) => out.push(code.clone()),
            Block::Table { rows, .. } => {
                for cell in rows.iter().flatten() {
                    out.push(cell.iter().map(|s| s.text.as_str()).collect());
                }
            }
            Block::Image { .. } | Block::Rule => {}
        }
    }

    /// The block's text parts joined by newlines: character `offset` of a position in this block
    /// is character `offset` of this string.
    pub fn part_text(&self) -> String {
        self.text_parts().join("\n")
    }

    /// Image blocks inside this block (itself included).
    pub fn image_count(&self) -> usize {
        match self {
            Block::Image { .. } => 1,
            Block::List { items, .. } => items.iter().flatten().map(Block::image_count).sum(),
            Block::Quote(blocks) => blocks.iter().map(Block::image_count).sum(),
            _ => 0,
        }
    }
}

impl Document {
    pub fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .map(Block::plain_text)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The text without images' alt text (what the source showed as text).
    pub fn text_without_alt(&self) -> String {
        let mut out = String::new();
        self.blocks
            .iter()
            .for_each(|b| b.push_text(&mut out, false));
        out
    }

    pub fn image_count(&self) -> usize {
        self.blocks.iter().map(Block::image_count).sum()
    }
}
