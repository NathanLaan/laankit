//! PrintLiner's own files.
//!
//! In `~/.config/PrintLiner` (the platform equivalent elsewhere):
//!
//! | File                  | Holds |
//! |-----------------------|-------|
//! | `settings.json`       | `region`, `email`, `tokenStorage` (`keychain` or `file`), `printerOrder` (serials), `printerNames` (serial to display name) |
//! | `ui-preferences.json` | `customTitlebar`, `statusBarVisible`, `trendRange` (`24h` or `7d`) |
//! | `token.json`          | The access token, only when `tokenStorage` is `file` |
//!
//! In `~/.local/share/printliner`: `history/` (the trend samples), `covers/`
//! (cached task images), and eframe's `app.ron`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use printliner_core::Region;
use serde_json::{json, Map, Value};

/// Names the config folder.
pub const PRODUCT_NAME: &str = "PrintLiner";

/// Where the app's files live. Tests point both at a temp folder.
#[derive(Clone, Debug)]
pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
}

/// Where the access token is kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TokenStorage {
    #[default]
    Keychain,
    File,
}

/// `settings.json`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    pub region: Region,
    pub email: Option<String>,
    pub token_storage: TokenStorage,
    pub printer_order: Vec<String>,
    pub printer_names: BTreeMap<String, String>,
}

/// The trends chart's span.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrendRange {
    #[default]
    Day,
    Week,
}

impl TrendRange {
    pub fn id(self) -> &'static str {
        match self {
            TrendRange::Day => "24h",
            TrendRange::Week => "7d",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TrendRange::Day => "24 Hours",
            TrendRange::Week => "7 Days",
        }
    }

    pub fn duration(self) -> chrono::Duration {
        match self {
            TrendRange::Day => chrono::Duration::hours(24),
            TrendRange::Week => chrono::Duration::days(7),
        }
    }
}

/// `ui-preferences.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    pub custom_titlebar: bool,
    pub status_bar_visible: bool,
    pub trend_range: TrendRange,
}

impl Paths {
    pub fn default_location(data: Option<PathBuf>) -> Self {
        let config = laankit_core::paths::user_data_dir(PRODUCT_NAME);
        let data = data.unwrap_or_else(|| config.join("data"));
        Self { config, data }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    pub fn ui_prefs(&self) -> PathBuf {
        self.config.join("ui-preferences.json")
    }

    pub fn token_file(&self) -> PathBuf {
        self.config.join("token.json")
    }

    pub fn history_dir(&self) -> PathBuf {
        self.data.join("history")
    }

    pub fn covers_dir(&self) -> PathBuf {
        self.data.join("covers")
    }

    pub fn settings(&self) -> Settings {
        let m = laankit_core::json::read_object(&self.settings_file());
        let text = |k: &str| {
            m.get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        Settings {
            region: text("region")
                .and_then(|r| Region::parse(&r))
                .unwrap_or_default(),
            email: text("email"),
            token_storage: match text("tokenStorage").as_deref() {
                Some("file") => TokenStorage::File,
                _ => TokenStorage::Keychain,
            },
            printer_order: m
                .get("printerOrder")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            printer_names: m
                .get("printerNames")
                .and_then(Value::as_object)
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, v)| {
                            let name = v.as_str()?.trim();
                            (!name.is_empty()).then(|| (k.clone(), name.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    pub fn save_settings(&self, s: &Settings) -> anyhow::Result<()> {
        let mut m = laankit_core::json::read_object(&self.settings_file());
        let patch = json!({
            "region": s.region.id(),
            "email": s.email,
            "tokenStorage": match s.token_storage {
                TokenStorage::Keychain => "keychain",
                TokenStorage::File => "file",
            },
            "printerOrder": s.printer_order,
            "printerNames": s.printer_names,
        });
        if let Value::Object(p) = patch {
            for (k, v) in p {
                m.insert(k, v);
            }
        }
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
            trend_range: match m.get("trendRange").and_then(Value::as_str) {
                Some("7d") => TrendRange::Week,
                _ => TrendRange::Day,
            },
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
        "trendRange": "24h",
    });
    v.as_object().cloned().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config: dir.path().join("PrintLiner"),
            data: dir.path().join("printliner"),
        };
        (dir, paths)
    }

    #[test]
    fn defaults_without_files() {
        let (_tmp, paths) = paths();
        assert_eq!(paths.settings(), Settings::default());
        assert_eq!(
            paths.prefs(),
            Prefs {
                custom_titlebar: false,
                status_bar_visible: true,
                trend_range: TrendRange::Day,
            }
        );
    }

    #[test]
    fn settings_round_trip_and_keep_unknown_keys() {
        let (_tmp, paths) = paths();
        laankit_core::json::write(&paths.settings_file(), &json!({ "future": 1 })).unwrap();
        let s = Settings {
            region: Region::Europe,
            email: Some("a@b.c".into()),
            token_storage: TokenStorage::File,
            printer_order: vec!["B".into(), "A".into()],
            printer_names: [("A".to_string(), "Left".to_string())].into(),
        };
        paths.save_settings(&s).unwrap();
        assert_eq!(paths.settings(), s);
        let raw: Value =
            serde_json::from_str(&std::fs::read_to_string(paths.settings_file()).unwrap()).unwrap();
        assert_eq!(raw["future"], 1);
        assert_eq!(raw["tokenStorage"], "file");
    }

    #[test]
    fn prefs_merge() {
        let (_tmp, paths) = paths();
        paths.set_prefs(&json!({ "trendRange": "7d" })).unwrap();
        let p = paths.prefs();
        assert_eq!(p.trend_range, TrendRange::Week);
        assert!(p.status_bar_visible);
    }
}
