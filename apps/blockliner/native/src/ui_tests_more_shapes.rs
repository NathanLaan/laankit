//! The four more shapes, headlessly: the More Shapes button and its list
//! (from the corner or a right-click), the button keeping the last pick,
//! their letters and default sizes, a click in a diamond's empty corner,
//! and an arrow glued to a diamond's vertex.

use blockliner_core::{write, Anchor, Connector, ConnectorKind, End, Rect, Shape, ShapeKind};
use egui::{Key, PointerButton};
use egui_kittest::kittest::Queryable;

use crate::canvas::Tool;
use crate::ui_tests::{at, click, drag, harness, H};

fn kind(h: &H, i: usize) -> (ShapeKind, Rect) {
    let s = &h.state().diagram().shapes[i];
    (s.kind.clone(), s.rect)
}

#[test]
fn the_more_shapes_button_keeps_the_last_pick() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);

    // It starts as the diamond, and a click picks it.
    h.get_by_label("More Shapes: Diamond").click();
    h.run_steps(2);
    assert_eq!(h.state().tool(), Tool::Diamond);

    // Its corner opens the four; picking one closes the list.
    h.get_by_label("Show More Shapes").click();
    h.run_steps(3);
    for name in ["Diamond", "Hexagon", "Database", "Subroutine"] {
        h.get_by_label(name);
    }
    h.get_by_label("Hexagon").click();
    h.run_steps(3);
    assert_eq!(h.state().tool(), Tool::Hexagon);
    assert!(h.query_by_label("Database").is_none(), "closed");

    // The button now shows and picks the hexagon, as 4 does.
    h.key_press(Key::V);
    h.run_steps(2);
    h.get_by_label("More Shapes: Hexagon").click();
    h.run_steps(2);
    assert_eq!(h.state().tool(), Tool::Hexagon);
    h.key_press(Key::V);
    h.key_press(Key::Num4);
    h.run_steps(2);
    assert_eq!(h.state().tool(), Tool::Hexagon);

    // A right-click opens the list too; a letter changes the button.
    h.get_by_label("More Shapes: Hexagon").click_secondary();
    h.run_steps(3);
    h.get_by_label("Subroutine").click();
    h.run_steps(3);
    assert_eq!(h.state().tool(), Tool::Subroutine);
    h.key_press(Key::C);
    h.run_steps(2);
    assert_eq!(h.state().tool(), Tool::Database);
    h.get_by_label("More Shapes: Database");
}

#[test]
fn each_letter_makes_its_shape_at_its_default_size() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    for (key, x) in [
        (Key::D, 200.0),
        (Key::H, 400.0),
        (Key::C, 600.0),
        (Key::S, 800.0),
    ] {
        h.key_press(key);
        h.run_steps(2);
        click(&mut h, at(x, 300.0));
        h.run_steps(2);
        h.key_press(Key::Escape);
        h.run_steps(2);
    }
    // 8 × 4, 8 × 3, 6 × 4 and 8 × 3 cells of 20, centred on the click and
    // snapped to the grid.
    assert_eq!(
        kind(&h, 0),
        (ShapeKind::Diamond, Rect::new(120.0, 260.0, 160.0, 80.0))
    );
    assert_eq!(
        kind(&h, 1),
        (ShapeKind::Hexagon, Rect::new(320.0, 280.0, 160.0, 60.0))
    );
    assert_eq!(
        kind(&h, 2),
        (ShapeKind::Database, Rect::new(540.0, 260.0, 120.0, 80.0))
    );
    assert_eq!(
        kind(&h, 3),
        (ShapeKind::Subroutine, Rect::new(720.0, 280.0, 160.0, 60.0))
    );
    let text = write(h.state().diagram());
    let ids: Vec<&str> = h
        .state()
        .diagram()
        .shapes
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    for (id, open, close) in [
        (ids[0], "{", "}"),
        (ids[1], "{{", "}}"),
        (ids[2], "[(", ")]"),
        (ids[3], "[[", "]]"),
    ] {
        let line = format!("  {id}{open}\"\"{close}\n");
        assert!(text.contains(&line), "{line:?} in\n{text}");
    }
}

