//! The Mermaid file format end to end: golden files under `fixtures/`, and a
//! round trip over generated diagrams.
//!
//! `BLOCKLINER_BLESS=1 cargo test -p blockliner-core` rewrites the
//! `*.expected.mmd` files from the current output; review the diff.

use std::path::PathBuf;

use blockliner_core::style::{Colour, Style};

use blockliner_core::{
    read, write, Anchor, Connector, ConnectorKind, Diagram, Direction, End, Mid, Point, Rect,
    Route, Shape, ShapeKind,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn load(name: &str) -> String {
    std::fs::read_to_string(fixture(name)).unwrap()
}

/// `write(read(input))` must equal `name`'s contents (or, blessing, becomes them).
fn check_golden(input: &str, name: &str) -> Diagram {
    let d = read(input).unwrap();
    let out = write(&d);
    if std::env::var_os("BLOCKLINER_BLESS").is_some() {
        std::fs::write(fixture(name), &out).unwrap();
    }
    assert_eq!(out, load(name), "{name}");
    d
}

#[test]
fn blockliner_files_are_unchanged_by_a_save() {
    let input = load("written.mmd");
    let d = read(&input).unwrap();
    assert_eq!(write(&d), input);

    assert_eq!(d.shapes.len(), 12);
    assert_eq!(d.shape("orders").unwrap().label, "Orders \"v2\"\nService");
    assert_eq!(d.shape("caption").unwrap().kind, ShapeKind::Text);
    // The shapes BlockLiner draws as themselves, delimiters unchanged.
    let kind = |id| d.shape(id).unwrap().kind.clone();
    assert_eq!(kind("db"), ShapeKind::Database);
    assert_eq!(kind("valid"), ShapeKind::Diamond);
    assert_eq!(kind("prepare"), ShapeKind::Hexagon);
    assert_eq!(kind("auth"), ShapeKind::Subroutine);
    assert_eq!(kind("start"), ShapeKind::Stadium);
    assert_eq!(kind("hub"), ShapeKind::Circle);
    assert_eq!(kind("input"), ShapeKind::Parallelogram);
    assert_eq!(kind("queue"), ShapeKind::Trapezoid);
    // The second gateway → orders edge has its own anchors.
    assert_eq!(d.connectors[1].from, End::glued("gateway", None));
    assert_eq!(d.connectors[3].from, End::glued("gateway", Some(Anchor::S)));
    assert_eq!(d.connectors[2].label.as_deref(), Some("SQL"));
    assert!(d.has_free_lines());
    assert_eq!(d.connectors[5].to, End::Free(Point::new(700.0, 300.0)));
    // `style db …` is db's own now, as is the second edge's `linkStyle`
    // and the free line's colour.
    assert_eq!(d.shape("db").unwrap().style.props(), "fill:#eef");
    assert_eq!(
        d.connectors[1].style.props(),
        "stroke:#3b82f6,stroke-width:2px"
    );
    assert_eq!(d.connectors[5].style.props(), "stroke:#ef4444");
    assert!(d.passthrough.is_empty());
}

#[test]
fn hand_written_flowcharts_are_normalised_once() {
    let d = check_golden(&load("handwritten.mmd"), "handwritten.expected.mmd");
    // A second save changes nothing.
    let again = write(&read(&load("handwritten.expected.mmd")).unwrap());
    assert_eq!(again, load("handwritten.expected.mmd"));

    assert_eq!(d.direction, Direction::TD);
    assert_eq!(d.preamble.len(), 4, "front matter and init directive");
    let shapes: Vec<_> = d
        .shapes
        .iter()
        .map(|s| (s.id.as_str(), s.label.as_str(), s.external))
        .collect();
    assert_eq!(
        shapes,
        [
            ("web", "Web App", false),
            ("api", "API Gateway", false),
            ("orders", "orders", false),
            ("bus", "Event Bus", false),
            ("orders_db", "Orders DB", false),
            ("audit", "audit", true),
            ("pay", "Payment Service", true),
            ("psp", "psp", true),
            ("a1", "a1", true),
            ("a2", "a2", true),
            ("note1", "Retries are async", false),
        ]
    );
    let edges: Vec<_> = d
        .connectors
        .iter()
        .map(|c| {
            (
                c.from.shape().unwrap(),
                c.to.shape().unwrap(),
                c.kind,
                c.label.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        edges,
        [
            ("web", "api", ConnectorKind::Arrow, None),
            ("api", "orders", ConnectorKind::Arrow, Some("REST")),
            ("api", "bus", ConnectorKind::Arrow, Some("events")),
            ("orders", "orders_db", ConnectorKind::Line, None),
        ]
    );
    // Every node got a position on the grid, and none overlap.
    for (i, a) in d.shapes.iter().enumerate() {
        assert_eq!(
            (a.rect.x % d.grid, a.rect.y % d.grid),
            (0.0, 0.0),
            "{}",
            a.id
        );
        for b in &d.shapes[i + 1..] {
            let apart = a.rect.x + a.rect.w <= b.rect.x
                || b.rect.x + b.rect.w <= a.rect.x
                || a.rect.bottom() <= b.rect.y
                || b.rect.bottom() <= a.rect.y;
            assert!(apart, "{} overlaps {}", a.id, b.id);
        }
    }
}

/// A small deterministic generator, so failures reproduce from the seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// Multiples of 0.5, some negative.
    fn coord(&mut self) -> f32 {
        (self.below(4000) as f32 - 1000.0) / 2.0
    }

    fn text(&mut self) -> String {
        const PARTS: &[&str] = &[
            "API",
            " ",
            "Orders DB",
            "\"",
            "#",
            "#quot;",
            "C#",
            "\n",
            "<br>",
            "<b>x</b>",
            "|",
            ";",
            "[",
            "]",
            "(",
            ")",
            "{",
            "}",
            "-->",
            "Ü",
            "✓",
            "%%",
            "a & b",
            ":::",
            "  ",
        ];
        (0..self.below(5)).map(|_| *self.pick(PARTS)).collect()
    }

    fn style(&mut self) -> Style {
        const COLOURS: &[&str] = &["#dbeafe", "#f9f", "red", "#11223380", "rgb(1,2,3)", "none"];
        let mut s = Style::default();
        let colour = |rng: &mut Rng| rng.chance(50).then(|| Colour::parse(rng.pick(COLOURS)));
        s.fill = colour(self);
        s.stroke = colour(self);
        s.color = colour(self);
        if self.chance(30) {
            s.extra.push(("stroke-width".into(), "4px".into()));
        }
        if self.chance(20) {
            s.extra.push(("stroke-dasharray".into(), "5 5".into()));
        }
        s
    }

    fn route(&mut self) -> Route {
        match self.below(4) {
            0 | 1 => Route::Straight,
            2 => Route::ELBOW,
            _ => Route::Elbow {
                mid: Some(if self.chance(50) {
                    Mid::X(self.coord())
                } else {
                    Mid::Y(self.coord())
                }),
            },
        }
    }

    fn anchor(&mut self) -> Option<Anchor> {
        *self.pick(&[
            None,
            None,
            Some(Anchor::N),
            Some(Anchor::E),
            Some(Anchor::S),
            Some(Anchor::W),
        ])
    }
}

fn generate(rng: &mut Rng) -> Diagram {
    let mut d = Diagram::new();
    d.direction = *rng.pick(&[
        Direction::LR,
        Direction::TB,
        Direction::TD,
        Direction::RL,
        Direction::BT,
    ]);
    d.grid = *rng.pick(&[10.0, 20.0, 40.0, 12.5]);
    for _ in 0..rng.below(8) {
        let label = rng.text();
        let kind = rng
            .pick(&[
                ShapeKind::Rect,
                ShapeKind::RoundRect,
                ShapeKind::Text,
                ShapeKind::Diamond,
                ShapeKind::Hexagon,
                ShapeKind::Database,
                ShapeKind::Subroutine,
                ShapeKind::Stadium,
                ShapeKind::Circle,
                ShapeKind::Parallelogram,
                ShapeKind::Trapezoid,
                ShapeKind::DoubleCircle,
                ShapeKind::ReverseParallelogram,
                ShapeKind::InvertedTrapezoid,
                ShapeKind::Asymmetric,
            ])
            .clone();
        let rect = Rect::new(
            rng.coord(),
            rng.coord(),
            rng.below(400) as f32 + 0.5,
            rng.below(200) as f32 + 1.0,
        );
        let mut shape = Shape::new(d.new_id(&label), kind, label, rect);
        if shape.kind != ShapeKind::Text && rng.chance(20) {
            shape.class = Some("hot".into());
        }
        if rng.chance(40) {
            shape.style = rng.style();
        }
        d.shapes.push(shape);
    }
    let ids: Vec<String> = d.shapes.iter().map(|s| s.id.clone()).collect();
    let kinds = [ConnectorKind::Line, ConnectorKind::Arrow];
    // Mermaid edges first, then free lines: the order a read gives back.
    if !ids.is_empty() {
        for _ in 0..rng.below(8) {
            let mut c = Connector::new(
                *rng.pick(&kinds),
                End::glued(rng.pick(&ids).clone(), rng.anchor()),
                End::glued(rng.pick(&ids).clone(), rng.anchor()),
            );
            if rng.chance(30) {
                c.label = Some(rng.text());
            }
            c.route = rng.route();
            if rng.chance(30) {
                c.style = rng.style();
            }
            d.connectors.push(c);
        }
    }
    for _ in 0..rng.below(4) {
        let end = |rng: &mut Rng| {
            if ids.is_empty() || rng.chance(50) {
                End::Free(Point::new(rng.coord(), rng.coord()))
            } else {
                End::glued(rng.pick(&ids).clone(), rng.anchor())
            }
        };
        let (from, to) = (end(rng), End::Free(Point::new(rng.coord(), rng.coord())));
        let (from, to) = if rng.chance(50) {
            (from, to)
        } else {
            (to, from)
        };
        let route = rng.route();
        let mut line = Connector::new(*rng.pick(&kinds), from, to).with_route(route);
        // A free line keeps only its colour.
        if rng.chance(30) {
            line.style.stroke = rng.style().stroke;
        }
        d.connectors.push(line);
    }
    if rng.chance(20) {
        d.passthrough.push("  %% kept".into());
    }
    if rng.chance(20) {
        d.preamble.push("%%{init: {\"theme\": \"dark\"}}%%".into());
    }
    d
}

#[test]
fn generated_diagrams_round_trip() {
    for seed in 0..2000 {
        let d = generate(&mut Rng(seed));
        let text = write(&d);
        let back = read(&text).unwrap_or_else(|e| panic!("seed {seed}: {e}\n{text}"));
        assert_eq!(back, d, "seed {seed}:\n{text}");
        assert_eq!(write(&back), text, "seed {seed}");
    }
}
