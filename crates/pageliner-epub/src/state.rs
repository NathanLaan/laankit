//! What the reader saves: the reading position, bookmarks and highlights per book, and the
//! reading preferences shared by all books.
//!
//! Per-book state lives in PageLiner's `state/<id>.json` under one key, [`STATE_KEY`]
//! (`"native"`). The Tauri/Electron build keeps its own keys there (`cfi`, `bookmarks`,
//! `highlights`, all epub.js CFIs); the native reader never reads or writes them, and
//! `pageliner_core::Library::set_state` merges top-level keys, so the two builds don't clobber
//! each other while both are in use. Positions are not converted from CFIs: books open at the
//! start the first time the native reader opens them.
//!
//! Reading preferences live in the app's UI preferences, under the keys the Svelte reader uses
//! (`epubFontSize`, `epubTheme`) plus `epubFace`, which only the native reader has.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The key the native reader's per-book state lives under.
pub const STATE_KEY: &str = "native";

/// A place in a book: the spine document, the top-level block inside it, and a character offset
/// in that block (see `laankit_richtext::Position`).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Location {
    pub spine: usize,
    pub block: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: String,
    pub location: Location,
    /// The chapter's table-of-contents label.
    pub label: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Highlight {
    pub id: String,
    pub start: Location,
    pub end: Location,
    /// The highlighted text, whitespace collapsed and cut to 120 characters (as the Svelte
    /// reader stores it).
    pub text: String,
    pub created_at: String,
}

/// One book's native reading state: `state/<id>.json`'s [`STATE_KEY`] object.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NativeState {
    pub position: Option<Location>,
    pub bookmarks: Vec<Bookmark>,
    pub highlights: Vec<Highlight>,
}

impl NativeState {
    /// Reads the native state from a book's whole state object (`Library::get_state`). Missing
    /// or unreadable state is empty.
    pub fn from_book_state(state: &Map<String, Value>) -> Self {
        state
            .get(STATE_KEY)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    /// The patch for `Library::set_state`: `{ "native": { … } }`, nothing else.
    pub fn patch(&self) -> Value {
        json!({ STATE_KEY: self })
    }
}

/// The reading theme: the page's colours, independent of the app theme (as in the Svelte
/// reader).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadingTheme {
    #[default]
    Light,
    Sepia,
    Dark,
}

impl ReadingTheme {
    pub const ALL: [ReadingTheme; 3] =
        [ReadingTheme::Light, ReadingTheme::Sepia, ReadingTheme::Dark];

    pub fn id(self) -> &'static str {
        match self {
            ReadingTheme::Light => "light",
            ReadingTheme::Sepia => "sepia",
            ReadingTheme::Dark => "dark",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ReadingTheme::Light => "Light",
            ReadingTheme::Sepia => "Sepia",
            ReadingTheme::Dark => "Dark",
        }
    }

    /// (text, background), from `EpubReader.svelte`'s `THEME_VARS`.
    pub fn colors(self) -> (egui::Color32, egui::Color32) {
        use egui::Color32;
        match self {
            ReadingTheme::Light => (Color32::from_rgb(0x1a, 0x1a, 0x1a), Color32::WHITE),
            ReadingTheme::Sepia => (
                Color32::from_rgb(0x5b, 0x46, 0x36),
                Color32::from_rgb(0xf4, 0xec, 0xd8),
            ),
            ReadingTheme::Dark => (
                Color32::from_rgb(0xcd, 0xd6, 0xf4),
                Color32::from_rgb(0x1e, 0x1e, 0x2e),
            ),
        }
    }
}

/// Font size, theme and face, shared by all books.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReaderPrefs {
    /// Percent of the default size: 60–220, in steps of 10 (`setFontSize`).
    pub font_size: u32,
    pub theme: ReadingTheme,
    pub face: laankit_richtext::Face,
}

impl Default for ReaderPrefs {
    fn default() -> Self {
        Self {
            font_size: 100,
            theme: ReadingTheme::Light,
            face: laankit_richtext::Face::Serif,
        }
    }
}

pub const FONT_SIZE_MIN: u32 = 60;
pub const FONT_SIZE_MAX: u32 = 220;
pub const FONT_SIZE_STEP: u32 = 10;

impl ReaderPrefs {
    /// Reads `epubFontSize`, `epubTheme` and `epubFace` from the UI preferences object.
    pub fn from_ui_prefs(prefs: &Map<String, Value>) -> Self {
        let mut p = Self::default();
        if let Some(n) = prefs.get("epubFontSize").and_then(Value::as_f64) {
            p.font_size = (n.round() as u32).clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
        }
        if let Some(t) = prefs
            .get("epubTheme")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            p.theme = t;
        }
        match prefs.get("epubFace").and_then(Value::as_str) {
            Some("sans") => p.face = laankit_richtext::Face::Sans,
            Some("serif") => p.face = laankit_richtext::Face::Serif,
            _ => {}
        }
        p
    }

    /// The patch for the UI preferences: the three keys above.
    pub fn patch(&self) -> Value {
        json!({
            "epubFontSize": self.font_size,
            "epubTheme": self.theme.id(),
            "epubFace": match self.face {
                laankit_richtext::Face::Sans => "sans",
                laankit_richtext::Face::Serif => "serif",
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips_under_its_own_key() {
        let state = NativeState {
            position: Some(Location {
                spine: 3,
                block: 12,
                offset: 40,
            }),
            bookmarks: vec![Bookmark {
                id: "b1".into(),
                location: Location::default(),
                label: "One".into(),
                created_at: "2026-10-17T00:00:00.000Z".into(),
            }],
            highlights: Vec::new(),
        };
        let patch = state.patch();
        assert_eq!(patch.as_object().unwrap().len(), 1);
        assert_eq!(patch["native"]["position"]["spine"], 3);
        assert_eq!(
            patch["native"]["bookmarks"][0]["createdAt"],
            "2026-10-17T00:00:00.000Z"
        );
        let mut whole = Map::new();
        whole.insert("cfi".into(), json!("epubcfi(/6/4!/4/2)"));
        whole.extend(patch.as_object().unwrap().clone());
        assert_eq!(NativeState::from_book_state(&whole), state);
        // The other build's state alone reads as empty.
        let mut other = Map::new();
        other.insert("highlights".into(), json!([{ "cfi": "x" }]));
        assert_eq!(NativeState::from_book_state(&other), NativeState::default());
    }

    #[test]
    fn prefs_use_the_svelte_keys() {
        let mut prefs = Map::new();
        prefs.insert("epubFontSize".into(), json!(130));
        prefs.insert("epubTheme".into(), json!("sepia"));
        prefs.insert("unrelated".into(), json!(true));
        let p = ReaderPrefs::from_ui_prefs(&prefs);
        assert_eq!(p.font_size, 130);
        assert_eq!(p.theme, ReadingTheme::Sepia);
        assert_eq!(p.face, laankit_richtext::Face::Serif);
        let patch = p.patch();
        assert_eq!(patch["epubTheme"], "sepia");
        assert_eq!(patch["epubFace"], "serif");
        prefs.insert("epubFontSize".into(), json!(999));
        assert_eq!(ReaderPrefs::from_ui_prefs(&prefs).font_size, FONT_SIZE_MAX);
    }
}
