//! `Diagram` → `.mmd` text. Deterministic: the same diagram always gives the
//! same bytes, so saves only change what changed.

use std::collections::HashMap;
use std::fmt::Write;

use super::label::encode;
use super::{edge_key, FORMAT_VERSION, TEXT_CLASS};
use crate::model::{Connector, ConnectorKind, Diagram, End, Mid, Route, Shape, ShapeKind};

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
    for s in d
        .shapes
        .iter()
        .filter(|s| !s.external && !s.style.is_empty())
    {
        line(
            &mut out,
            format_args!("  style {} {}", s.id, s.style.props()),
        );
    }
    // Numbered as Mermaid does: the edges above come first in the file.
    for (n, c) in d.connectors.iter().filter(|c| c.is_edge()).enumerate() {
        if !c.style.is_empty() {
            line(
                &mut out,
                format_args!("  linkStyle {n} {}", c.style.props()),
            );
        }
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
        format_args!("%% blockliner {FORMAT_VERSION} grid={}", num(d.grid)),
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
        if a.is_none() && b.is_none() && c.route == Route::Straight {
            continue;
        }
        let mut entry = format!("%% @edge {}", edge_key(from, to, *n));
        if let Some(a) = a {
            let _ = write!(entry, " from={}", a.as_str());
        }
        if let Some(b) = b {
            let _ = write!(entry, " to={}", b.as_str());
        }
        entry.push_str(&route(c.route));
        line(&mut out, format_args!("{entry}"));
    }
    for c in d.connectors.iter().filter(|c| !c.is_edge()) {
        line(
            &mut out,
            format_args!(
                "%% @line {} from={} to={}{}{}",
                kind(c.kind),
                end(&c.from),
                end(&c.to),
                route(c.route),
                line_stroke(c)
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
        ShapeKind::Text => format!("{}[\"{label}\"]:::{TEXT_CLASS}", s.id),
        kind => {
            let (open, close) = kind.delimiters();
            format!("{}{open}\"{label}\"{close}{}", s.id, class(s))
        }
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

/// ` stroke=#3b82f6`: a free line's colour (only a colour without spaces
/// fits a layout field).
fn line_stroke(c: &Connector) -> String {
    c.style
        .stroke
        .as_ref()
        .filter(|s| !s.text.is_empty() && !s.text.contains(char::is_whitespace))
        .map(|s| format!(" stroke={}", s.text))
        .unwrap_or_default()
}

fn kind(k: ConnectorKind) -> &'static str {
    match k {
        ConnectorKind::Line => "line",
        ConnectorKind::Arrow => "arrow",
    }
}

/// ` route=elbow[ mid=x:240]`, or nothing for a straight connector.
fn route(r: Route) -> String {
    match r {
        Route::Straight => String::new(),
        Route::Elbow { mid: None } => " route=elbow".into(),
        Route::Elbow {
            mid: Some(Mid::X(x)),
        } => format!(" route=elbow mid=x:{}", num(x)),
        Route::Elbow {
            mid: Some(Mid::Y(y)),
        } => format!(" route=elbow mid=y:{}", num(y)),
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
        d.connectors.push(
            Connector::new(
                ConnectorKind::Arrow,
                End::Free(Point::new(-0.0, 10.0)),
                End::glued("note", Some(Anchor::N)),
            )
            .with_route(Route::ELBOW),
        );
        // A right-angle edge needs an @edge line even with dynamic ends.
        d.connectors.push(
            Connector::new(
                ConnectorKind::Arrow,
                End::glued("note", None),
                End::glued("orders", None),
            )
            .with_route(Route::Elbow {
                mid: Some(Mid::X(260.5)),
            }),
        );
        assert_eq!(
            write(&d),
            "flowchart LR\n\
             \x20 api[\"API #quot;v2#quot;\"]\n\
             \x20 orders(\"Orders<br>Service\")\n\
             \x20 note[\"async\"]:::bl_text\n\
             \x20 api --> orders\n\
             \x20 orders ---|\"reads\"| api\n\
             \x20 note --> orders\n\
             \x20 classDef bl_text fill:none,stroke:none\n\
             \n\
             %% blockliner 1 grid=20\n\
             %% @shape api x=40 y=40 w=160 h=60\n\
             %% @shape orders x=240 y=40 w=160 h=60\n\
             %% @shape note x=40 y=140.5 w=200 h=40\n\
             %% @edge api->orders from=E\n\
             %% @edge note->orders route=elbow mid=x:260.5\n\
             %% @line arrow from=0,10 to=note:N route=elbow\n"
        );
    }
}
