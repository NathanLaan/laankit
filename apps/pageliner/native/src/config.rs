//! The app's own files in `~/.config/PageLiner`, shared byte-for-byte with the
//! Electron and Tauri builds:
//!
//! | Path                      | Holds                                                         |
//! |---------------------------|---------------------------------------------------------------|
//! | `ui-preferences.json`     | `customTitlebar`, `sidebarVisible`, `statusBarVisible`, `gitSyncEnabled` |
//! | `library/`                | The library (`pageliner.json`, `books/`, `covers/`, `state/`), through `pageliner_core::library::Library` |

use std::path::PathBuf;

use serde_json::{json, Map, Value};

/// Electron's `productName`: names the user-data folder.
pub const PRODUCT_NAME: &str = "PageLiner";

/// Where the app's own files live. Tests point it at a temp folder.
#[derive(Clone, Debug)]
pub struct Paths {
    pub user_data: PathBuf,
}

/// The UI prefs PageLiner reads, with the Tauri host's defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub custom_titlebar: bool,
    pub sidebar_visible: bool,
    pub status_bar_visible: bool,
    pub git_sync_enabled: bool,
}

impl Paths {
    /// `~/.config/PageLiner` (`laankit_core::paths` mirrors Electron's `userData`).
    pub fn default_location() -> Self {
        Self {
            user_data: laankit_core::paths::user_data_dir(PRODUCT_NAME),
        }
    }

    /// The default library, as in the Electron and Tauri builds.
    pub fn library_dir(&self) -> PathBuf {
        self.user_data.join("library")
    }

    pub fn ui_prefs(&self) -> PathBuf {
        self.user_data.join("ui-preferences.json")
    }

    /// The defaults merged with the file, in the Tauri host's key order
    /// (`reload_prefs`): defaults first, then whatever else the file has.
    pub fn read_prefs_map(&self) -> Map<String, Value> {
        let mut merged = defaults();
        for (k, v) in laankit_core::json::read_object(&self.ui_prefs()) {
            merged.insert(k, v);
        }
        merged
    }

    pub fn prefs(&self) -> Prefs {
        let m = self.read_prefs_map();
        let flag = |k: &str| m.get(k).is_some_and(truthy);
        Prefs {
            custom_titlebar: flag("customTitlebar"),
            sidebar_visible: flag("sidebarVisible"),
            status_bar_visible: flag("statusBarVisible"),
            git_sync_enabled: flag("gitSyncEnabled"),
        }
    }

    /// `ui:setPrefs`: merges `patch` into the merged prefs and writes them all,
    /// as the Tauri host does (so defaults end up in the file too).
    pub fn set_prefs(&self, patch: &Value) -> anyhow::Result<()> {
        let mut prefs = self.read_prefs_map();
        laankit_core::json::merge(&mut prefs, patch);
        laankit_core::json::write(&self.ui_prefs(), &prefs)
    }
}

/// `prefs_defaults` in the Tauri host.
fn defaults() -> Map<String, Value> {
    let v = json!({
        "customTitlebar": false,
        "sidebarVisible": true,
        "statusBarVisible": true,
        "gitSyncEnabled": false,
    });
    v.as_object().cloned().unwrap_or_default()
}

/// JavaScript truthiness for the JSON values the prefs hold.
pub fn truthy(v: &Value) -> bool {
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
            user_data: dir.path().join("PageLiner"),
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
                status_bar_visible: true,
                git_sync_enabled: false,
            }
        );
    }

    #[test]
    fn set_prefs_writes_the_tauri_shape() {
        let (_tmp, paths) = paths();
        laankit_core::json::write(
            &paths.ui_prefs(),
            &json!({ "zoom": 2, "sidebarVisible": false }),
        )
        .unwrap();
        paths.set_prefs(&json!({ "gitSyncEnabled": true })).unwrap();
        let raw = std::fs::read_to_string(paths.ui_prefs()).unwrap();
        assert_eq!(
            raw,
            "{\n  \"customTitlebar\": false,\n  \"sidebarVisible\": false,\n  \"statusBarVisible\": true,\n  \"gitSyncEnabled\": true,\n  \"zoom\": 2\n}"
        );
        assert!(paths.prefs().git_sync_enabled);
        assert!(!paths.prefs().sidebar_visible);
    }
}
