//! MediaLiner's own files.
//!
//! In `~/.config/MediaLiner` (the platform equivalent elsewhere):
//!
//! | File                  | Holds |
//! |-----------------------|-------|
//! | `settings.json`       | `folders` (the folders you added), `expanded` (open folders in the tree), `selectedFolder` |
//! | `ui-preferences.json` | `customTitlebar`, `statusBarVisible`, `thumbSize`, `includeSubfolders`, `showLocation`, `foldersVisible`, `filesVisible`, `infoVisible` |
//!
//! In `~/.local/share/medialiner`: `merges/` (the merge journals) and
//! eframe's `app.ron`. In `~/.cache/medialiner`: `thumbs/`.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

/// Names the config folder.
pub const PRODUCT_NAME: &str = "MediaLiner";
/// The thumbnail size range, in points.
pub const THUMB_MIN: f32 = 96.0;
pub const THUMB_MAX: f32 = 320.0;
pub const THUMB_DEFAULT: f32 = 160.0;

/// Where the app's files live. Tests point them all at a temp folder.
#[derive(Clone, Debug)]
pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
}

/// `settings.json`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    pub folders: Vec<PathBuf>,
    pub expanded: Vec<PathBuf>,
    pub selected_folder: Option<PathBuf>,
}

/// `ui-preferences.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    pub custom_titlebar: bool,
    pub status_bar_visible: bool,
    pub thumb_size: f32,
    pub include_subfolders: bool,
    pub show_location: bool,
    pub folders_visible: bool,
    pub files_visible: bool,
    pub info_visible: bool,
}

fn paths_of(v: Option<&Value>) -> Vec<PathBuf> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

impl Paths {
    pub fn default_location(data: Option<PathBuf>) -> Self {
        let config = laankit_core::paths::user_data_dir(PRODUCT_NAME);
        let data = data.unwrap_or_else(|| config.join("data"));
        Self {
            config,
            data,
            cache: laankit_core::paths::cache_dir("medialiner"),
        }
    }

    /// A temp layout for tests: everything under `root`.
    #[cfg(test)]
    pub fn under(root: &Path) -> Self {
        Self {
            config: root.join("MediaLiner"),
            data: root.join("medialiner"),
            cache: root.join("cache"),
        }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    pub fn ui_prefs(&self) -> PathBuf {
        self.config.join("ui-preferences.json")
    }

    pub fn merges_dir(&self) -> PathBuf {
        self.data.join("merges")
    }

    pub fn thumbs_dir(&self) -> PathBuf {
        self.cache.join("thumbs")
    }

    pub fn settings(&self) -> Settings {
        let m = laankit_core::json::read_object(&self.settings_file());
        Settings {
            folders: paths_of(m.get("folders")),
            expanded: paths_of(m.get("expanded")),
            selected_folder: m
                .get("selectedFolder")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
        }
    }

    pub fn save_settings(&self, s: &Settings) -> anyhow::Result<()> {
        let mut m = laankit_core::json::read_object(&self.settings_file());
        laankit_core::json::merge(
            &mut m,
            &json!({
                "folders": s.folders,
                "expanded": s.expanded,
                "selectedFolder": s.selected_folder,
            }),
        );
        laankit_core::json::write(&self.settings_file(), &m)
    }

    fn read_prefs_map(&self) -> Map<String, Value> {
        let mut merged = prefs_defaults();
        for (k, v) in laankit_core::json::read_object(&self.ui_prefs()) {
            merged.insert(k, v);
        }
        merged
    }

    pub fn prefs(&self) -> Prefs {
        let m = self.read_prefs_map();
        let flag = |k: &str| m.get(k).and_then(Value::as_bool).unwrap_or(false);
        Prefs {
            custom_titlebar: flag("customTitlebar"),
            status_bar_visible: flag("statusBarVisible"),
            thumb_size: m
                .get("thumbSize")
                .and_then(Value::as_f64)
                .map_or(THUMB_DEFAULT, |v| (v as f32).clamp(THUMB_MIN, THUMB_MAX)),
            include_subfolders: flag("includeSubfolders"),
            show_location: flag("showLocation"),
            folders_visible: flag("foldersVisible"),
            files_visible: flag("filesVisible"),
            info_visible: flag("infoVisible"),
        }
    }

    /// Merges `patch` into the prefs and writes them all, defaults included.
    pub fn set_prefs(&self, patch: &Value) -> anyhow::Result<()> {
        let mut prefs = self.read_prefs_map();
        laankit_core::json::merge(&mut prefs, patch);
        laankit_core::json::write(&self.ui_prefs(), &prefs)
    }
}

fn prefs_defaults() -> Map<String, Value> {
    let v = json!({
        "customTitlebar": false,
        "statusBarVisible": true,
        "thumbSize": THUMB_DEFAULT,
        "includeSubfolders": false,
        "showLocation": false,
        "foldersVisible": true,
        "filesVisible": true,
        "infoVisible": false,
    });
    v.as_object().cloned().unwrap_or_default()
}

/// Whether `path` is shown with its folder: two listed folders share a name.
pub fn shares_name(path: &Path, others: &[PathBuf]) -> bool {
    let name = path.file_name();
    others.iter().any(|o| o != path && o.file_name() == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::under(tmp.path());
        assert_eq!(paths.settings(), Settings::default());
        let p = paths.prefs();
        assert!(p.status_bar_visible && p.folders_visible && p.files_visible);
        assert!(!p.info_visible && !p.include_subfolders && !p.show_location);
        assert_eq!(p.thumb_size, THUMB_DEFAULT);

        laankit_core::json::write(&paths.settings_file(), &json!({ "future": 1 })).unwrap();
        let s = Settings {
            folders: vec!["/a".into(), "/b".into()],
            expanded: vec!["/a/x".into()],
            selected_folder: Some("/a/x".into()),
        };
        paths.save_settings(&s).unwrap();
        assert_eq!(paths.settings(), s);
        let raw = laankit_core::json::read_object(&paths.settings_file());
        assert_eq!(raw["future"], 1);

        paths
            .set_prefs(&json!({ "thumbSize": 9999, "infoVisible": true }))
            .unwrap();
        let p = paths.prefs();
        assert_eq!(p.thumb_size, THUMB_MAX);
        assert!(p.info_visible);
        assert!(shares_name(Path::new("/a/x"), &["/b/x".into()]));
        assert!(!shares_name(
            Path::new("/a/x"),
            &["/a/x".into(), "/b/y".into()]
        ));
    }
}
