//! Headless measurements: open time, time to first page, per-page render and text extraction
//! times, and the memory 20 cached page textures take.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use hayro::hayro_interpret::{InterpreterCache, InterpreterSettings};
use hayro::RenderCache;
use spike_common::Stats;
use spike_pdf::{doc, text};

/// Textures the reader keeps (`reader::CACHE_PAGES`).
const CACHED_PAGES: usize = 20;
/// A typical fit-width zoom: a 612 pt page in a ~900 px column.
const FIT_WIDTH_ZOOM: f32 = 1.5;

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let mut pages_limit = usize::MAX;
    let mut png_dir: Option<String> = None;
    let mut words_page: Option<usize> = None;
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--pages" => pages_limit = it.next().and_then(|n| n.parse().ok()).unwrap_or(usize::MAX),
            "--png-dir" => png_dir = it.next().cloned(),
            "--words" => words_page = it.next().and_then(|n| n.parse().ok()),
            _ => files.push(a.clone()),
        }
    }
    if let Some(page) = words_page {
        for file in &files {
            print_words(Path::new(file), page)?;
        }
        return Ok(());
    }
    for file in files {
        bench_file(Path::new(&file), pages_limit, png_dir.as_deref())?;
    }
    Ok(())
}

fn bench_file(path: &Path, pages_limit: usize, png_dir: Option<&str>) -> anyhow::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let settings = InterpreterSettings::default();

    // Time to first page: read, parse, and render page 1 at 1x with cold caches.
    let start = Instant::now();
    let bytes = Arc::new(std::fs::read(path)?);
    let pdf = doc::open(bytes.clone())?;
    let opened = start.elapsed();
    let sizes = doc::page_sizes(&pdf);
    {
        let cache = RenderCache::new();
        doc::render_page(&pdf.pages()[0], &cache, &settings, 1.0);
    }
    let first = start.elapsed();
    println!(
        "== {name}: {} pages, open {:.1} ms, first page (1x) {:.1} ms",
        sizes.len(),
        ms(opened),
        ms(first)
    );

    let n = sizes.len().min(pages_limit);
    let cache = RenderCache::new();
    for scale in [1.0, 2.0, 3.0] {
        let mut stats = Stats::default();
        for i in 0..n {
            let image = stats.time(|| doc::render_page(&pdf.pages()[i], &cache, &settings, scale));
            if let Some(dir) = png_dir.filter(|_| scale == 2.0 && i < 3) {
                save_png(
                    &image,
                    &Path::new(dir).join(format!("{}-p{}-hayro.png", stem(path), i + 1)),
                )?;
            }
        }
        println!("  render {scale}x: {}", stats.summary("per page"));
    }

    let interp = InterpreterCache::new();
    let mut stats = Stats::default();
    let mut chars = 0;
    let mut empty_pages = Vec::new();
    for i in 0..n {
        let t = stats.time(|| text::extract(&pdf.pages()[i], &interp, &settings));
        chars += t.chars.len();
        if t.chars.is_empty() {
            empty_pages.push(i + 1);
        }
    }
    println!(
        "  text: {}; {chars} chars, pages without text: {empty_pages:?}",
        stats.summary("per page")
    );
    if let Some(t) = (0..n)
        .map(|i| text::extract(&pdf.pages()[i], &interp, &settings))
        .find(|t| t.chars.len() > 200)
    {
        let sample = t.range(0, 199);
        println!("  text sample: {:?}", sample.text);
    }

    // Memory for the reader's texture cache at fit width, 1x and 2x (HiDPI).
    for ppp in [1.0, 2.0] {
        let scale = FIT_WIDTH_ZOOM * ppp;
        let before = rss_bytes();
        let images: Vec<_> = (0..CACHED_PAGES.min(sizes.len()))
            .map(|i| doc::render_page(&pdf.pages()[i], &cache, &settings, scale))
            .collect();
        let pixels: usize = images.iter().map(|im| im.pixels.len() * 4).sum();
        let after = rss_bytes();
        println!(
            "  {CACHED_PAGES} cached pages at {scale}x: {:.1} MB of RGBA (RSS +{:.1} MB)",
            mb(pixels),
            mb(after.saturating_sub(before))
        );
        drop(images);
    }
    Ok(())
}

/// The first words on 1-based `page` with their boxes (x0, y0, x1, y1), to compare with
/// `pdftotext -bbox`.
fn print_words(path: &Path, page: usize) -> anyhow::Result<()> {
    let pdf = doc::open(Arc::new(std::fs::read(path)?))?;
    let t = text::extract(
        &pdf.pages()[page - 1],
        &InterpreterCache::new(),
        &InterpreterSettings::default(),
    );
    let mut start = 0;
    let mut shown = 0;
    for (i, c) in t.chars.iter().enumerate() {
        let end_of_word = c.text.trim().is_empty()
            || t.chars.get(i + 1).is_none_or(|n| {
                n.rect[0] < c.rect[0] || n.rect[0] - (c.rect[0] + c.rect[2]) > 0.2 * c.rect[3]
            });
        if !end_of_word {
            continue;
        }
        let last = if c.text.trim().is_empty() {
            i.saturating_sub(1)
        } else {
            i
        };
        if last >= start && !t.chars[start].text.trim().is_empty() {
            let w = t.range(start, last);
            let r = w.rects[0];
            println!(
                "{:<16} {:7.2} {:7.2} {:7.2} {:7.2}",
                w.text,
                r[0],
                r[1],
                r[0] + r[2],
                r[1] + r[3]
            );
            shown += 1;
            if shown == 12 {
                break;
            }
        }
        start = i + 1;
    }
    Ok(())
}

fn save_png(image: &egui::ColorImage, out: &Path) -> anyhow::Result<()> {
    let [w, h] = image.size;
    let raw: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    image::RgbaImage::from_raw(w as u32, h as u32, raw)
        .ok_or_else(|| anyhow::anyhow!("bad image size"))?
        .save(out)?;
    Ok(())
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().replace(' ', "_"))
        .unwrap_or_default()
}

/// Resident set size (Linux; 0 elsewhere).
fn rss_bytes() -> usize {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<usize>().ok())
        .map_or(0, |pages| pages * 4096)
}

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
