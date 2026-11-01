//! A NoteLiner project: a git repo holding `noteliner.json` (the index),
//! Markdown notes, `_attachments/`, and `_templates/`. Port of
//! `apps/noteliner/src/main/project-service.js`.
//!
//! The index is kept as raw JSON so fields the renderer adds survive
//! round-trips, and is written exactly as `JSON.stringify(index, null, 2)`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use crate::frontmatter::{self, Data};
use crate::git::NoteGit;
use crate::yaml::Yaml;

pub const INDEX_FILE: &str = "noteliner.json";
pub const ATTACHMENTS_DIR: &str = "_attachments";
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

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

pub struct Project {
    pub path: Option<PathBuf>,
    pub index: Option<Value>,
    pub write_frontmatter: bool,
    pub git: NoteGit,
}

impl Project {
    pub fn new(git: NoteGit) -> Self {
        Self {
            path: None,
            index: None,
            write_frontmatter: true,
            git,
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

    /// Resolve a renderer-supplied project-relative filename, refusing
    /// anything that would escape the project folder.
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

    pub fn write_index(&self) -> Result<()> {
        let index = self
            .index
            .as_ref()
            .ok_or_else(|| anyhow!("No project is open"))?;
        laankit_core::json::write(&self.index_path()?, index)
    }

    async fn commit_and_schedule(&self, message: &str) -> Result<()> {
        let dir = self.dir()?.to_path_buf();
        self.git.commit(&dir, message).await?;
        self.git.schedule_push(&dir);
        Ok(())
    }

    // --- Open / init ------------------------------------------------------

    pub async fn open(&mut self, folder: &Path) -> Result<Value> {
        self.path = Some(folder.to_path_buf());
        let is_git = self.git.is_repo(folder).await;
        let has_index = folder.join(INDEX_FILE).exists();

        if is_git && has_index {
            self.git.pull(folder).await;
            let raw = fs::read_to_string(folder.join(INDEX_FILE))?;
            self.index = Some(serde_json::from_str(&raw)?);
            self.migrate_index()?;
            let needs_git_config = self.check_git_config().await;
            if !needs_git_config {
                let rewritten = self.reconcile_frontmatter(true)?;
                if rewritten > 0 {
                    let noun = if rewritten == 1 { "note" } else { "notes" };
                    self.commit_and_schedule(&format!("Sync frontmatter on {rewritten} {noun}"))
                        .await?;
                }
            } else {
                self.reconcile_frontmatter(false)?;
            }
            return Ok(
                json!({ "status": "loaded", "index": self.index, "needsGitConfig": needs_git_config }),
            );
        }

        if is_git {
            self.index = Some(json!({ "version": 2, "files": [] }));
            self.write_index()?;
            let needs_git_config = self.check_git_config().await;
            if !needs_git_config {
                self.git.commit(folder, "Initialize noteliner.json").await?;
            }
            return Ok(
                json!({ "status": "loaded", "index": self.index, "needsGitConfig": needs_git_config }),
            );
        }

        Ok(json!({ "status": "needs_setup" }))
    }

    pub async fn init(&mut self, folder: &Path, remote_url: Option<&str>) -> Result<Value> {
        self.path = Some(folder.to_path_buf());
        if let Some(url) = remote_url.filter(|u| !u.is_empty()) {
            self.git.git.clone_here(url, folder).await?;
            if folder.join(INDEX_FILE).exists() {
                let raw = fs::read_to_string(folder.join(INDEX_FILE))?;
                self.index = Some(serde_json::from_str(&raw)?);
                self.migrate_index()?;
            } else {
                self.index = Some(json!({ "version": 2, "files": [] }));
                self.write_index()?;
                self.git.commit(folder, "Initialize noteliner.json").await?;
                self.git.push(folder).await;
            }
        } else {
            self.git.git.init(folder).await?;
            self.index = Some(json!({ "version": 2, "files": [] }));
            self.write_index()?;
            self.git.commit(folder, "Initialize noteliner.json").await?;
        }
        let needs_git_config = self.check_git_config().await;
        Ok(json!({ "status": "loaded", "index": self.index, "needsGitConfig": needs_git_config }))
    }

    pub fn close(&mut self) {
        self.path = None;
        self.index = None;
    }

    pub async fn check_git_config(&self) -> bool {
        let Ok(dir) = self.dir() else { return true };
        let (name, email) = self.git.check_config(dir).await;
        name.is_none_or(|n| n.is_empty()) || email.is_none_or(|e| e.is_empty())
    }

    pub async fn get_git_config(&self) -> Value {
        let Ok(dir) = self.dir() else {
            return json!({ "name": null, "email": null });
        };
        let (name, email) = self.git.check_config(dir).await;
        json!({ "name": name, "email": email })
    }

    pub async fn set_git_config(&self, name: &str, email: &str) -> Result<()> {
        let dir = self.dir()?;
        self.git.git.set_config(dir, "user.name", name).await?;
        self.git.git.set_config(dir, "user.email", email).await?;
        Ok(())
    }

    // --- Index ------------------------------------------------------------

    pub async fn save_index(&mut self, index: Value) -> Result<()> {
        let old: Vec<Value> = self.files().to_vec();
        self.index = Some(index);
        self.write_index()?;
        if self.write_frontmatter {
            let files = self.files().to_vec();
            for entry in &files {
                let prev = old.iter().find(|o| o["filename"] == entry["filename"]);
                if prev.is_none_or(|p| mirror_fields_differ(p, entry)) {
                    self.rewrite_frontmatter(entry)?;
                }
            }
        }
        self.commit_and_schedule("Update index").await
    }

    fn migrate_index(&mut self) -> Result<()> {
        let dir = self.dir()?.to_path_buf();
        let version = self
            .index
            .as_ref()
            .and_then(|i| i["version"].as_u64())
            .unwrap_or(0);
        let mut dirty = false;
        if version < 2 {
            for f in self.files_mut()?.iter_mut() {
                if f.get("attachments").is_none_or(|a| a.is_null()) {
                    f["attachments"] = json!([]);
                }
            }
            self.index.as_mut().unwrap()["version"] = json!(2);
            dirty = true;
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
            self.index.as_mut().unwrap()["version"] = json!(3);
            dirty = true;
        }
        if dirty {
            self.write_index()?;
        }
        Ok(())
    }

    /// Refresh each note's derived `deck` flag and, when `rewrite`, any
    /// frontmatter mirror that drifted from the index. Returns rewrites.
    pub fn reconcile_frontmatter(&mut self, rewrite: bool) -> Result<usize> {
        let can_rewrite = rewrite && self.write_frontmatter;
        let dir = self.dir()?.to_path_buf();
        let mut rewritten = 0;
        let n = self.files().len();
        for i in 0..n {
            let entry = self.files()[i].clone();
            let path = dir.join(s(&entry["filename"]));
            let Ok(raw) = fs::read_to_string(&path) else {
                continue;
            };
            let parsed = frontmatter::parse(&raw);
            apply_deck_flag(&mut self.files_mut()?[i], &parsed.data);
            if !can_rewrite {
                continue;
            }
            let wanted = frontmatter::mirror_from_entry(&entry, &parsed.data);
            if frontmatter::mirror_diverges(&parsed.data, &wanted) {
                let next = frontmatter::serialize(&parsed.body, &wanted);
                if next != raw {
                    fs::write(&path, next)?;
                    rewritten += 1;
                }
            }
        }
        Ok(rewritten)
    }

    // --- Notes --------------------------------------------------------------

    pub fn read_file(&self, filename: &str) -> Result<String> {
        let path = self.file_path(filename)?;
        match fs::read_to_string(path) {
            Ok(raw) => Ok(frontmatter::strip_body(&raw)),
            Err(_) => Ok(String::new()),
        }
    }

    pub fn read_frontmatter(&self, filename: &str) -> Value {
        match self
            .file_path(filename)
            .ok()
            .and_then(|p| fs::read_to_string(p).ok())
        {
            Some(raw) => frontmatter::to_json(&frontmatter::parse(&raw).data),
            None => json!({}),
        }
    }

    pub async fn write_file(&mut self, filename: &str, body: &str) -> Result<()> {
        let path = self.file_path(filename)?;
        let pos = self.position_by_filename(filename);
        let mut out = body.to_string();
        if self.write_frontmatter {
            let existing = fs::read_to_string(&path)
                .map(|r| frontmatter::parse(&r).data)
                .unwrap_or_default();
            let data = match pos {
                Some(i) => frontmatter::mirror_from_entry(&self.files()[i], &existing),
                None => existing,
            };
            out = frontmatter::serialize(body, &data);
            if let Some(i) = pos {
                apply_deck_flag(&mut self.files_mut()?[i], &data);
            }
        }
        fs::write(&path, out)?;
        if let Some(i) = pos {
            self.files_mut()?[i]["modifiedAt"] = json!(now_iso());
            let _ = self.write_index();
        }
        self.commit_and_schedule(&format!("Update {filename}"))
            .await
    }

    /// Refresh only the on-disk frontmatter for `entry`, keeping the body.
    pub fn rewrite_frontmatter(&self, entry: &Value) -> Result<bool> {
        if !self.write_frontmatter {
            return Ok(false);
        }
        let path = self.file_path(s(&entry["filename"]))?;
        let Ok(raw) = fs::read_to_string(&path) else {
            return Ok(false);
        };
        let parsed = frontmatter::parse(&raw);
        let data = frontmatter::mirror_from_entry(entry, &parsed.data);
        let next = frontmatter::serialize(&parsed.body, &data);
        if next != raw {
            fs::write(&path, next)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub async fn create_file(
        &mut self,
        name: &str,
        tags: &Value,
        body: Option<String>,
        parent_id: Option<&str>,
    ) -> Result<Value> {
        if name.trim().is_empty() {
            bail!("File name cannot be empty");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let filename = self.unique_filename(&format!("{}.md", slugify(name)))?;
        let parent = parent_id
            .filter(|p| self.find(p).is_some())
            .map(str::to_string);
        let order = self
            .files()
            .iter()
            .filter(|f| f["parentId"].as_str() == parent.as_deref())
            .count();
        let now = now_iso();
        let mut entry = json!({
            "id": id,
            "name": name,
            "filename": filename,
            "parentId": parent,
            "order": order,
            "tags": if tags.is_array() { tags.clone() } else { json!([]) },
            "attachments": [],
            "createdAt": now,
            "modifiedAt": now,
        });
        let path = self.file_path(&filename)?;
        let initial = body
            .unwrap_or_else(|| format!("# {name} {}\n", chrono::Local::now().format("%Y-%m-%d")));
        if self.write_frontmatter {
            // A template may carry its own frontmatter (a deck template's
            // `presentation:`); keep it, minus the template-only `kind`.
            let seeded = frontmatter::parse(&initial);
            let mut template_data: Data = seeded.data;
            frontmatter::remove(&mut template_data, "kind");
            let data = frontmatter::mirror_from_entry(&entry, &template_data);
            fs::write(&path, frontmatter::serialize(&seeded.body, &data))?;
            apply_deck_flag(&mut entry, &data);
        } else {
            fs::write(&path, &initial)?;
        }
        self.files_mut()?.push(entry.clone());
        self.write_index()?;
        self.commit_and_schedule(&format!("Add {name}")).await?;
        Ok(entry)
    }

    pub async fn delete_file(&mut self, id: &str) -> Result<()> {
        let Some(entry) = self.find(id).cloned() else {
            return Ok(());
        };
        let _ = fs::remove_file(self.file_path(s(&entry["filename"]))?);
        for f in self.files_mut()?.iter_mut() {
            if f["parentId"].as_str() == Some(id) {
                f["parentId"] = entry["parentId"].clone();
            }
        }
        self.files_mut()?.retain(|f| s(&f["id"]) != id);
        self.write_index()?;
        self.commit_and_schedule(&format!("Delete {}", s(&entry["name"])))
            .await
    }

    pub async fn rename_file(&mut self, id: &str, new_name: &str) -> Result<Option<Value>> {
        if new_name.trim().is_empty() {
            return Ok(None);
        }
        let Some(i) = self.position(id) else {
            return Ok(None);
        };
        let old_filename = s(&self.files()[i]["filename"]).to_string();
        let new_filename = format!("{}.md", slugify(new_name));
        let (old_path, new_path) = (
            self.file_path(&old_filename)?,
            self.file_path(&new_filename)?,
        );
        if old_path.exists() {
            fs::rename(old_path, new_path)?;
        }
        {
            let f = &mut self.files_mut()?[i];
            f["name"] = json!(new_name);
            f["filename"] = json!(new_filename);
        }
        self.write_index()?;
        let entry = self.files()[i].clone();
        self.rewrite_frontmatter(&entry)?;
        self.commit_and_schedule(&format!("Rename {old_filename} to {new_filename}"))
            .await?;
        Ok(Some(entry))
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
        let parsed = frontmatter::parse(&fs::read_to_string(&path)?);
        let mut next = parsed.data;
        if presentation.is_null() {
            frontmatter::remove(&mut next, "presentation");
        } else {
            frontmatter::set(&mut next, "presentation", Yaml::from_json(presentation));
        }
        fs::write(&path, frontmatter::serialize(&parsed.body, &next))?;
        if let Some(i) = self.position_by_filename(filename) {
            let f = &mut self.files_mut()?[i];
            apply_deck_flag(f, &next);
            f["modifiedAt"] = json!(now_iso());
            let _ = self.write_index();
        }
        let verb = if presentation.is_null() {
            "Convert to note"
        } else {
            "Update presentation settings"
        };
        self.commit_and_schedule(&format!("{verb}: {filename}"))
            .await?;
        Ok(frontmatter::get_field(&next, "presentation")
            .map(Yaml::to_json)
            .unwrap_or(Value::Null))
    }

    // --- Attachments --------------------------------------------------------

    pub fn attachments_dir(&self) -> Result<PathBuf> {
        Ok(self.dir()?.join(ATTACHMENTS_DIR))
    }

    pub fn attachment_path(&self, filename: &str) -> Result<PathBuf> {
        Ok(self.attachments_dir()?.join(filename))
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
        let id = uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap()
            .to_string();
        let stored = format!("att-{id}{ext}");
        let dir = self.attachments_dir()?;
        fs::create_dir_all(&dir)?;
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
        self.commit_and_schedule(&format!("Attach {original_name}"))
            .await?;
        Ok(attachment)
    }

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
        let att = atts[k].clone();
        let _ = fs::remove_file(self.attachments_dir()?.join(s(&att["filename"])));
        self.files_mut()?[i]["attachments"]
            .as_array_mut()
            .unwrap()
            .remove(k);
        self.write_index()?;
        self.commit_and_schedule(&format!("Remove attachment {}", s(&att["originalName"])))
            .await
    }

    // --- Search -------------------------------------------------------------

    /// Line-level substring search over note bodies (frontmatter excluded).
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
            let body = frontmatter::strip_body(&raw);
            let matches: Vec<Value> = body
                .split('\n')
                .enumerate()
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
                    "matches": matches,
                }));
            }
        }
        results
    }

    fn unique_filename(&self, filename: &str) -> Result<String> {
        let dir = self.dir()?;
        let taken =
            |c: &str| self.files().iter().any(|f| s(&f["filename"]) == c) || dir.join(c).exists();
        if !taken(filename) {
            return Ok(filename.to_string());
        }
        let ext = extname(filename);
        let stem = &filename[..filename.len() - ext.len()];
        for i in 2..10000 {
            let candidate = format!("{stem}-{i}{ext}");
            if !taken(&candidate) {
                return Ok(candidate);
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

fn mirror_fields_differ(a: &Value, b: &Value) -> bool {
    if a["name"] != b["name"] {
        return true;
    }
    let tags = |v: &Value| v["tags"].as_array().cloned().unwrap_or_default();
    tags(a) != tags(b)
}

fn stat_times(path: &Path) -> (String, String) {
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

#[cfg(test)]
impl Project {
    async fn set_git_config_for_test(&mut self, dir: &Path) {
        self.path = Some(dir.to_path_buf());
        self.set_git_config("Test", "test@localhost").await.unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        let git = NoteGit::new(Arc::new(|_| {}));
        (dir, Project::new(git))
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("  Été 2026 "), "t-2026");
        assert_eq!(extname("a/b.tar.gz"), ".gz");
        assert_eq!(extname(".bashrc"), "");
    }

    #[tokio::test]
    async fn note_lifecycle() {
        let (tmp, mut p) = project();
        let dir = tmp.path().to_path_buf();
        // Global identity may exist on dev machines; force a local one.
        p.git.git.init(&dir).await.unwrap();
        p.set_git_config_for_test(&dir).await;
        let res = p.open(&dir).await.unwrap();
        assert_eq!(res["status"], "loaded");

        let a = p
            .create_file("First Note", &json!(["x"]), None, None)
            .await
            .unwrap();
        assert_eq!(a["filename"], "first-note.md");
        let raw = fs::read_to_string(dir.join("first-note.md")).unwrap();
        assert!(raw.starts_with("---\nid: "));
        assert!(raw.contains("\nname: First Note\ntags:\n  - x\ncreated: '"));

        let b = p
            .create_file(
                "First Note",
                &json!([]),
                Some("body [[First Note]]".into()),
                Some(a["id"].as_str().unwrap()),
            )
            .await
            .unwrap();
        assert_eq!(b["filename"], "first-note-2.md");
        assert_eq!(b["parentId"], a["id"]);
        assert_eq!(
            p.read_file("first-note-2.md").unwrap(),
            "body [[First Note]]\n"
        );

        p.write_file("first-note.md", "new body\nsecond LINE")
            .await
            .unwrap();
        assert_eq!(
            p.read_file("first-note.md").unwrap(),
            "new body\nsecond LINE\n"
        );
        let hits = p.search("line", false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["matches"][0]["line"], 2);
        assert!(p.search("line", true).is_empty());

        let pres = p
            .set_presentation("first-note.md", &json!({ "theme": "dark" }))
            .await
            .unwrap();
        assert_eq!(pres, json!({ "theme": "dark" }));
        assert_eq!(p.find(a["id"].as_str().unwrap()).unwrap()["deck"], true);
        assert_eq!(
            p.read_frontmatter("first-note.md")["presentation"]["theme"],
            "dark"
        );

        let att = p
            .add_attachment(a["id"].as_str().unwrap(), b"PNGDATA", "pic.PNG")
            .await
            .unwrap();
        assert_eq!(att["mimeType"], "image/png");
        assert!(p
            .attachment_path(att["filename"].as_str().unwrap())
            .unwrap()
            .exists());
        p.remove_attachment(a["id"].as_str().unwrap(), att["id"].as_str().unwrap())
            .await
            .unwrap();

        let renamed = p
            .rename_file(a["id"].as_str().unwrap(), "Renamed")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(renamed["filename"], "renamed.md");
        assert!(fs::read_to_string(dir.join("renamed.md"))
            .unwrap()
            .contains("name: Renamed"));

        p.delete_file(a["id"].as_str().unwrap()).await.unwrap();
        assert_eq!(p.files().len(), 1);
        assert_eq!(p.files()[0]["parentId"], Value::Null);
        assert!(p.read_file("../escape").is_err());

        let log = p.git.file_log(&dir, "first-note-2.md").await;
        assert_eq!(log.len(), 1);
    }
}
