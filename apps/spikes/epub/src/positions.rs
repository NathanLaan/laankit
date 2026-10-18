//! Reading positions, one per book: spine index + top-level block + character offset. No CFI
//! (decision 3 in the plan), so the format only has to round-trip through this reader.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub chapter: usize,
    pub block: usize,
    pub offset: usize,
}

/// `positions.json`: book id → position.
pub struct Positions {
    path: PathBuf,
    books: BTreeMap<String, Position>,
}

impl Positions {
    pub fn default_path() -> PathBuf {
        let dir = std::env::var_os("SPIKE_EPUB_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("marina-spike-epub"));
        dir.join("positions.json")
    }

    /// Reads `path`; a missing or unreadable file is an empty list.
    pub fn load(path: PathBuf) -> Self {
        let books = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, books }
    }

    pub fn get(&self, book: &str) -> Option<Position> {
        self.books.get(book).copied()
    }

    /// Records `pos`; true when it changed.
    pub fn set(&mut self, book: &str, pos: Position) -> bool {
        self.books.insert(book.to_string(), pos) != Some(pos)
    }

    pub fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, serde_json::to_string_pretty(&self.books)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/positions.json");
        let mut p = Positions::load(path.clone());
        assert_eq!(p.get("book"), None);
        let pos = Position {
            chapter: 3,
            block: 17,
            offset: 42,
        };
        assert!(p.set("book", pos));
        assert!(!p.set("book", pos));
        p.save().unwrap();
        assert_eq!(Positions::load(path).get("book"), Some(pos));
    }
}
