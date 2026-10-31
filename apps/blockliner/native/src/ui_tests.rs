//! Drives the app headlessly (egui_kittest) against a temp config folder
//! and project folder: picking tools from the toolbar and the keyboard, zooming and panning the
//! canvas, and the grid settings reaching `ui-preferences.json`.

use blockliner_core::{Diagram, Rect, Shape, ShapeKind};
use egui::{pos2, Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::Value;

use crate::app::BlockLinerApp;
use crate::canvas::Tool;
use crate::config::Paths;

pub(crate) type H = Harness<'static, BlockLinerApp>;

/// The app with the diagram `Diagram` open from `<tmp>/project`, the
/// sidebar hidden (so the canvas starts just right of the toolbar).
pub(crate) fn harness() -> (tempfile::TempDir, Paths, Harness<'static, BlockLinerApp>) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(
        project.join("Diagram.mmd"),
        blockliner_core::write(&Diagram::new()),
    )
    .unwrap();
    let paths = Paths {
        user_data: tmp.path().join("BlockLiner"),
    };
    paths
        .set_prefs(&serde_json::json!({
            "lastProject": project,
            "sidebarVisible": false,
        }))
        .unwrap();
    app_harness(paths.clone(), tmp)
}

/// The app over `paths` as they are.
pub(crate) fn app_harness(
    paths: Paths,
    tmp: tempfile::TempDir,
) -> (tempfile::TempDir, Paths, Harness<'static, BlockLinerApp>) {
    let h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        // 60 fps, so two clicks a few frames apart are a double click
        // (the default step is 0.25 s).
        .with_step_dt(1.0 / 60.0)
        .with_context_setup(marina_egui::fonts::install)
        .build_ui_state(
            |ui, app: &mut BlockLinerApp| app.show(ui),
            BlockLinerApp::new(None, paths.clone()),
        );
    (tmp, paths, h)
}

fn prefs(paths: &Paths) -> Value {
    serde_json::from_str(&std::fs::read_to_string(paths.ui_prefs()).unwrap()).unwrap()
}

/// A press-move-release with `button`, a frame per event.
pub(crate) fn drag(
    h: &mut Harness<'static, BlockLinerApp>,
    button: PointerButton,
    from: Pos2,
    to: Pos2,
) {
    let press = |pos, pressed| Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    };
    for event in [
        Event::PointerMoved(from),
        press(from, true),
        Event::PointerMoved(from.lerp(to, 0.5)),
        Event::PointerMoved(to),
        press(to, false),
    ] {
        h.event(event);
        h.step();
    }
}

/// kittest wraps the app in an 8 px outer margin (the real window has none).
const KITTEST_MARGIN: f32 = 8.0;

/// The canvas starts at 100 % with the model origin at its top-left corner,
/// just right of the 48 px toolbar, so `at(x, y)` is model (x, y).
pub(crate) fn at(x: f32, y: f32) -> Pos2 {
    pos2(
        KITTEST_MARGIN + marina_egui::toolbar::WIDTH + x,
        KITTEST_MARGIN + y,
    )
}

