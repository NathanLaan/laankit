//! `.mmd` text → `Diagram`. Reads back everything the writer emits, and any
//! hand-written flowchart: statements it doesn't model are kept verbatim, and
//! nodes without an `@shape` line are placed by `layout`.

use std::collections::HashMap;
use std::fmt;

use super::lex::{self, LinkKind, NodeRef, Token};
use super::{edge_key, TEXT_CLASS};
use crate::layout;
use crate::model::{
    Anchor, Connector, ConnectorKind, Diagram, Direction, End, Point, Rect, Shape, ShapeKind,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// Another Mermaid diagram type (`sequenceDiagram`, `classDiagram`, …).
    Unsupported(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(kind) => {
                write!(
                    f,
                    "`{kind}` diagrams aren't supported; BlockLiner edits Mermaid flowcharts"
                )
            }
        }
    }
}

impl std::error::Error for ReadError {}

/// Statements kept verbatim without looking for nodes in them.
const KEYWORDS: &[&str] = &[
    "classDef",
    "class",
    "style",
    "click",
    "linkStyle",
    "direction",
    "end",
];

pub fn read(text: &str) -> Result<Diagram, ReadError> {
    let mut d = Diagram::new();
    let mut layout = Layout::default();
    let mut lines = text.lines().peekable();

    // Front matter, `%%{init}%%` and comments, up to the header.
    let mut header = false;
    let mut first = true;
    while let Some(line) = lines.next() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if first && t == "---" {
            d.preamble.push(line.to_string());
            for line in lines.by_ref() {
                d.preamble.push(line.to_string());
                if line.trim() == "---" {
                    break;
                }
            }
        } else if t.starts_with("%%") {
            if !layout.comment(t) {
                d.preamble.push(line.to_string());
            }
        } else {
            let mut words = t.trim_end_matches(';').split_whitespace();
            match words.next() {
                Some("flowchart" | "graph") => {
                    d.direction = words
                        .next()
                        .and_then(Direction::parse)
                        .unwrap_or(Direction::TB);
                    header = true;
                    break;
                }
                Some(kind) => return Err(ReadError::Unsupported(kind.to_string())),
                None => {}
            }
        }
        first = false;
    }
    if !header {
        // Empty, or only comments: a new diagram.
        d.direction = Direction::default();
    }

    let mut body = Body::default();
    let mut depth = 0usize;
    for line in lines {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("%%") {
            if !layout.comment(t) {
                body.passthrough.push(line.trim_end().to_string());
            }
            continue;
        }
        let word = t.split_whitespace().next().unwrap_or("");
        if depth > 0 || word == "subgraph" {
            // Subgraph blocks are kept whole; their nodes are shown but stay
            // declared inside the block.
            body.passthrough.push(line.trim_end().to_string());
            match word {
                "subgraph" => depth += 1,
                "end" => depth -= 1,
                _ => {
                    for stmt in statements(t) {
                        body.external(stmt);
                    }
                }
            }
            continue;
        }
        for stmt in statements(t) {
            body.statement(stmt);
        }
    }

    if let Some(grid) = layout.grid {
        d.grid = grid;
    }
    body.build(&mut d, &layout);
    Ok(d)
}