/// A diamond `a` at (100, 100), 160 × 80 (vertices N (180, 100), E (260,
/// 140), S (180, 180), W (100, 140)), and a box `b` at (400, 300).
fn diamond_and_box(h: &mut H) {
    let d = h.state_mut().diagram_mut();
    d.shapes.push(Shape::new(
        "a",
        ShapeKind::Diamond,
        "Valid?",
        Rect::new(100.0, 100.0, 160.0, 80.0),
    ));
    d.shapes.push(Shape::new(
        "b",
        ShapeKind::Rect,
        "B",
        Rect::new(400.0, 300.0, 160.0, 60.0),
    ));
    h.run_steps(2);
}

#[test]
fn a_diamonds_empty_corner_selects_nothing() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    diamond_and_box(&mut h);
    click(&mut h, at(110.0, 108.0));
    h.run_steps(2);
    assert!(h.state().selection().is_empty());
    click(&mut h, at(180.0, 140.0));
    h.run_steps(2);
    assert_eq!(h.state().selection(), ["a"]);
}

#[test]
fn an_arrow_glues_to_a_diamonds_vertex() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    diamond_and_box(&mut h);
    h.key_press(Key::A);
    h.run_steps(2);
    // From near the diamond's E vertex to near b's W anchor.
    drag(
        &mut h,
        PointerButton::Primary,
        at(258.0, 142.0),
        at(403.0, 327.0),
    );
    h.run_steps(2);
    assert_eq!(
        h.state().diagram().connectors,
        [Connector::new(
            ConnectorKind::Arrow,
            End::glued("a", Some(Anchor::E)),
            End::glued("b", Some(Anchor::W)),
        )]
    );
    let text = write(h.state().diagram());
    assert!(text.contains("\n  a{\"Valid?\"}\n"), "{text}");
    assert!(text.contains("\n  a --> b\n"), "{text}");
}

#[test]
fn the_later_four_come_from_the_list() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    for (name, x) in [
        ("Stadium", 200.0),
        ("Circle", 400.0),
        ("Parallelogram", 600.0),
        ("Trapezoid", 800.0),
    ] {
        h.get_by_label("Show More Shapes").click();
        h.run_steps(3);
        h.get_by_label(name).click();
        h.run_steps(3);
        assert_eq!(h.state().tool().name(), name);
        // The button now shows it; 4 picks it, with no letter of its own.
        h.get_by_label(&format!("More Shapes: {name}"));
        click(&mut h, at(x, 300.0));
        h.run_steps(2);
        h.key_press(Key::Escape);
        h.run_steps(2);
    }
    // 8 × 3 cells, and a circle 4 × 4, centred on the click and snapped.
    assert_eq!(
        kind(&h, 0),
        (ShapeKind::Stadium, Rect::new(120.0, 280.0, 160.0, 60.0))
    );
    assert_eq!(
        kind(&h, 1),
        (ShapeKind::Circle, Rect::new(360.0, 260.0, 80.0, 80.0))
    );
    assert_eq!(kind(&h, 2).0, ShapeKind::Parallelogram);
    assert_eq!(kind(&h, 3).0, ShapeKind::Trapezoid);
    let text = write(h.state().diagram());
    let ids: Vec<String> = h
        .state()
        .diagram()
        .shapes
        .iter()
        .map(|s| s.id.clone())
        .collect();
    for (id, open, close) in [
        (&ids[0], "([", "])"),
        (&ids[1], "((", "))"),
        (&ids[2], "[/", "/]"),
        (&ids[3], "[/", "\\]"),
    ] {
        let line = format!("  {id}{open}\"\"{close}\n");
        assert!(text.contains(&line), "{line:?} in\n{text}");
    }
    // A click in a circle's empty corner misses it.
    click(&mut h, at(364.0, 264.0));
    h.run_steps(2);
    assert!(h.state().selection().is_empty());
}
