//! Grid snapping. Everything snaps in model space, so the zoom never changes
//! where an item lands.

use crate::model::Point;

/// `v` to the nearest multiple of `grid`.
pub fn snap(v: f32, grid: f32) -> f32 {
    (v / grid).round() * grid
}

pub fn snap_point(p: Point, grid: f32) -> Point {
    Point::new(snap(p.x, grid), snap(p.y, grid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_to_the_nearest_line() {
        assert_eq!(snap(29.0, 20.0), 20.0);
        assert_eq!(snap(31.0, 20.0), 40.0);
        assert_eq!(snap(-31.0, 20.0), -40.0);
        assert_eq!(snap(7.0, 10.0), 10.0);
        assert_eq!(
            snap_point(Point::new(11.0, 49.0), 20.0),
            Point::new(20.0, 40.0)
        );
    }
}
