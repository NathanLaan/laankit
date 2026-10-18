//! `spike-epub report <book.epub>…`: open and per-chapter conversion and layout times, headless.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use spike_common::Stats;
use spike_richtext::fonts::Face;
use spike_richtext::headless::Headless;

use crate::book::Book;

pub fn run(paths: &[PathBuf]) -> Result<()> {
    let headless = Headless::new(Face::Serif, 1100.0, 900.0);
    for path in paths {
        let start = Instant::now();
        let mut book = Book::open(path)?;
        let open = start.elapsed();
        let (mut convert, mut measure, mut steady, mut jump) = (
            Stats::default(),
            Stats::default(),
            Stats::default(),
            Stats::default(),
        );
        let (mut blocks, mut images, mut chars, mut biggest) = (0, 0, 0, (0, 0));
        for i in 0..book.spine.len() {
            let doc = convert.time(|| book.convert_chapter(i));
            let t = headless.time(&doc);
            measure.push(t.measure);
            steady.push(t.steady);
            jump.push(t.jump);
            blocks += doc.blocks.len();
            images += doc.image_count();
            chars += doc.plain_text().chars().count();
            biggest = biggest.max((doc.blocks.len(), i));
        }
        println!(
            "{} ({})",
            book.title,
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        println!(
            "  open (container, OPF, NCX, CSS): {:.2} ms",
            open.as_secs_f64() * 1000.0
        );
        println!("  {} spine documents, {} TOC entries, {blocks} blocks, {images} images, {chars} characters", book.spine.len(), book.toc.len());
        println!(
            "  largest section: #{} with {} blocks",
            biggest.1, biggest.0
        );
        println!("  {}", convert.summary("read + convert per section"));
        println!(
            "  {}",
            measure.summary("first frame per section (measure all)")
        );
        println!("  {}", steady.summary("steady frame"));
        println!("  {}", jump.summary("frame after jumping to the middle"));
    }
    Ok(())
}
