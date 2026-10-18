//! Spike 4 (see `docs/plans/plan-native-rust-egui.md`, Phase 0): a native EPUB reader. Chapters
//! go through the same rich-text blocks as ThreadLiner's articles (`spike-richtext`).
//!
//! ```text
//! spike-epub <book.epub>              # the reader (MARINA_CHAPTER=<n> opens a section)
//! spike-epub report <book.epub>…      # headless timings
//! ```
//!
//! Positions are saved in `$SPIKE_EPUB_DIR/positions.json`.

mod app;
mod book;
mod positions;
mod report;

use std::path::PathBuf;

use anyhow::{anyhow, Result};

fn main() -> Result<()> {
    spike_common::init_logging();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("report") => report::run(&args[1..].iter().map(PathBuf::from).collect::<Vec<_>>()),
        Some(path) => {
            let book = book::Book::open(&PathBuf::from(path))?;
            spike_common::run("Spike: EPUB", move |cc| app::ReaderApp::new(cc, book))
                .map_err(|e| anyhow!("{e}"))
        }
        None => Err(anyhow!(
            "usage: spike-epub <book.epub> | spike-epub report <book.epub>…"
        )),
    }
}
