//! Import Word (.docx) and PowerPoint (.pptx) documents as notes. Port of
//! `apps/noteliner/src/main/import-service.js` (which used mammoth for Word and walked
//! the PPTX XML directly).

mod docx;
mod pptx;

use std::fs;
use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use regex::Regex;
use serde_json::{json, Value};

use crate::project::{extname, Project, MAX_ATTACHMENT_SIZE};

const MAX_DOC_SIZE: u64 = 100 * 1024 * 1024;

#[derive(Default)]
pub struct Stats {
    pub images: usize,
    pub slides: usize,
    pub tables_stripped: usize,
    pub warnings: Vec<String>,
}

pub async fn import(project: &mut Project, source: &Path) -> Result<Value> {
    if !source.is_absolute() {
        bail!("Invalid source path");
    }
    let ext = extname(&source.to_string_lossy()).to_lowercase();
    match ext.as_str() {
        ".docx" | ".pptx" => {}
        ".ppt" => bail!(
            "Legacy .ppt files are not supported. Open in PowerPoint or LibreOffice Impress and re-save as .pptx, then try again."
        ),
        "" => bail!("Unsupported file type: (none)"),
        other => bail!("Unsupported file type: {other}"),
    }
    if !project.is_open() {
        bail!("No project is open");
    }
    let meta = fs::metadata(source).map_err(|_| anyhow!("Source file does not exist"))?;
    if meta.len() > MAX_DOC_SIZE {
        bail!("Document exceeds {}MB limit", MAX_DOC_SIZE / 1024 / 1024);
    }
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Imported")
        .to_string();
    let bytes = fs::read(source)?;
    if ext == ".docx" {
        docx::import(project, &bytes, &stem).await
    } else {
        pptx::import(project, &bytes, &stem).await
    }
}

// --- Shared helpers -------------------------------------------------------------

pub(crate) struct Package {
    zip: zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
}

impl Package {
    pub fn open(bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            zip: zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec()))?,
        })
    }

    pub fn has(&mut self, name: &str) -> bool {
        self.zip.by_name(name).is_ok()
    }

    pub fn bytes(&mut self, name: &str) -> Option<Vec<u8>> {
        let mut f = self.zip.by_name(name).ok()?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).ok()?;
        Some(buf)
    }

    pub fn text(&mut self, name: &str) -> Option<String> {
        self.bytes(name)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// `Id` → (`Type`, `Target`, external?) from a `.rels` part.
    pub fn rels(&mut self, name: &str) -> Vec<(String, String, String, bool)> {
        let Some(xml) = self.text(name) else {
            return Vec::new();
        };
        let Ok(doc) = roxmltree::Document::parse(&xml) else {
            return Vec::new();
        };
        doc.descendants()
            .filter(|n| n.tag_name().name() == "Relationship")
            .map(|n| {
                (
                    n.attribute("Id").unwrap_or("").to_string(),
                    n.attribute("Type").unwrap_or("").to_string(),
                    n.attribute("Target").unwrap_or("").to_string(),
                    n.attribute("TargetMode") == Some("External"),
                )
            })
            .collect()
    }
}

/// POSIX-relative zip path resolution that can't escape the package root.
pub(crate) fn resolve_zip_path(base_dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.trim_start_matches('/').to_string();
    }
    let mut parts: Vec<&str> = base_dir.split('/').filter(|p| !p.is_empty()).collect();
    for p in target.split('/') {
        match p {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            p => parts.push(p),
        }
    }
    parts.join("/")
}

pub(crate) fn content_type_ext(ct: &str) -> &'static str {
    match ct {
        "image/png" => ".png",
        "image/jpeg" | "image/jpg" => ".jpg",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "image/svg+xml" => ".svg",
        "image/bmp" => ".bmp",
        "image/tiff" => ".tiff",
        _ => ".bin",
    }
}

pub(crate) fn ext_content_type(ext: &str) -> &'static str {
    match ext {
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".gif" => "image/gif",
        ".webp" => "image/webp",
        ".svg" => "image/svg+xml",
        ".bmp" => "image/bmp",
        ".tif" | ".tiff" => "image/tiff",
        ".emf" => "image/x-emf",
        ".wmf" => "image/x-wmf",
        _ => "",
    }
}

