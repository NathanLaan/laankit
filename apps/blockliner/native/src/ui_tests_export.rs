//! Exporting, headlessly: the SVG for a small diagram (a golden file;
//! `BLOCKLINER_BLESS=1` rewrites it), the PNG's size and colours, the grid
//! option, and the Export dialog from the keyboard to the written file.

use std::path::Path;

use blockliner_core::export::{scope, Objects};
use blockliner_core::style::Colour;
use blockliner_core::{
    Anchor, Connector, ConnectorKind, Diagram, End, Rect, Route, Shape, ShapeKind,
};
use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;

use crate::export::{self, Format, Options};
use crate::ui_tests::{at, click, harness, two_shapes};

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/export.svg");

/// A context with the app's fonts, ready to lay out text.
fn context() -> egui::Context {
    let ctx = egui::Context::default();
    laankit_egui::fonts::install(&ctx);
    let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
    // No renderer to hand the font atlas to.
    out.textures_delta.clear();
    ctx
}

/// An arrow from a box to a round box (in the Blue swatch), a long label
/// that wraps, a text box, an edge label, a right-angle line from the text
/// box up to the round box, and a row of the four more shapes (the
/// database in the Green swatch).
fn diagram() -> Diagram {
    let mut d = Diagram::new();
    d.shapes.push(Shape::new(
        "api",
        ShapeKind::Rect,
        "API Gateway & <Auth>",
        Rect::new(40.0, 40.0, 160.0, 60.0),
    ));
    let mut db = Shape::new(
        "db",
        ShapeKind::RoundRect,
        "Orders database with a long name",
        Rect::new(300.0, 40.0, 120.0, 60.0),
    );
    // The Blue swatch: its own fill, stroke and label colour.
    db.style
        .set_fill(Some(Colour::parse(blockliner_core::style::PRESETS[1].fill)));
    d.shapes.push(db);
    d.shapes.push(Shape::new(
        "note",
        ShapeKind::Text,
        "Async",
        Rect::new(40.0, 140.0, 200.0, 40.0),
    ));
    for (id, kind, label, x, w, h) in [
        ("valid", ShapeKind::Diamond, "Valid?", 40.0, 160.0, 80.0),
        ("prepare", ShapeKind::Hexagon, "Prepare", 240.0, 160.0, 60.0),
        (
            "orders",
            ShapeKind::Database,
            "Orders DB",
            440.0,
            120.0,
            80.0,
        ),
        ("auth", ShapeKind::Subroutine, "Auth", 600.0, 160.0, 60.0),
    ] {
        let mut s = Shape::new(id, kind, label, Rect::new(x, 220.0, w, h));
        if id == "orders" {
            s.style
                .set_fill(Some(Colour::parse(blockliner_core::style::PRESETS[2].fill)));
        }
        d.shapes.push(s);
    }
    // Phase 16's four, a row lower.
    for (id, kind, x, w, h) in [
        ("start", ShapeKind::Stadium, 40.0, 160.0, 60.0),
        ("hub", ShapeKind::Circle, 240.0, 80.0, 80.0),
        ("input", ShapeKind::Parallelogram, 360.0, 160.0, 60.0),
        ("queue", ShapeKind::Trapezoid, 560.0, 160.0, 60.0),
    ] {
        let label = kind_name(&kind);
        d.shapes
            .push(Shape::new(id, kind, label, Rect::new(x, 340.0, w, h)));
    }
    let mut arrow = Connector::new(
        ConnectorKind::Arrow,
        End::glued("api", None),
        End::glued("db", None),
    );
    arrow.label = Some("SQL".into());
    d.connectors.push(arrow);
    // A right-angle line, drawn as a polyline, in the Red swatch's stroke.
    let mut elbow = Connector::new(
        ConnectorKind::Line,
        End::glued("note", Some(Anchor::E)),
        End::glued("db", Some(Anchor::S)),
    )
    .with_route(Route::ELBOW);
    elbow.style.stroke = Some(Colour::parse(blockliner_core::style::PRESETS[6].stroke));
    d.connectors.push(elbow);
    d
}

fn kind_name(kind: &ShapeKind) -> &'static str {
    match kind {
        ShapeKind::Stadium => "Start",
        ShapeKind::Circle => "Hub",
        ShapeKind::Parallelogram => "Input",
        _ => "Queue",
    }
}

fn options(format: Format, grid: bool, scale: u32) -> Options {
    Options {
        format,
        grid,
        scale,
    }
}

