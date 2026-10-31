//! The app's own files in `~/.config/NoteLiner`, shared byte-for-byte with the
//! Electron and Tauri builds:
//!
//! | Path                   | Holds                                                                  |
//! |------------------------|------------------------------------------------------------------------|
//! | `ui-preferences.json`  | `customTitlebar`, `writeFrontmatter`, `mcp*`, `spellCheckEnabled`, `filesSortMode`, `commandRecents` |
//! | `recent-projects.json` | `[{ path, name, openedAt }]`, newest first, at most 5                  |
//! | `window-state.json`    | Per project folder: the panel `layout` and window `bounds`             |
//! | `mcp-runtime.json`     | The running MCP server's socket (written by `noteliner_core::mcp`)     |
//!
//! Projects themselves (`noteliner.json` and the notes) live in the folders
//! the user opens, through `noteliner_core::project::Project`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// Electron's `productName`: names the user-data folder.
pub const PRODUCT_NAME: &str = "NoteLiner";
/// `MAX_RECENT` in the Tauri host.
pub const MAX_RECENT: usize = 5;

/// Where the app's own files live. Tests point it at a temp folder.
#[derive(Clone, Debug)]
pub struct Paths {
    pub user_data: PathBuf,
}

/// The UI prefs NoteLiner reads, with the Tauri host's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    pub custom_titlebar: bool,
    pub write_frontmatter: bool,
    pub mcp_enabled: bool,
    pub mcp_confirm_writes: bool,
    pub mcp_disabled_tools: Vec<String>,
    pub spell_check: bool,
    pub files_sort_mode: String,
    pub command_recents: Vec<String>,
}

/// A recent project (`recent-projects.json`).
#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    pub path: String,
    pub name: String,
}

impl Paths {
    /// `~/.config/NoteLiner` (`marina_core::paths` mirrors Electron's `userData`).
    pub fn default_location() -> Self {
        Self {
            user_data: marina_core::paths::user_data_dir(PRODUCT_NAME),
        }
    }

    pub fn ui_prefs(&self) -> PathBuf {
        self.user_data.join("ui-preferences.json")
    }

    pub fn recent_path(&self) -> PathBuf {
        self.user_data.join("recent-projects.json")
    }

    pub fn window_state(&self) -> PathBuf {
        self.user_data.join("window-state.json")
    }

    pub fn mcp_runtime(&self) -> PathBuf {
        self.user_data.join("mcp-runtime.json")
    }

    /// The defaults merged with the file, in the Tauri host's key order
    /// (`reload_prefs`): defaults first, then whatever else the file has.
    pub fn read_prefs_map(&self) -> Map<String, Value> {
        let mut merged = defaults();
        for (k, v) in marina_core::json::read_object(&self.ui_prefs()) {
            merged.insert(k, v);
        }
        merged
    }

