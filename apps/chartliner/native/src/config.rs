//! The app's own files in `~/.config/ChartLiner` (the platform equivalent
//! elsewhere):
//!
//! | Path                  | Holds                                                          |
//! |-----------------------|----------------------------------------------------------------|
//! | `ui-preferences.json` | `customTitlebar`, `statusBarVisible`, `showGrid`, `gridSize`   |
//!
//! Diagrams live in a folder the user picks, not here (plan phase 5).

use std::path::PathBuf;

use chartliner_core::model::DEFAULT_GRID;
use serde_json::{json, Map, Value};

/// Names the user-data folder.
pub const PRODUCT_NAME: &str = "ChartLiner";
/// The grid sizes Settings offers, in diagram pixels.
pub const GRID_SIZES: [f32; 3] = [10.0, 20.0, 40.0];

/// Where the app's own files live. Tests point it at a temp folder.
#[derive(Clone, Debug)]
pub struct Paths {
    pub user_data: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prefs {
    pub custom_titlebar: bool,
    pub status_bar_visible: bool,
    pub show_grid: bool,
    /// The grid for new diagrams; one of [`GRID_SIZES`].
    pub grid_size: f32,
}

impl Paths {
    pub fn default_location() -> Self {
        Self {
            user_data: marina_core::paths::user_data_dir(PRODUCT_NAME),
        }
    }

    pub fn ui_prefs(&self) -> PathBuf {
        self.user_data.join("ui-preferences.json")
    }

    /// The defaults merged with the file: defaults first, then whatever else
    /// the file has.
    pub fn read_prefs_map(&self) -> Map<String, Value> {
        let mut merged = defaults();
        for (k, v) in marina_core::json::read_object(&self.ui_prefs()) {
            merged.insert(k, v);
        }
        merged
    }

    pub fn prefs(&self) -> Prefs {
        let m = self.read_prefs_map();
        let flag = |k: &str| m.get(k).is_some_and(truthy);
        let grid_size = m
            .get("gridSize")
            .and_then(Value::as_f64)
            .map(|g| g as f32)
            .filter(|g| GRID_SIZES.contains(g))
            .unwrap_or(DEFAULT_GRID);
        Prefs {
            custom_titlebar: flag("customTitlebar"),
            status_bar_visible: flag("statusBarVisible"),
            show_grid: flag("showGrid"),
            grid_size,
        }
    }

    /// Merges `patch` into the merged prefs and writes them all, so the
    /// defaults end up in the file too.
    pub fn set_prefs(&self, patch: &Value) -> anyhow::Result<()> {
        let mut prefs = self.read_prefs_map();
        marina_core::json::merge(&mut prefs, patch);
        marina_core::json::write(&self.ui_prefs(), &prefs)
    }
}

fn defaults() -> Map<String, Value> {
    let v = json!({
        "customTitlebar": false,
        "statusBarVisible": true,
        "showGrid": true,
        // Whole numbers, so the file says `20`, not `20.0`.
        "gridSize": DEFAULT_GRID as i64,
    });
    v.as_object().cloned().unwrap_or_default()
}

/// JavaScript truthiness, as the other apps read their prefs.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            user_data: dir.path().join("ChartLiner"),
        };
        (dir, paths)
    }

    #[test]
    fn defaults_without_a_file() {
        let (_tmp, paths) = paths();
        assert_eq!(
            paths.prefs(),
            Prefs {
                custom_titlebar: false,
                status_bar_visible: true,
                show_grid: true,
                grid_size: 20.0,
            }
        );
    }

    #[test]
    fn set_prefs_merges_and_keeps_unknown_keys() {
        let (_tmp, paths) = paths();
        marina_core::json::write(
            &paths.ui_prefs(),
            &json!({ "future": 1, "showGrid": false }),
        )
        .unwrap();
        paths.set_prefs(&json!({ "gridSize": 40 })).unwrap();
        let raw = std::fs::read_to_string(paths.ui_prefs()).unwrap();
        assert_eq!(
            raw,
            "{\n  \"customTitlebar\": false,\n  \"statusBarVisible\": true,\n  \"showGrid\": false,\n  \"gridSize\": 40,\n  \"future\": 1\n}"
        );
        let prefs = paths.prefs();
        assert!(!prefs.show_grid);
        assert_eq!(prefs.grid_size, 40.0);
    }

    #[test]
    fn unknown_grid_sizes_fall_back() {
        let (_tmp, paths) = paths();
        paths.set_prefs(&json!({ "gridSize": 13 })).unwrap();
        assert_eq!(paths.prefs().grid_size, 20.0);
    }
}
