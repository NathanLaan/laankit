//! `spike-export [--font FILE]... [--family NAME] [--typ OUT.typ] [--repeat N] <in.md> <out.pdf|out.html|out.print.html>`
//!
//! Exports a NoteLiner note the way the native app would: frontmatter
//! stripped, images resolved against the note's folder (its `_attachments/`).
//! `.html` output goes through `noteliner_core::export::html_document`;
//! `.print.html` writes the webview print document (`export::pdf_document`).
//! `--repeat N` exports N more times and reports cold vs. warm timings.
//! `--gen-pathological OUT.md` writes the stress fixture instead.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use noteliner_core::{export, frontmatter};
use spike_export::{markup, Exporter, Fonts};

fn main() -> Result<()> {
    let mut fonts = Vec::new();
    let (mut family, mut typ_out, mut repeat) = (None, None, 0usize);
    let mut paths = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--font" => fonts.push(std::fs::read(args.next().context("--font FILE")?)?),
            "--family" => family = Some(args.next().context("--family NAME")?),
            "--typ" => typ_out = Some(PathBuf::from(args.next().context("--typ FILE")?)),
            "--repeat" => repeat = args.next().context("--repeat N")?.parse()?,
            "--gen-pathological" => {
                let out = args.next().context("--gen-pathological OUT.md")?;
                std::fs::write(&out, pathological())?;
                return Ok(());
            }
            _ => paths.push(PathBuf::from(arg)),
        }
    }
    let [input, output] = paths.as_slice() else {
        bail!("usage: spike-export [--font FILE]... [--family NAME] [--typ OUT.typ] [--repeat N] <in.md> <out.pdf|out.html|out.print.html>")
    };
    let raw =
        std::fs::read_to_string(input).with_context(|| format!("reading {}", input.display()))?;
    let body = frontmatter::strip_body(&raw);
    let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("Note");
    let project_dir = input
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));

    if output.to_string_lossy().ends_with(".print.html") {
        // The webview print document (`export::pdf_document`), for comparing against the old path.
        let attachments = std::path::absolute(project_dir.join("_attachments"))?;
        std::fs::write(output, export::pdf_document(name, &body, &attachments))?;
        return Ok(());
    }
    if output.extension().is_some_and(|e| e == "html") {
        std::fs::write(output, export::html_document(name, &body))?;
        return Ok(());
    }

    let start = Instant::now();
    let fonts = Fonts::new(fonts);
    let font_load = start.elapsed();
    let mut exporter = Exporter::new(fonts);
    if let Some(family) = &family {
        exporter = exporter.with_family(family.clone());
    }
    if let Some(typ_out) = &typ_out {
        let images = markup::DirImages { root: project_dir };
        std::fs::write(
            typ_out,
            markup::document(name, &body, family.as_deref(), &images),
        )?;
    }

    let t = Instant::now();
    let markup_only = markup::document(
        name,
        &body,
        family.as_deref(),
        &markup::DirImages { root: project_dir },
    );
    let convert = t.elapsed();

    let t = Instant::now();
    let pdf = exporter.pdf(name, &body, Some(project_dir))?;
    let cold = t.elapsed();
    std::fs::write(output, &pdf)?;

    let mut warm = Vec::new();
    for _ in 0..repeat {
        let t = Instant::now();
        exporter.pdf(name, &body, Some(project_dir))?;
        warm.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    warm.sort_by(f64::total_cmp);
    println!(
        "input: {} lines, {} bytes; markup {} bytes",
        raw.lines().count(),
        raw.len(),
        markup_only.len()
    );
    println!(
        "font load: {:.1} ms; md→typst: {:.2} ms; first export (cold): {:.1} ms; pdf {} bytes",
        font_load.as_secs_f64() * 1000.0,
        convert.as_secs_f64() * 1000.0,
        cold.as_secs_f64() * 1000.0,
        pdf.len()
    );
    if !warm.is_empty() {
        println!(
            "warm exports: n={}, median {:.1} ms, max {:.1} ms",
            warm.len(),
            warm[warm.len() / 2],
            warm[warm.len() - 1]
        );
    }
    Ok(())
}

/// A ~2,000-line stress note: long prose, many headings and code blocks, a
/// list nested 40 deep, a 300 × 8 table and an unbreakable 600-character URL.
fn pathological() -> String {
    use std::fmt::Write;
    let mut md = String::from("---\ntitle: Pathological\n---\n\n");
    for section in 0..120 {
        let _ = writeln!(md, "## Section {section} with `code` and *emphasis*\n");
        for p in 0..4 {
            let _ = writeln!(md, "Paragraph {p} of section {section}: lorem ipsum dolor sit amet, **consectetur** adipiscing elit, sed do [[Wiki {section}]] eiusmod tempor_incididunt ut labore #{section} et dolore magna aliqua $5 [x] <y>.\n");
        }
        if section % 10 == 0 {
            md.push_str("```rust\n");
            for i in 0..20 {
                let _ = writeln!(md, "fn f{i}(x: u32) -> u32 {{ x * {i} + 1 }} // \"#{i}\"");
            }
            md.push_str("```\n\n");
        }
    }
    for depth in 0..40 {
        let _ = writeln!(md, "{}- level {depth}", "  ".repeat(depth));
    }
    md.push_str("\n| # | A | B | C | D | E | F | G |\n|---|---|---|---|---|---|---|---|\n");
    for r in 0..300 {
        let _ = writeln!(
            md,
            "| {r} | alpha {r} | beta | gamma delta | `c{r}` | **e** | f_{r} | g#{r} |"
        );
    }
    let _ = writeln!(md, "\nhttps://example.com/{}\n", "a".repeat(600));
    md
}
