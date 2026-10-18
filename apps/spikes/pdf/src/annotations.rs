//! PageLiner's stored highlights (`state/<book id>.json`, written by `pageliner-core`):
//! `{ "page": 3, "format": "pdf", "highlights": [{ id, kind: "highlight", page, rects: [{x, y, w, h}], text, createdAt }] }`.
//! Rects are in page units, so a highlight made here shows up in the Svelte reader and back.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Highlight {
    pub id: String,
    pub kind: String,
    /// 1-based, as pdf.js numbers pages.
    pub page: usize,
    pub rects: Vec<HighlightRect>,
    pub text: String,
    pub created_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HighlightRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Highlight {
    /// A new highlight from a selection on 0-based page `page_index`. The Svelte reader keeps
    /// at most 120 characters of the text.
    pub fn new(page_index: usize, rects: &[[f32; 4]], text: &str) -> Self {
        let text: String = text.chars().take(120).collect();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "highlight".into(),
            page: page_index + 1,
            rects: rects
                .iter()
                .map(|r| HighlightRect {
                    x: r[0],
                    y: r[1],
                    w: r[2],
                    h: r[3],
                })
                .collect(),
            text: if text.is_empty() {
                "Highlight".into()
            } else {
                text
            },
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        }
    }
}

/// The highlights in a state object (missing or malformed entries are skipped).
pub fn highlights_from_state(state: &serde_json::Map<String, Value>) -> Vec<Highlight> {
    let Some(Value::Array(items)) = state.get("highlights") else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect()
}

/// The patch the Svelte reader sends through `setBookState`.
pub fn highlights_patch(highlights: &[Highlight]) -> Value {
    json!({ "highlights": highlights })
}

/// Reads the highlights from a `state/<id>.json` file.
pub fn load(path: &Path) -> Vec<Highlight> {
    highlights_from_state(&marina_core::json::read_object(path))
}

/// Merges `highlights` into a `state/<id>.json` file, keeping its other keys.
pub fn save(path: &Path, highlights: &[Highlight]) -> anyhow::Result<()> {
    let mut state = marina_core::json::read_object(path);
    marina_core::json::merge(&mut state, &highlights_patch(highlights));
    marina_core::json::write(path, &state)
}
