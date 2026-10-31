//! The PDF reader's bookmarks and highlights, in the shape `PdfReader.svelte`
//! keeps in `state/<book id>.json`:
//!
//! ```json
//! { "page": 3, "format": "pdf",
//!   "bookmarks":  [{ "id", "kind": "bookmark",  "page", "label", "createdAt" }],
//!   "highlights": [{ "id", "kind": "highlight", "page", "rects": [{ "x", "y", "w", "h" }], "text", "createdAt" }] }
//! ```
//!
//! Pages are 1-based; rects are in page units, so highlights made here show up
//! in the Svelte reader and back.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The Svelte reader keeps at most this many characters of a highlight's text.
const HIGHLIGHT_TEXT_CHARS: usize = 120;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: String,
    pub kind: String,
    /// 1-based.
    pub page: usize,
    pub label: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Highlight {
    pub id: String,
    pub kind: String,
    /// 1-based.
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

impl Bookmark {
    /// A bookmark on 1-based `page` (`toggleBookmark`).
    pub fn new(page: usize) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "bookmark".into(),
            page,
            label: format!("Page {page}"),
            created_at: now_iso(),
        }
    }
}

impl Highlight {
    /// A highlight from a selection on 1-based `page` (`addHighlight`).
    pub fn new(page: usize, rects: &[[f32; 4]], text: &str) -> Self {
        let text: String = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(HIGHLIGHT_TEXT_CHARS)
            .collect();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "highlight".into(),
            page,
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
            created_at: now_iso(),
        }
    }
}

/// What a book's state says about the PDF reader.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PdfState {
    /// 1-based, clamped by the reader.
    pub page: usize,
    pub bookmarks: Vec<Bookmark>,
    pub highlights: Vec<Highlight>,
}

impl PdfState {
    /// Reads `page`, `bookmarks` and `highlights`, skipping malformed entries.
    pub fn from_state(state: &Map<String, Value>) -> Self {
        Self {
            page: state
                .get("page")
                .and_then(Value::as_f64)
                .filter(|p| *p >= 1.0)
                .map_or(1, |p| p as usize),
            bookmarks: list(state, "bookmarks"),
            highlights: list(state, "highlights"),
        }
    }
}

fn list<T: for<'de> Deserialize<'de>>(state: &Map<String, Value>, key: &str) -> Vec<T> {
    let Some(Value::Array(items)) = state.get(key) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect()
}

/// `saveProgress`: `{ page, format: 'pdf' }`.
pub fn progress_patch(page: usize) -> Value {
    json!({ "page": page, "format": "pdf" })
}

/// `saveBookmarks`.
pub fn bookmarks_patch(bookmarks: &[Bookmark]) -> Value {
    json!({ "bookmarks": bookmarks })
}

/// `saveHighlights`.
pub fn highlights_patch(highlights: &[Highlight]) -> Value {
    json!({ "highlights": highlights })
}

/// `new Date().toISOString()`.
fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_state_written_by_the_svelte_reader() {
        let state = json!({
            "page": 4, "format": "pdf", "lastOpenedAt": "2026-09-30T10:00:00.000Z",
            "bookmarks": [{ "id": "b1", "kind": "bookmark", "page": 2, "label": "Page 2", "createdAt": "2026-09-30T10:00:00.000Z" }],
            "highlights": [
                { "id": "a1", "kind": "highlight", "page": 4, "rects": [{ "x": 56.1, "y": 101.25, "w": 210.4, "h": 13.2 }], "text": "quoted text", "createdAt": "2026-09-30T10:00:00.000Z" },
                { "broken": true }
            ],
        });
        let s = PdfState::from_state(state.as_object().unwrap());
        assert_eq!(s.page, 4);
        assert_eq!(s.bookmarks[0].label, "Page 2");
        assert_eq!(s.highlights.len(), 1);
        assert_eq!(s.highlights[0].rects[0].y, 101.25);
        assert_eq!(PdfState::from_state(&Map::new()).page, 1);
    }

    #[test]
    fn patches_have_the_svelte_shape() {
        let h = Highlight::new(3, &[[1.0, 2.0, 3.0, 4.0]], "  some   text ");
        let patch = highlights_patch(std::slice::from_ref(&h));
        let v = &patch["highlights"][0];
        assert_eq!(v["kind"], "highlight");
        assert_eq!(v["page"], 3);
        assert_eq!(v["text"], "some text");
        assert_eq!(
            v["rects"][0],
            json!({ "x": 1.0, "y": 2.0, "w": 3.0, "h": 4.0 })
        );
        assert!(v["createdAt"].as_str().unwrap().ends_with('Z'));
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["id", "kind", "page", "rects", "text", "createdAt"]);
        assert_eq!(progress_patch(7), json!({ "page": 7, "format": "pdf" }));
        let b = Bookmark::new(5);
        assert_eq!(bookmarks_patch(&[b])["bookmarks"][0]["label"], "Page 5");
        assert_eq!(Highlight::new(1, &[], "").text, "Highlight");
    }
}