    pub fn prefs(&self) -> Prefs {
        let m = self.read_prefs_map();
        let flag = |k: &str, default: bool| m.get(k).and_then(Value::as_bool).unwrap_or(default);
        let strings = |k: &str| -> Vec<String> {
            m.get(k)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        Prefs {
            custom_titlebar: flag("customTitlebar", false),
            write_frontmatter: flag("writeFrontmatter", true),
            mcp_enabled: flag("mcpEnabled", false),
            mcp_confirm_writes: flag("mcpConfirmWrites", false),
            mcp_disabled_tools: strings("mcpDisabledTools"),
            // `prefs?.spellCheckEnabled !== false`.
            spell_check: m.get("spellCheckEnabled") != Some(&Value::Bool(false)),
            files_sort_mode: m
                .get("filesSortMode")
                .and_then(Value::as_str)
                .unwrap_or("user")
                .to_string(),
            command_recents: strings("commandRecents"),
        }
    }

    /// `ui:setPrefs`: merges `patch` into the merged prefs and writes them all,
    /// as the Tauri host does (so defaults end up in the file too).
    pub fn set_prefs(&self, patch: &Value) -> anyhow::Result<()> {
        let mut prefs = self.read_prefs_map();
        marina_core::json::merge(&mut prefs, patch);
        marina_core::json::write(&self.ui_prefs(), &prefs)
    }

    // --- Recent projects (projects:getRecent / addRecent / removeRecent) ---

    fn recent_raw(&self) -> Vec<Value> {
        marina_core::json::read::<Vec<Value>>(&self.recent_path()).unwrap_or_default()
    }

    pub fn recent(&self) -> Vec<Recent> {
        self.recent_raw()
            .iter()
            .filter_map(|v| {
                Some(Recent {
                    path: v["path"].as_str()?.to_string(),
                    name: v["name"].as_str().unwrap_or_default().to_string(),
                })
            })
            .collect()
    }

    /// `projects:addRecent`: moves `folder` to the front, keeping five.
    pub fn add_recent(&self, folder: &str) -> anyhow::Result<()> {
        let mut list: Vec<Value> = self
            .recent_raw()
            .into_iter()
            .filter(|p| p["path"] != folder)
            .collect();
        let name = Path::new(folder)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        list.insert(
            0,
            json!({ "path": folder, "name": name, "openedAt": noteliner_core::project::now_iso() }),
        );
        list.truncate(MAX_RECENT);
        marina_core::json::write(&self.recent_path(), &list)
    }

    /// `projects:removeRecent`.
    pub fn remove_recent(&self, folder: &str) -> anyhow::Result<()> {
        let list: Vec<Value> = self
            .recent_raw()
            .into_iter()
            .filter(|p| p["path"] != folder)
            .collect();
        marina_core::json::write(&self.recent_path(), &list)
    }
}

/// `prefs_defaults` in the Tauri host.
fn defaults() -> Map<String, Value> {
    let v = json!({
        "customTitlebar": false,
        "writeFrontmatter": true,
        "mcpEnabled": false,
        "mcpConfirmWrites": false,
        "mcpDisabledTools": [],
        "spellCheckEnabled": true,
        "filesSortMode": "user",
    });
    v.as_object().cloned().unwrap_or_default()
}

/// The sidebar panes (`VALID_PANE_KEYS`), in their default order.
pub const PANE_KEYS: [&str; 6] = ["files", "outline", "slides", "tags", "search", "backlinks"];

/// `DEFAULT_LAYOUT` in `App.svelte`: which panels show and their sizes,
/// saved per project folder in `window-state.json` (`window-state:saveLayout`).
/// Keys the native build doesn't know are kept, so the Svelte build's
/// additions survive a round trip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Layout {
    pub show_slides: bool,
    #[serde(serialize_with = "js_number")]
    pub slides_height: f32,
    pub show_preview: bool,
    pub show_history: bool,
    pub show_log: bool,
    pub show_status_bar: bool,
    pub show_toolbar: bool,
    pub show_sidebar: bool,
    pub show_outline: bool,
    pub show_tags: bool,
    pub show_attachments: bool,
    pub show_search: bool,
    pub show_backlinks: bool,
    #[serde(serialize_with = "js_number")]
    pub sidebar_width: f32,
    #[serde(serialize_with = "js_number")]
    pub log_panel_height: f32,
    #[serde(serialize_with = "js_number")]
    pub attachment_panel_width: f32,
    #[serde(serialize_with = "js_number")]
    pub preview_width: f32,
    #[serde(serialize_with = "js_number")]
    pub files_height: f32,
    #[serde(serialize_with = "js_number")]
    pub outline_height: f32,
    #[serde(serialize_with = "js_number")]
    pub tags_height: f32,
    #[serde(serialize_with = "js_number")]
    pub search_height: f32,
    #[serde(serialize_with = "js_number")]
    pub backlinks_height: f32,
    pub pane_order: Vec<String>,
    /// The note that was open and where its caret and scroll were (native
    /// only): reopened there with the project. Only one note is remembered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_file_position: Option<LastFilePosition>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `lastFilePosition` in the layout: the open note's id, its caret (anchor
/// and head, in characters) and the editor's scroll offset.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LastFilePosition {
    pub file_id: String,
    pub anchor: usize,
    pub cursor: usize,
    #[serde(serialize_with = "js_number")]
    pub scroll_x: f32,
    #[serde(serialize_with = "js_number")]
    pub scroll: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            show_slides: false,
            slides_height: 220.0,
            show_preview: false,
            show_history: false,
            show_log: false,
            show_status_bar: true,
            show_toolbar: true,
            show_sidebar: true,
            show_outline: false,
            show_tags: true,
            show_attachments: false,
            show_search: false,
            show_backlinks: false,
            sidebar_width: 260.0,
            log_panel_height: 300.0,
            attachment_panel_width: 220.0,
            preview_width: 500.0,
            files_height: 200.0,
            outline_height: 150.0,
            tags_height: 100.0,
            search_height: 200.0,
            backlinks_height: 180.0,
            pane_order: PANE_KEYS.iter().map(|k| k.to_string()).collect(),
            last_file_position: None,
            extra: Map::new(),
        }
    }
}