/// `a --> b; b --> c` → its statements: split at `;` outside quotes and
/// brackets.
fn statements(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, mut depth, mut quoted) = (0, 0usize, false);
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '[' | '(' | '{' if !quoted => depth += 1,
            ']' | ')' | '}' if !quoted => depth = depth.saturating_sub(1),
            ';' if !quoted && depth == 0 => {
                out.push(line[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(line[start..].trim());
    out.retain(|s| !s.is_empty());
    out
}

struct Node {
    id: String,
    shape: Option<(ShapeKind, String)>,
    class: Option<String>,
    external: bool,
}

struct Edge {
    from: String,
    to: String,
    kind: ConnectorKind,
    label: Option<String>,
}

#[derive(Default)]
struct Body {
    nodes: Vec<Node>,
    index: HashMap<String, usize>,
    edges: Vec<Edge>,
    /// Edges in statements kept verbatim: not connectors, but they still
    /// rank the nodes they join in the layout.
    hints: Vec<(String, String)>,
    passthrough: Vec<String>,
}

impl Body {
    fn statement(&mut self, stmt: &str) {
        let word = stmt.split_whitespace().next().unwrap_or("");
        if word == "classDef" && stmt.split_whitespace().nth(1) == Some(TEXT_CLASS) {
            return; // The writer adds it back when a text box needs it.
        }
        if KEYWORDS.contains(&word) {
            self.passthrough.push(format!("  {stmt}"));
            return;
        }
        let Some(tokens) = lex::tokenize(stmt) else {
            self.passthrough.push(format!("  {stmt}"));
            return;
        };
        let modelled = tokens.iter().all(|t| match t {
            Token::Amp => false,
            Token::Link { kind, .. } => *kind != LinkKind::Other,
            Token::Node(_) => true,
        });
        if !modelled {
            self.passthrough.push(format!("  {stmt}"));
            self.declare_all(tokens, true);
            return;
        }
        let mut prev: Option<String> = None;
        let mut link = None;
        for token in tokens {
            match token {
                Token::Node(n) => {
                    let id = n.id.clone();
                    self.declare(n, false);
                    if let (Some(from), Some((kind, label))) = (prev.take(), link.take()) {
                        let kind = match kind {
                            LinkKind::Line => ConnectorKind::Line,
                            _ => ConnectorKind::Arrow,
                        };
                        self.edges.push(Edge {
                            from,
                            to: id.clone(),
                            kind,
                            label,
                        });
                    }
                    prev = Some(id);
                }
                Token::Link { kind, label } => link = Some((kind, label)),
                Token::Amp => unreachable!("checked above"),
            }
        }
    }

    /// The nodes in a statement kept verbatim.
    fn external(&mut self, stmt: &str) {
        let word = stmt.split_whitespace().next().unwrap_or("");
        if !KEYWORDS.contains(&word) {
            if let Some(tokens) = lex::tokenize(stmt) {
                self.declare_all(tokens, true);
            }
        }
    }

    /// Declares a verbatim statement's nodes and notes its edges as layout
    /// hints: `a & b --> c` joins each of `a`, `b` to `c`.
    fn declare_all(&mut self, tokens: Vec<Token>, external: bool) {
        let (mut prev, mut group) = (Vec::new(), Vec::new());
        for token in tokens {
            match token {
                Token::Node(n) => {
                    group.push(n.id.clone());
                    self.declare(n, external);
                }
                Token::Amp => {}
                Token::Link { .. } => {
                    self.hint(&prev, &group);
                    prev = std::mem::take(&mut group);
                }
            }
        }
        self.hint(&prev, &group);
    }

    fn hint(&mut self, from: &[String], to: &[String]) {
        for a in from {
            for b in to {
                self.hints.push((a.clone(), b.clone()));
            }
        }
    }

    /// Mermaid lets any mention declare a node, and a later shape replaces an
    /// earlier one. A verbatim statement never overrides a modelled one.
    fn declare(&mut self, n: NodeRef, external: bool) {
        let Some(&i) = self.index.get(&n.id) else {
            self.index.insert(n.id.clone(), self.nodes.len());
            self.nodes.push(Node {
                id: n.id,
                shape: n.shape,
                class: n.class,
                external,
            });
            return;
        };
        let node = &mut self.nodes[i];
        if external && !node.external {
            return;
        }
        node.external = external;
        if n.shape.is_some() {
            node.shape = n.shape;
        }
        if n.class.is_some() {
            node.class = n.class;
        }
    }

    fn build(mut self, d: &mut Diagram, layout: &Layout) {
        // Shapes keep the order of their `@shape` lines, which is the model's
        // order when BlockLiner wrote the file; new ones follow in file order.
        // Without this, nodes declared in verbatim statements would move
        // behind the others on every save.
        let rank = |n: &Node| layout.shapes.get(&n.id).map_or(usize::MAX, |&(i, _)| i);
        self.nodes.sort_by_key(rank);
        let mut unplaced = Vec::new();
        for (i, n) in self.nodes.into_iter().enumerate() {
            let text = n.class.as_deref() == Some(TEXT_CLASS);
            let (kind, label) = n.shape.unwrap_or_else(|| (ShapeKind::Rect, n.id.clone()));
            let kind = if text { ShapeKind::Text } else { kind };
            let rect = layout
                .shapes
                .get(&n.id)
                .map(|&(_, r)| r)
                .unwrap_or_else(|| {
                    unplaced.push(i);
                    Rect::default()
                });
            d.shapes.push(Shape {
                id: n.id,
                kind,
                label,
                rect,
                class: n.class.filter(|_| !text),
                external: n.external,
            });
        }

        let mut seen: HashMap<(String, String), usize> = HashMap::new();
        for e in self.edges {
            let n = seen.entry((e.from.clone(), e.to.clone())).or_default();
            *n += 1;
            let (a, b) = layout
                .edges
                .get(&edge_key(&e.from, &e.to, *n))
                .copied()
                .unwrap_or_default();
            d.connectors.push(Connector {
                kind: e.kind,
                from: End::glued(e.from, a),
                to: End::glued(e.to, b),
                label: e.label,
            });
        }
        for line in &layout.lines {
            let exists = |end: &End| end.shape().is_none_or(|id| d.shape(id).is_some());
            if exists(&line.from) && exists(&line.to) {
                d.connectors.push(line.clone());
            }
        }

        d.passthrough = self.passthrough;
        layout::layered(d, &unplaced, &self.hints);
    }
}

/// What the `%% blockliner` and `%% @…` comments say.
#[derive(Default)]
struct Layout {
    grid: Option<f32>,
    /// Each `@shape`'s rectangle and its position among the `@shape` lines.
    shapes: HashMap<String, (usize, Rect)>,
    edges: HashMap<String, (Option<Anchor>, Option<Anchor>)>,
    lines: Vec<Connector>,
}

impl Layout {
    /// Takes in a `%%` comment; `false` if it isn't BlockLiner's. A malformed
    /// `@` line is still BlockLiner's, and dropped.
    fn comment(&mut self, t: &str) -> bool {
        let body = t.trim_start_matches('%').trim();
        let mut words = body.split_whitespace();
        let (positional, fields) = split_fields(words.clone().skip(1));
        match words.next() {
            Some("blockliner") if positional.first().is_some_and(|v| v.parse::<u32>().is_ok()) => {
                if let Some(g) = fields
                    .get("grid")
                    .and_then(|v| number(v))
                    .filter(|g| *g > 0.0)
                {
                    self.grid = Some(g);
                }
            }
            Some("@shape") => {
                let rect = (|| {
                    let n = |k: &str| fields.get(k).and_then(|v| number(v));
                    Some(Rect::new(n("x")?, n("y")?, n("w")?, n("h")?))
                })();
                if let (Some(id), Some(rect)) = (positional.first(), rect) {
                    let order = self.shapes.len();
                    self.shapes.entry(id.to_string()).or_insert((order, rect)).1 = rect;
                }
            }
            Some("@edge") => {
                if let Some(key) = positional.first() {
                    let anchor = |k: &str| fields.get(k).and_then(|v| Anchor::parse(v));
                    self.edges
                        .insert(key.to_string(), (anchor("from"), anchor("to")));
                }
            }
            Some("@line") => {
                let kind = match positional.first() {
                    Some(&"line") => Some(ConnectorKind::Line),
                    Some(&"arrow") => Some(ConnectorKind::Arrow),
                    _ => None,
                };
                let end = |k: &str| fields.get(k).and_then(|v| parse_end(v));
                if let (Some(kind), Some(from), Some(to)) = (kind, end("from"), end("to")) {
                    self.lines.push(Connector::new(kind, from, to));
                }
            }
            Some(w) if w.starts_with('@') => {}
            _ => return false,
        }
        true
    }
}

fn split_fields<'a>(
    words: impl Iterator<Item = &'a str>,
) -> (Vec<&'a str>, HashMap<&'a str, &'a str>) {
    let mut positional = Vec::new();
    let mut fields = HashMap::new();
    for w in words {
        match w.split_once('=') {
            Some((k, v)) => {
                fields.insert(k, v);
            }
            None => positional.push(w),
        }
    }
    (positional, fields)
}

fn number(s: &str) -> Option<f32> {
    s.parse::<f32>().ok().filter(|v| v.is_finite())
}

/// `x,y` or `id[:anchor]`.
fn parse_end(s: &str) -> Option<End> {
    if let Some((x, y)) = s.split_once(',') {
        return Some(End::Free(Point::new(number(x)?, number(y)?)));
    }
    match s.split_once(':') {
        Some((id, a)) => Some(End::glued(id, Some(Anchor::parse(a)?))),
        None => Some(End::glued(s, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_statements() {
        assert_eq!(statements("a --> b; b --> c;"), vec!["a --> b", "b --> c"]);
        assert_eq!(
            statements("a[\"x; y\"] --> b(c;d)"),
            vec!["a[\"x; y\"] --> b(c;d)"]
        );
    }

    #[test]
    fn rejects_other_diagram_types() {
        assert_eq!(
            read("%% hi\nsequenceDiagram\n  a->>b: x\n"),
            Err(ReadError::Unsupported("sequenceDiagram".into()))
        );
    }

    #[test]
    fn empty_is_a_new_diagram() {
        assert_eq!(read(""), Ok(Diagram::new()));
        assert_eq!(read("\n\n%% blockliner 1 grid=10\n").unwrap().grid, 10.0);
    }

    #[test]
    fn implicit_nodes_and_chains() {
        let d = read("graph\n  a --> b --- c\n").unwrap();
        assert_eq!(d.direction, Direction::TB);
        let ids: Vec<_> = d
            .shapes
            .iter()
            .map(|s| (s.id.as_str(), s.label.as_str()))
            .collect();
        assert_eq!(ids, [("a", "a"), ("b", "b"), ("c", "c")]);
        assert_eq!(d.connectors.len(), 2);
        assert_eq!(d.connectors[1].kind, ConnectorKind::Line);
        assert_eq!(d.connectors[1].from, End::glued("b", None));
    }

    #[test]
    fn layout_comments() {
        let d = read(
            "flowchart LR\n  a[A]\n  b(B)\n  a --> b\n  a --> b\n\
             %% blockliner 1 grid=10\n\
             %% @shape a x=10 y=20 w=80 h=30\n\
             %% @edge a->b#2 from=S\n\
             %% @line line from=5,5 to=b:N\n\
             %% @line arrow from=gone to=1,1\n\
             %% @shape\n\
             %% just a note\n",
        )
        .unwrap();
        assert_eq!(d.grid, 10.0);
        assert_eq!(d.shapes[0].rect, Rect::new(10.0, 20.0, 80.0, 30.0));
        // `b` had no `@shape`: placed below `a`, on the grid.
        assert_eq!(d.shapes[1].rect, Rect::new(20.0, 70.0, 80.0, 30.0));
        assert_eq!(d.connectors[0].from, End::glued("a", None));
        assert_eq!(d.connectors[1].from, End::glued("a", Some(Anchor::S)));
        assert_eq!(
            d.connectors.len(),
            3,
            "the line to a missing shape is dropped"
        );
        assert_eq!(d.connectors[2].from, End::Free(Point::new(5.0, 5.0)));
        assert_eq!(d.connectors[2].to, End::glued("b", Some(Anchor::N)));
        assert_eq!(d.passthrough, ["%% just a note"]);
    }

    #[test]
    fn verbatim_statements_declare_external_nodes() {
        let d = read(
            "flowchart LR\n  a --> b\n  b -.-> c[(Store)]\n  subgraph s [Group]\n    d --> a\n  end\n",
        )
        .unwrap();
        let ext: Vec<_> = d
            .shapes
            .iter()
            .map(|s| (s.id.as_str(), s.external))
            .collect();
        assert_eq!(ext, [("a", false), ("b", false), ("c", true), ("d", true)]);
        assert_eq!(d.shapes[2].label, "Store");
        assert_eq!(d.connectors.len(), 1);
        assert_eq!(
            d.passthrough,
            [
                "  b -.-> c[(Store)]",
                "  subgraph s [Group]",
                "    d --> a",
                "  end"
            ]
        );
    }
}
