//! Option C spike 5: NoteLiner's PDF export without a webview.
//!
//! Markdown → Typst markup ([`markup`]) → `typst::compile` against an
//! in-memory [`world::ExportWorld`] → `typst_pdf::pdf`. HTML export stays on
//! `noteliner_core::export::html_document` (comrak), unchanged.
//! See `docs/plans/plan-native-rust-egui.md`, Phase 0.

pub mod markup;
pub mod world;

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use typst_layout::PagedDocument;

pub use world::Fonts;

/// Exports notes to PDF. Keep one per app: the fonts and Typst's caches are
/// reused, so later exports are much faster than the first.
pub struct Exporter {
    fonts: Arc<Fonts>,
    family: Option<String>,
}

impl Exporter {
    pub fn new(fonts: Fonts) -> Self {
        Self {
            fonts: Arc::new(fonts),
            family: None,
        }
    }

    /// Sets the body font family (one of the fonts passed to [`Fonts::new`]).
    pub fn with_family(mut self, family: impl Into<String>) -> Self {
        self.family = Some(family.into());
        self
    }

    /// Note `name` (its body, frontmatter already stripped) as PDF bytes.
    /// Images resolve against `project_dir` (where `_attachments/` lives).
    pub fn pdf(&self, name: &str, body_md: &str, project_dir: Option<&Path>) -> Result<Vec<u8>> {
        let markup = match project_dir {
            Some(root) => markup::document(
                name,
                body_md,
                self.family.as_deref(),
                &markup::DirImages { root },
            ),
            None => markup::document(name, body_md, self.family.as_deref(), &markup::NoImages),
        };
        self.compile(markup, project_dir)
    }

    /// Compiles generated markup; `root` is where image paths resolve.
    pub fn compile(&self, markup: String, root: Option<&Path>) -> Result<Vec<u8>> {
        let world =
            world::ExportWorld::new(self.fonts.clone(), markup, root.map(Path::to_path_buf));
        let doc: PagedDocument = typst::compile(&world).output.map_err(|errs| {
            anyhow!(
                "typst: {}",
                errs.iter()
                    .map(|e| e.message.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        })?;
        let pdf = typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default()).map_err(|errs| {
            anyhow!(
                "pdf: {}",
                errs.iter()
                    .map(|e| e.message.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        })?;
        // Bound Typst's memoization cache between exports (as the Typst CLI's watcher does).
        typst::comemo::evict(10);
        Ok(pdf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_a_pdf_without_system_fonts() {
        let exporter = Exporter::new(Fonts::new([]));
        let pdf = exporter.pdf("Title: with *chars* #", "Body with **bold**, `code`, [[Wiki|link]] and a table:\n\n| a | b |\n|---|---|\n| 1 | 2 |\n", None).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
    }

    /// Random Markdown built from block markers, inline syntax and characters
    /// special to Typst must always compile: escaping or structure bugs show up
    /// as Typst errors.
    #[test]
    fn fuzz_random_markdown_compiles() {
        const PIECES: &[&str] = &[
            "# ",
            "###### ",
            "- ",
            "  - ",
            "1. ",
            "3) ",
            "> ",
            ">> ",
            "- [ ] ",
            "- [x] ",
            "```\n",
            "```rust\n",
            "~~~",
            "    ",
            "| a | b |\n|---|:-:|\n| 1 | 2 |\n",
            "[[",
            "]]",
            "|",
            "[^n]",
            "\n[^n]: note\n",
            "**",
            "*",
            "_",
            "~~",
            "`",
            "![x](_attachments/a.png)",
            "[l](http://x.y/a_b)",
            "<b>",
            "</b>",
            "<br>",
            "<div>",
            "\n",
            "\n\n",
            "\\",
            "#",
            "$",
            "[",
            "]",
            "<lbl>",
            "@r",
            "=",
            "+",
            "/",
            "~",
            "...",
            "--",
            "'",
            "\"",
            ":",
            "{",
            "}",
            "é😀",
            "word ",
            "---\n",
            "***\n",
            "Term\n: def\n",
            "&amp;",
            "&#35;",
            "\t",
        ];
        let exporter = Exporter::new(Fonts::new([]));
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for case in 0..150 {
            let len = 5 + (next() % 60) as usize;
            let md: String = (0..len)
                .map(|_| PIECES[(next() % PIECES.len() as u64) as usize])
                .collect();
            if let Err(e) = exporter.pdf("Fuzz #*_[]", &md, None) {
                panic!(
                    "case {case}: {e}\nmarkdown: {md:?}\ntypst:\n{}",
                    markup::markdown_to_typst(&md, &markup::NoImages)
                );
            }
        }
    }
}
