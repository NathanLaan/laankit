//! The app's own files in `~/.config/BlockLiner` (the platform equivalent
//! elsewhere):
//!
//! | Path                  | Holds                                                                    |
//! |-----------------------|--------------------------------------------------------------------------|
//! | `ui-preferences.json` | `customTitlebar`, `sidebarVisible`, `propertiesVisible`, `statusBarVisible`, `showGrid`, |
//! |                       | `gridSize`, `lastProject` (a folder path; empty after Close Folder), `lastDiagram` (a name in it), |
//! |                       | `exportFormat` (`png`/`svg`), `exportGrid`, `exportScale` (1, 2 or 3)    |
//!
//! Diagrams live in the folder the user opens, not here.

use std::path::PathBuf;

use blockliner_core::model::DEFAULT_GRID;

use crate::export::{self, Format};
use serde_json::{json, Map, Value};

/// Names the user-data folder.
pub const PRODUCT_NAME: &str = "BlockLiner";
/// The grid sizes Settings offers, in diagram pixels.
pub const GRID_SIZES: [f32; 3] = [10.0, 20.0, 40.0];

/// Where the app's own files live. Tests point it at a temp folder.
#[derive(Clone, Debug)]
pub struct Paths {
    pub user_data: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    pub custom_titlebar: bool,
    pub sidebar_visible: bool,
    /// The Properties pane, below the diagram list.
    pub properties_visible: bool,
    pub status_bar_visible: bool,
    pub show_grid: bool,
    /// The grid for new diagrams; one of [`GRID_SIZES`].
    pub grid_size: f32,
    /// The folder open when BlockLiner last closed.
    pub last_project: Option<PathBuf>,
    /// The diagram open in it.
    pub last_diagram: Option<String>,
    /// The Export dialog's last choices.
    pub export: export::Options,
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
        let text = |k: &str| {
            m.get(k)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        Prefs {
            custom_titlebar: flag("customTitlebar"),
            sidebar_visible: flag("sidebarVisible"),
            properties_visible: flag("propertiesVisible"),
            status_bar_visible: flag("statusBarVisible"),
            show_grid: flag("showGrid"),
            grid_size,
            last_project: text("lastProject").map(PathBuf::from),
            last_diagram: text("lastDiagram"),
            export: export::Options {
                format: text("exportFormat")
                    .and_then(|f| Format::parse(&f))
                    .unwrap_or_default(),
                grid: flag("exportGrid"),
                scale: m
                    .get("exportScale")
                    .and_then(Value::as_u64)
                    .map(|s| s as u32)
                    .filter(|s| export::SCALES.contains(s))
                    .unwrap_or(2),
            },
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
        "sidebarVisible": true,
        "propertiesVisible": true,
        "statusBarVisible": true,
        "showGrid": true,
        // Whole numbers, so the file says `20`, not `20.0`.
        "gridSize": DEFAULT_GRID as i64,
        "lastProject": null,
        "lastDiagram": null,
        "exportFormat": "png",
        "exportGrid": false,
        "exportScale": 2,
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
            user_data: dir.path().join("BlockLiner"),
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
                sidebar_visible: true,
                properties_visible: true,
                status_bar_visible: true,
                show_grid: true,
                grid_size: 20.0,
                last_project: None,
                last_diagram: None,
                export: export::Options {
                    format: Format::Png,
                    grid: false,
                    scale: 2,
                },
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
            "{\n  \"customTitlebar\": false,\n  \"sidebarVisible\": true,\n  \"propertiesVisible\": true,\n  \"statusBarVisible\": true,\n  \"showGrid\": false,\n  \"gridSize\": 40,\n  \"lastProject\": null,\n  \"lastDiagram\": null,\n  \"exportFormat\": \"png\",\n  \"exportGrid\": false,\n  \"exportScale\": 2,\n  \"future\": 1\n}"
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
