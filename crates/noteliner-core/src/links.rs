//! Wikilink graph (`[[Note Name]]` / `[[Note Name|label]]`). Port of
//! `apps/noteliner/src/main/link-graph-service.js`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Value};

use crate::frontmatter;
use crate::project::Project;

pub static WIKILINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\[\]|]+)(?:\|([^\[\]]+))?\]\]").unwrap());

#[derive(Default)]
pub struct LinkGraph {
    outgoing: HashMap<String, HashSet<String>>,
    incoming: HashMap<String, HashSet<String>>,
    dangling: HashMap<String, HashSet<String>>,
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn name_index(p: &Project) -> HashMap<String, String> {
    p.files()
        .iter()
        .map(|f| (s(&f["name"]).to_lowercase(), s(&f["id"]).to_string()))
        .collect()
}

fn body_of(p: &Project, filename: &str) -> Option<String> {
    let path = p.file_path(filename).ok()?;
    fs::read_to_string(path)
        .ok()
        .map(|raw| frontmatter::strip_body(&raw))
}

pub fn parse_links(content: &str) -> Vec<String> {
    WIKILINK_RE
        .captures_iter(content)
        .map(|c| c[1].trim().to_string())
        .collect()
}

impl LinkGraph {
    pub fn reset(&mut self) {
        self.outgoing.clear();
        self.incoming.clear();
        self.dangling.clear();
    }

    pub fn rebuild(&mut self, p: &Project) {
        self.reset();
        if !p.is_open() {
            return;
        }
        let names = name_index(p);
        for f in p.files() {
            if let Some(body) = body_of(p, s(&f["filename"])) {
                self.apply(s(&f["id"]), &body, &names);
            }
        }
    }

    fn apply(&mut self, file_id: &str, content: &str, names: &HashMap<String, String>) {
        if let Some(prev) = self.outgoing.get(file_id) {
            for target in prev.clone() {
                if let Some(set) = self.incoming.get_mut(&target) {
                    set.remove(file_id);
                }
            }
        }
        let mut out = HashSet::new();
        let mut dangling = HashSet::new();
        for name in parse_links(content) {
            match names.get(&name.to_lowercase()) {
                None => {
                    dangling.insert(name);
                }
                Some(target) if target != file_id => {
                    out.insert(target.clone());
                }
                _ => {}
            }
        }
        for target in &out {
            self.incoming
                .entry(target.clone())
                .or_default()
                .insert(file_id.to_string());
        }
        self.outgoing.insert(file_id.to_string(), out);
        self.dangling.insert(file_id.to_string(), dangling);
    }

    pub fn scan_file(&mut self, p: &Project, file_id: &str) {
        let Some(f) = p.find(file_id) else { return };
        if let Some(body) = body_of(p, s(&f["filename"])) {
            self.apply(file_id, &body, &name_index(p));
        }
    }

    pub fn remove_file(&mut self, file_id: &str) {
        for target in self.outgoing.remove(file_id).unwrap_or_default() {
            if let Some(set) = self.incoming.get_mut(&target) {
                set.remove(file_id);
            }
        }
        self.dangling.remove(file_id);
        for source in self.incoming.remove(file_id).unwrap_or_default() {
            if let Some(set) = self.outgoing.get_mut(&source) {
                set.remove(file_id);
            }
        }
    }

    /// `[{ sourceId, sourceName, matches: [{ line, text }] }]`, by source name.
    pub fn backlink_snippets(&self, p: &Project, file_id: &str) -> Vec<Value> {
        let Some(target) = p.find(file_id) else {
            return Vec::new();
        };
        let target_name = s(&target["name"]).to_lowercase();
        let mut results: Vec<(String, Value)> = Vec::new();
        for source_id in self.incoming.get(file_id).into_iter().flatten() {
            let Some(src) = p.find(source_id) else {
                continue;
            };
            let Some(body) = body_of(p, s(&src["filename"])) else {
                continue;
            };
            let matches: Vec<Value> = body
                .split('\n')
                .enumerate()
                .filter(|(_, line)| {
                    WIKILINK_RE
                        .captures_iter(line)
                        .any(|c| c[1].trim().to_lowercase() == target_name)
                })
                .map(|(i, line)| json!({ "line": i + 1, "text": line.trim() }))
                .collect();
            if !matches.is_empty() {
                let name = s(&src["name"]).to_string();
                results.push((
                    name.clone(),
                    json!({ "sourceId": src["id"], "sourceName": name, "matches": matches }),
                ));
            }
        }
        results.sort_by(|a, b| {
            a.0.to_lowercase()
                .cmp(&b.0.to_lowercase())
                .then(a.0.cmp(&b.0))
        });
        results.into_iter().map(|(_, v)| v).collect()
    }

    pub fn all_note_names(p: &Project) -> Vec<Value> {
        p.files().iter().map(|f| f["name"].clone()).collect()
    }
}
