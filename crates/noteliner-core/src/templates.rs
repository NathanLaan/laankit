//! Note templates under `_templates/` (`apps/noteliner/src/main/template-service.js`).
//! Templates are versioned with the project but never indexed.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::frontmatter;
use crate::project::{slugify, Project};
use crate::yaml::Yaml;

pub const TEMPLATES_DIR: &str = "_templates";

fn dir(p: &Project) -> Option<PathBuf> {
    p.path.as_ref().map(|d| d.join(TEMPLATES_DIR))
}

/// "meeting-notes.md" → "Meeting Notes".
pub fn prettify(filename: &str) -> String {
    let base = if filename.to_lowercase().ends_with(".md") {
        &filename[..filename.len() - 3]
    } else {
        filename
    };
    let spaced: String = base
        .chars()
        .map(|c| if c == '-' || c == '_' { ' ' } else { c })
        .collect();
    let collapsed = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    // /\b\w/g → uppercase: \w and \b are ASCII-only in JS.
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::new();
    let mut prev_word = false;
    for c in collapsed.chars() {
        let w = is_word(c);
        out.push(if w && !prev_word {
            c.to_ascii_uppercase()
        } else {
            c
        });
        prev_word = w;
    }
    out
}

fn kind_of(path: &Path) -> &'static str {
    let Ok(raw) = fs::read_to_string(path) else {
        return "note";
    };
    match frontmatter::get_field(&frontmatter::parse(&raw).data, "kind") {
        Some(Yaml::Str(k)) if k == "deck" => "deck",
        Some(Yaml::Str(k)) if k == "slide" => "slide",
        _ => "note",
    }
}

/// `[{ id, name, kind }]`, sorted by display name.
pub fn list(p: &Project) -> Vec<Value> {
    let Some(dir) = dir(p) else { return Vec::new() };
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, Value)> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|f| f.to_lowercase().ends_with(".md"))
        .map(|f| {
            let name = prettify(&f);
            let kind = kind_of(&dir.join(&f));
            (name.clone(), json!({ "id": f, "name": name, "kind": kind }))
        })
        .collect();
    out.sort_by(|a, b| {
        a.0.to_lowercase()
            .cmp(&b.0.to_lowercase())
            .then(a.0.cmp(&b.0))
    });
    out.into_iter().map(|(_, v)| v).collect()
}

/// Replace `{{title}}`, `{{date}}`, `{{time}}`, `{{datetime}}`; leave
/// unknown placeholders untouched.
pub fn substitute(text: &str, title: &str) -> String {
    let now = chrono::Local::now();
    let date = now.format("%Y-%m-%d").to_string();
    let time = now.format("%H:%M").to_string();
    let re = regex::Regex::new(r"\{\{(\w+)\}\}").unwrap();
    re.replace_all(text, |c: &regex::Captures| match &c[1] {
        "title" => title.to_string(),
        "date" => date.clone(),
        "time" => time.clone(),
        "datetime" => format!("{date} {time}"),
        _ => c[0].to_string(),
    })
    .into_owned()
}

pub fn body_for(p: &Project, id: &str, title: &str) -> Option<String> {
    let dir = dir(p)?;
    // Bare filenames only — no traversal out of _templates/.
    let safe = Path::new(id).file_name()?;
    let raw = fs::read_to_string(dir.join(safe)).ok()?;
    Some(substitute(&raw, title))
}

pub async fn save(p: &Project, name: &str, body: &str) -> Result<Value> {
    let Some(dir) = dir(p) else {
        bail!("No project open")
    };
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("Template name cannot be empty");
    }
    fs::create_dir_all(&dir)?;
    let slug = slugify(trimmed);
    let filename = format!("{}.md", if slug.is_empty() { "template" } else { &slug });
    fs::write(dir.join(&filename), body)?;
    let root = p.dir()?.to_path_buf();
    p.git
        .commit(&root, &format!("Save template {trimmed}"))
        .await?;
    p.git.schedule_push(&root);
    Ok(json!({ "id": filename, "name": prettify(&filename) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prettify_like_js() {
        assert_eq!(prettify("meeting-notes.md"), "Meeting Notes");
        assert_eq!(prettify("a__b--c.MD"), "A B C");
        assert_eq!(prettify("2026-plan.md"), "2026 Plan");
    }

    #[test]
    fn substitutes_known_placeholders_only() {
        let out = substitute("# {{title}} {{unknown}} {{date}}", "T");
        assert!(out.starts_with("# T {{unknown}} 20"));
    }
}