#[test]
fn the_svg_matches_its_golden_file() {
    let ctx = context();
    let d = diagram();
    let s = scope(&d, Objects::All).unwrap();
    let svg = export::svg(&ctx, &d, &s, false);
    if std::env::var_os("BLOCKLINER_BLESS").is_some() {
        std::fs::write(GOLDEN, &svg).unwrap();
    }
    assert_eq!(svg, std::fs::read_to_string(GOLDEN).unwrap());
    // It parses, and escapes what it must.
    assert!(svg.contains(">API Gateway &amp;</tspan>"), "{svg}");
    assert!(svg.contains(">&lt;Auth&gt;</tspan>"), "{svg}");
    assert!(!svg.contains("class=\"grid\""));
    assert!(
        svg.contains(
            "<polyline points=\"240,160 360,160 360,100\" fill=\"none\" stroke=\"#ef4444\""
        ),
        "{svg}"
    );
    assert!(
        svg.contains(r#"<ellipse cx="280" cy="380" rx="39.25" ry="39.25""#),
        "{svg}"
    );
    // A diamond is its four side midpoints, the stroke inside the shape.
    assert!(
        svg.contains(r#"<polygon points="120,220.75 199.25,260 120,299.25 40.75,260""#),
        "{svg}"
    );
    assert!(export::svg(&ctx, &d, &s, true).contains("class=\"grid\""));
    export::png(&svg, 1.0).expect("resvg reads it");
}

#[test]
fn a_png_is_the_page_times_the_scale() {
    let ctx = context();
    let d = diagram();
    let s = scope(&d, Objects::All).unwrap();
    // The page: x 40..760, y 40..420, plus two cells around.
    assert_eq!(s.bounds, Rect::new(0.0, 0.0, 800.0, 460.0));
    for scale in [1, 2] {
        let o = options(Format::Png, false, scale);
        assert_eq!(export::size(&s, &o), (800 * scale, 460 * scale));
        let bytes = export::render(&ctx, &d, &s, &o).unwrap();
        let png = image::load_from_memory(&bytes).unwrap().into_rgba8();
        assert_eq!(png.dimensions(), (800 * scale, 460 * scale));
        let px = |x: u32, y: u32| png.get_pixel(x * scale, y * scale).0;
        assert_eq!(px(5, 5), [255, 255, 255, 255], "white background");
        // The api box's left edge, in the stroke's ink.
        let edge = px(40, 70);
        assert!(edge[0] < 140 && edge[3] == 255, "{edge:?}");
    }
}

fn written(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn the_export_dialog_writes_the_selection_as_svg() {
    let (tmp, paths, mut h) = harness();
    h.run_steps(2);
    two_shapes(&mut h);
    let out = tmp.path().join("out.svg");
    h.state_mut().set_export_to(out.clone());

    // Nothing selected: the selection choice is off.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    h.run_steps(3);
    assert!(h.state().has_overlay());
    // Clicking it does nothing: still all of a and b, as a 2× PNG.
    h.get_by_label("Selected Objects Only").click();
    h.run_steps(2);
    h.get_by_label("1080 × 680 px");
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(!h.state().has_overlay());

    // Select a, then export it alone as SVG with the grid.
    click(&mut h, at(150.0, 120.0));
    h.run_steps(2);
    h.get_by_label("Export").click();
    h.run_steps(3);
    h.get_by_label("SVG").click();
    h.run_steps(2);
    h.get_by_label("Selected Objects Only").click();
    h.run_steps(2);
    h.get_by_label("Show grid").click();
    h.run_steps(2);
    // The page around a (100..260 × 100..160) and its margin.
    h.get_by_label("240 × 140");
    h.get_by_label("Export…").click();
    h.run_steps(3);

    assert!(!h.state().has_overlay(), "closed once written");
    let svg = written(&out);
    assert!(
        svg.contains("<g id=\"a\">") && !svg.contains("<g id=\"b\">"),
        "{svg}"
    );
    assert!(svg.contains("class=\"grid\""));
    // The choices are remembered, except which objects.
    let prefs = paths.prefs().export;
    assert_eq!(prefs, options(Format::Svg, true, 2));

    // Next time: SVG and the grid again; a PNG export of everything.
    let png = tmp.path().join("out.png");
    h.state_mut().set_export_to(png.clone());
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    h.run_steps(3);
    h.get_by_label("PNG").click();
    h.run_steps(2);
    h.get_by_label("1×").click();
    h.run_steps(2);
    // a and b: 100..560 × 100..360, and the margin.
    h.get_by_label("540 × 340 px");
    h.get_by_label("Export…").click();
    h.run_steps(3);
    let image = image::open(&png).unwrap();
    assert_eq!((image.width(), image.height()), (540, 340));
}

#[test]
fn the_mirrored_shapes_export_as_themselves() {
    let ctx = context();
    let mut d = Diagram::new();
    for (id, kind, x, w, h) in [
        ("ring", ShapeKind::DoubleCircle, 40.0, 100.0, 100.0),
        ("out", ShapeKind::ReverseParallelogram, 160.0, 160.0, 60.0),
        ("manual", ShapeKind::InvertedTrapezoid, 340.0, 160.0, 60.0),
        ("flag", ShapeKind::Asymmetric, 520.0, 160.0, 60.0),
    ] {
        d.shapes
            .push(Shape::new(id, kind, id, Rect::new(x, 40.0, w, h)));
    }
    let s = scope(&d, Objects::All).unwrap();
    let svg = export::svg(&ctx, &d, &s, false);
    // Two rings, the inner one unfilled and 5 in.
    assert!(
        svg.contains(r#"<ellipse cx="90" cy="90" rx="49.25" ry="49.25""#),
        "{svg}"
    );
    assert!(
        svg.contains(r#"<ellipse cx="90" cy="90" rx="44.25" ry="44.25" fill="none""#),
        "{svg}"
    );
    // Polygons with the stroke inside: leaning left, wider at the top, and
    // notched on the left.
    assert!(
        svg.contains(r#"<polygon points="160.75,40.75 299.75,40.75 319.25,99.25 180.25,99.25""#),
        "{svg}"
    );
    assert!(
        svg.contains(r#"<polygon points="340.75,40.75 499.25,40.75 479.75,99.25 360.25,99.25""#),
        "{svg}"
    );
    assert!(
        svg.contains(
            r#"<polygon points="520.75,40.75 679.25,40.75 679.25,99.25 520.75,99.25 550,70""#
        ),
        "{svg}"
    );
    export::png(&svg, 1.0).expect("resvg reads it");
}
