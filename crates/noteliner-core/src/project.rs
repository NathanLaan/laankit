//! A NoteLiner project: a folder of Markdown notes and diagrams, in
//! sub-folders at any depth, with `noteliner.json` (the index) at the top.
//!
//! The folders on disk say what exists and where ([`crate::tree`]). The index
//! holds what the files can't: an id for each note and diagram, its name (a
//! file's name is a slug), a diagram's tags, and a note's attachments. A
//! note's tags and created date are also in its frontmatter.
//!
//! The index is kept as raw JSON so fields this build doesn't know survive
//! round-trips, and is written exactly as `JSON.stringify(index, null, 2)`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use crate::frontmatter::{self, Block, Data};
use crate::tree::{self, Changes};
use crate::yaml::Yaml;

pub const INDEX_FILE: &str = "noteliner.json";
pub const ATTACHMENTS_DIR: &str = "_attachments";
/// The index's version: 4 has paths in `filename` and no `parentId`.
pub const INDEX_VERSION: u64 = 4;
pub const MAX_ATTACHMENT_SIZE: usize = 30 * 1024 * 1024;

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn mime_for(ext: &str) -> &'static str {
    match ext {
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".gif" => "image/gif",
        ".webp" => "image/webp",
        ".svg" => "image/svg+xml",
        ".pdf" => "application/pdf",
        ".doc" => "application/msword",
        ".docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ".xls" => "application/vnd.ms-excel",
        ".xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ".csv" => "text/csv",
        ".txt" => "text/plain",
        ".zip" => "application/zip",
        ".mp3" => "audio/mpeg",
        ".mp4" => "video/mp4",
        ".json" => "application/json",
        _ => "application/octet-stream",
    }
}

/// `path.extname`: the last `.ext` of the basename, lowercased by callers.
pub fn extname(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    match base.rfind('.') {
        Some(i) if i > 0 => base[i..].to_string(),
        _ => String::new(),
    }
}

/// What a project file is, read from its extension: a Markdown note, or a
/// BlockLiner diagram (a Mermaid flowchart, `.mmd`). A diagram's file holds
/// only the diagram: its name, tags and dates live in the index, and no
/// frontmatter is written into it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileKind {
    #[default]
    Note,
    Diagram,
}

impl FileKind {
    /// The kind of the file named `filename`.
    pub fn of(filename: &str) -> Self {
        match extname(filename).to_lowercase().as_str() {
            ".mmd" | ".mermaid" => FileKind::Diagram,
            _ => FileKind::Note,
        }
    }

    /// The kind of an index entry.
    pub fn of_entry(entry: &Value) -> Self {
        Self::of(s(&entry["filename"]))
    }

    /// The extension of a new file of this kind, with its dot.
    pub fn ext(self) -> &'static str {
        match self {
            FileKind::Note => ".md",
            FileKind::Diagram => ".mmd",
        }
    }

    /// `note` or `diagram`.
    pub fn name(self) -> &'static str {
        match self {
            FileKind::Note => "note",
            FileKind::Diagram => "diagram",
        }
    }

    /// The kind named `name`, as [`FileKind::name`] spells it.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "note" => Some(FileKind::Note),
            "diagram" => Some(FileKind::Diagram),
            _ => None,
        }
    }
}

/// An entry's kind as the MCP listings give it: `note`, `deck` (a note with
/// a `presentation:` block) or `diagram`.
pub fn entry_kind(entry: &Value) -> &'static str {
    match FileKind::of_entry(entry) {
        FileKind::Diagram => "diagram",
        FileKind::Note if entry["deck"] == true => "deck",
        FileKind::Note => "note",
    }
}

/// A new diagram's text: an empty flowchart.
pub fn blank_diagram() -> String {
    blockliner_core::write(&blockliner_core::Diagram::new())
}

/// Checks that `text` is a diagram BlockLiner can open.
pub fn check_diagram(text: &str) -> Result<()> {
    blockliner_core::read(text)
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Not a diagram BlockLiner can read: {e}"))
}

/// Lowercase, collapse non-alphanumerics to `-`, trim dashes.
pub fn slugify(text: &str) -> String {
    let lower = text.to_lowercase();
    let mut out = String::new();
    let mut dash = false;
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    let out = out.strip_prefix('-').unwrap_or(&out);
    out.strip_suffix('-').unwrap_or(out).to_string()
}

/// The file name for a note called `name`, without its extension. A name
/// with no letters or digits still gets one.
pub fn file_slug(name: &str) -> String {
    let slug = slugify(name);
    if slug.is_empty() {
        "untitled".to_string()
    } else {
        slug
    }
}

/// Sends a file or folder to the system's trash.
pub type Trash = Arc<dyn Fn(&Path) -> Result<()> + Send + Sync>;

/// The trash couldn't take a file (a network drive, some sandboxes). The
/// app asks before deleting it for good.
#[derive(Debug)]
pub struct TrashUnavailable(pub String);

impl std::fmt::Display for TrashUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "The trash isn't available: {}", self.0)
    }
}

impl std::error::Error for TrashUnavailable {}

/// The system's trash, as a file manager uses it.
pub fn system_trash() -> Trash {
    Arc::new(|path| {
        trash::delete(path).map_err(|e| anyhow::Error::new(TrashUnavailable(e.to_string())))
    })
}

/// What opening a folder found.
#[derive(Clone, Debug)]
pub struct Opened {
    pub index: Value,
    /// Every folder under the project, by path.
    pub folders: Vec<String>,
    /// The folder has more files than NoteLiner reads ([`tree::MAX_FILES`]).
    pub truncated: bool,
}

/// How a note's save ended.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Written {
    /// The file as written, when NoteLiner put a missing `tags` or `created`
    /// back into its frontmatter. `None` when it was saved as given.
    pub rewritten: Option<String>,
    /// The frontmatter can't be read as YAML: the file was saved as typed,
    /// and the note's tags and deck settings keep their last good values.
    pub unreadable: bool,
}

/// What a folder holds, for the question before deleting it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FolderCounts {
    pub notes: usize,
    pub diagrams: usize,
    pub others: usize,
}

/// Counts everything under `dir`: notes, diagrams, and every other file,
/// attachments among them.
pub fn folder_counts(dir: &Path) -> FolderCounts {
    let mut counts = FolderCounts::default();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_dir() {
                stack.push(entry.path());
            } else if !tree::is_project_file(&name) || tree::skipped(&name) {
                counts.others += 1;
            } else if FileKind::of(&name) == FileKind::Diagram {
                counts.diagrams += 1;
            } else {
                counts.notes += 1;
            }
        }
    }
    counts
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

