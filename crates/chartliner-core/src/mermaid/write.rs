//! `Diagram` → `.mmd` text. Deterministic: the same diagram always gives the
//! same bytes, so saves only change what changed.

use std::collections::HashMap;
use std::fmt::Write;

use super::label::encode;
use super::{edge_key, FORMAT_VERSION, TEXT_CLASS};
use crate::model::{Connector, ConnectorKind, Diagram, End, Shape, ShapeKind};

pub fn write(d: &Diagram) -> String {
    let mut out = String::new();
    for line in &d.preamble {
        out.push_str(line);
        out.push('\n');
    }
    line(&mut out, format_args!("flowchart {}", d.direction.as_str()));
    for s in d.shapes.iter().filter(|s| !s.external) {
        line(&mut out, format_args!("  {}", node(s)));
    }
    for c in d.connectors.iter().filter(|c| c.is_edge()) {
        line(&mut out, format_args!("  {}", edge(c)));
    }
    for p in &d.passthrough {
        line(&mut out, format_args!("{p}"));
    }
    if d.shapes.iter().any(|s| s.kind == ShapeKind::Text) {
        line(
            &mut out,
            format_args!("  classDef {TEXT_CLASS} fill:none,stroke:none"),
        );
    }

    out.push('\n');
    line(
        &mut out,
        format_args!("%% chartliner {FORMAT_VERSION} grid={}", num(d.grid)),
    );
    for s in &d.shapes {
        let r = s.rect;
        line(
            &mut out,
            format_args!(
                "%% @shape {} x={} y={} w={} h={}",
                s.id,
                num(r.x),
                num(r.y),
                num(r.w),
                num(r.h)
            ),
        );
    }
    let mut seen: HashMap<(&str, &str), usize> = HashMap::new();
    for c in d.connectors.iter().filter(|c| c.is_edge()) {
        let (
            End::Glued {
                shape: from,
                anchor: a,
            },
            End::Glued {
                shape: to,
                anchor: b,
            },
        ) = (&c.from, &c.to)
        else {
            unreachable!("is_edge");
        };
        let n = seen.entry((from, to)).or_default();
        *n += 1;
        if a.is_none() && b.is_none() {
            continue;
        }
        let mut entry = format!("%% @edge {}", edge_key(from, to, *n));
        if let Some(a) = a {
            let _ = write!(entry, " from={}", a.as_str());
        }
        if let Some(b) = b {
            let _ = write!(entry, " to={}", b.as_str());
        }
        line(&mut out, format_args!("{entry}"));
    }
    for c in d.connectors.iter().filter(|c| !c.is_edge()) {
        line(
            &mut out,
            format_args!(
                "%% @line {} from={} to={}",
                kind(c.kind),
                end(&c.from),
                end(&c.to)
            ),
        );
    }
    out
}

fn line(out: &mut String, args: std::fmt::Arguments) {
    let _ = out.write_fmt(args);
    out.push('\n');
}

fn node(s: &Shape) -> String {
    let label = encode(&s.label);
    match &s.kind {
        ShapeKind::Rect => format!("{}[\"{label}\"]{}", s.id, class(s)),
        ShapeKind::RoundRect => format!("{}(\"{label}\"){}", s.id, class(s)),
        ShapeKind::Text => format!("{}[\"{label}\"]:::{TEXT_CLASS}", s.id),
        ShapeKind::Other { open, close } => format!("{}{open}\"{label}\"{close}{}", s.id, class(s)),
    }
}

fn class(s: &Shape) -> String {
    s.class
        .as_ref()
        .map(|c| format!(":::{c}"))
        .unwrap_or_default()
}

fn edge(c: &Connector) -> String {
    let link = match c.kind {
        ConnectorKind::Line => "---",
        ConnectorKind::Arrow => "-->",
    };
    let label = c
        .label
        .as_ref()
        .map(|l| format!("|\"{}\"|", encode(l)))
        .unwrap_or_default();
    format!(
        "{} {link}{label} {}",
        c.from.shape().unwrap_or_default(),
        c.to.shape().unwrap_or_default()
    )
}

fn kind(k: ConnectorKind) -> &'static str {
    match k {
        ConnectorKind::Line => "line",
        ConnectorKind::Arrow => "arrow",
    }
}

fn end(e: &End) -> String {
    match e {
        End::Free(p) => format!("{},{}", num(p.x), num(p.y)),
        End::Glued {
            shape,
            anchor: Some(a),
        } => format!("{shape}:{}", a.as_str()),
        End::Glued {
            shape,
            anchor: None,
        } => shape.clone(),
    }
}

/// Shortest form that reads back exactly: `40`, `40.5`, never `-0`.
fn num(v: f32) -> String {
    if v == 0.0 {
        "0".into()
    } else {
        v.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Anchor, Point, Rect};

    #[test]
    fn writes_nodes_edges_and_layout() {
        let mut d = Diagram::new();
        let r = |x| Rect::new(x, 40.0, 160.0, 60.0);
        d.shapes
            .push(Shape::new("api", ShapeKind::Rect, "API \"v2\"", r(40.0)));
        d.shapes.push(Shape::new(
            "orders",
            ShapeKind::RoundRect,
            "Orders\nService",
            r(240.0),
        ));
        d.shapes.push(Shape::new(
            "note",
            ShapeKind::Text,
            "async",
            Rect::new(40.0, 140.5, 200.0, 40.0),
        ));
        d.connectors.push(Connector::new(
            ConnectorKind::Arrow,
            End::glued("api", Some(Anchor::E)),
            End::glued("orders", None),
        ));
        d.connectors.push(Connector {
            label: Some("reads".into()),
            ..Connector::new(
                ConnectorKind::Line,
                End::glued("orders", None),
                End::glued("api", None),
            )
        });
        d.connectors.push(Connector::new(
            ConnectorKind::Arrow,
            End::Free(Point::new(-0.0, 10.0)),
            End::glued("note", Some(Anchor::N)),
        ));
        assert_eq!(
            write(&d),
            "flowchart LR\n\
             \x20 api[\"API #quot;v2#quot;\"]\n\
             \x20 orders(\"Orders<br>Service\")\n\
             \x20 note[\"async\"]:::cl_text\n\
             \x20 api --> orders\n\
             \x20 orders ---|\"reads\"| api\n\
             \x20 classDef cl_text fill:none,stroke:none\n\
             \n\
             %% chartliner 1 grid=20\n\
             %% @shape api x=40 y=40 w=160 h=60\n\
             %% @shape orders x=240 y=40 w=160 h=60\n\
             %% @shape note x=40 y=140.5 w=200 h=40\n\
             %% @edge api->orders from=E\n\
             %% @line arrow from=0,10 to=note:N\n"
        );
    }
}