fn base36(mut n: u128) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

/// Shared image path for both importers. Returns the stored attachment
/// filename, or None when skipped (with a warning recorded).
pub(crate) async fn attach_image(
    project: &mut Project,
    file_id: &str,
    data: &[u8],
    content_type: &str,
    stats: &mut Stats,
) -> Option<String> {
    stats.images += 1;
    let ct = content_type.to_lowercase();
    if matches!(
        ct.as_str(),
        "image/x-emf" | "image/x-wmf" | "image/emf" | "image/wmf"
    ) {
        stats
            .warnings
            .push(format!("Unsupported image format skipped: {ct}"));
        return None;
    }
    let ext = content_type_ext(&ct);
    if ext == ".bin" {
        stats.warnings.push(format!(
            "Unknown image content type: {}",
            if ct.is_empty() { "(none)" } else { &ct }
        ));
    }
    if data.is_empty() {
        stats.warnings.push("Empty image skipped".into());
        return None;
    }
    if data.len() > MAX_ATTACHMENT_SIZE {
        stats
            .warnings
            .push("Image exceeded 30MB limit, skipped".into());
        return None;
    }
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let original = format!("image-{}-{}{ext}", base36(millis), stats.images);
    match project.add_attachment(file_id, data, &original).await {
        Ok(att) => att["filename"].as_str().map(str::to_string),
        Err(e) => {
            stats.warnings.push(format!("Failed to attach image: {e}"));
            None
        }
    }
}

/// Drop empty image refs, collapse blank runs, trim line ends.
pub(crate) fn tidy(md: &str) -> String {
    let empty_img = Regex::new(r"!\[[^\]]*\]\(\s*\)\n?").unwrap();
    let blank_runs = Regex::new(r"\n{3,}").unwrap();
    let md = empty_img.replace_all(md, "");
    let md = blank_runs.replace_all(&md, "\n\n");
    md.split('\n')
        .map(|l| l.trim_end_matches([' ', '\t']))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn finish(md: &str) -> String {
    let md = md.trim_start_matches('\n');
    let trimmed = md.trim_end_matches('\n');
    if trimmed.len() == md.len() {
        md.to_string()
    } else {
        format!("{trimmed}\n")
    }
}

pub(crate) fn stats_json(stats: &Stats, docx: bool) -> Value {
    if docx {
        json!({ "images": stats.images, "tablesStripped": stats.tables_stripped, "warnings": stats.warnings })
    } else {
        json!({ "slides": stats.slides, "images": stats.images, "warnings": stats.warnings })
    }
}

/// Differential check against the Electron importer. Not run by default:
///   IMPORT_DIFF_FILE=<doc> IMPORT_DIFF_OUT=<md> cargo test -p noteliner-core import_diff -- --ignored
#[cfg(test)]
#[tokio::test]
#[ignore]
async fn import_diff() {
    use std::sync::Arc;
    let src =
        std::path::PathBuf::from(std::env::var("IMPORT_DIFF_FILE").expect("IMPORT_DIFF_FILE"));
    let tmp = tempfile::tempdir().unwrap();
    let mut p = Project::new(crate::git::NoteGit::new(Arc::new(|_| {})));
    p.git.git.init(tmp.path()).await.unwrap();
    p.path = Some(tmp.path().to_path_buf());
    p.set_git_config("T", "t@x").await.unwrap();
    p.open(tmp.path()).await.unwrap();
    let res = import(&mut p, &src).await.unwrap();
    let body = p
        .read_file(res["entry"]["filename"].as_str().unwrap())
        .unwrap();
    // Name attachments att-1, att-2… like the JS reference stub.
    let mut n = 0;
    let body = Regex::new(r"att-[0-9a-f]{8}")
        .unwrap()
        .replace_all(&body, |_: &regex::Captures| {
            n += 1;
            format!("att-{n}")
        });
    std::fs::write(std::env::var("IMPORT_DIFF_OUT").unwrap(), body.as_bytes()).unwrap();
    println!("{}", res["stats"]);
}