pub(crate) fn click(h: &mut H, p: Pos2) {
    for pressed in [true, false] {
        h.event(Event::PointerMoved(p));
        h.event(Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
}

/// `a` at (100, 100) and `b` at (400, 300), both 160 × 60.
pub(crate) fn two_shapes(h: &mut H) {
    let d = h.state_mut().diagram_mut();
    d.shapes.push(Shape::new(
        "a",
        ShapeKind::Rect,
        "A",
        Rect::new(100.0, 100.0, 160.0, 60.0),
    ));
    d.shapes.push(Shape::new(
        "b",
        ShapeKind::RoundRect,
        "B",
        Rect::new(400.0, 300.0, 160.0, 60.0),
    ));
    h.run_steps(2);
}

#[test]
fn tools_from_the_toolbar_and_the_keyboard() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    assert_eq!(h.state().tool(), Tool::Select);

    h.get_by_label("Rectangle").click();
    h.run_steps(2);
    assert_eq!(h.state().tool(), Tool::Rect);
    h.get_by_label("Rounded Rectangle").click();
    h.run_steps(2);
    assert_eq!(h.state().tool(), Tool::RoundRect);

    for (key, tool) in [
        (Key::A, Tool::Arrow),
        (Key::L, Tool::Line),
        (Key::T, Tool::Text),
        (Key::J, Tool::ElbowLine),
        (Key::E, Tool::ElbowArrow),
        (Key::D, Tool::Diamond),
        (Key::H, Tool::Hexagon),
        (Key::C, Tool::Database),
        (Key::S, Tool::Subroutine),
        (Key::V, Tool::Select),
    ] {
        h.key_press(key);
        h.run_steps(2);
        assert_eq!(h.state().tool(), tool, "{key:?}");
    }

    // The numbers follow the toolbar; with Shift the tool stays. 4 is More
    // Shapes: the last of the four picked (S, above).
    for (key, tool) in [
        (Key::Num2, Tool::Rect),
        (Key::Num3, Tool::RoundRect),
        (Key::Num4, Tool::Subroutine),
        (Key::Num5, Tool::Line),
        (Key::Num6, Tool::Arrow),
        (Key::Num7, Tool::ElbowLine),
        (Key::Num8, Tool::ElbowArrow),
        (Key::Num9, Tool::Text),
        (Key::Num1, Tool::Select),
    ] {
        h.key_press(key);
        h.run_steps(2);
        assert_eq!(h.state().tool(), tool, "{key:?}");
    }
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    h.key_press_modifiers(Modifiers::SHIFT, Key::Num2);
    h.run_steps(2);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    assert_eq!(h.state().tool(), Tool::Rect);
    click(&mut h, at(200.0, 200.0));
    h.run_steps(3);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(h.state().diagram().shapes.len(), 1);
    assert_eq!(h.state().tool(), Tool::Rect, "kept after the shape");
    h.key_press(Key::Num1);
    h.run_steps(2);

    // With a dialog open the single-key shortcuts are off.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run_steps(2);
    assert!(h.state().has_overlay());
    h.key_press(Key::R);
    h.run_steps(2);
    assert_eq!(h.state().tool(), Tool::Select);
}

#[test]
fn zoom_and_pan() {
    let (_tmp, _paths, mut h) = harness();
    h.run_steps(2);
    let zoom = |h: &Harness<'static, BlockLinerApp>| h.state().canvas().view.zoom;

    h.key_press_modifiers(Modifiers::COMMAND, Key::Equals);
    h.run_steps(2);
    assert_eq!(zoom(&h), 1.25);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Minus);
    h.run_steps(1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Minus);
    h.run_steps(2);
    assert_eq!(zoom(&h), 0.75);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num0);
    h.run_steps(2);
    assert_eq!(zoom(&h), 1.0);

    // Pinch / Ctrl+wheel zooms about the pointer: the point under it stays.
    let at = pos2(600.0, 400.0);
    h.hover_at(at);
    h.run_steps(2);
    let before = h.state().canvas().pointer.unwrap();
    h.event(Event::Zoom(2.0));
    h.run_steps(2);
    assert_eq!(zoom(&h), 2.0);
    let after = h.state().canvas().pointer.unwrap();
    assert!((after - before).length() < 0.01, "{before:?} → {after:?}");

    // A middle drag pans: the content follows the pointer (at 2×, 100
    // screen points are 50 model units).
    let pan = h.state().canvas().view.pan;
    drag(
        &mut h,
        PointerButton::Middle,
        at,
        at + egui::vec2(100.0, -40.0),
    );
    h.run_steps(2);
    let moved = h.state().canvas().view.pan - pan;
    assert!(
        (moved - egui::vec2(-50.0, 20.0)).length() < 0.01,
        "{moved:?}"
    );

    // A plain left drag with the Select tool doesn't pan.
    let pan = h.state().canvas().view.pan;
    drag(
        &mut h,
        PointerButton::Primary,
        at,
        at + egui::vec2(80.0, 80.0),
    );
    h.run_steps(2);
    assert_eq!(h.state().canvas().view.pan, pan);
}

#[test]
fn grid_settings_are_saved() {
    let (_tmp, paths, mut h) = harness();
    h.run_steps(2);
    assert!(h.state().canvas().show_grid);
    assert_eq!(h.state().diagram().grid, 20.0);

    h.key_press_modifiers(Modifiers::COMMAND, Key::Quote);
    h.run_steps(2);
    assert!(!h.state().canvas().show_grid);
    assert_eq!(prefs(&paths)["showGrid"], false);

    // And back on from Settings.
    h.get_by_label("Settings").click();
    h.run_steps(3);
    assert!(h.state().has_overlay());
    h.get_by_label("Show Grid").click();
    h.run_steps(2);
    assert!(h.state().canvas().show_grid);
    assert_eq!(prefs(&paths)["showGrid"], true);
}

#[test]
fn the_zoom_readout_resets_to_100() {
    let (_tmp, paths, mut h) = harness();
    h.run_steps(2);
    let zoom = |h: &H| h.state().canvas().view.zoom;
    let file = paths.prefs().last_project.unwrap().join("Diagram.mmd");
    let saved = std::fs::read_to_string(&file).unwrap();

    for _ in 0..2 {
        h.key_press_modifiers(Modifiers::COMMAND, Key::Equals);
        h.run_steps(2);
    }
    assert_eq!(zoom(&h), 1.5);
    h.get_by_label("Zoom 150%").click();
    h.run_steps(2);
    assert_eq!(zoom(&h), 1.0);
    h.get_by_label("Zoom 100%");

    // A view change, not an edit: nothing to undo, nothing saved.
    assert!(!h.state().can_undo());
    assert!(!h.state().is_saving());
    h.state().flush();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), saved);

    // At 100 % a click does nothing visible.
    h.get_by_label("Zoom 100%").click();
    h.run_steps(2);
    assert_eq!(zoom(&h), 1.0);

    // With the status bar hidden there's no readout; Ctrl+0 still works.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Equals);
    h.key_press_modifiers(Modifiers::COMMAND, Key::J);
    h.run_steps(2);
    assert!(h.query_by_label("Zoom 125%").is_none());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num0);
    h.run_steps(2);
    assert_eq!(zoom(&h), 1.0);
}
