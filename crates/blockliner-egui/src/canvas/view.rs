//! The canvas's view of the diagram: pan and zoom. Model coordinates are
//! diagram pixels at 100 % (`blockliner_core::model`); screen coordinates are
//! egui points. `screen = origin + (model - pan) * zoom`, where `origin` is
//! the canvas's top-left corner.

use egui::{Pos2, Vec2};

/// The zoom levels Ctrl+= / Ctrl+- step through; the wheel zooms smoothly
/// between them.
pub const ZOOM_STEPS: [f32; 11] = [0.1, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0, 6.0];
pub const MIN_ZOOM: f32 = ZOOM_STEPS[0];
pub const MAX_ZOOM: f32 = ZOOM_STEPS[ZOOM_STEPS.len() - 1];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// The model point at the canvas's top-left corner.
    pub pan: Vec2,
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
        }
    }
}

impl View {
    pub fn to_screen(self, origin: Pos2, model: Pos2) -> Pos2 {
        origin + (model.to_vec2() - self.pan) * self.zoom
    }

    pub fn to_model(self, origin: Pos2, screen: Pos2) -> Pos2 {
        (self.pan + (screen - origin) / self.zoom).to_pos2()
    }

    /// Moves the content by `delta` screen points (a drag or a scroll).
    pub fn pan_by(&mut self, delta: Vec2) {
        self.pan -= delta / self.zoom;
    }

    /// Zooms to `zoom` (clamped), keeping the model point under `anchor` (a
    /// screen position) where it is.
    pub fn zoom_at(&mut self, origin: Pos2, anchor: Pos2, zoom: f32) {
        let fixed = self.to_model(origin, anchor);
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan = fixed.to_vec2() - (anchor - origin) / self.zoom;
    }

    /// Brings the model rectangle `min`..`max` to the middle of a canvas of
    /// `size` screen points, at the same zoom, when part of it is out of
    /// view. One already in view, or one too big to fit that is partly in
    /// view, leaves the pan alone. Returns whether the view moved.
    pub fn reveal(&mut self, size: Vec2, min: Pos2, max: Pos2) -> bool {
        let seen = size / self.zoom;
        let (lo, hi) = (self.pan.to_pos2(), (self.pan + seen).to_pos2());
        let inside = min.x >= lo.x && min.y >= lo.y && max.x <= hi.x && max.y <= hi.y;
        let touches = min.x < hi.x && max.x > lo.x && min.y < hi.y && max.y > lo.y;
        let too_big = max.x - min.x > seen.x || max.y - min.y > seen.y;
        if inside || (too_big && touches) {
            return false;
        }
        self.pan = (min.to_vec2() + max.to_vec2()) / 2.0 - seen / 2.0;
        true
    }

    /// The next step up (`dir > 0`) or down from the current zoom.
    pub fn step(&self, dir: i32) -> f32 {
        // A little slack, so a zoom a hair off a step still moves.
        const EPS: f32 = 1e-3;
        if dir > 0 {
            ZOOM_STEPS
                .into_iter()
                .find(|&z| z > self.zoom + EPS)
                .unwrap_or(MAX_ZOOM)
        } else {
            ZOOM_STEPS
                .into_iter()
                .rev()
                .find(|&z| z < self.zoom - EPS)
                .unwrap_or(MIN_ZOOM)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    #[test]
    fn reveal_moves_only_for_what_is_out_of_view() {
        let mut v = View {
            pan: vec2(0.0, 0.0),
            zoom: 2.0,
        };
        // An 800 x 600 canvas at 200 % shows 400 x 300 of the diagram.
        let size = vec2(800.0, 600.0);
        assert!(!v.reveal(size, pos2(10.0, 10.0), pos2(110.0, 60.0)));
        assert_eq!(v.pan, vec2(0.0, 0.0));
        // Off to the right: centred, at the same zoom.
        assert!(v.reveal(size, pos2(1000.0, 100.0), pos2(1100.0, 200.0)));
        assert_eq!(v.pan, vec2(850.0, 0.0));
        assert_eq!(v.zoom, 2.0);
        // Partly in view: centred too.
        assert!(v.reveal(size, pos2(800.0, 0.0), pos2(900.0, 50.0)));
        // Bigger than the view and partly in it: left alone.
        let pan = v.pan;
        assert!(!v.reveal(size, pos2(0.0, 0.0), pos2(5000.0, 50.0)));
        assert_eq!(v.pan, pan);
    }

    #[test]
    fn maps_both_ways() {
        let v = View {
            pan: vec2(100.0, -40.0),
            zoom: 2.0,
        };
        let origin = pos2(48.0, 32.0);
        let s = v.to_screen(origin, pos2(110.0, -30.0));
        assert_eq!(s, pos2(68.0, 52.0));
        assert_eq!(v.to_model(origin, s), pos2(110.0, -30.0));
    }

    #[test]
    fn zoom_keeps_the_anchor_still() {
        let mut v = View::default();
        let origin = pos2(10.0, 10.0);
        let anchor = pos2(310.0, 210.0);
        let before = v.to_model(origin, anchor);
        v.zoom_at(origin, anchor, 2.0);
        assert_eq!(v.to_model(origin, anchor), before);
        v.zoom_at(origin, anchor, 100.0);
        assert_eq!(v.zoom, MAX_ZOOM);
    }

    #[test]
    fn pans_with_the_content() {
        let mut v = View {
            pan: Vec2::ZERO,
            zoom: 2.0,
        };
        v.pan_by(vec2(20.0, -10.0));
        // The content moved right and up by those screen points.
        assert_eq!(v.to_screen(Pos2::ZERO, Pos2::ZERO), pos2(20.0, -10.0));
    }

    #[test]
    fn steps() {
        let mut v = View::default();
        assert_eq!(v.step(1), 1.25);
        assert_eq!(v.step(-1), 0.75);
        v.zoom = 1.1;
        assert_eq!((v.step(1), v.step(-1)), (1.25, 1.0));
        v.zoom = MAX_ZOOM;
        assert_eq!(v.step(1), MAX_ZOOM);
    }
}