impl Layout {
    /// `{ ...DEFAULT_LAYOUT, ...saved }` with the pane order normalized.
    pub fn from_saved(saved: &Value) -> Self {
        let mut layout: Layout = if saved.is_object() {
            serde_json::from_value(saved.clone()).unwrap_or_default()
        } else {
            Layout::default()
        };
        layout.pane_order = normalize_pane_order(&layout.pane_order);
        layout
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// The stored height of a sidebar pane (`filesHeight`, …).
    pub fn pane_height(&self, pane: &str) -> f32 {
        match pane {
            "files" => self.files_height,
            "outline" => self.outline_height,
            "slides" => self.slides_height,
            "tags" => self.tags_height,
            "search" => self.search_height,
            "backlinks" => self.backlinks_height,
            _ => 200.0,
        }
    }

    pub fn set_pane_height(&mut self, pane: &str, h: f32) {
        let h = h.round();
        match pane {
            "files" => self.files_height = h,
            "outline" => self.outline_height = h,
            "slides" => self.slides_height = h,
            "tags" => self.tags_height = h,
            "search" => self.search_height = h,
            "backlinks" => self.backlinks_height = h,
            _ => {}
        }
    }

    /// Whether the sidebar pane is switched on.
    pub fn pane_visible(&self, pane: &str) -> bool {
        match pane {
            "files" => self.show_sidebar,
            "outline" => self.show_outline,
            "slides" => self.show_slides,
            "tags" => self.show_tags,
            "search" => self.show_search,
            "backlinks" => self.show_backlinks,
            _ => false,
        }
    }

    /// `handleClosePane`.
    pub fn close_pane(&mut self, pane: &str) {
        match pane {
            "files" => self.show_sidebar = false,
            "outline" => self.show_outline = false,
            "slides" => self.show_slides = false,
            "tags" => self.show_tags = false,
            "search" => self.show_search = false,
            "backlinks" => self.show_backlinks = false,
            _ => {}
        }
    }
}

/// Whole numbers as JSON integers (`260`, not `260.0`), as JavaScript writes them.
fn js_number<S: serde::Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        s.serialize_i64(*v as i64)
    } else {
        s.serialize_f64(f64::from(*v))
    }
}

/// `normalizePaneOrder`: known keys in their saved order, then the missing ones.
pub fn normalize_pane_order(order: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for k in order {
        if PANE_KEYS.contains(&k.as_str()) && !out.contains(k) {
            out.push(k.clone());
        }
    }
    for k in PANE_KEYS {
        if !out.iter().any(|o| o == k) {
            out.push(k.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            user_data: dir.path().join("NoteLiner"),
        };
        (dir, paths)
    }

    #[test]
    fn defaults_without_a_file() {
        let (_tmp, paths) = paths();
        let p = paths.prefs();
        assert!(!p.custom_titlebar && p.write_frontmatter && p.spell_check && !p.mcp_enabled);
        assert_eq!(p.files_sort_mode, "user");
    }

    #[test]
    fn set_prefs_writes_the_tauri_shape() {
        let (_tmp, paths) = paths();
        marina_core::json::write(
            &paths.ui_prefs(),
            &json!({ "zoom": 2, "spellCheckEnabled": false }),
        )
        .unwrap();
        paths.set_prefs(&json!({ "mcpEnabled": true })).unwrap();
        let raw = std::fs::read_to_string(paths.ui_prefs()).unwrap();
        assert_eq!(
            raw,
            "{\n  \"customTitlebar\": false,\n  \"writeFrontmatter\": true,\n  \"mcpEnabled\": true,\n  \"mcpConfirmWrites\": false,\n  \"mcpDisabledTools\": [],\n  \"spellCheckEnabled\": false,\n  \"filesSortMode\": \"user\",\n  \"zoom\": 2\n}"
        );
        assert!(paths.prefs().mcp_enabled);
        assert!(!paths.prefs().spell_check);
    }

    #[test]
    fn recent_projects_round_trip() {
        let (_tmp, paths) = paths();
        for i in 0..7 {
            paths.add_recent(&format!("/tmp/p{i}")).unwrap();
        }
        paths.add_recent("/tmp/p3").unwrap();
        let list = paths.recent();
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0].path, "/tmp/p3");
        assert_eq!(list[0].name, "p3");
        paths.remove_recent("/tmp/p3").unwrap();
        assert!(paths.recent().iter().all(|r| r.path != "/tmp/p3"));
        // The Tauri host's entry shape.
        let raw: Vec<Value> = marina_core::json::read(&paths.recent_path()).unwrap();
        let keys: Vec<&String> = raw[0].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["path", "name", "openedAt"]);
    }

    #[test]
    fn layout_matches_default_layout_and_keeps_unknown_keys() {
        let v = Layout::default().to_value();
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys[0], "showSlides");
        assert_eq!(keys.last().unwrap().as_str(), "paneOrder");
        assert_eq!(v["sidebarWidth"], json!(260));

        let saved =
            json!({ "showPreview": true, "paneOrder": ["tags", "bogus", "files"], "future": 1 });
        let l = Layout::from_saved(&saved);
        assert!(l.show_preview && l.show_tags);
        assert_eq!(
            l.pane_order,
            ["tags", "files", "outline", "slides", "search", "backlinks"]
        );
        assert_eq!(l.to_value()["future"], json!(1));
    }
}
