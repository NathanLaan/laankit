//! Typed views of the rows `threadliner_core::store::Store` returns in the
//! renderer format (`all_feeds`, `entries`, `all_tags`), and the date
//! formats the Svelte components use.

use chrono::{DateTime, Local};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feed {
    pub id: String,
    pub title: String,
    pub url: String,
    pub site_url: Option<String>,
    pub unread_count: usize,
    pub tag_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub id: String,
    pub feed_id: String,
    pub title: Option<String>,
    pub link: Option<String>,
    pub content: Option<String>,
    pub author: Option<String>,
    pub published_at: Option<String>,
    pub is_read: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tag {
    pub id: String,
    pub name: String,
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn opt(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

impl Feed {
    pub fn from_row(v: &Value) -> Self {
        Self {
            id: text(v, "id"),
            title: text(v, "title"),
            url: text(v, "url"),
            site_url: opt(v, "site_url"),
            unread_count: v.get("unread_count").and_then(Value::as_u64).unwrap_or(0) as usize,
            tag_ids: v
                .get("tag_ids")
                .and_then(Value::as_array)
                .map(|ids| {
                    ids.iter()
                        .filter_map(|t| t.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

impl Entry {
    pub fn from_row(v: &Value) -> Self {
        Self {
            id: text(v, "id"),
            feed_id: text(v, "feed_id"),
            title: opt(v, "title"),
            link: opt(v, "link"),
            content: opt(v, "content"),
            author: opt(v, "author"),
            published_at: opt(v, "published_at"),
            // The renderer format stores 1/0, as SQLite did.
            is_read: v
                .get("is_read")
                .is_some_and(|r| r.as_u64() == Some(1) || r.as_bool() == Some(true)),
        }
    }

    /// `entry.title || '(Untitled)'`.
    pub fn display_title(&self) -> &str {
        self.title.as_deref().unwrap_or("(Untitled)")
    }
}

impl Tag {
    pub fn from_row(v: &Value) -> Self {
        Self {
            id: text(v, "id"),
            name: text(v, "name"),
        }
    }
}

fn local(date: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(date)
        .ok()
        .map(|d| d.with_timezone(&Local))
}

/// The entry list's date: `toLocaleDateString(undefined, { month: 'short',
/// day: 'numeric', year: 'numeric' })`, e.g. "Oct 17, 2026".
pub fn short_date(date: &str) -> String {
    local(date)
        .map(|d| d.format("%b %-d, %Y").to_string())
        .unwrap_or_default()
}

/// The article header's date, with the weekday and full month: "Saturday,
/// October 17, 2026".
pub fn long_date(date: &str) -> String {
    local(date)
        .map(|d| d.format("%A, %B %-d, %Y").to_string())
        .unwrap_or_default()
}

/// The sync log's time: `toLocaleTimeString([], { hour, minute, second })`.
pub fn log_time(timestamp_ms: i64) -> String {
    DateTime::from_timestamp_millis(timestamp_ms)
        .map(|d| d.with_timezone(&Local).format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

/// Settings → Sync → Last Sync: `new Date(iso).toLocaleString()`, or "Never".
pub fn last_sync(iso: Option<&str>) -> String {
    match iso {
        None => "Never".into(),
        Some(iso) => local(iso)
            .map(|d| d.format("%x, %X").to_string())
            .unwrap_or_else(|| "Unknown".into()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn rows_parse() {
        let feed = Feed::from_row(&json!({
            "id": "f1", "title": "Blog", "url": "https://b/feed", "site_url": null,
            "unread_count": 3, "tag_ids": ["t1"]
        }));
        assert_eq!(feed.unread_count, 3);
        assert_eq!(feed.tag_ids, ["t1"]);
        assert_eq!(feed.site_url, None);
        let entry = Entry::from_row(&json!({
            "id": "e1", "feed_id": "f1", "title": null, "is_read": 1, "content": ""
        }));
        assert!(entry.is_read);
        assert_eq!(entry.display_title(), "(Untitled)");
        assert_eq!(entry.content, None);
        assert!(!Entry::from_row(&json!({ "is_read": 0 })).is_read);
    }

    #[test]
    fn dates() {
        // Midday UTC is the same calendar day in every time zone but ±12+.
        assert_eq!(short_date("2026-10-17T12:00:00.000Z"), "Oct 17, 2026");
        assert_eq!(
            long_date("2026-10-17T12:00:00.000Z"),
            "Saturday, October 17, 2026"
        );
        assert_eq!(short_date("not a date"), "");
        assert_eq!(last_sync(None), "Never");
        assert_eq!(last_sync(Some("x")), "Unknown");
    }
}
