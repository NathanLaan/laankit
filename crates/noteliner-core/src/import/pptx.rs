//! PowerPoint import: slides in `<p:sldIdLst>` order, each rendered as
//!   ## Slide N[: HIDDEN][: Title]
//!   body paragraphs / bullets, images, ### Notes

use anyhow::{anyhow, bail, Result};
use roxmltree::{Document, Node};
use serde_json::{json, Value};

use super::{
    attach_image, ext_content_type, finish, resolve_zip_path, stats_json, tidy, Package, Stats,
};
use crate::project::{extname, Project};

const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NOTES_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";

/// Elements named as in the source (`p:sp`), like xmldom's
/// getElementsByTagName.
fn qname(n: Node) -> String {
    let t = n.tag_name();
    match t.namespace().and_then(|ns| n.lookup_prefix(ns)) {
        Some(p) if !p.is_empty() => format!("{p}:{}", t.name()),
        _ => t.name().to_string(),
    }
}

fn by_tag<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Vec<Node<'a, 'i>> {
    n.descendants()
        .filter(|d| d.is_element() && *d != n && qname(*d) == name)
        .collect()
}

fn first<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.descendants()
        .find(|d| d.is_element() && *d != n && qname(*d) == name)
}

fn text_content(n: Node) -> String {
    n.descendants()
        .filter(|d| d.is_text())
        .filter_map(|d| d.text())
        .collect()
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn placeholder_type(sp: Node) -> Option<String> {
    first(sp, "p:ph").map(|ph| ph.attribute("type").unwrap_or("").to_string())
}

/// One paragraph → a Markdown line, or None when empty. Body placeholders
/// bullet by default unless `<a:buNone/>` says otherwise.
fn render_paragraph(p: Node, default_bullet: bool) -> Option<String> {
    let runs: String = by_tag(p, "a:t").into_iter().map(text_content).collect();
    let text = collapse_ws(&runs);
    if text.is_empty() {
        return None;
    }
    let ppr = first(p, "a:pPr");
    let lvl = ppr
        .and_then(|n| n.attribute("lvl"))
        .and_then(|l| l.parse::<usize>().ok())
        .unwrap_or(0)
        .min(6);
    let mut bullet = default_bullet;
    if let Some(ppr) = ppr {
        if first(ppr, "a:buNone").is_some() {
            bullet = false;
        } else if first(ppr, "a:buChar").is_some() || first(ppr, "a:buAutoNum").is_some() {
            bullet = true;
        }
    }
    Some(if bullet {
        format!("{}- {text}", "  ".repeat(lvl))
    } else {
        text
    })
}

fn rel_id(n: Node, local: &str) -> Option<String> {
    n.attribute((REL_NS, local)).map(str::to_string)
}

pub async fn import(project: &mut Project, bytes: &[u8], stem: &str) -> Result<Value> {
    let mut pkg = Package::open(bytes).map_err(|e| anyhow!("Not a valid .pptx archive: {e}"))?;
    if !pkg.has("ppt/presentation.xml") {
        bail!("Not a valid .pptx file (missing ppt/presentation.xml)");
    }
    let entry = project
        .create_file(stem, &json!([]), Some(String::new()), None)
        .await?;
    let file_id = entry["id"].as_str().unwrap_or("").to_string();
    let mut stats = Stats::default();

    let slides =
        slide_order(&mut pkg).map_err(|e| anyhow!("Failed to resolve slide order: {e}"))?;
    let date = chrono::Utc::now().format("%Y-%m-%d");
    let mut out = vec![format!("# {stem} (Imported {date})"), String::new()];
    for (i, path) in slides.iter().enumerate() {
        stats.slides += 1;
        let n = i + 1;
        match render_slide(project, &mut pkg, path, n, &file_id, &mut stats).await {
            Ok(md) => out.push(md),
            Err(e) => {
                stats.warnings.push(format!("Slide {n}: {e}"));
                out.extend([
                    format!("## Slide {n}"),
                    String::new(),
                    format!("_[Slide {n} failed to import: {e}]_"),
                    String::new(),
                ]);
            }
        }
    }
    let markdown = finish(&tidy(&out.join("\n")));
    let filename = entry["filename"].as_str().unwrap_or("").to_string();
    project.write_file(&filename, &markdown).await?;
    Ok(json!({ "entry": entry, "stats": stats_json(&stats, false) }))
}

fn slide_order(pkg: &mut Package) -> Result<Vec<String>> {
    let pres = pkg
        .text("ppt/presentation.xml")
        .ok_or_else(|| anyhow!("missing presentation.xml"))?;
    let doc = Document::parse(&pres)?;
    let rels = pkg.rels("ppt/_rels/presentation.xml.rels");
    Ok(by_tag(doc.root(), "p:sldId")
        .into_iter()
        .filter_map(|s| rel_id(s, "id").or_else(|| s.attribute("r:id").map(str::to_string)))
        .filter_map(|rid| {
            rels.iter()
                .find(|r| r.0 == rid)
                .map(|r| resolve_zip_path("ppt/", &r.2))
        })
        .collect())
}

async fn render_slide(
    project: &mut Project,
    pkg: &mut Package,
    slide_path: &str,
    n: usize,
    file_id: &str,
    stats: &mut Stats,
) -> Result<String> {
    let xml = pkg
        .text(slide_path)
        .ok_or_else(|| anyhow!("missing {slide_path}"))?;
    let doc = Document::parse(&xml)?;
    let hidden = first(doc.root(), "p:sld")
        .or_else(|| (qname(doc.root_element()) == "p:sld").then(|| doc.root_element()))
        .and_then(|s| s.attribute("show"))
        == Some("0");
    let slide_dir = &slide_path[..slide_path.rfind('/').unwrap_or(0)];
    let slide_file = slide_path.rsplit('/').next().unwrap_or(slide_path);
    let rels = pkg.rels(&format!("{slide_dir}/_rels/{slide_file}.rels"));

    // Walk the shape tree first (sync), collecting images to attach after.
    let mut title = String::new();
    let mut body = Vec::new();
    let mut pics: Vec<(String, Vec<u8>)> = Vec::new();
    if let Some(tree) = first(doc.root(), "p:spTree") {
        for node in tree.children().filter(|c| c.is_element()) {
            match qname(node).as_str() {
                "p:sp" => {
                    let ph = placeholder_type(node);
                    let is_title = matches!(ph.as_deref(), Some("title" | "ctrTitle"));
                    let is_body = matches!(ph.as_deref(), Some("body" | "subTitle" | ""));
                    let Some(tx) = first(node, "p:txBody") else {
                        continue;
                    };
                    if is_title && title.is_empty() {
                        title = collapse_ws(
                            &by_tag(tx, "a:t")
                                .into_iter()
                                .map(text_content)
                                .collect::<Vec<_>>()
                                .join(" "),
                        );
                        continue;
                    }
                    body.extend(
                        by_tag(tx, "a:p")
                            .into_iter()
                            .filter_map(|p| render_paragraph(p, is_body)),
                    );
                }
                "p:pic" => {
                    let Some(blip) = first(node, "a:blip") else {
                        continue;
                    };
                    let Some(rid) = rel_id(blip, "embed")
                        .or_else(|| blip.attribute("r:embed").map(str::to_string))
                    else {
                        continue;
                    };
                    let Some(rel) = rels.iter().find(|r| r.0 == rid) else {
                        continue;
                    };
                    let media = resolve_zip_path("ppt/slides/", &rel.2);
                    match pkg.bytes(&media) {
                        Some(data) => pics.push((
                            ext_content_type(&extname(&media).to_lowercase()).to_string(),
                            data,
                        )),
                        None => stats
                            .warnings
                            .push(format!("Missing embedded image: {media}")),
                    }
                }
                "p:graphicFrame" => stats
                    .warnings
                    .push("Skipped embedded graphic frame (table/chart/SmartArt)".into()),
                _ => {}
            }
        }
    }
    let mut images = Vec::new();
    for (ct, data) in pics {
        if let Some(f) = attach_image(project, file_id, &data, &ct, stats).await {
            images.push(f);
        }
    }

    let mut suffix = Vec::new();
    if hidden {
        suffix.push("HIDDEN".to_string());
    }
    if !title.is_empty() {
        suffix.push(title);
    }
    let heading = if suffix.is_empty() {
        format!("## Slide {n}")
    } else {
        format!("## Slide {n}: {}", suffix.join(": "))
    };
    let mut lines = vec![heading, String::new()];
    if !body.is_empty() {
        lines.extend(body);
        lines.push(String::new());
    }
    for f in images {
        lines.push(format!("![](./_attachments/{f})"));
        lines.push(String::new());
    }
    if let Some(notes) = rels.iter().find(|r| r.1 == NOTES_REL) {
        let notes_path = resolve_zip_path(&format!("{slide_dir}/"), &notes.2);
        let notes = render_notes(pkg, &notes_path);
        if !notes.is_empty() {
            lines.push("### Notes".into());
            lines.push(String::new());
            lines.extend(notes);
            lines.push(String::new());
        }
    }
    Ok(lines.join("\n"))
}

fn render_notes(pkg: &mut Package, path: &str) -> Vec<String> {
    let Some(xml) = pkg.text(path) else {
        return Vec::new();
    };
    let Ok(doc) = Document::parse(&xml) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for sp in by_tag(doc.root(), "p:sp") {
        if placeholder_type(sp).as_deref() == Some("sldImg") {
            continue;
        }
        let Some(tx) = first(sp, "p:txBody") else {
            continue;
        };
        lines.extend(
            by_tag(tx, "a:p")
                .into_iter()
                .filter_map(|p| render_paragraph(p, false)),
        );
    }
    lines
}