pub struct Project {
    pub path: Option<PathBuf>,
    pub index: Option<Value>,
    pub write_frontmatter: bool,
    /// Where deleted notes, folders and attachments go.
    pub trash: Trash,
    /// The folders under the project, from the last walk.
    folders: Vec<String>,
    /// Each file's size at the last walk, to match a file renamed outside.
    sizes: HashMap<String, u64>,
    /// Each folder's modification time at the last walk.
    times: HashMap<String, SystemTime>,
    /// `noteliner.json`'s modification time when NoteLiner last wrote it.
    index_time: Option<SystemTime>,
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Project {
    pub fn new() -> Self {
        Self {
            path: None,
            index: None,
            write_frontmatter: true,
            trash: system_trash(),
            folders: Vec::new(),
            sizes: HashMap::new(),
            times: HashMap::new(),
            index_time: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.path.is_some() && self.index.is_some()
    }

    pub fn dir(&self) -> Result<&Path> {
        self.path
            .as_deref()
            .ok_or_else(|| anyhow!("No project is open"))
    }

    fn index_path(&self) -> Result<PathBuf> {
        Ok(self.dir()?.join(INDEX_FILE))
    }

    /// Resolve a project-relative path, refusing anything that would escape
    /// the project folder.
    pub fn file_path(&self, filename: &str) -> Result<PathBuf> {
        let rel = Path::new(filename);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            bail!("Invalid file path: {filename}");
        }
        Ok(self.dir()?.join(rel))
    }

    pub fn files(&self) -> &[Value] {
        self.index
            .as_ref()
            .and_then(|i| i["files"].as_array())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn files_mut(&mut self) -> Result<&mut Vec<Value>> {
        let index = self
            .index
            .as_mut()
            .ok_or_else(|| anyhow!("No project is open"))?;
        if !index["files"].is_array() {
            index["files"] = json!([]);
        }
        Ok(index["files"].as_array_mut().unwrap())
    }

    pub fn find(&self, id: &str) -> Option<&Value> {
        self.files().iter().find(|f| s(&f["id"]) == id)
    }

    pub fn find_by_filename(&self, filename: &str) -> Option<&Value> {
        self.files().iter().find(|f| s(&f["filename"]) == filename)
    }

    fn position(&self, id: &str) -> Option<usize> {
        self.files().iter().position(|f| s(&f["id"]) == id)
    }

    fn position_by_filename(&self, filename: &str) -> Option<usize> {
        self.files()
            .iter()
            .position(|f| s(&f["filename"]) == filename)
    }

    pub fn write_index(&mut self) -> Result<()> {
        let index = self
            .index
            .as_ref()
            .ok_or_else(|| anyhow!("No project is open"))?;
        let path = self.index_path()?;
        laankit_core::json::write(&path, index)?;
        self.index_time = modified(&path);
        Ok(())
    }

    // --- Open ---------------------------------------------------------------

    /// Opens any folder: reads `noteliner.json` if there is one, walks the
    /// folder, and brings the index in line with what is there. Only
    /// `noteliner.json` is written, and only when it changed.
    pub async fn open(&mut self, folder: &Path) -> Result<Opened> {
        if !folder.is_dir() {
            bail!("{} is not a folder", folder.display());
        }
        let index_path = folder.join(INDEX_FILE);
        let on_disk: Option<Value> = match fs::read_to_string(&index_path) {
            Ok(raw) => Some(
                serde_json::from_str(&raw)
                    .map_err(|e| anyhow!("{INDEX_FILE} can't be read: {e}"))?,
            ),
            Err(_) => None,
        };
        self.close();
        self.path = Some(folder.to_path_buf());
        self.index = Some(on_disk.clone().unwrap_or_else(tree::empty_index));
        self.index_time = modified(&index_path);
        self.migrate_index()?;
        let walk = tree::walk(folder);
        let truncated = walk.truncated;
        let changes = tree::reconcile(self.files_mut()?, &walk, None, folder);
        self.adopt(walk);
        self.carry_moved(&changes);
        self.refresh_deck_flags()?;
        if on_disk.as_ref() != self.index.as_ref() {
            self.write_index()?;
        }
        Ok(Opened {
            index: self.index.clone().unwrap_or(Value::Null),
            folders: self.folders.clone(),
            truncated,
        })
    }

    pub fn close(&mut self) {
        self.path = None;
        self.index = None;
        self.folders.clear();
        self.sizes.clear();
        self.times.clear();
        self.index_time = None;
    }

    /// Keeps what a walk found: the folders, each file's size, and each
    /// folder's time.
    fn adopt(&mut self, walk: tree::Walk) {
        self.sizes = walk.files.into_iter().map(|f| (f.path, f.size)).collect();
        self.folders = walk.folders;
        self.times = walk.times;
    }

    /// Reads the folders again after NoteLiner changed them itself.
    fn rewalk(&mut self) {
        if let Some(dir) = self.path.clone() {
            self.adopt(tree::walk(&dir));
        }
    }

    /// The attachments of entries a reconcile found at a new path follow
    /// them, if another program left them behind.
    fn carry_moved(&mut self, changes: &Changes) {
        for (id, old, new) in &changes.moved {
            if let Some(entry) = self.find(id).cloned() {
                self.carry_attachments(&entry, old, new);
            }
        }
    }

    /// Checks for changes made by other programs: notes and folders added,
    /// moved, renamed or removed, and `noteliner.json` itself. Each folder's
    /// modification time is compared with the last walk, so a quiet project
    /// costs one `stat` per folder. `None` when nothing changed.
    pub fn rescan(&mut self) -> Result<Option<Changes>> {
        let dir = self.dir()?.to_path_buf();
        let index_path = dir.join(INDEX_FILE);
        let index_changed = index_path.exists() && modified(&index_path) != self.index_time;
        let folders_changed = self.times.iter().any(|(folder, time)| {
            let path = if folder.is_empty() {
                dir.clone()
            } else {
                dir.join(folder)
            };
            modified(&path) != Some(*time)
        });
        if !index_changed && !folders_changed {
            return Ok(None);
        }
        let before = (self.index.clone(), self.folders.clone());
        if index_changed {
            let read = fs::read_to_string(&index_path)
                .ok()
                .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
            // A half-written index is read at the next check.
            match read {
                Some(index) => {
                    self.index = Some(index);
                    self.index_time = modified(&index_path);
                    self.migrate_index()?;
                }
                None => return Ok(None),
            }
        }
        let on_disk = self.index.clone();
        let walk = tree::walk(&dir);
        let sizes = std::mem::take(&mut self.sizes);
        let changes = tree::reconcile(self.files_mut()?, &walk, Some(&sizes), &dir);
        self.adopt(walk);
        self.carry_moved(&changes);
        if on_disk != self.index {
            self.write_index()?;
        }
        if before.0 == self.index && before.1 == self.folders {
            return Ok(None);
        }
        Ok(Some(changes))
    }

    // --- Index --------------------------------------------------------------

    /// Takes the app's changes to the index: each entry's tags, and the tag
    /// colours and palette. What exists and where is never taken from the
    /// app, since the folders say that. Returns the notes whose frontmatter
    /// was rewritten with their new tags.
    pub async fn save_index(&mut self, theirs: Value) -> Result<Vec<String>> {
        let mut retagged: Vec<usize> = Vec::new();
        let n = self.files().len();
        for i in 0..n {
            let id = s(&self.files()[i]["id"]).to_string();
            let Some(tags) = theirs["files"]
                .as_array()
                .and_then(|fs| fs.iter().find(|f| s(&f["id"]) == id))
                .and_then(|f| f["tags"].as_array())
            else {
                continue;
            };
            let mine = self.files()[i]["tags"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if &mine != tags {
                self.files_mut()?[i]["tags"] = json!(tags);
                retagged.push(i);
            }
        }
        if let (Some(mine), Some(theirs)) = (
            self.index.as_mut().and_then(Value::as_object_mut),
            theirs.as_object(),
        ) {
            for (k, v) in theirs {
                if k != "files" && k != "version" {
                    mine.insert(k.clone(), v.clone());
                }
            }
        }
        self.write_index()?;
        let mut rewritten = Vec::new();
        for i in retagged {
            let entry = self.files()[i].clone();
            if self.rewrite_frontmatter(&entry)? {
                rewritten.push(s(&entry["filename"]).to_string());
            }
        }
        Ok(rewritten)
    }

    /// Brings an older index up to [`INDEX_VERSION`]. Nothing is written here.
    fn migrate_index(&mut self) -> Result<()> {
        let dir = self.dir()?.to_path_buf();
        if !self.index.as_ref().is_some_and(Value::is_object) {
            self.index = Some(tree::empty_index());
        }
        let version = self
            .index
            .as_ref()
            .and_then(|i| i["version"].as_u64())
            .unwrap_or(0);
        // Entries without an id or a file can't be matched to anything.
        self.files_mut()?
            .retain(|f| !s(&f["id"]).is_empty() && !s(&f["filename"]).is_empty());
        if version < 2 {
            for f in self.files_mut()?.iter_mut() {
                if f.get("attachments").is_none_or(|a| a.is_null()) {
                    f["attachments"] = json!([]);
                }
            }
        }
        if version < 3 {
            for f in self.files_mut()?.iter_mut() {
                let has = |f: &Value, k: &str| f.get(k).is_some_and(|v| !v.is_null() && v != "");
                let (has_created, has_modified) = (has(f, "createdAt"), has(f, "modifiedAt"));
                if has_created && has_modified {
                    continue;
                }
                let (created, modified) = stat_times(&dir.join(s(&f["filename"])));
                if !has_created {
                    f["createdAt"] = json!(created);
                }
                if !has_modified {
                    f["modifiedAt"] = json!(modified);
                }
            }
        }
        if version < 4 {
            // The folders are the tree: a note is no longer under a note,
            // and there is no order to keep.
            for f in self.files_mut()?.iter_mut() {
                if let Some(obj) = f.as_object_mut() {
                    obj.remove("parentId");
                    obj.remove("order");
                }
            }
        }
        if version < INDEX_VERSION {
            self.index.as_mut().unwrap()["version"] = json!(INDEX_VERSION);
        }
        Ok(())
    }

    /// Refreshes each note's derived `deck` flag from its frontmatter.
    fn refresh_deck_flags(&mut self) -> Result<()> {
        let dir = self.dir()?.to_path_buf();
        for f in self.files_mut()?.iter_mut() {
            // A diagram has no frontmatter and is never a deck.
            if FileKind::of_entry(f) == FileKind::Diagram {
                continue;
            }
            let Ok(raw) = fs::read_to_string(dir.join(s(&f["filename"]))) else {
                continue;
            };
            // A block that can't be read keeps the flag it had.
            if let Block::Data(data) = frontmatter::block(&raw) {
                apply_deck_flag(f, &data);
            } else if frontmatter::split(&raw).is_none() {
                apply_deck_flag(f, &Vec::new());
            }
        }
        Ok(())
    }

    // --- Notes --------------------------------------------------------------

    /// The file as it is on disk: a note with its frontmatter, a diagram
    /// whole.
    pub fn read_file(&self, filename: &str) -> Result<String> {
        let path = self.file_path(filename)?;
        Ok(fs::read_to_string(path).unwrap_or_default())
    }

    /// A note's text without its frontmatter (for search, export, links and
    /// MCP). A diagram is its whole file.
    pub fn read_body(&self, filename: &str) -> Result<String> {
        let raw = self.read_file(filename)?;
        Ok(match FileKind::of(filename) {
            FileKind::Diagram => raw,
            FileKind::Note => frontmatter::strip_body(&raw),
        })
    }

    pub fn read_frontmatter(&self, filename: &str) -> Value {
        if FileKind::of(filename) == FileKind::Diagram {
            return json!({});
        }
        match self
            .file_path(filename)
            .ok()
            .and_then(|p| fs::read_to_string(p).ok())
        {
            Some(raw) => frontmatter::to_json(&frontmatter::parse(&raw).data),
            None => json!({}),
        }
    }

    /// Saves a whole file. A note's frontmatter is read as typed: its tags
    /// and deck settings reach the index, and a missing `tags` or `created`
    /// is put back. A block with both is written byte for byte, and so is
    /// one that can't be read as YAML. With Write YAML frontmatter off,
    /// nothing is added.
    pub async fn write_file(&mut self, filename: &str, content: &str) -> Result<Written> {
        let path = self.file_path(filename)?;
        let pos = self.position_by_filename(filename);
        let mut written = Written::default();
        if let (Some(i), FileKind::Note) = (pos, FileKind::of(filename)) {
            match frontmatter::block(content) {
                Block::Unreadable => written.unreadable = true,
                found => {
                    let mut data = match found {
                        Block::Data(d) => d,
                        _ => Vec::new(),
                    };
                    if self.write_frontmatter {
                        if let Some(tags) = frontmatter::tags_of(&data) {
                            self.set_tags_at(i, &tags)?;
                        }
                        if let Some(next) = frontmatter::restored(&self.files()[i], &data) {
                            written.rewritten = Some(
                                frontmatter::block_text(&next) + &frontmatter::strip_body(content),
                            );
                            data = next;
                        }
                    }
                    apply_deck_flag(&mut self.files_mut()?[i], &data);
                }
            }
        }
        let out = written.rewritten.as_deref().unwrap_or(content);
        fs::write(&path, out)?;
        self.sizes.insert(filename.to_string(), out.len() as u64);
        if let Some(i) = pos {
            self.files_mut()?[i]["modifiedAt"] = json!(now_iso());
            let _ = self.write_index();
        }
        Ok(written)
    }

    /// Replaces a note's text and keeps its frontmatter (an AI assistant's
    /// `update_note`, and the importers). A diagram is its whole file.
    pub async fn write_body(&mut self, filename: &str, body: &str) -> Result<Written> {
        if FileKind::of(filename) == FileKind::Diagram {
            return self.write_file(filename, body).await;
        }
        let raw = fs::read_to_string(self.file_path(filename)?).unwrap_or_default();
        let block = frontmatter::split(&raw).map_or("", |s| s.block);
        let text = if body.ends_with('\n') || (body.is_empty() && block.is_empty()) {
            body.to_string()
        } else {
            format!("{body}\n")
        };
        let block = if block.is_empty() || block.ends_with('\n') {
            block.to_string()
        } else {
            format!("{block}\n")
        };
        self.write_file(filename, &(block + &text)).await
    }

    /// The tags typed in a note's frontmatter become the note's tags, and
    /// join the project's palette.
    fn set_tags_at(&mut self, i: usize, tags: &[String]) -> Result<()> {
        self.files_mut()?[i]["tags"] = json!(tags);
        if let Some(palette) = self
            .index
            .as_mut()
            .and_then(|index| index["tagPalette"].as_array_mut())
        {
            for t in tags {
                if !palette.iter().any(|p| p == t) {
                    palette.push(json!(t));
                }
            }
        }
        Ok(())
    }

    /// Writes an entry's tags into its note's frontmatter, keeping the text.
    /// A block that can't be read is left as it is.
    pub fn rewrite_frontmatter(&mut self, entry: &Value) -> Result<bool> {
        if !self.write_frontmatter || FileKind::of_entry(entry) == FileKind::Diagram {
            return Ok(false);
        }
        let filename = s(&entry["filename"]);
        let path = self.file_path(filename)?;
        let Ok(raw) = fs::read_to_string(&path) else {
            return Ok(false);
        };
        // A note with no block and no tags needs none.
        let no_tags = entry["tags"].as_array().is_none_or(Vec::is_empty);
        if no_tags && frontmatter::split(&raw).is_none() {
            return Ok(false);
        }
        match frontmatter::retag(&raw, entry) {
            Some(next) if next != raw => {
                fs::write(&path, &next)?;
                self.sizes.insert(filename.to_string(), next.len() as u64);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// A new note.
    pub async fn create_file(
        &mut self,
        name: &str,
        tags: &Value,
        body: Option<String>,
        folder: Option<&str>,
    ) -> Result<Value> {
        self.create(FileKind::Note, name, tags, body, folder).await
    }

    /// A new file of `kind` in `folder` (the project folder for `None`),
    /// which is made if it isn't there. A diagram starts blank, or as
    /// `body`, which must be one BlockLiner can read.
    pub async fn create(
        &mut self,
        kind: FileKind,
        name: &str,
        tags: &Value,
        body: Option<String>,
        folder: Option<&str>,
    ) -> Result<Value> {
        if name.trim().is_empty() {
            bail!("File name cannot be empty");
        }
        if kind == FileKind::Diagram {
            if let Some(body) = &body {
                check_diagram(body)?;
            }
        }
        let folder = folder.unwrap_or("");
        tree::check_folder_path(folder)?;
        fs::create_dir_all(self.dir()?.join(folder))?;
        let id = uuid::Uuid::new_v4().to_string();
        let filename =
            self.unique_filename(folder, &format!("{}{}", file_slug(name), kind.ext()))?;
        let now = now_iso();
        let mut entry = json!({
            "id": id,
            "name": name,
            "filename": filename,
            "tags": if tags.is_array() { tags.clone() } else { json!([]) },
            "attachments": [],
            "createdAt": now,
            "modifiedAt": now,
        });
        let path = self.file_path(&filename)?;
        let initial = body.unwrap_or_else(|| match kind {
            FileKind::Note => format!("# {name} {}\n", chrono::Local::now().format("%Y-%m-%d")),
            FileKind::Diagram => blank_diagram(),
        });
        if self.write_frontmatter && kind == FileKind::Note {
            // A template or a copied note may carry its own frontmatter (a
            // deck's `presentation:`); keep it, minus the template-only
            // `kind` and the date the other file was created.
            let seeded = frontmatter::parse(&initial);
            let mut template_data: Data = seeded.data;
            frontmatter::remove(&mut template_data, "kind");
            frontmatter::remove(&mut template_data, "created");
            let data = frontmatter::mirror_from_entry(&entry, &template_data);
            fs::write(&path, frontmatter::serialize(&seeded.body, &data))?;
            apply_deck_flag(&mut entry, &data);
        } else {
            fs::write(&path, &initial)?;
        }
        self.files_mut()?.push(entry.clone());
        self.write_index()?;
        self.rewalk();
        Ok(entry)
    }

    /// Deletes a note or diagram: its file and its attachments go to the
    /// trash, or are removed for good with `permanent`. Fails with
    /// [`TrashUnavailable`] when the trash can't take the file.
    pub async fn delete_file(&mut self, id: &str, permanent: bool) -> Result<()> {
        let Some(entry) = self.find(id).cloned() else {
            return Ok(());
        };
        let filename = s(&entry["filename"]).to_string();
        self.discard(&self.file_path(&filename)?, permanent)?;
        for att in entry["attachments"].as_array().into_iter().flatten() {
            if let Ok(path) = self.attachment_path(&filename, s(&att["filename"])) {
                let _ = self.discard(&path, permanent);
            }
        }
        self.drop_empty_attachments(tree::folder_of(&filename));
        self.files_mut()?.retain(|f| s(&f["id"]) != id);
        self.sizes.remove(&filename);
        self.write_index()
    }

    /// Sends `path` to the trash, or removes it. A path that isn't there is
    /// already gone.
    fn discard(&self, path: &Path, permanent: bool) -> Result<()> {
        if fs::symlink_metadata(path).is_err() {
            return Ok(());
        }
        if !permanent {
            return (self.trash)(path);
        }
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Renames a note or diagram. Its file is renamed within its folder, to
    /// a slug of the new name. Nothing in the file changes.
    pub async fn rename_file(&mut self, id: &str, new_name: &str) -> Result<Option<Value>> {
        if new_name.trim().is_empty() {
            return Ok(None);
        }
        let Some(i) = self.position(id) else {
            return Ok(None);
        };
        let old_filename = s(&self.files()[i]["filename"]).to_string();
        let folder = tree::folder_of(&old_filename).to_string();
        // The extension, and so the kind, stays.
        let ext = extname(&old_filename);
        let wanted = tree::join(&folder, &format!("{}{ext}", file_slug(new_name)));
        let new_filename = if wanted == old_filename {
            old_filename.clone()
        } else {
            self.unique_filename_but(&folder, tree::file_name(&wanted), Some(&old_filename))?
        };
        if new_filename != old_filename {
            let (old_path, new_path) = (
                self.file_path(&old_filename)?,
                self.file_path(&new_filename)?,
            );
            if old_path.exists() {
                fs::rename(old_path, new_path)?;
            }
            if let Some(size) = self.sizes.remove(&old_filename) {
                self.sizes.insert(new_filename.clone(), size);
            }
        }
        {
            let f = &mut self.files_mut()?[i];
            f["name"] = json!(new_name);
            f["filename"] = json!(new_filename);
        }
        self.write_index()?;
        Ok(Some(self.files()[i].clone()))
    }

    /// Moves a note or diagram to `folder`, with a note's attachments. A
    /// file name already taken there gets `-2`; the name doesn't change.
    pub async fn move_file(&mut self, id: &str, folder: &str) -> Result<Option<Value>> {
        tree::check_folder_path(folder)?;
        let Some(i) = self.position(id) else {
            return Ok(None);
        };
        let old_filename = s(&self.files()[i]["filename"]).to_string();
        if tree::folder_of(&old_filename) == folder {
            return Ok(Some(self.files()[i].clone()));
        }
        fs::create_dir_all(self.dir()?.join(folder))?;
        let new_filename = self.unique_filename(folder, tree::file_name(&old_filename))?;
        let (old_path, new_path) = (
            self.file_path(&old_filename)?,
            self.file_path(&new_filename)?,
        );
        if old_path.exists() {
            fs::rename(old_path, new_path)?;
        }
        let entry = self.files()[i].clone();
        self.carry_attachments(&entry, &old_filename, &new_filename);
        self.files_mut()?[i]["filename"] = json!(new_filename);
        self.write_index()?;
        self.rewalk();
        Ok(Some(self.files()[i].clone()))
    }

    /// Add, replace, or (with null) remove a note's `presentation:` block.
    pub async fn set_presentation(
        &mut self,
        filename: &str,
        presentation: &Value,
    ) -> Result<Value> {
        let path = self.file_path(filename)?;
        if !path.exists() {
            bail!("File does not exist");
        }
        if FileKind::of(filename) == FileKind::Diagram {
            bail!("A diagram can't be a presentation");
        }
        let raw = fs::read_to_string(&path)?;
        let Some(next) = frontmatter::with_presentation(&raw, presentation) else {
            bail!("The note's frontmatter can't be read");
        };
        fs::write(&path, &next)?;
        self.sizes.insert(filename.to_string(), next.len() as u64);
        let data = frontmatter::parse(&next).data;
        if let Some(i) = self.position_by_filename(filename) {
            let f = &mut self.files_mut()?[i];
            apply_deck_flag(f, &data);
            f["modifiedAt"] = json!(now_iso());
            let _ = self.write_index();
        }
        Ok(frontmatter::get_field(&data, "presentation")
            .map(Yaml::to_json)
            .unwrap_or(Value::Null))
    }

    // --- Folders --------------------------------------------------------------

    /// Every folder under the project, by path, parents before children.
    pub fn folders(&self) -> &[String] {
        &self.folders
    }

    /// A folder at `path` is already there, ignoring case (`except` is the
    /// folder being renamed or moved).
    fn folder_taken(&self, path: &str, except: Option<&str>) -> bool {
        let lower = path.to_lowercase();
        let listed = self
            .folders
            .iter()
            .any(|f| Some(f.as_str()) != except && f.to_lowercase() == lower);
        let on_disk = except.is_none_or(|e| e.to_lowercase() != lower)
            && self.path.as_ref().is_some_and(|d| d.join(path).exists());
        listed || on_disk
    }

    /// Makes the folder at `path`, with the folders above it.
    pub async fn create_folder(&mut self, path: &str) -> Result<String> {
        let path = path.trim().trim_matches('/').to_string();
        if path.is_empty() {
            bail!("A folder name can't be empty");
        }
        tree::check_folder_path(&path)?;
        if self.folder_taken(&path, None) {
            bail!("A folder named {} is already there", tree::file_name(&path));
        }
        fs::create_dir_all(self.dir()?.join(&path))?;
        self.rewalk();
        Ok(path)
    }

    /// Gives the entries under a folder their new paths.
    fn repath(&mut self, from: &str, to: &str) -> Result<()> {
        for f in self.files_mut()?.iter_mut() {
            let filename = s(&f["filename"]).to_string();
            if tree::is_under(&filename, from) && filename != from {
                f["filename"] = json!(format!("{to}{}", &filename[from.len()..]));
            }
        }
        Ok(())
    }

    /// Renames a folder on disk. Every entry under it gets its new path.
    /// Returns the folder's new path.
    pub async fn rename_folder(&mut self, path: &str, new_name: &str) -> Result<String> {
        tree::check_folder_path(path)?;
        let new_name = new_name.trim();
        tree::check_folder_name(new_name)?;
        if path.is_empty() || !self.dir()?.join(path).is_dir() {
            bail!("No folder at {path}");
        }
        let to = tree::join(tree::folder_of(path), new_name);
        if to == path {
            return Ok(to);
        }
        if self.folder_taken(&to, Some(path)) {
            bail!("A folder named {new_name} is already there");
        }
        fs::rename(self.dir()?.join(path), self.dir()?.join(&to))?;
        self.repath(path, &to)?;
        self.write_index()?;
        self.rewalk();
        Ok(to)
    }

    /// Moves a folder into `into` (the project folder for an empty path),
    /// with everything in it. Returns the folder's new path.
    pub async fn move_folder(&mut self, path: &str, into: &str) -> Result<String> {
        tree::check_folder_path(path)?;
        tree::check_folder_path(into)?;
        if path.is_empty() || !self.dir()?.join(path).is_dir() {
            bail!("No folder at {path}");
        }
        if tree::is_under(into, path) {
            bail!("A folder can't move into itself");
        }
        let to = tree::join(into, tree::file_name(path));
        if to == path {
            return Ok(to);
        }
        if self.folder_taken(&to, None) {
            bail!("A folder named {} is already there", tree::file_name(path));
        }
        fs::create_dir_all(self.dir()?.join(into))?;
        fs::rename(self.dir()?.join(path), self.dir()?.join(&to))?;
        self.repath(path, &to)?;
        self.write_index()?;
        self.rewalk();
        Ok(to)
    }

    /// What is inside a folder: notes, diagrams, and every other file,
    /// attachments among them.
    pub fn folder_counts(&self, path: &str) -> FolderCounts {
        match self.file_path(path) {
            Ok(dir) => folder_counts(&dir),
            Err(_) => FolderCounts::default(),
        }
    }

    /// Deletes a folder and everything in it: to the trash, or for good with
    /// `permanent`. Returns the entries that were inside.
    pub async fn delete_folder(&mut self, path: &str, permanent: bool) -> Result<Vec<Value>> {
        tree::check_folder_path(path)?;
        if path.is_empty() {
            bail!("The project folder can't be deleted");
        }
        self.discard(&self.dir()?.join(path), permanent)?;
        let mut removed = Vec::new();
        self.files_mut()?.retain(|f| {
            let inside = tree::is_under(s(&f["filename"]), path);
            if inside {
                removed.push(f.clone());
            }
            !inside
        });
        self.write_index()?;
        self.rewalk();
        Ok(removed)
    }

    // --- Attachments --------------------------------------------------------

    /// The `_attachments/` folder beside the note at `note_filename`.
    pub fn attachments_dir(&self, note_filename: &str) -> Result<PathBuf> {
        Ok(self
            .dir()?
            .join(tree::folder_of(note_filename))
            .join(ATTACHMENTS_DIR))
    }

    /// Where the attachment stored as `stored` is, for the note at
    /// `note_filename`.
    pub fn attachment_path(&self, note_filename: &str, stored: &str) -> Result<PathBuf> {
        Ok(self.attachments_dir(note_filename)?.join(stored))
    }

    /// Where the attachment stored as `stored` is, whichever note has it.
    pub fn find_attachment(&self, stored: &str) -> Option<PathBuf> {
        let owner = self.files().iter().find(|f| {
            f["attachments"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|a| s(&a["filename"]) == stored)
        })?;
        self.attachment_path(s(&owner["filename"]), stored).ok()
    }

    /// Moves a note's attachments from beside `old` to beside `new`, when
    /// the note changed folders. An `_attachments/` left empty is removed.
    fn carry_attachments(&self, entry: &Value, old: &str, new: &str) {
        if tree::folder_of(old) == tree::folder_of(new) {
            return;
        }
        let (Ok(from), Ok(to)) = (self.attachments_dir(old), self.attachments_dir(new)) else {
            return;
        };
        for att in entry["attachments"].as_array().into_iter().flatten() {
            let stored = s(&att["filename"]);
            let (src, dst) = (from.join(stored), to.join(stored));
            if stored.is_empty() || !src.exists() || dst.exists() {
                continue;
            }
            if fs::create_dir_all(&to).is_ok() {
                let _ = fs::rename(src, dst);
            }
        }
        self.drop_empty_attachments(tree::folder_of(old));
    }

    /// Removes `folder`'s `_attachments/` if nothing is left in it.
    fn drop_empty_attachments(&self, folder: &str) {
        if let Ok(dir) = self.dir() {
            // Fails, as it should, when the folder still holds files.
            let _ = fs::remove_dir(dir.join(folder).join(ATTACHMENTS_DIR));
        }
    }

    pub async fn add_attachment(
        &mut self,
        file_id: &str,
        data: &[u8],
        original_name: &str,
    ) -> Result<Value> {
        if data.len() > MAX_ATTACHMENT_SIZE {
            bail!(
                "File exceeds 30MB limit ({:.1}MB)",
                data.len() as f64 / 1024.0 / 1024.0
            );
        }
        let i = self
            .position(file_id)
            .ok_or_else(|| anyhow!("File not found"))?;
        let ext = extname(original_name).to_lowercase();
        let dir = self.attachments_dir(s(&self.files()[i]["filename"]))?;
        fs::create_dir_all(&dir)?;
        // A stored name is unique across the project, so two folders never
        // hold different files with one name.
        let (id, stored) = loop {
            let id = uuid::Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap()
                .to_string();
            let stored = format!("att-{id}{ext}");
            if self.find_attachment(&stored).is_none() && !dir.join(&stored).exists() {
                break (id, stored);
            }
        };
        fs::write(dir.join(&stored), data)?;
        let attachment = json!({
            "id": id,
            "originalName": original_name,
            "filename": stored,
            "mimeType": mime_for(&ext),
            "size": data.len(),
            "addedAt": now_iso(),
        });
        let f = &mut self.files_mut()?[i];
        if !f["attachments"].is_array() {
            f["attachments"] = json!([]);
        }
        f["attachments"]
            .as_array_mut()
            .unwrap()
            .push(attachment.clone());
        self.write_index()?;
        Ok(attachment)
    }

    /// Removes an attachment from its note. The file goes to the trash, or
    /// is deleted if the trash can't take it.
    pub async fn remove_attachment(&mut self, file_id: &str, attachment_id: &str) -> Result<()> {
        let Some(i) = self.position(file_id) else {
            return Ok(());
        };
        let atts = self.files()[i]["attachments"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let Some(k) = atts.iter().position(|a| s(&a["id"]) == attachment_id) else {
            return Ok(());
        };
        let filename = s(&self.files()[i]["filename"]).to_string();
        let path = self.attachment_path(&filename, s(&atts[k]["filename"]))?;
        if self.discard(&path, false).is_err() {
            let _ = self.discard(&path, true);
        }
        self.drop_empty_attachments(tree::folder_of(&filename));
        self.files_mut()?[i]["attachments"]
            .as_array_mut()
            .unwrap()
            .remove(k);
        self.write_index()
    }

    // --- Search -------------------------------------------------------------

    /// Line-level substring search over note bodies (frontmatter excluded).
    /// A match's line counts from the start of the note's text.
    pub fn search(&self, query: &str, case_sensitive: bool) -> Vec<Value> {
        let Ok(dir) = self.dir() else {
            return Vec::new();
        };
        if query.is_empty() {
            return Vec::new();
        }
        let needle = if case_sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };
        let mut results = Vec::new();
        for f in self.files() {
            let Ok(raw) = fs::read_to_string(dir.join(s(&f["filename"]))) else {
                continue;
            };
            // A diagram's `%%` lines are its layout, not its text.
            let diagram = FileKind::of_entry(f) == FileKind::Diagram;
            let body = if diagram {
                raw
            } else {
                frontmatter::strip_body(&raw)
            };
            let matches: Vec<Value> = body
                .split('\n')
                .enumerate()
                .filter(|(_, line)| !(diagram && line.trim_start().starts_with("%%")))
                .filter(|(_, line)| {
                    if case_sensitive {
                        line.contains(&needle)
                    } else {
                        line.to_lowercase().contains(&needle)
                    }
                })
                .map(|(i, line)| json!({ "line": i + 1, "text": line.trim_end() }))
                .collect();
            if !matches.is_empty() {
                results.push(json!({
                    "fileId": f["id"],
                    "fileName": f["name"],
                    "filename": f["filename"],
                    "folder": tree::folder_of(s(&f["filename"])),
                    "matches": matches,
                }));
            }
        }
        results
    }

    /// `filename` in `folder`, or with `-2`, `-3`… when a file of that name
    /// is there. Names are compared without case, so a project made on Linux
    /// still works on Windows and macOS.
    fn unique_filename(&self, folder: &str, filename: &str) -> Result<String> {
        self.unique_filename_but(folder, filename, None)
    }

    /// As [`Self::unique_filename`], for a file being renamed: the name it
    /// has now (`own`) is free for it to keep.
    fn unique_filename_but(
        &self,
        folder: &str,
        filename: &str,
        own: Option<&str>,
    ) -> Result<String> {
        let dir = self.dir()?;
        let own = own.map(str::to_lowercase);
        let taken = |c: &str| {
            let path = tree::join(folder, c);
            let lower = path.to_lowercase();
            if own.as_deref() == Some(lower.as_str()) {
                return false;
            }
            self.files()
                .iter()
                .any(|f| s(&f["filename"]).to_lowercase() == lower)
                || dir.join(&path).exists()
        };
        if !taken(filename) {
            return Ok(tree::join(folder, filename));
        }
        let ext = extname(filename);
        let stem = &filename[..filename.len() - ext.len()];
        for i in 2..10000 {
            let candidate = format!("{stem}-{i}{ext}");
            if !taken(&candidate) {
                return Ok(tree::join(folder, &candidate));
            }
        }
        bail!("Could not find a unique filename")
    }
}

/// `deck` caches "this note's frontmatter has a presentation block".
pub fn apply_deck_flag(entry: &mut Value, data: &Data) -> bool {
    let is_deck = match frontmatter::get_field(data, "presentation") {
        None | Some(Yaml::Null) | Some(Yaml::Bool(false)) => false,
        Some(Yaml::Str(s)) => !s.is_empty(),
        Some(Yaml::Int(0)) => false,
        Some(Yaml::Float(f)) => *f != 0.0 && !f.is_nan(),
        Some(_) => true,
    };
    if let Some(obj) = entry.as_object_mut() {
        if is_deck {
            obj.insert("deck".into(), json!(true));
        } else {
            obj.remove("deck");
        }
    }
    is_deck
}

pub(crate) fn stat_times(path: &Path) -> (String, String) {
    let iso = |t: std::time::SystemTime| {
        chrono::DateTime::<chrono::Utc>::from(t)
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    };
    match fs::metadata(path) {
        Ok(m) => {
            let mtime = m.modified().map(iso).unwrap_or_else(|_| now_iso());
            let birth = m
                .created()
                .ok()
                .filter(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() > 0)
                        .unwrap_or(false)
                })
                .map(iso)
                .unwrap_or_else(|| mtime.clone());
            (birth, mtime)
        }
        Err(_) => (now_iso(), now_iso()),
    }
}

/// A trash that is a folder: files and folders move into `dir` under their
/// own names, as a system's trash takes them. For tests, which mustn't fill
/// the real one.
pub fn folder_trash(dir: PathBuf) -> Trash {
    Arc::new(move |path| {
        fs::create_dir_all(&dir)?;
        let name = path.file_name().unwrap_or_default();
        let mut to = dir.join(name);
        let mut n = 1;
        while to.exists() {
            n += 1;
            to = dir.join(format!("{}-{n}", name.to_string_lossy()));
        }
        fs::rename(path, to)?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project in a temporary folder, with its trash beside it.
    struct Fixture {
        _tmp: tempfile::TempDir,
        dir: PathBuf,
        trash: PathBuf,
        p: Project,
    }

    async fn project() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("notes");
        let trash = tmp.path().join("trash");
        fs::create_dir_all(&dir).unwrap();
        let mut p = Project::new();
        p.trash = folder_trash(trash.clone());
        p.open(&dir).await.unwrap();
        Fixture {
            _tmp: tmp,
            dir,
            trash,
            p,
        }
    }

    fn read(dir: &Path, path: &str) -> String {
        fs::read_to_string(dir.join(path)).unwrap()
    }

    fn id(v: &Value) -> &str {
        v["id"].as_str().unwrap()
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("  Été 2026 "), "t-2026");
        assert_eq!(extname("a/b.tar.gz"), ".gz");
        assert_eq!(extname(".bashrc"), "");
        assert_eq!(file_slug("SDLC / ALM"), "sdlc-alm");
        assert_eq!(file_slug("!!!"), "untitled");
    }

    /// A box with a subroutine, as BlockLiner writes them.
    const DIAGRAM: &str = "flowchart LR\n  api[\"API Gateway\"]\n  jobs[[\"Job Runner\"]]\n  api --> jobs\n%% blockliner 1 grid=20\n%% @shape api x=40 y=40 w=160 h=80\n%% @shape jobs x=280 y=40 w=160 h=80\n";

    #[test]
    fn kinds_come_from_the_extension() {
        assert_eq!(FileKind::of("a.md"), FileKind::Note);
        assert_eq!(FileKind::of("a.mmd"), FileKind::Diagram);
        assert_eq!(FileKind::of("a.MERMAID"), FileKind::Diagram);
        assert_eq!(FileKind::of("mmd"), FileKind::Note);
        assert_eq!(entry_kind(&json!({ "filename": "a.md" })), "note");
        assert_eq!(
            entry_kind(&json!({ "filename": "a.md", "deck": true })),
            "deck"
        );
        assert_eq!(entry_kind(&json!({ "filename": "a.mmd" })), "diagram");
        assert!(check_diagram(&blank_diagram()).is_ok());
        assert!(check_diagram(DIAGRAM).is_ok());
    }

    #[tokio::test]
    async fn opening_a_plain_folder_writes_only_the_index() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let note = "# Kickoff\n\nNo frontmatter here.\n";
        fs::create_dir_all(dir.join("work/meetings")).unwrap();
        fs::create_dir_all(dir.join("empty")).unwrap();
        fs::write(dir.join("work/meetings/kickoff.md"), note).unwrap();
        fs::write(dir.join("map.mmd"), DIAGRAM).unwrap();
        fs::write(dir.join("report.pdf"), "x").unwrap();

        let mut p = Project::new();
        let opened = p.open(dir).await.unwrap();
        assert!(!opened.truncated);
        assert_eq!(opened.folders, ["empty", "work", "work/meetings"]);
        assert_eq!(opened.index["version"], 4);
        assert_eq!(p.files().len(), 2);
        let kickoff = p.find_by_filename("work/meetings/kickoff.md").unwrap();
        assert_eq!(kickoff["name"], "kickoff");
        assert!(kickoff.get("parentId").is_none() && kickoff.get("order").is_none());
        // A note with no frontmatter keeps none until it is edited.
        assert_eq!(read(dir, "work/meetings/kickoff.md"), note);
        assert_eq!(read(dir, "map.mmd"), DIAGRAM);
        assert!(dir.join("noteliner.json").exists());

        // A second open changes nothing, the index included.
        let index = read(dir, "noteliner.json");
        p.close();
        p.open(dir).await.unwrap();
        assert_eq!(read(dir, "noteliner.json"), index);

        // Something that isn't a folder is refused.
        assert!(p.open(&dir.join("report.pdf")).await.is_err());
    }

    #[tokio::test]
    async fn a_git_repository_opens_and_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::create_dir_all(dir.join(".git/objects")).unwrap();
        fs::write(dir.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(dir.join(".git/objects/note.md"), "not a note").unwrap();
        fs::write(dir.join("a.md"), "body\n").unwrap();
        let snapshot = |d: &Path| -> Vec<(PathBuf, Vec<u8>)> {
            let mut out = Vec::new();
            let mut stack = vec![d.join(".git")];
            while let Some(dir) = stack.pop() {
                for e in fs::read_dir(dir).unwrap().flatten() {
                    if e.path().is_dir() {
                        stack.push(e.path());
                    } else {
                        out.push((e.path(), fs::read(e.path()).unwrap()));
                    }
                }
            }
            out.sort();
            out
        };
        let before = snapshot(dir);

        let mut p = Project::new();
        p.trash = folder_trash(tmp.path().join("../trash-unused"));
        p.open(dir).await.unwrap();
        assert_eq!(p.files().len(), 1);
        assert!(p.folders().is_empty());
        let a = p.find_by_filename("a.md").unwrap().clone();
        p.write_file("a.md", "edited\n").await.unwrap();
        p.create_file("New", &json!(["t"]), None, Some("sub"))
            .await
            .unwrap();
        p.rename_file(id(&a), "Renamed").await.unwrap();
        assert_eq!(snapshot(dir), before);
    }

    #[tokio::test]
    async fn older_indexes_open_with_notes_where_their_files_are() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::write(
            dir.join("parent.md"),
            "---\nid: p\nname: Parent\ntags: []\n---\nP\n",
        )
        .unwrap();
        fs::write(dir.join("child.md"), "C\n").unwrap();
        let v3 = json!({
            "version": 3,
            "files": [
                { "id": "p", "name": "Parent", "filename": "parent.md", "parentId": null, "order": 0, "tags": ["x"], "attachments": [], "createdAt": "2026-01-01T00:00:00.000Z", "modifiedAt": "2026-01-01T00:00:00.000Z" },
                { "id": "c", "name": "Child Note", "filename": "child.md", "parentId": "p", "order": 0, "tags": [], "attachments": [], "createdAt": "2026-01-01T00:00:00.000Z", "modifiedAt": "2026-01-01T00:00:00.000Z" }
            ],
            "tagMeta": { "x": { "color": "Tomato" } }
        });
        laankit_core::json::write(&dir.join("noteliner.json"), &v3).unwrap();
        let parent = read(dir, "parent.md");

        let mut p = Project::new();
        let opened = p.open(dir).await.unwrap();
        assert_eq!(opened.index["version"], 4);
        assert_eq!(opened.index["tagMeta"]["x"]["color"], "Tomato");
        assert_eq!(p.files().len(), 2);
        for f in p.files() {
            assert!(f.get("parentId").is_none(), "{f}");
            assert!(f.get("order").is_none(), "{f}");
        }
        // Names, ids and tags are the index's.
        assert_eq!(p.find("c").unwrap()["name"], "Child Note");
        assert_eq!(p.find("p").unwrap()["tags"], json!(["x"]));
        // No note was rewritten.
        assert_eq!(read(dir, "parent.md"), parent);

        let index = read(dir, "noteliner.json");
        p.close();
        p.open(dir).await.unwrap();
        assert_eq!(read(dir, "noteliner.json"), index);

        // An index that can't be read is not replaced.
        fs::write(dir.join("noteliner.json"), "{ not json").unwrap();
        assert!(p.open(dir).await.is_err());
        assert_eq!(read(dir, "noteliner.json"), "{ not json");
    }

    #[tokio::test]
    async fn diagram_lifecycle() {
        let Fixture { dir, mut p, .. } = project().await;
        assert!(p.write_frontmatter);

        // Created blank, as `.mmd`, with no frontmatter.
        let d = p
            .create(
                FileKind::Diagram,
                "System Map",
                &json!(["arch"]),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(d["filename"], "system-map.mmd");
        assert_eq!(d["tags"], json!(["arch"]));
        assert!(d.get("deck").is_none());
        let raw = read(&dir, "system-map.mmd");
        assert_eq!(raw, blank_diagram());
        assert!(!raw.starts_with("---"));

        // Text that isn't a diagram is refused.
        assert!(p
            .create(
                FileKind::Diagram,
                "Bad",
                &json!([]),
                Some("# A note".into()),
                None
            )
            .await
            .is_err());
        assert!(!dir.join("bad.mmd").exists());

        // Written and read back byte for byte.
        p.write_file("system-map.mmd", DIAGRAM).await.unwrap();
        assert_eq!(read(&dir, "system-map.mmd"), DIAGRAM);
        assert_eq!(p.read_file("system-map.mmd").unwrap(), DIAGRAM);
        assert_eq!(p.read_body("system-map.mmd").unwrap(), DIAGRAM);
        assert_eq!(p.read_frontmatter("system-map.mmd"), json!({}));

        // Search finds a label, not the layout lines.
        let hits = p.search("job runner", false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["matches"].as_array().unwrap().len(), 1);
        assert!(p.search("@shape", false).is_empty());
        assert!(p.search("blockliner", false).is_empty());

        // Renaming keeps the extension and the content.
        let renamed = p.rename_file(id(&d), "Service Map").await.unwrap().unwrap();
        assert_eq!(renamed["filename"], "service-map.mmd");
        assert!(!dir.join("system-map.mmd").exists());
        assert_eq!(read(&dir, "service-map.mmd"), DIAGRAM);

        // A diagram can't become a presentation.
        assert!(p
            .set_presentation("service-map.mmd", &json!({ "theme": "x" }))
            .await
            .is_err());
        // Its tags are in the index only.
        let mut index = p.index.clone().unwrap();
        index["files"][0]["tags"] = json!(["arch", "ops"]);
        assert!(p.save_index(index).await.unwrap().is_empty());
        assert_eq!(read(&dir, "service-map.mmd"), DIAGRAM);

        // Reopening the project doesn't touch it either.
        p.close();
        p.open(&dir).await.unwrap();
        assert_eq!(read(&dir, "service-map.mmd"), DIAGRAM);
        assert_eq!(p.files()[0]["tags"], json!(["arch", "ops"]));
    }

    #[tokio::test]
    async fn diagrams_are_link_targets_but_hold_no_links() {
        let Fixture { mut p, .. } = project().await;
        let jobs = p
            .create_file("Job Runner", &json!([]), Some("jobs".into()), None)
            .await
            .unwrap();
        // An unquoted subroutine label reads exactly like a wikilink.
        let text = "flowchart LR\n  jobs[[Job Runner]]\n";
        assert_eq!(crate::links::parse_links(text), ["Job Runner"]);
        let d = p
            .create(
                FileKind::Diagram,
                "Map",
                &json!([]),
                Some(text.into()),
                None,
            )
            .await
            .unwrap();
        // A link finds a note by name, in any folder.
        let note = p
            .create_file(
                "Overview",
                &json!([]),
                Some("See [[Map]].".into()),
                Some("docs/deep"),
            )
            .await
            .unwrap();
        let mut links = crate::links::LinkGraph::default();
        links.rebuild(&p);
        // `jobs[["Job Runner"]]` in the diagram is a shape, not a link.
        assert!(links.backlink_snippets(&p, id(&jobs)).is_empty());
        let back = links.backlink_snippets(&p, id(&d));
        assert_eq!(back.len(), 1);
        assert_eq!(back[0]["sourceId"], note["id"]);
    }

    #[tokio::test]
    async fn note_lifecycle() {
        let Fixture {
            dir, trash, mut p, ..
        } = project().await;
        let a = p
            .create_file("First Note", &json!(["x"]), None, None)
            .await
            .unwrap();
        assert_eq!(a["filename"], "first-note.md");
        assert!(a.get("parentId").is_none() && a.get("order").is_none());
        // The block holds tags and created, and nothing else.
        let raw = read(&dir, "first-note.md");
        assert!(raw.starts_with("---\ntags:\n  - x\ncreated: '"), "{raw}");
        let block = frontmatter::split(&raw).unwrap().block;
        assert_eq!(block.lines().count(), 5, "{block}");
        for gone in ["id:", "name:", "updated:"] {
            assert!(!raw.contains(gone), "{gone} in {raw}");
        }

        // A taken name gets -2, ignoring case; the name shown doesn't change.
        let b = p
            .create_file(
                "FIRST note",
                &json!([]),
                Some("body [[First Note]]".into()),
                None,
            )
            .await
            .unwrap();
        assert_eq!(b["filename"], "first-note-2.md");
        assert_eq!(b["name"], "FIRST note");
        assert_eq!(
            p.read_body("first-note-2.md").unwrap(),
            "body [[First Note]]\n"
        );
        assert!(p.read_file("first-note-2.md").unwrap().starts_with("---\n"));

        // Search counts lines from the start of the text.
        let block = frontmatter::split(&raw).unwrap().block.to_string();
        p.write_file("first-note.md", &format!("{block}new body\nsecond LINE"))
            .await
            .unwrap();
        assert_eq!(
            p.read_body("first-note.md").unwrap(),
            "new body\nsecond LINE"
        );
        let hits = p.search("line", false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["matches"][0]["line"], 2);
        assert_eq!(hits[0]["folder"], "");
        assert!(p.search("line", true).is_empty());
        assert!(p.search("created", false).is_empty());

        let pres = p
            .set_presentation("first-note.md", &json!({ "theme": "dark" }))
            .await
            .unwrap();
        assert_eq!(pres, json!({ "theme": "dark" }));
        assert_eq!(p.find(id(&a)).unwrap()["deck"], true);
        assert_eq!(
            p.read_frontmatter("first-note.md")["presentation"]["theme"],
            "dark"
        );

        // A rename changes the index and the file's name, and nothing in it.
        let before = read(&dir, "first-note.md");
        let renamed = p.rename_file(id(&a), "Renamed").await.unwrap().unwrap();
        assert_eq!(renamed["filename"], "renamed.md");
        assert_eq!(read(&dir, "renamed.md"), before);
        // Renaming to a name another note has keeps both files.
        let again = p
            .rename_file(id(&a), "First Note 2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(again["filename"], "first-note-2-2.md");
        assert!(dir.join("first-note-2.md").exists());
        // A change of case alone keeps the file.
        let cased = p
            .rename_file(id(&a), "FIRST NOTE 2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cased["filename"], "first-note-2-2.md");
        assert_eq!(cased["name"], "FIRST NOTE 2");

        // Deleting sends the file to the trash.
        p.delete_file(id(&a), false).await.unwrap();
        assert_eq!(p.files().len(), 1);
        assert!(!dir.join("first-note-2-2.md").exists());
        assert!(trash.join("first-note-2-2.md").exists());
        assert!(p.read_file("../escape").is_err());
    }

    #[tokio::test]
    async fn saving_a_whole_file() {
        let Fixture { dir, mut p, .. } = project().await;
        let a = p
            .create_file("Note", &json!(["x"]), Some("one\n".into()), None)
            .await
            .unwrap();
        let created = a["createdAt"].as_str().unwrap().to_string();
        let mut index = p.index.clone().unwrap();
        index["tagPalette"] = json!(["x"]);
        p.save_index(index).await.unwrap();
        let tags = |p: &Project| p.files()[0]["tags"].clone();

        // A block with tags and created is saved byte for byte, comments,
        // spacing and unknown fields included; its tags reach the index.
        let typed = "---\n# mine\ntags: [x,   w]\ncreated: 2020-01-01\nid: old\nname: Old\nupdated: never\ncustom:  1\n---\ntext";
        let w = p.write_file("note.md", typed).await.unwrap();
        assert_eq!(w, Written::default());
        assert_eq!(read(&dir, "note.md"), typed);
        assert_eq!(tags(&p), json!(["x", "w"]));
        assert_eq!(p.index.as_ref().unwrap()["tagPalette"], json!(["x", "w"]));
        // The index's own id and name are untouched by the lines in the file.
        assert_eq!(p.files()[0]["id"], a["id"]);
        assert_eq!(p.files()[0]["name"], "Note");

        // A removed `tags` line is put back from the index; other fields stay.
        let w = p
            .write_file("note.md", "---\ncreated: 2020-01-01\ncustom: 1\n---\ntext")
            .await
            .unwrap();
        let rewritten = w.rewritten.expect("tags put back");
        assert_eq!(
            rewritten,
            "---\ntags:\n  - x\n  - w\ncreated: 2020-01-01T00:00:00.000Z\ncustom: 1\n---\ntext"
        );
        assert_eq!(read(&dir, "note.md"), rewritten);
        assert_eq!(tags(&p), json!(["x", "w"]));

        // A removed `created` line comes back with the note's date.
        let w = p
            .write_file("note.md", "---\ntags: []\n---\ntext\n")
            .await
            .unwrap();
        assert_eq!(
            w.rewritten.as_deref(),
            Some(format!("---\ncreated: '{created}'\ntags: []\n---\ntext\n").as_str())
        );
        assert_eq!(tags(&p), json!([]));

        // A deleted block comes back, with the note's tags and created date.
        p.write_file("note.md", "---\ntags: [z]\ncreated: 2020-01-01\n---\nt\n")
            .await
            .unwrap();
        let w = p.write_file("note.md", "just text\n").await.unwrap();
        assert_eq!(
            w.rewritten.as_deref(),
            Some(format!("---\ntags:\n  - z\ncreated: '{created}'\n---\njust text\n").as_str())
        );
        // `id`, `name` and `updated` are never written.
        let raw = read(&dir, "note.md");
        assert!(!raw.contains("id:") && !raw.contains("name:") && !raw.contains("updated:"));

        // A block that isn't YAML is saved as typed and changes nothing.
        let bad = "---\ntags: [a\ncreated: x\n---\nt\n";
        let w = p.write_file("note.md", bad).await.unwrap();
        assert!(w.unreadable && w.rewritten.is_none());
        assert_eq!(read(&dir, "note.md"), bad);
        assert_eq!(tags(&p), json!(["z"]));
        // NoteLiner doesn't rewrite a block it can't read.
        let mut index = p.index.clone().unwrap();
        index["files"][0]["tags"] = json!(["q"]);
        assert!(p.save_index(index).await.unwrap().is_empty());
        assert_eq!(read(&dir, "note.md"), bad);

        // The deck flag follows the block.
        p.write_file(
            "note.md",
            "---\ntags: []\ncreated: 2020-01-01\npresentation:\n  theme: dark\n---\nt\n",
        )
        .await
        .unwrap();
        assert_eq!(p.files()[0]["deck"], true);
        p.write_file("note.md", "---\ntags: []\ncreated: 2020-01-01\n---\nt\n")
            .await
            .unwrap();
        assert!(p.files()[0].get("deck").is_none());
    }

    #[tokio::test]
    async fn bodies_tags_and_the_setting() {
        let Fixture { dir, mut p, .. } = project().await;
        let a = p
            .create_file("Note", &json!([]), Some("one\n".into()), None)
            .await
            .unwrap();
        let block = frontmatter::split(&read(&dir, "note.md"))
            .unwrap()
            .block
            .to_string();

        // `write_body` replaces the text and keeps the block.
        p.write_body("note.md", "two").await.unwrap();
        assert_eq!(read(&dir, "note.md"), format!("{block}two\n"));
        assert_eq!(p.read_body("note.md").unwrap(), "two\n");

        // A tag change from the app rewrites the tags line and nothing else.
        let mut index = p.index.clone().unwrap();
        index["files"][0]["tags"] = json!(["work"]);
        index["files"][0]["name"] = json!("Not Taken");
        index["files"][0]["filename"] = json!("elsewhere.md");
        index["files"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "id": "ghost", "filename": "ghost.md" }));
        index["tagMeta"] = json!({ "work": { "color": "Tomato" } });
        assert_eq!(p.save_index(index).await.unwrap(), ["note.md"]);
        let raw = read(&dir, "note.md");
        assert!(raw.starts_with("---\ntags:\n  - work\ncreated: '"), "{raw}");
        assert!(raw.ends_with("---\ntwo\n"));
        // What exists and where is never taken from the app.
        assert_eq!(p.files().len(), 1);
        assert_eq!(p.files()[0]["name"], "Note");
        assert_eq!(p.files()[0]["filename"], "note.md");
        assert_eq!(
            p.index.as_ref().unwrap()["tagMeta"]["work"]["color"],
            "Tomato"
        );

        // With the setting off nothing is added to a block, or read from it.
        p.write_frontmatter = false;
        let w = p
            .write_file("note.md", "---\ntags: [other]\n---\nplain\n")
            .await
            .unwrap();
        assert_eq!(w, Written::default());
        assert_eq!(read(&dir, "note.md"), "---\ntags: [other]\n---\nplain\n");
        assert_eq!(p.find(id(&a)).unwrap()["tags"], json!(["work"]));
        let w = p.write_file("note.md", "plain\n").await.unwrap();
        assert_eq!(w, Written::default());
        assert_eq!(read(&dir, "note.md"), "plain\n");
        let mut index = p.index.clone().unwrap();
        index["files"][0]["tags"] = json!(["later"]);
        assert!(p.save_index(index).await.unwrap().is_empty());
        assert_eq!(read(&dir, "note.md"), "plain\n");
        let b = p
            .create_file("Bare", &json!(["t"]), Some("b\n".into()), None)
            .await
            .unwrap();
        assert_eq!(read(&dir, b["filename"].as_str().unwrap()), "b\n");
    }

    #[tokio::test]
    async fn folders() {
        let Fixture {
            dir, trash, mut p, ..
        } = project().await;
        // Create, with the name rules.
        assert_eq!(p.create_folder("Work").await.unwrap(), "Work");
        assert_eq!(
            p.create_folder("Work/Meetings").await.unwrap(),
            "Work/Meetings"
        );
        assert!(dir.join("Work/Meetings").is_dir());
        assert_eq!(p.folders(), ["Work", "Work/Meetings"]);
        for bad in [
            "",
            " ",
            ".hidden",
            "_x",
            "a:b",
            "../up",
            "Work/../x",
            "work",
            "WORK/meetings",
        ] {
            assert!(p.create_folder(bad).await.is_err(), "{bad}");
        }

        // A new note lands in its folder; a folder that isn't there is made.
        let a = p
            .create_file("Kickoff Notes", &json!(["m"]), None, Some("Work/Meetings"))
            .await
            .unwrap();
        assert_eq!(a["filename"], "Work/Meetings/kickoff-notes.md");
        let d = p
            .create(FileKind::Diagram, "Map", &json!([]), None, Some("Work"))
            .await
            .unwrap();
        let top = p
            .create_file("Kickoff Notes", &json!([]), None, None)
            .await
            .unwrap();
        assert_eq!(top["filename"], "kickoff-notes.md");
        p.create_file("Deep", &json!([]), None, Some("Archive/2025"))
            .await
            .unwrap();
        assert_eq!(
            p.folders(),
            ["Archive", "Archive/2025", "Work", "Work/Meetings"]
        );
        assert!(p
            .create_file("X", &json!([]), None, Some("../out"))
            .await
            .is_err());
        assert!(p
            .create_file("X", &json!([]), None, Some("_attachments"))
            .await
            .is_err());

        // Rename: the entries under it follow.
        assert!(p.rename_folder("Work", "Archive").await.is_err());
        assert!(p.rename_folder("Work", "a/b").await.is_err());
        assert_eq!(
            p.rename_folder("Work", "Projects").await.unwrap(),
            "Projects"
        );
        assert_eq!(
            p.find(id(&a)).unwrap()["filename"],
            "Projects/Meetings/kickoff-notes.md"
        );
        assert_eq!(p.find(id(&d)).unwrap()["filename"], "Projects/map.mmd");
        assert!(dir.join("Projects/Meetings/kickoff-notes.md").exists());
        assert!(!dir.join("Work").exists());
        // A change of case alone is a rename.
        assert_eq!(
            p.rename_folder("Projects", "projects").await.unwrap(),
            "projects"
        );
        assert_eq!(
            p.rename_folder("projects", "Projects").await.unwrap(),
            "Projects"
        );

        // Move: not into itself, and not onto a folder of that name.
        assert!(p.move_folder("Projects", "Projects").await.is_err());
        assert!(p
            .move_folder("Projects", "Projects/Meetings")
            .await
            .is_err());
        p.create_folder("Archive/Meetings").await.unwrap();
        assert!(p.move_folder("Projects/Meetings", "Archive").await.is_err());
        assert_eq!(
            p.move_folder("Projects/Meetings", "Archive/2025")
                .await
                .unwrap(),
            "Archive/2025/Meetings"
        );
        assert_eq!(
            p.find(id(&a)).unwrap()["filename"],
            "Archive/2025/Meetings/kickoff-notes.md"
        );
        // To the top.
        assert_eq!(
            p.move_folder("Archive/2025/Meetings", "").await.unwrap(),
            "Meetings"
        );
        assert_eq!(
            p.find(id(&a)).unwrap()["filename"],
            "Meetings/kickoff-notes.md"
        );

        // Moving a note: a file name taken in the target gets a suffix.
        let moved = p.move_file(id(&top), "Meetings").await.unwrap().unwrap();
        assert_eq!(moved["filename"], "Meetings/kickoff-notes-2.md");
        assert_eq!(moved["name"], "Kickoff Notes");
        assert_eq!(moved["id"], top["id"]);
        assert!(dir.join("Meetings/kickoff-notes-2.md").exists());
        assert!(!dir.join("kickoff-notes.md").exists());
        let back = p.move_file(id(&top), "").await.unwrap().unwrap();
        assert_eq!(back["filename"], "kickoff-notes-2.md");

        // Delete: the folder goes to the trash and its notes leave the index.
        fs::write(dir.join("Meetings/agenda.pdf"), "x").unwrap();
        assert_eq!(
            p.folder_counts("Meetings"),
            FolderCounts {
                notes: 1,
                diagrams: 0,
                others: 1
            }
        );
        assert_eq!(
            p.folder_counts("Projects"),
            FolderCounts {
                notes: 0,
                diagrams: 1,
                others: 0
            }
        );
        let removed = p.delete_folder("Meetings", false).await.unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0]["id"], a["id"]);
        assert!(p.find(id(&a)).is_none());
        assert!(!dir.join("Meetings").exists());
        assert!(trash.join("Meetings/kickoff-notes.md").exists());
        assert!(p.delete_folder("", false).await.is_err());
        assert!(!p.folders().iter().any(|f| f == "Meetings"));

        // The index on disk matches.
        let on_disk: Value = serde_json::from_str(&read(&dir, "noteliner.json")).unwrap();
        assert_eq!(Some(&on_disk), p.index.as_ref());
    }

    #[tokio::test]
    async fn attachments_live_beside_their_note() {
        let Fixture {
            dir, trash, mut p, ..
        } = project().await;
        let top = p.create_file("Top", &json!([]), None, None).await.unwrap();
        let note = p
            .create_file("Note", &json!([]), None, Some("Work"))
            .await
            .unwrap();
        let t = p.add_attachment(id(&top), b"TOP", "top.txt").await.unwrap();
        let att = p
            .add_attachment(id(&note), b"PNGDATA", "pic.PNG")
            .await
            .unwrap();
        assert_eq!(att["mimeType"], "image/png");
        let stored = att["filename"].as_str().unwrap().to_string();
        // A note at the top uses `_attachments/` at the top.
        assert!(dir
            .join("_attachments")
            .join(t["filename"].as_str().unwrap())
            .exists());
        assert!(dir.join("Work/_attachments").join(&stored).exists());
        assert_eq!(
            p.attachment_path("Work/note.md", &stored).unwrap(),
            dir.join("Work/_attachments").join(&stored)
        );
        assert_eq!(
            p.find_attachment(&stored),
            Some(dir.join("Work/_attachments").join(&stored))
        );
        assert_eq!(p.find_attachment("att-nope.png"), None);
        // `_attachments` is not a folder of the tree.
        assert_eq!(p.folders(), ["Work"]);

        // Moving the note moves its attachment; the empty folder goes.
        p.move_file(id(&note), "Archive").await.unwrap();
        assert!(dir.join("Archive/_attachments").join(&stored).exists());
        assert!(!dir.join("Work/_attachments").exists());
        // Moving or renaming a folder carries it, since it is inside.
        p.rename_folder("Archive", "Old").await.unwrap();
        assert!(dir.join("Old/_attachments").join(&stored).exists());
        assert_eq!(
            p.find_attachment(&stored),
            Some(dir.join("Old/_attachments").join(&stored))
        );

        // Removing one sends it to the trash.
        let second = p
            .add_attachment(id(&note), b"TWO", "two.txt")
            .await
            .unwrap();
        p.remove_attachment(id(&note), second["id"].as_str().unwrap())
            .await
            .unwrap();
        assert!(trash.join(second["filename"].as_str().unwrap()).exists());
        assert_eq!(
            p.find(id(&note)).unwrap()["attachments"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        // Deleting the note trashes its attachments with it.
        p.delete_file(id(&note), false).await.unwrap();
        assert!(trash.join(&stored).exists());
        assert!(!dir.join("Old/_attachments").exists());
        assert!(dir
            .join("_attachments")
            .join(t["filename"].as_str().unwrap())
            .exists());
    }

    #[tokio::test]
    async fn the_trash_can_be_unavailable() {
        let Fixture { dir, mut p, .. } = project().await;
        p.trash = Arc::new(|_| Err(anyhow::Error::new(TrashUnavailable("no trash".into()))));
        let a = p
            .create_file("Note", &json!([]), None, Some("Work"))
            .await
            .unwrap();
        // Nothing is removed until the app asks and deletes for good.
        let err = p.delete_file(id(&a), false).await.unwrap_err();
        assert!(err.downcast_ref::<TrashUnavailable>().is_some());
        assert!(dir.join("Work/note.md").exists());
        assert_eq!(p.files().len(), 1);
        assert!(p.delete_folder("Work", false).await.is_err());
        assert!(dir.join("Work").exists());

        p.delete_file(id(&a), true).await.unwrap();
        assert!(!dir.join("Work/note.md").exists());
        assert!(p.files().is_empty());
        p.delete_folder("Work", true).await.unwrap();
        assert!(!dir.join("Work").exists());
    }

    /// Lets a folder's modification time move on, on file systems that keep
    /// it to the second.
    fn touch_dir(dir: &Path) {
        let later = SystemTime::now() + std::time::Duration::from_secs(5);
        let f = fs::File::open(dir).unwrap();
        f.set_modified(later).unwrap();
    }

    #[tokio::test]
    async fn changes_made_outside() {
        let Fixture { dir, mut p, .. } = project().await;
        let a = p
            .create_file("Kickoff Notes", &json!(["m"]), None, None)
            .await
            .unwrap();
        let d = p
            .create(FileKind::Diagram, "Map", &json!(["arch"]), None, None)
            .await
            .unwrap();
        let att = p.add_attachment(id(&a), b"X", "x.png").await.unwrap();
        let stored = att["filename"].as_str().unwrap().to_string();
        // Nothing changed: the check is quiet, twice.
        p.rescan().unwrap();
        assert!(p.rescan().unwrap().is_none());

        // A folder and a note added by another program appear.
        fs::create_dir_all(dir.join("Inbox")).unwrap();
        fs::write(
            dir.join("Inbox/from-phone.md"),
            "---\ntags: [inbox]\n---\nhello\n",
        )
        .unwrap();
        touch_dir(&dir);
        let c = p.rescan().unwrap().expect("changes");
        assert_eq!(c.added.len(), 1);
        assert_eq!(p.folders(), ["Inbox"]);
        let new = p.find_by_filename("Inbox/from-phone.md").unwrap().clone();
        assert_eq!(new["name"], "from-phone");
        assert_eq!(new["tags"], json!(["inbox"]));
        // Opening changed no note.
        assert_eq!(
            read(&dir, "Inbox/from-phone.md"),
            "---\ntags: [inbox]\n---\nhello\n"
        );
        assert!(p.rescan().unwrap().is_none());

        // A note moved without its attachments: NoteLiner moves them.
        fs::rename(
            dir.join("kickoff-notes.md"),
            dir.join("Inbox/kickoff-notes.md"),
        )
        .unwrap();
        fs::rename(dir.join("map.mmd"), dir.join("Inbox/map.mmd")).unwrap();
        touch_dir(&dir);
        touch_dir(&dir.join("Inbox"));
        let c = p.rescan().unwrap().expect("changes");
        assert_eq!(c.moved.len(), 2);
        assert!(c.added.is_empty() && c.removed.is_empty());
        let moved = p.find(id(&a)).unwrap();
        assert_eq!(moved["filename"], "Inbox/kickoff-notes.md");
        assert_eq!(moved["name"], "Kickoff Notes");
        assert_eq!(moved["tags"], json!(["m"]));
        assert!(dir.join("Inbox/_attachments").join(&stored).exists());
        assert!(!dir.join("_attachments").exists());
        // A moved diagram keeps its name and tags too.
        assert_eq!(p.find(id(&d)).unwrap()["filename"], "Inbox/map.mmd");
        assert_eq!(p.find(id(&d)).unwrap()["tags"], json!(["arch"]));

        // A folder renamed outside: its files are matched one by one.
        fs::rename(dir.join("Inbox"), dir.join("Sorted")).unwrap();
        touch_dir(&dir);
        let c = p.rescan().unwrap().expect("changes");
        assert_eq!(c.moved.len(), 3);
        assert_eq!(p.folders(), ["Sorted"]);
        assert_eq!(
            p.find(id(&a)).unwrap()["filename"],
            "Sorted/kickoff-notes.md"
        );

        // A diagram renamed outside is matched by its size, in one check.
        fs::rename(dir.join("Sorted/map.mmd"), dir.join("Sorted/big-map.mmd")).unwrap();
        touch_dir(&dir.join("Sorted"));
        let c = p.rescan().unwrap().expect("changes");
        assert_eq!(c.moved.len(), 1);
        let renamed = p.find(id(&d)).unwrap();
        assert_eq!(renamed["name"], "big-map");
        assert_eq!(renamed["tags"], json!(["arch"]));

        // A note removed outside leaves the index.
        fs::remove_file(dir.join("Sorted/from-phone.md")).unwrap();
        touch_dir(&dir.join("Sorted"));
        let c = p.rescan().unwrap().expect("changes");
        assert_eq!(c.removed.len(), 1);
        assert_eq!(c.removed[0]["id"], new["id"]);
        assert!(p.find(id(&new)).is_none());

        // `noteliner.json` changed outside is read again.
        let mut index = p.index.clone().unwrap();
        index["tagMeta"] = json!({ "m": { "color": "Tomato" } });
        laankit_core::json::write(&dir.join("noteliner.json"), &index).unwrap();
        let f = fs::File::options()
            .write(true)
            .open(dir.join("noteliner.json"))
            .unwrap();
        f.set_modified(SystemTime::now() + std::time::Duration::from_secs(9))
            .unwrap();
        drop(f);
        assert!(p.rescan().unwrap().is_some());
        assert_eq!(p.index.as_ref().unwrap()["tagMeta"]["m"]["color"], "Tomato");
        assert!(p.rescan().unwrap().is_none());
    }
}
