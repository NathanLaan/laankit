//! Drives the app headlessly (egui_kittest) against a temp config folder:
//! picking tools from the toolbar and the keyboard, zooming and panning the
//! canvas, and the grid settings reaching `ui-preferences.json`.

use egui::{pos2, Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::Value;

use crate::app::BlockLinerApp;
use crate::canvas::Tool;
use crate::config::Paths;

pub(crate) fn harness() -> (tempfile::TempDir, Paths, Harness<'static, BlockLinerApp>) {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths {
        user_data: tmp.path().join("BlockLiner"),
    };
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
        (Key::V, Tool::Select),
    ] {
        h.key_press(key);
        h.run_steps(2);
        assert_eq!(h.state().tool(), tool, "{key:?}");
    }

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
