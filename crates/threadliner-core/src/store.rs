//! Git-synced JSON data store. Port of `apps/threadliner/src/main/data-store.js`: same
//! layout, field names, and key order, so a data folder can be shared by
//! the Electron and Tauri builds.
//!
//!   configuration/settings.json
//!   data/feeds.json                 { feeds: [...] }
//!   data/tags.json                  { tags: [...] }
//!   data/feeds/<feedId>/entries.json { feedId, entries: [...] }

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};

use crate::feed::NewEntry;

type Obj = Map<String, Value>;

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn s(o: &Obj, k: &str) -> String {
    o.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn opt(v: Option<String>) -> Value {
    v.filter(|s| !s.is_empty())
        .map(Value::String)
        .unwrap_or(Value::Null)
}

pub struct Store {
    dir: PathBuf,
    feeds: Option<Vec<Obj>>,
    tags: Option<Vec<Obj>>,
}

impl Store {
    pub fn open(dir: &Path) -> Result<Self> {
        for d in ["configuration", "data", "data/feeds"] {
            fs::create_dir_all(dir.join(d))?;
        }
        let store = Self {
            dir: dir.to_path_buf(),
            feeds: None,
            tags: None,
        };
        if !store.feeds_path().exists() {
            laankit_core::json::write(&store.feeds_path(), &json!({ "feeds": [] }))?;
        }
        if !store.settings_path().exists() {
            laankit_core::json::write(&store.settings_path(), &json!({}))?;
        }
        if !store.tags_path().exists() {
            laankit_core::json::write(&store.tags_path(), &json!({ "tags": [] }))?;
        }
        Ok(store)
    }

    fn feeds_path(&self) -> PathBuf {
        self.dir.join("data/feeds.json")
    }
    fn tags_path(&self) -> PathBuf {
        self.dir.join("data/tags.json")
    }
    fn settings_path(&self) -> PathBuf {
        self.dir.join("configuration/settings.json")
    }
    fn feed_dir(&self, id: &str) -> PathBuf {
        self.dir.join("data/feeds").join(id)
    }
    fn entries_path(&self, id: &str) -> PathBuf {
        self.feed_dir(id).join("entries.json")
    }

    fn read_list(path: &Path, key: &str) -> Vec<Obj> {
        laankit_core::json::read::<Value>(path)
            .and_then(|v| v.get(key).and_then(Value::as_array).cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| match v {
                Value::Object(o) => Some(o),
                _ => None,
            })
            .collect()
    }

    // --- Feeds ---------------------------------------------------------------

    fn feeds(&mut self) -> &mut Vec<Obj> {
        if self.feeds.is_none() {
            self.feeds = Some(Self::read_list(&self.feeds_path(), "feeds"));
        }
        self.feeds.as_mut().unwrap()
    }

    fn save_feeds(&mut self) -> Result<()> {
        let feeds = self.feeds().clone();
        laankit_core::json::write(&self.feeds_path(), &json!({ "feeds": feeds }))
    }

    pub fn feed(&mut self, id: &str) -> Option<Obj> {
        self.feeds().iter().find(|f| s(f, "id") == id).cloned()
    }

    /// Renderer format (snake_case + unread_count), sorted by title.
    pub fn all_feeds(&mut self) -> Vec<Value> {
        let feeds = self.feeds().clone();
        let mut out: Vec<(String, Value)> = feeds
            .iter()
            .map(|f| {
                let id = s(f, "id");
                let unread = self.load_entries(&id).iter().filter(|e| !e.get("isRead").and_then(Value::as_bool).unwrap_or(false)).count();
                let v = json!({
                    "id": f.get("id"),
                    "title": f.get("title"),
                    "url": f.get("url"),
                    "site_url": f.get("siteUrl"),
                    "description": f.get("description"),
                    "created_at": f.get("createdAt"),
                    "updated_at": f.get("updatedAt"),
                    "unread_count": unread,
                    "tag_ids": f.get("tagIds").filter(|t| t.is_array()).cloned().unwrap_or(json!([])),
                });
                (s(f, "title").to_lowercase(), v)
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out.into_iter().map(|(_, v)| v).collect()
    }

    pub fn add_feed(
        &mut self,
        title: &str,
        url: &str,
        site_url: Option<String>,
        description: Option<String>,
    ) -> Result<Obj> {
        if self.feeds().iter().any(|f| s(f, "url") == url) {
            bail!("Feed with this URL already exists");
        }
        let ts = now();
        let feed = json!({
            "id": uuid(),
            "title": title,
            "url": url,
            "siteUrl": opt(site_url),
            "description": opt(description),
            "createdAt": ts,
            "updatedAt": ts,
        });
        let feed = feed.as_object().unwrap().clone();
        self.feeds().push(feed.clone());
        self.save_feeds()?;
        let id = s(&feed, "id");
        fs::create_dir_all(self.feed_dir(&id))?;
        laankit_core::json::write(
            &self.entries_path(&id),
            &json!({ "feedId": id, "entries": [] }),
        )?;
        Ok(feed)
    }

    pub fn edit_feed(&mut self, id: &str, data: &Value) -> Result<Obj> {
        let feed = self
            .feeds()
            .iter_mut()
            .find(|f| s(f, "id") == id)
            .ok_or_else(|| anyhow!("Feed not found"))?;
        for (from, to) in [
            ("title", "title"),
            ("url", "url"),
            ("siteUrl", "siteUrl"),
            ("description", "description"),
        ] {
            if let Some(v) = data.get(from) {
                feed.insert(to.into(), v.clone());
            }
        }
        feed.insert("updatedAt".into(), Value::String(now()));
        let out = feed.clone();
        self.save_feeds()?;
        Ok(out)
    }

    pub fn remove_feed(&mut self, id: &str) -> Result<()> {
        self.feeds().retain(|f| s(f, "id") != id);
        self.save_feeds()?;
        let dir = self.feed_dir(id);
        if dir.exists() {
            fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    // --- Entries -------------------------------------------------------------

    fn load_entries(&self, feed_id: &str) -> Vec<Obj> {
        Self::read_list(&self.entries_path(feed_id), "entries")
    }

    fn save_entries(&self, feed_id: &str, entries: &[Obj]) -> Result<()> {
        fs::create_dir_all(self.feed_dir(feed_id))?;
        laankit_core::json::write(
            &self.entries_path(feed_id),
            &json!({ "feedId": feed_id, "entries": entries }),
        )
    }

    /// Renderer format, newest first.
    pub fn entries(&self, feed_id: &str) -> Vec<Value> {
        let mut entries = self.load_entries(feed_id);
        let key = |e: &Obj| {
            [e.get("publishedAt"), e.get("createdAt")]
                .into_iter()
                .flatten()
                .find_map(|v| v.as_str().filter(|s| !s.is_empty()))
                .unwrap_or("")
                .to_string()
        };
        entries.sort_by_key(|e| std::cmp::Reverse(key(e)));
        entries
            .iter()
            .map(|e| {
                json!({
                    "id": e.get("id"),
                    "feed_id": feed_id,
                    "guid": e.get("guid"),
                    "title": e.get("title"),
                    "link": e.get("link"),
                    "content": e.get("content"),
                    "author": e.get("author"),
                    "published_at": e.get("publishedAt"),
                    "is_read": if e.get("isRead").and_then(Value::as_bool).unwrap_or(false) { 1 } else { 0 },
                    "created_at": e.get("createdAt"),
                })
            })
            .collect()
    }

    /// Append entries whose guid isn't stored yet. Returns how many.
    pub fn insert_entries(&self, feed_id: &str, new: &[NewEntry]) -> Result<usize> {
        let mut existing = self.load_entries(feed_id);
        let mut guids: std::collections::HashSet<String> = existing
            .iter()
            .filter_map(|e| e.get("guid").and_then(Value::as_str).map(str::to_string))
            .collect();
        let mut inserted = 0;
        for e in new {
            if guids.insert(e.guid.clone()) {
                let entry = json!({
                    "id": uuid(),
                    "guid": e.guid,
                    "title": opt(e.title.clone()),
                    "link": opt(e.link.clone()),
                    "content": opt(e.content.clone()),
                    "author": opt(e.author.clone()),
                    "publishedAt": opt(e.published_at.clone()),
                    "isRead": false,
                    "createdAt": now(),
                });
                existing.push(entry.as_object().unwrap().clone());
                inserted += 1;
            }
        }
        if inserted > 0 {
            self.save_entries(feed_id, &existing)?;
        }
        Ok(inserted)
    }

    pub fn set_read(&self, entry_id: &str, feed_id: &str, read: bool) -> Result<()> {
        let mut entries = self.load_entries(feed_id);
        if let Some(e) = entries.iter_mut().find(|e| s(e, "id") == entry_id) {
            e.insert("isRead".into(), Value::Bool(read));
            self.save_entries(feed_id, &entries)?;
        }
        Ok(())
    }

    pub fn set_all_read(&self, feed_id: &str, read: bool) -> Result<()> {
        let mut entries = self.load_entries(feed_id);
        let mut changed = false;
        for e in entries.iter_mut() {
            if e.get("isRead").and_then(Value::as_bool).unwrap_or(false) != read {
                e.insert("isRead".into(), Value::Bool(read));
                changed = true;
            }
        }
        if changed {
            self.save_entries(feed_id, &entries)?;
        }
        Ok(())
    }

    // --- Tags ----------------------------------------------------------------

    fn tags(&mut self) -> &mut Vec<Obj> {
        if self.tags.is_none() {
            self.tags = Some(Self::read_list(&self.tags_path(), "tags"));
        }
        self.tags.as_mut().unwrap()
    }

    fn save_tags(&mut self) -> Result<()> {
        let tags = self.tags().clone();
        laankit_core::json::write(&self.tags_path(), &json!({ "tags": tags }))
    }

    pub fn tag(&mut self, id: &str) -> Option<Obj> {
        self.tags().iter().find(|t| s(t, "id") == id).cloned()
    }

    pub fn all_tags(&mut self) -> Vec<Value> {
        self.tags()
            .iter()
            .map(|t| {
                json!({
                    "id": t.get("id"),
                    "name": t.get("name"),
                    "created_at": t.get("createdAt"),
                    "updated_at": t.get("updatedAt"),
                })
            })
            .collect()
    }

    fn check_tag_name(&mut self, name: &str, except: Option<&str>) -> Result<String> {
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            bail!("Tag name cannot be empty");
        }
        let lower = trimmed.to_lowercase();
        if self
            .tags()
            .iter()
            .any(|t| Some(s(t, "id").as_str()) != except && s(t, "name").to_lowercase() == lower)
        {
            bail!("A tag with this name already exists");
        }
        Ok(trimmed)
    }

    pub fn add_tag(&mut self, name: &str) -> Result<Obj> {
        let name = self.check_tag_name(name, None)?;
        let ts = now();
        let tag = json!({ "id": uuid(), "name": name, "createdAt": ts, "updatedAt": ts });
        let tag = tag.as_object().unwrap().clone();
        self.tags().push(tag.clone());
        self.save_tags()?;
        Ok(tag)
    }

    pub fn edit_tag(&mut self, id: &str, data: &Value) -> Result<Obj> {
        if !self.tags().iter().any(|t| s(t, "id") == id) {
            bail!("Tag not found");
        }
        let new_name = match data.get("name").and_then(Value::as_str) {
            Some(n) => Some(self.check_tag_name(n, Some(id))?),
            None => None,
        };
        let tag = self.tags().iter_mut().find(|t| s(t, "id") == id).unwrap();
        if let Some(n) = new_name {
            tag.insert("name".into(), Value::String(n));
        }
        tag.insert("updatedAt".into(), Value::String(now()));
        let out = tag.clone();
        self.save_tags()?;
        Ok(out)
    }

    pub fn remove_tag(&mut self, id: &str) -> Result<()> {
        self.tags().retain(|t| s(t, "id") != id);
        self.save_tags()?;
        let mut changed = false;
        for feed in self.feeds().iter_mut() {
            if let Some(Value::Array(ids)) = feed.get_mut("tagIds") {
                let before = ids.len();
                ids.retain(|t| t.as_str() != Some(id));
                changed |= ids.len() != before;
            }
        }
        if changed {
            self.save_feeds()?;
        }
        Ok(())
    }

    pub fn assign_tag(&mut self, feed_id: &str, tag_id: &str) -> Result<()> {
        if self.feed(feed_id).is_none() {
            bail!("Feed not found");
        }
        if self.tag(tag_id).is_none() {
            bail!("Tag not found");
        }
        let feed = self
            .feeds()
            .iter_mut()
            .find(|f| s(f, "id") == feed_id)
            .unwrap();
        if !feed.get("tagIds").is_some_and(Value::is_array) {
            feed.insert("tagIds".into(), json!([]));
        }
        let ids = feed.get_mut("tagIds").unwrap().as_array_mut().unwrap();
        if !ids.iter().any(|t| t.as_str() == Some(tag_id)) {
            ids.push(Value::String(tag_id.into()));
            self.save_feeds()?;
        }
        Ok(())
    }

    pub fn unassign_tag(&mut self, feed_id: &str, tag_id: &str) -> Result<()> {
        let feed = self
            .feeds()
            .iter_mut()
            .find(|f| s(f, "id") == feed_id)
            .ok_or_else(|| anyhow!("Feed not found"))?;
        if let Some(Value::Array(ids)) = feed.get_mut("tagIds") {
            if ids.iter().any(|t| t.as_str() == Some(tag_id)) {
                ids.retain(|t| t.as_str() != Some(tag_id));
                self.save_feeds()?;
            }
        }
        Ok(())
    }

    // --- Settings ------------------------------------------------------------

    pub fn get_setting(&self, key: &str) -> Value {
        laankit_core::json::read_object(&self.settings_path())
            .get(key)
            .cloned()
            .unwrap_or(Value::Null)
    }

    pub fn set_setting(&self, key: &str, value: Value) -> Result<()> {
        let mut settings = laankit_core::json::read_object(&self.settings_path());
        settings.insert(key.into(), value);
        laankit_core::json::write(&self.settings_path(), &settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(guid: &str) -> NewEntry {
        NewEntry {
            guid: guid.into(),
            title: Some("t".into()),
            link: None,
            content: None,
            author: None,
            published_at: None,
        }
    }

    #[test]
    fn feeds_entries_tags_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut st = Store::open(dir.path()).unwrap();
        let f = st.add_feed("Zeta", "https://z/feed", None, None).unwrap();
        let fid = s(&f, "id");
        st.add_feed("alpha", "https://a/feed", Some("https://a".into()), None)
            .unwrap();
        assert!(st.add_feed("dup", "https://z/feed", None, None).is_err());

        assert_eq!(
            st.insert_entries(&fid, &[entry("1"), entry("2")]).unwrap(),
            2
        );
        assert_eq!(
            st.insert_entries(&fid, &[entry("2"), entry("3")]).unwrap(),
            1
        );

        let feeds = st.all_feeds();
        assert_eq!(feeds[0]["title"], "alpha");
        assert_eq!(feeds[1]["unread_count"], 3);

        let entries = st.entries(&fid);
        let eid = entries[0]["id"].as_str().unwrap().to_string();
        st.set_read(&eid, &fid, true).unwrap();
        assert_eq!(st.all_feeds()[1]["unread_count"], 2);
        st.set_all_read(&fid, true).unwrap();
        assert_eq!(st.all_feeds()[1]["unread_count"], 0);

        let tag = st.add_tag("  News ").unwrap();
        assert_eq!(tag["name"], "News");
        assert!(st.add_tag("news").is_err());
        let tid = s(&tag, "id");
        st.assign_tag(&fid, &tid).unwrap();
        assert_eq!(st.all_feeds()[1]["tag_ids"], json!([tid]));
        st.remove_tag(&tid).unwrap();
        assert_eq!(st.all_feeds()[1]["tag_ids"], json!([]));

        st.set_setting("pollInterval", json!(5)).unwrap();
        assert_eq!(st.get_setting("pollInterval"), json!(5));
        assert_eq!(st.get_setting("missing"), Value::Null);

        st.remove_feed(&fid).unwrap();
        assert_eq!(st.all_feeds().len(), 1);
    }

    #[test]
    fn feed_file_key_order_matches_electron() {
        let dir = tempfile::tempdir().unwrap();
        let mut st = Store::open(dir.path()).unwrap();
        st.add_feed("T", "https://t", None, None).unwrap();
        let raw = fs::read_to_string(dir.path().join("data/feeds.json")).unwrap();
        let keys: Vec<&str> = raw
            .lines()
            .filter_map(|l| l.trim().strip_prefix('"'))
            .map(|l| l.split('"').next().unwrap())
            .collect();
        assert_eq!(
            keys,
            [
                "feeds",
                "id",
                "title",
                "url",
                "siteUrl",
                "description",
                "createdAt",
                "updatedAt"
            ]
        );
    }
}
