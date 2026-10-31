//! The on-disk library: `pageliner.json` plus `books/`, `covers/`, `state/`.
//! Port of `apps/pageliner/src/main/library-service.js`; files stay byte-compatible.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::metadata;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    pub id: String,
    pub title: String,
    pub author: Option<String>,
    pub format: String,
    pub file: String,
    pub cover: Option<String>,
    #[serde(default)]
    pub tags: Vec<Value>,
    pub added_at: String,
    /// Fields written by other versions are carried through untouched.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub books: Vec<Book>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn one() -> u32 {
    1
}

impl Default for Index {
    fn default() -> Self {
        Self {
            version: 1,
            books: Vec::new(),
            extra: Map::new(),
        }
    }
}

pub struct Library {
    pub dir: PathBuf,
    index: Index,
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl Library {
    pub fn open(dir: PathBuf) -> Result<Self> {
        for sub in ["books", "covers", "state"] {
            fs::create_dir_all(dir.join(sub))?;
        }
        let path = dir.join("pageliner.json");
        let mut lib = Self {
            index: Index::default(),
            dir,
        };
        if path.exists() {
            lib.index = laankit_core::json::read(&path).unwrap_or_default();
        } else {
            lib.save()?;
        }
        Ok(lib)
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("pageliner.json")
    }
    fn books_dir(&self) -> PathBuf {
        self.dir.join("books")
    }
    fn covers_dir(&self) -> PathBuf {
        self.dir.join("covers")
    }
    fn state_path(&self, id: &str) -> PathBuf {
        self.dir.join("state").join(format!("{id}.json"))
    }

    pub fn save(&self) -> Result<()> {
        laankit_core::json::write(&self.index_path(), &self.index)
    }

    pub fn books(&self) -> &[Book] {
        &self.index.books
    }

    pub fn get(&self, id: &str) -> Option<&Book> {
        self.index.books.iter().find(|b| b.id == id)
    }

    /// Copy a document in, extract metadata + cover, and index it.
    pub fn add(&mut self, source: &Path) -> Result<Book> {
        if !source.is_file() {
            bail!("Source file not found");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let format = metadata::format_from_ext(source);
        let ext = match source.extension().and_then(|e| e.to_str()) {
            Some(e) => format!(".{}", e.to_lowercase()),
            None => format!(".{format}"),
        };
        let stored = format!("{id}{ext}");
        fs::copy(source, self.books_dir().join(&stored))?;

        let meta = metadata::extract(source);
        let cover = meta.cover.and_then(|c| {
            let name = format!("{id}.{}", c.ext);
            fs::write(self.covers_dir().join(&name), c.data)
                .ok()
                .map(|_| name)
        });

        let book = Book {
            id,
            title: meta.title,
            author: meta.author,
            format,
            file: stored,
            cover,
            tags: Vec::new(),
            added_at: now_iso(),
            extra: Map::new(),
        };
        self.index.books.push(book.clone());
        self.save()?;
        Ok(book)
    }

    pub fn delete(&mut self, id: &str) -> Result<bool> {
        let Some(book) = self.get(id).cloned() else {
            return Ok(false);
        };
        let _ = fs::remove_file(self.books_dir().join(&book.file));
        if let Some(c) = &book.cover {
            let _ = fs::remove_file(self.covers_dir().join(c));
        }
        let _ = fs::remove_file(self.state_path(id));
        self.index.books.retain(|b| b.id != id);
        self.save()?;
        Ok(true)
    }

    pub fn book_file(&self, id: &str) -> Option<PathBuf> {
        self.get(id).map(|b| self.books_dir().join(&b.file))
    }

    pub fn cover_file(&self, id: &str) -> Option<PathBuf> {
        self.get(id)
            .and_then(|b| b.cover.as_ref())
            .map(|c| self.covers_dir().join(c))
    }

    pub fn get_state(&self, id: &str) -> Map<String, Value> {
        laankit_core::json::read_object(&self.state_path(id))
    }

    pub fn set_state(&self, id: &str, patch: &Value) -> Result<Map<String, Value>> {
        let mut next = self.get_state(id);
        laankit_core::json::merge(&mut next, patch);
        next.insert("lastOpenedAt".into(), Value::String(now_iso()));
        laankit_core::json::write(&self.state_path(id), &next)?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn add_list_state_delete() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("Paper.pdf");
        fs::write(&src, b"%PDF-1.4").unwrap();
        let mut lib = Library::open(dir.path().join("lib")).unwrap();
        let book = lib.add(&src).unwrap();
        assert_eq!(book.title, "Paper");
        assert_eq!(book.format, "pdf");
        assert!(lib.book_file(&book.id).unwrap().exists());

        let st = lib.set_state(&book.id, &json!({ "position": 3 })).unwrap();
        assert_eq!(st["position"], 3);
        assert!(st.contains_key("lastOpenedAt"));

        let reopened = Library::open(dir.path().join("lib")).unwrap();
        assert_eq!(reopened.books().len(), 1);

        assert!(lib.delete(&book.id).unwrap());
        assert!(lib.books().is_empty());
    }

    #[test]
    fn index_written_by_electron_round_trips() {
        let raw = r#"{
  "version": 1,
  "books": [
    {
      "id": "a",
      "title": "T",
      "author": null,
      "format": "pdf",
      "file": "a.pdf",
      "cover": null,
      "tags": [],
      "addedAt": "2026-01-01T00:00:00.000Z",
      "rating": 5
    }
  ]
}"#;
        let idx: Index = serde_json::from_str(raw).unwrap();
        assert_eq!(laankit_core::json::to_pretty(&idx).unwrap(), raw);
    }
}
