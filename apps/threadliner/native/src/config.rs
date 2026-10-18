//! The app's own files in `~/.config/ThreadLiner`, outside the synced data
//! folder, shared byte-for-byte with the Electron and Tauri builds:
//!
//! | File                      | Holds                                                     |
//! |---------------------------|-----------------------------------------------------------|
//! | `config.json`             | `dataDir` and `remoteUrl` (`setup:init`, `git:setRemoteUrl`) |
//! | `ui-preferences.json`     | `customTitlebar` (desktop-ui's `registerUIPrefsHandlers`) |
//!
//! Everything else (feeds, entries, tags, settings) lives in the data folder
//! and goes through `threadliner_core::store::Store`.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

/// Electron's `productName`: names the user-data folder.
pub const PRODUCT_NAME: &str = "ThreadLiner";

/// Where the app's own files live. Tests point it at a temp folder.
#[derive(Clone, Debug)]
pub struct Paths {
    pub user_data: PathBuf,
}

impl Paths {
    /// `~/.config/ThreadLiner` (`$XDG_CONFIG_HOME` respected).
    pub fn default_location() -> Self {
        Self {
            user_data: marina_core::paths::user_data_dir(PRODUCT_NAME),
        }
    }

    pub fn config(&self) -> PathBuf {
        self.user_data.join("config.json")
    }

    pub fn ui_prefs(&self) -> PathBuf {
        self.user_data.join("ui-preferences.json")
    }

    pub fn read_config(&self) -> Map<String, Value> {
        marina_core::json::read_object(&self.config())
    }

    pub fn write_config(&self, config: &Map<String, Value>) -> anyhow::Result<()> {
        marina_core::json::write(&self.config(), config)
    }

    /// The configured data folder, if any.
    pub fn configured_dir(&self) -> Option<PathBuf> {
        self.read_config()
            .get("dataDir")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    }

    /// `setup:isComplete`: a data folder is configured and exists.
    pub fn setup_complete(&self) -> bool {
        self.configured_dir().is_some_and(|d| d.exists())
    }

    /// `setup:init` writes a fresh config with the folder and remote.
    pub fn write_setup(&self, dir: &Path, remote: Option<&str>) -> anyhow::Result<()> {
        let mut config = Map::new();
        config.insert("dataDir".into(), json!(dir));
        config.insert("remoteUrl".into(), json!(remote));
        self.write_config(&config)
    }

    /// Updates `remoteUrl` (`git:setRemoteUrl` / `git:removeRemote`).
    pub fn set_remote_url(&self, url: Option<&str>) -> anyhow::Result<()> {
        let mut config = self.read_config();
        config.insert("remoteUrl".into(), json!(url));
        self.write_config(&config)
    }

    /// `customTitlebar` from the UI prefs (default off, as in the Tauri host).
    pub fn custom_titlebar(&self) -> bool {
        marina_core::json::read_object(&self.ui_prefs())
            .get("customTitlebar")
            .is_some_and(truthy)
    }

    /// Merges `customTitlebar` into the UI prefs, keeping other keys.
    pub fn set_custom_titlebar(&self, on: bool) -> anyhow::Result<()> {
        let mut prefs = marina_core::json::read_object(&self.ui_prefs());
        prefs.insert("customTitlebar".into(), json!(on));
        marina_core::json::write(&self.ui_prefs(), &prefs)
    }
}

/// `parseInt(value, 10)` for numbers and numeric strings.
pub fn parse_int(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_f64().map(|f| f as u64),
        Value::String(s) => {
            let digits: String = s
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            digits.parse().ok()
        }
        _ => None,
    }
}

/// JavaScript truthiness for the JSON values the settings hold.
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
            user_data: dir.path().join("ThreadLiner"),
        };
        (dir, paths)
    }

    #[test]
    fn setup_writes_the_tauri_shape() {
        let (tmp, paths) = paths();
        assert!(!paths.setup_complete());
        let data = tmp.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        paths.write_setup(&data, None).unwrap();
        let raw = std::fs::read_to_string(paths.config()).unwrap();
        // JSON.stringify(config, null, 2) with the keys in insertion order.
        assert_eq!(
            raw,
            format!(
                "{{\n  \"dataDir\": {},\n  \"remoteUrl\": null\n}}",
                serde_json::to_string(&data).unwrap()
            )
        );
        assert!(paths.setup_complete());
        paths
            .set_remote_url(Some("git@example.com:me/feeds.git"))
            .unwrap();
        assert_eq!(
            paths.read_config().get("remoteUrl"),
            Some(&json!("git@example.com:me/feeds.git"))
        );
        assert_eq!(paths.configured_dir(), Some(data));
    }

    #[test]
    fn ui_prefs_keep_other_keys() {
        let (_tmp, paths) = paths();
        assert!(!paths.custom_titlebar());
        marina_core::json::write(&paths.ui_prefs(), &json!({ "other": 1 })).unwrap();
        paths.set_custom_titlebar(true).unwrap();
        assert!(paths.custom_titlebar());
        let prefs = marina_core::json::read_object(&paths.ui_prefs());
        assert_eq!(prefs.get("other"), Some(&json!(1)));
    }

    #[test]
    fn parse_int_matches_js() {
        assert_eq!(parse_int(&json!("10")), Some(10));
        assert_eq!(parse_int(&json!(" 30s")), Some(30));
        assert_eq!(parse_int(&json!(5)), Some(5));
        assert_eq!(parse_int(&json!("x")), None);
        assert_eq!(parse_int(&Value::Null), None);
        assert!(!truthy(&json!("")));
        assert!(truthy(&json!("0")));
        assert!(!truthy(&json!(0)));
    }
}
